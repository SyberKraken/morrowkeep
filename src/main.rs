// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod body;
mod categories;
mod claude;
mod gcal;
mod journal;
mod import;
mod model;
mod notify;
mod parse;
mod shortcut;
mod store;
mod tray;
mod ui;
#[cfg(feature = "voice")]
mod voice;
mod worker;

use eframe::egui;
use std::sync::{mpsc, Arc, Mutex};

fn main() {
    if !tray::single_instance() {
        return; // the running instance opens its window
    }
    // `--hidden` starts in the tray (used by "Start with Windows").
    let hidden = std::env::args().any(|a| a == "--hidden");

    // Data, background worker and tray outlive the window, which is created on demand.
    shortcut::set_process_app_id();
    let shared = Arc::new(Mutex::new(store::Data::load()));
    {
        // Themes live as editable JSON in <data>\themes; activate the chosen one.
        let mut d = shared.lock().unwrap();
        ui::theme::install_builtins(&d.paths.root);
        let (root, name) = (d.paths.root.clone(), d.settings.theme.clone());
        if let Err(e) = ui::theme::activate(&root, &name) {
            d.status = format!("Theme not loaded, using the default: {e}");
        }
    }
    {
        let paths = shared.lock().unwrap().paths.clone();
        notify::register(&paths);
        // Keep the Start menu shortcut pointing at this build, carrying the app ID that
        // notifications need to appear in the Windows notification panel. Release builds only,
        // so a debug run doesn't repoint it.
        if !cfg!(debug_assertions) {
            std::thread::spawn(move || {
                shortcut::migrate_from_old_name();
                if let Err(e) = shortcut::ensure_start_menu_shortcut(&paths.root) {
                    eprintln!("Start menu shortcut: {e}");
                }
            });
        }
    }
    let ui_handle: worker::UiHandle = Arc::default();
    notify::init(shared.clone(), ui_handle.clone());
    let (jobs, rx) = mpsc::channel();
    worker::spawn(shared.clone(), ui_handle.clone(), rx);
    let tray = tray::create(jobs.clone());

    let mut open_window = !hidden || tray.is_none();
    loop {
        if open_window {
            let icon = egui::IconData { rgba: tray::icon_rgba(64), width: 64, height: 64 };
            let options = eframe::NativeOptions {
                viewport: egui::ViewportBuilder::default()
                    .with_title(tray::WINDOW_TITLE)
                    .with_inner_size([1120.0, 760.0])
                    .with_min_inner_size([680.0, 440.0])
                    .with_icon(Arc::new(icon)),
                ..Default::default()
            };
            let (s, j, u) = (shared.clone(), jobs.clone(), ui_handle.clone());
            let result = eframe::run_native(
                tray::WINDOW_TITLE,
                options,
                Box::new(move |cc| Ok(Box::new(ui::App::new(cc, s, j, u)))),
            );
            tray::HWND.store(0, std::sync::atomic::Ordering::Relaxed);
            *ui_handle.lock().unwrap() = None;
            tray::trim_memory();
            let keep_running = tray.is_some() && shared.lock().unwrap().settings.close_to_tray;
            if result.is_err() || !keep_running {
                return;
            }
        }
        tray::wait_for_open();
        open_window = true;
    }
}
