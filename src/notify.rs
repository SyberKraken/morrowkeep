//! Windows toast reminders: unfinished "anytime" tasks for today, no calories logged by the
//! evening, no weight logged by the morning, nothing written in the journal by the evening.
//! Checked by the background worker, so they also fire while Morrowkeep is only in the tray.
//!
//! When calories, weight or a journal entry are missing, the toast has input boxes and a
//! "Log" button, so they can be entered right from the notification.

use crate::model::{occurrences, CalorieEntry};
use crate::store::{Data, Paths, Settings};
use crate::tray;
use crate::worker::UiHandle;
use chrono::{Local, NaiveTime};
use std::os::windows::process::CommandExt;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use windows::core::{IInspectable, Interface, HSTRING};
use windows::Data::Xml::Dom::XmlDocument;
use windows::Foundation::{IPropertyValue, TypedEventHandler};
use windows::UI::Notifications::{ToastActivatedEventArgs, ToastNotification, ToastNotificationManager};

/// Identity the toasts are shown under (registered in HKCU so they say "Morrowkeep").
pub const APP_ID: &str = "Morrowkeep.Planner";
const ICON_PNG: &[u8] = include_bytes!("../assets/morrowkeep.png");

/// Unix time until which reminders are snoozed.
static SNOOZE_UNTIL: AtomicI64 = AtomicI64::new(0);
/// App state, so values typed into a toast can be saved (set once at startup).
static APP: OnceLock<(Arc<Mutex<Data>>, UiHandle)> = OnceLock::new();
/// A food description typed into a toast, waiting for the window to send it to Claude.
static PENDING_FOOD: Mutex<Option<String>> = Mutex::new(None);
/// The toasts currently shown; kept alive so their click handlers stay registered.
static LIVE: Mutex<Vec<ToastNotification>> = Mutex::new(Vec::new());

pub fn init(shared: Arc<Mutex<Data>>, ui: UiHandle) {
    let _ = APP.set((shared, ui));
}

/// A food description from a toast that the window should estimate with Claude.
pub fn take_pending_food() -> Option<String> {
    PENDING_FOOD.lock().unwrap().take()
}

