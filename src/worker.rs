//! Background thread: picks up inbox files, reloads items.json when it's edited by hand,
//! and runs Google sync on a timer or on request. Works while the window is hidden.

use crate::store::Data;
use crate::{gcal, import, notify};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub enum Job {
    SyncNow,
}

/// The window's egui context while a window exists (None while only in the tray).
pub type UiHandle = Arc<Mutex<Option<eframe::egui::Context>>>;

fn repaint(ui: &UiHandle) {
    if let Some(ctx) = ui.lock().unwrap().as_ref() {
        ctx.request_repaint();
    }
}

pub fn spawn(shared: Arc<Mutex<Data>>, ui: UiHandle, jobs: Receiver<Job>) {
    std::thread::spawn(move || {
        // First automatic sync shortly after startup.
        let mut next_sync = Instant::now() + Duration::from_secs(5);
        // First reminder check a couple of minutes after startup, then once a minute.
        let mut next_reminder_check = Instant::now() + Duration::from_secs(120);
        let mut last_reminder: Option<Instant> = None;
        // "starting soon" reminders already given (task id @ day)
        let mut reminded_soon: std::collections::HashSet<String> = std::collections::HashSet::new();
        let theme_mtime = |d: &Data| {
            std::fs::metadata(crate::ui::theme::theme_path(&d.paths.root, &d.settings.theme)).and_then(|m| m.modified()).ok()
        };
        let mut theme_seen = (String::new(), None);
        loop {
            let mut sync_requested = false;
            // phone server follows the settings (also while only in the tray)
            crate::phone::sync(&shared, &ui);
            match jobs.recv_timeout(Duration::from_secs(3)) {
                Ok(Job::SyncNow) => sync_requested = true,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }

            let (do_sync, interval) = {
                let mut d = shared.lock().unwrap();
                let mut changed = false;
                if d.files_changed_on_disk() {
                    d.reload_all();
                    d.status = "Reloaded data files (changed on disk)".into();
                    changed = true;
                }
                // live theme editing: re-activate when the theme file (or the choice) changes
                let now_theme = (d.settings.theme.clone(), theme_mtime(&d));
                if theme_seen.0.is_empty() {
                    theme_seen = now_theme;
                } else if now_theme != theme_seen {
                    theme_seen = now_theme;
                    let (root, name) = (d.paths.root.clone(), d.settings.theme.clone());
                    d.status = match crate::ui::theme::activate(&root, &name) {
                        Ok(()) => format!("Theme \"{name}\" loaded"),
                        Err(e) => format!("Theme error: {e}"),
                    };
                    changed = true;
                }
                if let Some(msg) = import::process_inbox(&mut d) {
                    d.status = msg;
                    changed = true;
                    // push the new items soon
                    next_sync = next_sync.min(Instant::now() + Duration::from_secs(2));
                }
                if changed {
                    repaint(&ui);
                }
                let due = d.settings.auto_sync && Instant::now() >= next_sync;
                let ok = d.google_connected && !d.syncing && d.load_error.is_none();
                (ok && (sync_requested || due), d.settings.sync_interval_min.max(1))
            };

            if Instant::now() >= next_reminder_check {
                next_reminder_check = Instant::now() + Duration::from_secs(60);
                let (reminder, interval, paths, route, soon) = {
                    let d = shared.lock().unwrap();
                    let may = notify::may_remind(&d.settings);
                    let reminder = if may { notify::pending_reminder(&d) } else { None };
                    let soon: Vec<(String, String)> =
                        if may { notify::upcoming(&d, chrono::Local::now()).into_iter().filter(|(k, _)| !reminded_soon.contains(k)).collect() } else { Vec::new() };
                    let route = notify::route(&d.settings, crate::phone::has_push(&d), notify::pc_idle_secs());
                    (reminder, d.settings.reminder_interval_min.max(5) as u64, d.paths.clone(), route, soon)
                };
                let deliver = |r: &notify::Reminder| {
                    let mut ok = false;
                    if route.windows {
                        ok |= notify::show(&paths, r).is_ok();
                    }
                    if route.phone {
                        ok |= crate::phone::push_all(&shared, &notify::phone_payload(r)) > 0;
                    }
                    // the phone couldn't be reached: fall back to Windows
                    if !ok && route.phone && !route.windows {
                        ok = notify::show(&paths, r).is_ok();
                    }
                    ok
                };
                // "starting soon" for timed tasks: once per task, not held back by the interval
                if !soon.is_empty() {
                    let r = notify::Reminder { lines: soon.iter().map(|(_, l)| l.clone()).collect(), ..Default::default() };
                    if deliver(&r) {
                        reminded_soon.extend(soon.into_iter().map(|(k, _)| k));
                    }
                }
                let waited = last_reminder.is_none_or(|t| t.elapsed() >= Duration::from_secs(interval * 60));
                if let (Some(reminder), true) = (reminder, waited) {
                    if deliver(&reminder) {
                        last_reminder = Some(Instant::now());
                    }
                }
            }

            if do_sync {
                shared.lock().unwrap().syncing = true;
                repaint(&ui);
                let result = gcal::sync(&shared);
                let mut d = shared.lock().unwrap();
                d.syncing = false;
                d.status = match result {
                    Ok(msg) => msg,
                    Err(e) => format!("Sync failed: {e}"),
                };
                next_sync = Instant::now() + Duration::from_secs(interval as u64 * 60);
                repaint(&ui);
            } else if sync_requested {
                let mut d = shared.lock().unwrap();
                if !d.google_connected {
                    d.status = "Not connected to Google. Open Settings to connect.".into();
                }
                repaint(&ui);
            }
        }
    });
}
