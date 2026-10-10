//! System tray icon, single-instance handling and the "window closed, still running" state.
//!
//! When the window is closed, eframe's window and GPU context are torn down completely and
//! the main thread just pumps Win32 messages for the tray (see `wait_for_open`). That keeps
//! Morrowkeep at ~0% CPU and a small memory footprint while it lives in the tray.

use crate::worker::Job;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateMutexW, GetCurrentThreadId, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE,
    INFINITE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, IsIconic, PostThreadMessageW, SetForegroundWindow, ShowWindow, TranslateMessage,
    MSG, SW_RESTORE, SW_SHOW, WM_APP,
};

pub const WINDOW_TITLE: &str = "Morrowkeep";
const WM_OPEN: u32 = WM_APP + 1;
const SHOW_EVENT: &str = r"Local\Morrowkeep-show";

/// HWND of the main window while it exists, 0 while Morrowkeep is only in the tray.
pub static HWND: AtomicIsize = AtomicIsize::new(0);
static MAIN_THREAD: AtomicU32 = AtomicU32::new(0);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Returns false if Morrowkeep is already running; that instance is asked to open its window.
/// Must be called on the main thread.
pub fn single_instance() -> bool {
    unsafe {
        CreateMutexW(std::ptr::null(), 0, wide(r"Local\Morrowkeep-single-instance").as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let ev = OpenEventW(EVENT_MODIFY_STATE, 0, wide(SHOW_EVENT).as_ptr());
            if !ev.is_null() {
                SetEvent(ev);
            }
            return false;
        }
        MAIN_THREAD.store(GetCurrentThreadId(), Ordering::Relaxed);
        // Another launch signals this event; open the window when it does.
        let ev = CreateEventW(std::ptr::null(), 0, 0, wide(SHOW_EVENT).as_ptr()) as isize;
        std::thread::spawn(move || loop {
            WaitForSingleObject(ev as *mut std::ffi::c_void, INFINITE);
            request_open();
        });
    }
    true
}

/// Brings the window to the front, or asks the main loop to create it.
pub fn request_open() {
    let h = HWND.load(Ordering::Relaxed);
    unsafe {
        if h != 0 {
            let hwnd = h as *mut std::ffi::c_void;
            ShowWindow(hwnd, SW_SHOW);
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            SetForegroundWindow(hwnd);
        } else {
            PostThreadMessageW(MAIN_THREAD.load(Ordering::Relaxed), WM_OPEN, 0, 0);
        }
    }
}

/// Hands memory the closed window no longer uses (GPU driver buffers etc.) back to Windows.
pub fn trim_memory() {
    unsafe {
        windows_sys::Win32::System::ProcessStatus::K32EmptyWorkingSet(
            windows_sys::Win32::System::Threading::GetCurrentProcess(),
        );
    }
}

/// Runs the tray's message loop (no window open) until something asks to open the window.
pub fn wait_for_open() {
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if msg.hwnd.is_null() && msg.message == WM_OPEN {
                return;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    std::process::exit(0); // WM_QUIT
}

/// A green rounded square with a white check mark.
pub fn icon_rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    let mut px = Vec::with_capacity((size * size * 4) as usize);
    let seg_dist = |p: (f32, f32), a: (f32, f32), b: (f32, f32)| {
        let (abx, aby) = (b.0 - a.0, b.1 - a.1);
        let t = (((p.0 - a.0) * abx + (p.1 - a.1) * aby) / (abx * abx + aby * aby)).clamp(0.0, 1.0);
        let (cx, cy) = (a.0 + t * abx, a.1 + t * aby);
        ((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt()
    };
    let r = s * 0.22;
    for y in 0..size {
        for x in 0..size {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            // rounded-rect coverage
            let dx = (r - p.0).max(p.0 - (s - r)).max(0.0);
            let dy = (r - p.1).max(p.1 - (s - r)).max(0.0);
            let inside = ((dx * dx + dy * dy).sqrt() - r).clamp(-0.5, 0.5) * -1.0 + 0.5;
            let check = seg_dist(p, (s * 0.27, s * 0.53), (s * 0.43, s * 0.69))
                .min(seg_dist(p, (s * 0.43, s * 0.69), (s * 0.74, s * 0.33)));
            let w = (s * 0.07 - check + 0.5).clamp(0.0, 1.0);
            let (g_r, g_g, g_b) = (46.0, 160.0, 103.0);
            let c = |g: f32| (g + (255.0 - g) * w) as u8;
            px.extend_from_slice(&[c(g_r), c(g_g), c(g_b), (inside * 255.0) as u8]);
        }
    }
    px
}

pub fn create(jobs: Sender<Job>) -> Option<TrayIcon> {
    let open = MenuItem::new("Open Morrowkeep", true, None);
    let sync = MenuItem::new("Sync with Google now", true, None);
    let quit = MenuItem::new("Quit", true, None);
    let menu = Menu::new();
    menu.append_items(&[&open, &sync, &PredefinedMenuItem::separator(), &quit]).ok()?;
    let (open_id, sync_id, quit_id) = (open.id().clone(), sync.id().clone(), quit.id().clone());

    let icon = tray_icon::Icon::from_rgba(icon_rgba(32), 32, 32).ok()?;
    let tray = TrayIconBuilder::new()
        .with_icon(icon)
        .with_tooltip("Morrowkeep")
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()
        .ok()?;

    TrayIconEvent::set_event_handler(Some(|e: TrayIconEvent| {
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
            request_open();
        }
    }));
    let jobs = std::sync::Mutex::new(jobs);
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        if e.id == open_id {
            request_open();
        } else if e.id == sync_id {
            let _ = jobs.lock().unwrap().send(Job::SyncNow);
        } else if e.id == quit_id {
            // Everything is saved on each change, so exiting directly is safe.
            std::process::exit(0);
        }
    }));
    Some(tray)
}
