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
        loop {
            let mut sync_requested = false;
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
                let (reminder, interval, paths) = {
                    let d = shared.lock().unwrap();
                    let reminder = if notify::may_remind(&d.settings) { notify::pending_reminder(&d) } else { None };
                    (reminder, d.settings.reminder_interval_min.max(5) as u64, d.paths.clone())
                };
                let waited = last_reminder.is_none_or(|t| t.elapsed() >= Duration::from_secs(interval * 60));
                if let (Some(reminder), true) = (reminder, waited) {
                    if notify::show(&paths, &reminder).is_ok() {
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