/// Registers the toast identity (name + icon) for this user. Cheap; run at startup.
pub fn register(paths: &Paths) {
    let icon = paths.root.join("morrowkeep.png");
    if !icon.exists() {
        let _ = std::fs::write(&icon, ICON_PNG);
    }
    let key = format!(r"HKCU\Software\Classes\AppUserModelId\{APP_ID}");
    let icon_path = icon.display().to_string();
    for (name, value) in [("DisplayName", "Morrowkeep"), ("IconUri", icon_path.as_str())] {
        let _ = std::process::Command::new("reg")
            .args(["add", &key, "/v", name, "/t", "REG_SZ", "/d", value, "/f"])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

fn parse_hm(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s.trim(), "%H:%M").ok()
}

/// True inside quiet hours (handles ranges that cross midnight, like 22:00–08:00).
fn in_quiet_hours(settings: &Settings, now: NaiveTime) -> bool {
    match (parse_hm(&settings.quiet_start), parse_hm(&settings.quiet_end)) {
        (Some(s), Some(e)) if s != e => {
            if s < e {
                now >= s && now < e
            } else {
                now >= s || now < e
            }
        }
        _ => false,
    }
}

/// Snoozes all reminders (Windows and phone) for a while.
pub fn snooze(hours: f64) {
    SNOOZE_UNTIL.store(Local::now().timestamp() + (hours * 3600.0) as i64, Ordering::Relaxed);
}

/// Seconds since the last keyboard or mouse input on this PC.
pub fn pc_idle_secs() -> u64 {
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    if unsafe { GetLastInputInfo(&mut info) } == 0 {
        return 0;
    }
    (unsafe { GetTickCount() }.wrapping_sub(info.dwTime) / 1000) as u64
}

/// Where a reminder should go.
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Route {
    pub windows: bool,
    pub phone: bool,
}

/// Routing by the settings: "PC only" wins; "away" sends to the phone when the PC hasn't been
/// used for `away_min` minutes. Without a phone that has notifications on, it's Windows.
pub fn route(settings: &Settings, phone_ready: bool, idle_secs: u64) -> Route {
    let windows_only = Route { windows: true, phone: false };
    if settings.pc_only || !phone_ready {
        return windows_only;
    }
    match settings.notify_to.as_str() {
        "windows" => windows_only,
        "phone" => Route { windows: false, phone: true },
        "both" => Route { windows: true, phone: true },
        _ => {
            let away = idle_secs >= settings.away_min.max(1) as u64 * 60;
            Route { windows: !away, phone: away }
        }
    }
}

/// The notification for the phone: same words, and which screen a tap opens.
pub fn phone_payload(r: &Reminder) -> serde_json::Value {
    let go = if r.ask_kcal {
        "food"
    } else if r.ask_journal {
        "journal"
    } else {
        "today"
    };
    serde_json::json!({"title": "Morrowkeep", "body": r.lines.join("\n"), "tag": "reminder", "go": go, "actions": true})
}

/// "Starting soon" reminders: today's timed tasks that start within `lead` minutes and
/// aren't done. Returns (key, line) pairs; the key is used to remind only once.
pub fn upcoming(data: &Data, now: chrono::DateTime<Local>) -> Vec<(String, String)> {
    let s = &data.settings;
    if !s.remind_upcoming {
        return Vec::new();
    }
    let today = now.date_naive();
    let lead = chrono::Duration::minutes(s.upcoming_lead_min.max(1) as i64);
    let mut out = Vec::new();
    for (_, i) in occurrences(&data.items, today, today) {
        let it = &data.items[i];
        let Some(t) = it.time else { continue };
        let start = today.and_time(t);
        let until = start - now.naive_local();
        if it.is_done_on(today) || until <= chrono::Duration::zero() || until > lead {
            continue;
        }
        let mins = (until.num_seconds() + 59) / 60;
        out.push((format!("{}@{today}", it.id), format!("{} at {} · in {mins} min", it.title, t.format("%H:%M"))));
    }
    out
}

/// What a reminder toast says and which input boxes it offers.
#[derive(Debug, Default)]
pub struct Reminder {
    pub lines: Vec<String>,
    pub ask_kcal: bool,
    pub ask_weight: bool,
    pub ask_journal: bool,
}

/// What to remind about right now, or None if there's nothing.
pub fn pending_reminder(data: &Data) -> Option<Reminder> {
    let s = &data.settings;
    let now = Local::now();
    let today = now.date_naive();
    let mut r = Reminder::default();
    if s.remind_anytime {
        let left: Vec<&str> = occurrences(&data.items, today, today)
            .into_iter()
            .map(|(_, i)| &data.items[i])
            .filter(|i| i.time.is_none() && !i.is_done_on(today))
            .map(|i| i.title.as_str())
            .collect();
        if !left.is_empty() {
            let mut names = left.iter().take(4).copied().collect::<Vec<_>>().join(", ");
            if left.len() > 4 {
                names += &format!(" +{} more", left.len() - 4);
            }
            let what = if left.len() == 1 { "thing" } else { "things" };
            r.lines.push(format!("{} {what} left today: {names}", left.len()));
        }
    }
    let after = |t: &str, default: u32| now.time() >= parse_hm(t).unwrap_or(NaiveTime::from_hms_opt(default, 0, 0).unwrap());
    r.ask_kcal = s.remind_calories && after(&s.calorie_reminder_after, 18) && data.calories_on(today) == 0;
    r.ask_weight = s.remind_weight && after(&s.weight_reminder_after, 10) && data.weight_on(today).is_none();
    r.ask_journal = s.remind_journal && after(&s.journal_reminder_after, 21) && data.journal_on(today) == 0;
    let missing: Vec<&str> = [(r.ask_kcal, "calories"), (r.ask_weight, "weight"), (r.ask_journal, "journal")]
        .into_iter()
        .filter_map(|(ask, what)| ask.then_some(what))
        .collect();
    match missing.as_slice() {
        [] => {}
        ["calories"] => r.lines.push("No calories logged today. What did you eat?".into()),
        ["weight"] => r.lines.push("No weight logged today. Enter it below.".into()),
        ["journal"] => r.lines.push("Nothing in your journal today. How was your day?".into()),
        many => r.lines.push(format!("Not logged yet today: {}. Enter them below.", many.join(", "))),
    }
    (!r.lines.is_empty()).then_some(r)
}

/// Whether reminders may be shown now (enabled, not snoozed, outside quiet hours).
pub fn may_remind(settings: &Settings) -> bool {
    settings.reminders_enabled
        && Local::now().timestamp() >= SNOOZE_UNTIL.load(Ordering::Relaxed)
        && !in_quiet_hours(settings, Local::now().time())
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn toast_xml(paths: &Paths, r: &Reminder) -> String {
    let icon = paths.root.join("morrowkeep.png");
    let mut xml = String::from(r#"<toast duration="long" launch="open"><visual><binding template="ToastGeneric"><text>Morrowkeep</text>"#);
    // ToastGeneric shows at most 3 text lines (title + 2)
    for l in r.lines.iter().take(2) {
        xml += &format!("<text>{}</text>", esc(l));
    }
    if icon.exists() {
        xml += &format!(r#"<image placement="appLogoOverride" hint-crop="none" src="file:///{}"/>"#, esc(&icon.display().to_string()));
    }
    xml += "</binding></visual><actions>";
    if r.ask_kcal {
        xml += r#"<input id="kcal" type="text" placeHolderContent="Calories (a number), or describe what you ate"/>"#;
    }
    if r.ask_weight {
        xml += r#"<input id="weight" type="text" placeHolderContent="Weight in kg, e.g. 82.4"/>"#;
    }
    if r.ask_journal {
        xml += r#"<input id="journal" type="text" placeHolderContent="A line about today for your journal (#tags work)"/>"#;
    }
    if r.ask_kcal || r.ask_weight || r.ask_journal {
        xml += r#"<action content="Log" arguments="log"/>"#;
    }
    xml += r#"<action content="Open Morrowkeep" arguments="open"/><action content="Snooze 2h" arguments="snooze"/></actions></toast>"#;
    xml
}

fn show_xml(xml: &str, keep: bool) -> windows::core::Result<()> {
    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(xml))?;
    let toast = ToastNotification::CreateToastNotification(&doc)?;
    toast.Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(|_, insp| {
        let args = insp.as_ref().and_then(|i| i.cast::<ToastActivatedEventArgs>().ok());
        let action = args.as_ref().and_then(|a| a.Arguments().ok()).map(|h| h.to_string()).unwrap_or_default();
        let input = |id: &str| -> String {
            args.as_ref()
                .and_then(|a| a.UserInput().ok())
                .and_then(|set| set.Lookup(&HSTRING::from(id)).ok())
                .and_then(|v| v.cast::<IPropertyValue>().ok())
                .and_then(|v| v.GetString().ok())
                .map(|h| h.to_string().trim().to_string())
                .unwrap_or_default()
        };
        on_action(&action, &input("kcal"), &input("weight"), &input("journal"));
        Ok(())
    }))?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))?.Show(&toast)?;
    if keep {
        let mut live = LIVE.lock().unwrap();
        live.push(toast);
        // older toasts have expired from the screen long ago; keep the list short
        let excess = live.len().saturating_sub(10);
        live.drain(..excess);
    }
    Ok(())
}

