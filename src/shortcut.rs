//! Start menu shortcut + app identity.
//!
//! Windows only shows toast notifications of a non-Store app in the notification panel when
//! a Start menu shortcut carries the same AppUserModelID the toasts are sent with. So at
//! startup Morrowkeep (re)writes its shortcut with that ID and tags its own process with it.

use crate::notify::APP_ID;
use std::path::PathBuf;
use windows::core::{Interface, HSTRING, PWSTR};
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemAlloc, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{IShellLinkW, SetCurrentProcessExplicitAppUserModelID, ShellLink};

const ICON_ICO: &[u8] = include_bytes!("../assets/morrowkeep.ico");

/// Removes what the app left behind under its previous name (LifeHub): the old Start menu
/// shortcut, notification identity and "Start with Windows" entry, which moves to the new
/// name. Cheap and idempotent; run at startup.
pub fn migrate_from_old_name() {
    use std::os::windows::process::CommandExt;
    let reg = |args: &[&str]| {
        std::process::Command::new("reg")
            .args(args)
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    if let Some(old) = dirs::data_dir().map(|d| d.join(r"Microsoft\Windows\Start Menu\Programs\LifeHub.lnk")) {
        let _ = std::fs::remove_file(old);
    }
    reg(&["delete", r"HKCU\Software\Classes\AppUserModelId\LifeHub.Planner", "/f"]);
    let run = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    if reg(&["query", run, "/v", "LifeHub"]) {
        reg(&["delete", run, "/v", "LifeHub", "/f"]);
        if let Ok(exe) = std::env::current_exe() {
            let value = format!("\"{}\" --hidden", exe.display());
            reg(&["add", run, "/v", "Morrowkeep", "/t", "REG_SZ", "/d", &value, "/f"]);
        }
    }
}

/// Tags this process with the app ID, so its window groups with the shortcut on the taskbar.
pub fn set_process_app_id() {
    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(APP_ID));
    }
}

pub fn shortcut_path() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join(r"Microsoft\Windows\Start Menu\Programs\Morrowkeep.lnk"))
}

/// Creates or updates the Start menu shortcut to point at this exe, with the app ID set.
/// `icon_dir`: where to keep the shortcut's .ico (the data folder).
pub fn ensure_start_menu_shortcut(icon_dir: &std::path::Path) -> Result<(), String> {
    let lnk = shortcut_path().ok_or("no Start menu folder")?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let icon = icon_dir.join("morrowkeep.ico");
    if !icon.exists() {
        std::fs::write(&icon, ICON_ICO).map_err(|e| e.to_string())?;
    }
    unsafe {
        // S_FALSE (already initialized) is fine
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
        link.SetPath(&HSTRING::from(exe.as_os_str())).map_err(|e| e.to_string())?;
        if let Some(dir) = exe.parent() {
            link.SetWorkingDirectory(&HSTRING::from(dir.as_os_str())).map_err(|e| e.to_string())?;
        }
        link.SetDescription(&HSTRING::from("Morrowkeep - tasks, chores, meals and workouts")).map_err(|e| e.to_string())?;
        link.SetIconLocation(&HSTRING::from(icon.as_os_str()), 0).map_err(|e| e.to_string())?;

        let store: IPropertyStore = link.cast().map_err(|e| e.to_string())?;
        // The string must come from the COM allocator: the property system may free it with
        // CoTaskMemFree, and a Rust-owned buffer freed that way corrupts the heap.
        let id: Vec<u16> = APP_ID.encode_utf16().chain(std::iter::once(0)).collect();
        let buf = CoTaskMemAlloc(id.len() * 2) as *mut u16;
        if buf.is_null() {
            return Err("out of memory".into());
        }
        std::ptr::copy_nonoverlapping(id.as_ptr(), buf, id.len());
        let value = PROPVARIANT {
            Anonymous: PROPVARIANT_0 {
                Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                    vt: VT_LPWSTR,
                    Anonymous: PROPVARIANT_0_0_0 { pwszVal: PWSTR(buf) },
                    ..Default::default()
                }),
            },
        };
        store.SetValue(&PKEY_AppUserModel_ID, &value).map_err(|e| e.to_string())?;
        store.Commit().map_err(|e| e.to_string())?;

        let file: IPersistFile = link.cast().map_err(|e| e.to_string())?;
        file.Save(&HSTRING::from(lnk.as_os_str()), true).map_err(|e| e.to_string())?;
    }
    Ok(())
}