pub fn show(paths: &Paths, r: &Reminder) -> Result<(), String> {
    show_xml(&toast_xml(paths, r), true).map_err(|e| format!("Couldn't show notification: {e}"))
}

/// A short "saved" confirmation toast without buttons.
fn confirm(text: &str) {
    let xml = format!(
        r#"<toast><visual><binding template="ToastGeneric"><text>Morrowkeep</text><text>{}</text></binding></visual></toast>"#,
        esc(text)
    );
    let _ = show_xml(&xml, false);
}

/// Handles a click on a toast (body or button) including values typed into it.
fn on_action(action: &str, kcal: &str, weight: &str, journal: &str) {
    match action {
        "snooze" => {
            SNOOZE_UNTIL.store(Local::now().timestamp() + 2 * 3600, Ordering::Relaxed);
            return;
        }
        "log" => {}
        // "Open Morrowkeep" or a click on the toast itself
        _ => {
            tray::request_open();
            return;
        }
    }
    let Some((shared, ui)) = APP.get() else { return };
    let today = Local::now().date_naive();
    let mut done = Vec::new();
    let mut problems = Vec::new();
    let mut open_for_food = false;
    {
        let mut d = shared.lock().unwrap();
        if !weight.is_empty() {
            match weight.replace(',', ".").trim_end_matches("kg").trim().parse::<f32>() {
                Ok(kg) if (20.0..=400.0).contains(&kg) => {
                    d.set_weight(today, Some(kg));
                    done.push(format!("{kg:.1} kg"));
                }
                _ => problems.push(format!("couldn't read weight \"{weight}\"")),
            }
        }
        if !journal.is_empty() {
            d.journal.push(crate::journal::JournalEntry::new(journal));
            d.save_journal();
            done.push("journal entry".to_string());
        }
        if !kcal.is_empty() {
            match kcal.trim_end_matches("kcal").trim().parse::<u32>() {
                Ok(n) if (1..=20000).contains(&n) => {
                    d.calories.push(CalorieEntry::new(today, n, "", "manual"));
                    d.save_calories();
                    done.push(format!("{n} kcal ({} today)", d.calories_on(today)));
                }
                // Not a number: a food description for Claude to estimate in the app.
                _ => {
                    *PENDING_FOOD.lock().unwrap() = Some(kcal.to_string());
                    open_for_food = true;
                }
            }
        }
        if !done.is_empty() {
            d.status = format!("Logged from notification: {}", done.join(", "));
        }
    }
    if let Some(ctx) = ui.lock().unwrap().as_ref() {
        ctx.request_repaint();
    }
    if open_for_food {
        tray::request_open();
    }
    let mut msg = String::new();
    if !done.is_empty() {
        msg = format!("Logged {}", done.join(" · "));
    }
    if open_for_food {
        msg += if msg.is_empty() { "Opening Morrowkeep to estimate your food with Claude" } else { ". Opening Morrowkeep to estimate your food." };
    }
    if !problems.is_empty() {
        if !msg.is_empty() {
            msg += ". ";
        }
        msg += &format!("Not saved: {}", problems.join(", "));
    }
    if !msg.is_empty() {
        confirm(&msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_hours_across_midnight() {
        let s = Settings::default(); // 22:00 - 08:00
        let t = |h, m| NaiveTime::from_hms_opt(h, m, 0).unwrap();
        assert!(in_quiet_hours(&s, t(23, 30)));
        assert!(in_quiet_hours(&s, t(6, 0)));
        assert!(!in_quiet_hours(&s, t(8, 0)));
        assert!(!in_quiet_hours(&s, t(19, 0)));
        let day = Settings { quiet_start: "12:00".into(), quiet_end: "13:00".into(), ..Settings::default() };
        assert!(in_quiet_hours(&day, t(12, 30)));
        assert!(!in_quiet_hours(&day, t(13, 30)));
    }

    #[test]
    fn toast_has_inputs_only_when_asked() {
        let paths = Paths::resolve();
        let plain = toast_xml(&paths, &Reminder { lines: vec!["2 things left".into()], ..Default::default() });
        assert!(!plain.contains("<input"));
        let both = toast_xml(&paths, &Reminder { lines: vec!["a & b".into()], ask_kcal: true, ask_weight: true, ask_journal: false });
        assert!(!both.contains(r#"id="journal""#));
        let j = toast_xml(&paths, &Reminder { lines: vec!["j".into()], ask_journal: true, ..Default::default() });
        assert!(j.contains(r#"id="journal""#) && j.contains(r#"arguments="log""#) && !j.contains(r#"id="kcal""#));
        assert!(both.contains(r#"id="kcal""#) && both.contains(r#"id="weight""#) && both.contains(r#"arguments="log""#));
        assert!(both.contains("a &amp; b"));
    }

    #[test]
    fn reminder_routing() {
        let s = Settings::default(); // "away", 10 min
        let both = |w, p| Route { windows: w, phone: p };
        assert_eq!(route(&s, true, 60), both(true, false)); // at the PC
        assert_eq!(route(&s, true, 11 * 60), both(false, true)); // away
        assert_eq!(route(&s, false, 11 * 60), both(true, false)); // no phone with notifications
        let pc_only = Settings { pc_only: true, ..Settings::default() };
        assert_eq!(route(&pc_only, true, 3600), both(true, false)); // the top-bar checkbox wins
        let all = Settings { notify_to: "both".into(), ..Settings::default() };
        assert_eq!(route(&all, true, 0), both(true, true));
        let phone = Settings { notify_to: "phone".into(), ..Settings::default() };
        assert_eq!(route(&phone, true, 0), both(false, true));
        let r = Reminder { lines: vec!["No calories logged today.".into()], ask_kcal: true, ..Default::default() };
        assert_eq!(phone_payload(&r)["go"], "food");
    }
}
