//! Local storage. Everything lives as human-readable JSON in one folder
//! (default: Documents\Morrowkeep, override with the MORROWKEEP_DIR environment variable).

use crate::model::{CalorieEntry, ExtEvent, Item, WeightEntry};
use chrono::{DateTime, Local};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const IMPORT_FORMAT: &str = include_str!("../IMPORT_FORMAT.md");

#[derive(Clone, Debug)]
pub struct Paths {
    pub root: PathBuf,
    pub items: PathBuf,
    pub calories: PathBuf,
    pub weight: PathBuf,
    pub journal: PathBuf,
    pub foods: PathBuf,
    pub exercises: PathBuf,
    pub workouts: PathBuf,
    pub workout_log: PathBuf,
    /// Journal voice recordings (WAV).
    pub journal_audio: PathBuf,
    pub settings: PathBuf,
    pub body: PathBuf,
    pub token: PathBuf,
    pub gcache: PathBuf,
    pub sync_state: PathBuf,
    pub inbox: PathBuf,
    pub backups: PathBuf,
    pub models: PathBuf,
}

impl Paths {
    pub fn resolve() -> Paths {
        // LIFEHUB_DIR: the project's name before it became Morrowkeep.
        let root = std::env::var_os("MORROWKEEP_DIR")
            .or_else(|| std::env::var_os("LIFEHUB_DIR"))
            .map(PathBuf::from)
            .or_else(|| dirs::document_dir().map(|d| d.join("Morrowkeep")))
            .unwrap_or_else(|| PathBuf::from("Morrowkeep"));
        Paths::at(root)
    }

    /// The files in a data folder.
    pub fn at(root: PathBuf) -> Paths {
        Paths {
            items: root.join("items.json"),
            calories: root.join("calories.json"),
            weight: root.join("weight.json"),
            journal: root.join("journal.json"),
            foods: root.join("foods.json"),
            exercises: root.join("exercises.json"),
            workouts: root.join("workouts.json"),
            workout_log: root.join("workout_log.json"),
            journal_audio: root.join("journal-audio"),
            settings: root.join("settings.json"),
            body: root.join("body.json"),
            token: root.join("google_token.json"),
            gcache: root.join("google_events_cache.json"),
            sync_state: root.join("sync_state.json"),
            inbox: root.join("inbox"),
            backups: root.join("backups"),
            models: root.join("models"),
            root,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// OAuth "Desktop app" client from Google Cloud Console.
    pub google_client_id: String,
    pub google_client_secret: String,
    /// Calendar that Morrowkeep items are mirrored into (created if missing).
    pub google_calendar_name: String,
    /// Calendars whose events are shown read-only ("primary" = your main calendar).
    pub pull_calendars: Vec<String>,
    pub auto_sync: bool,
    pub sync_interval_min: u32,
    /// Whisper model file name inside the models folder.
    pub whisper_model: String,
    /// Old home of the daily calorie limit; moved to body.json (`fallback_daily_limit`).
    /// Only read, to carry it over once.
    #[serde(alias = "calorie_goal", skip_serializing)]
    pub daily_calorie_limit: u32,
    /// "en", "sv", ... or "auto". Non-English needs a multilingual model (no ".en").
    pub voice_language: String,
    pub close_to_tray: bool,
    /// Keep the window above other apps.
    pub always_on_top: bool,
    /// Order of the main tabs ("habits", "calendar", ...); empty = the default order.
    pub tab_order: Vec<String>,
    /// Phone access (Settings > Phone): the web server for the phone app, its port, and the
    /// paired phones.
    pub phone_enabled: bool,
    pub phone_port: u16,
    pub phone_devices: Vec<crate::phone::PhoneDevice>,
    /// This PC's private key for signing phone notifications (made once; never shown).
    pub push_key: String,
    /// Where reminders go: "away" (phone when the PC hasn't been used for `away_min`
    /// minutes, else Windows), "windows", "phone" or "both".
    pub notify_to: String,
    pub away_min: u32,
    /// The "PC only" checkbox in the top bar: reminders only on this PC for now.
    pub pc_only: bool,
    /// "Starting soon" reminders for tasks with a time, `upcoming_lead_min` before.
    pub remind_upcoming: bool,
    pub upcoming_lead_min: u32,
    /// UI font: "inter" (default), "nunito", "segoe" or "egui".
    pub font: String,
    /// Theme file (stem) in the themes folder, e.g. "night-sky".
    pub theme: String,
    /// Periodic Windows notifications.
    pub reminders_enabled: bool,
    pub reminder_interval_min: u32,
    /// Remind about today's unfinished items without a time ("anytime").
    pub remind_anytime: bool,
    /// Remind when no calories are logged by `calorie_reminder_after`.
    pub remind_calories: bool,
    pub calorie_reminder_after: String,
    /// Remind when no weight is logged by `weight_reminder_after`.
    pub remind_weight: bool,
    pub weight_reminder_after: String,
    /// Remind when nothing is written in the journal by `journal_reminder_after`.
    pub remind_journal: bool,
    pub journal_reminder_after: String,
    /// No reminders between these times ("HH:MM"; may cross midnight).
    pub quiet_start: String,
    pub quiet_end: String,
    /// Mode the entry bar starts in: "local" (offline parser) or "claude" (Claude Code CLI).
    /// Tab or the mode chip switches it for the current session only.
    #[serde(alias = "input_mode")]
    pub default_input_mode: String,
    /// Path to claude.exe; empty = find it automatically.
    pub claude_path: String,
    /// Model alias passed to Claude Code: haiku, sonnet or opus.
    pub claude_model: String,
    /// Task categories (item kinds): icon, color and streak style. Edited in Settings.
    pub categories: Vec<crate::categories::Category>,
    /// Your colors for calories and weight (over the theme's).
    pub colors: crate::categories::ColorOverrides,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            google_client_id: String::new(),
            google_client_secret: String::new(),
            google_calendar_name: "Morrowkeep".into(),
            pull_calendars: vec!["primary".into()],
            auto_sync: true,
            sync_interval_min: 15,
            whisper_model: "ggml-base.en.bin".into(),
            daily_calorie_limit: 0,
            voice_language: "en".into(),
            close_to_tray: true,
            always_on_top: false,
            tab_order: Vec::new(),
            phone_enabled: false,
            phone_port: 8787,
            phone_devices: Vec::new(),
            push_key: String::new(),
            notify_to: "away".into(),
            away_min: 10,
            pc_only: false,
            remind_upcoming: true,
            upcoming_lead_min: 15,
            font: "inter".into(),
            theme: "night-sky".into(),
            reminders_enabled: true,
            reminder_interval_min: 60,
            remind_anytime: true,
            remind_calories: true,
            calorie_reminder_after: "18:00".into(),
            remind_weight: true,
            weight_reminder_after: "10:00".into(),
            remind_journal: true,
            journal_reminder_after: "21:00".into(),
            quiet_start: "22:00".into(),
            quiet_end: "08:00".into(),
            default_input_mode: "local".into(),
            claude_path: String::new(),
            claude_model: "sonnet".into(),
            categories: crate::categories::defaults(),
            colors: Default::default(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncState {
    pub calendar_id: Option<String>,
    /// Google event ids of deleted items that still need deleting remotely.
    pub pending_deletes: Vec<String>,
    pub last_sync: Option<DateTime<Local>>,
}

#[derive(Serialize, Deserialize)]
struct JournalFile {
    #[serde(default)]
    version: u32,
    entries: Vec<crate::journal::JournalEntry>,
}

#[derive(Serialize, Deserialize)]
struct WeightFile {
    #[serde(default)]
    version: u32,
    entries: Vec<WeightEntry>,
}

#[derive(Serialize, Deserialize)]
struct CaloriesFile {
    #[serde(default)]
    version: u32,
    entries: Vec<CalorieEntry>,
}

#[derive(Serialize, Deserialize)]
struct ItemsFile {
    #[serde(default)]
    version: u32,
    items: Vec<Item>,
}

/// All app state. Shared between the UI thread and the background worker behind a mutex.
pub struct Data {
    pub paths: Paths,
    pub items: Vec<Item>,
    pub calories: Vec<CalorieEntry>,
    /// Sorted by date, at most one per day.
    pub weights: Vec<WeightEntry>,
    /// Food library: ingredients and meals.
    pub foods: crate::foods::Library,
    /// Exercise library (built-ins plus your own).
    pub exercises: crate::workouts::ExerciseFile,
    /// Saved workouts (starter ones plus your own).
    pub workouts: crate::workouts::WorkoutFile,
    /// Finished workout sessions.
    pub workout_log: crate::workouts::WorkoutLog,
    /// Journal entries, sorted by date and time.
    pub journal: Vec<crate::journal::JournalEntry>,
    pub settings: Settings,
    /// Profile, goal weight and fallback calorie limit (body.json).
    pub body: crate::body::Body,
    /// body.json couldn't be parsed: it isn't overwritten until the app restarts with it fixed.
    pub body_error: Option<String>,
    pub sync: SyncState,
    pub gcache: Vec<ExtEvent>,
    pub google_connected: bool,
    pub syncing: bool,
    /// Short status line shown in the top bar.
    pub status: String,
    /// Set when items.json / calories.json can't be parsed; saving is blocked until it's fixed.
    pub load_error: Option<String>,
    items_mtime: Option<SystemTime>,
    calories_mtime: Option<SystemTime>,
    weight_mtime: Option<SystemTime>,
    journal_mtime: Option<SystemTime>,
    foods_mtime: Option<SystemTime>,
    exercises_mtime: Option<SystemTime>,
    workouts_mtime: Option<SystemTime>,
    workout_log_mtime: Option<SystemTime>,
}

impl Data {
    /// Moves Documents\LifeHub (the app's previous name) to the new data folder, once.
    fn migrate_old_folder(paths: &Paths) {
        let Some(old) = dirs::document_dir().map(|d| d.join("LifeHub")) else { return };
        if !paths.root.exists() && old.join("items.json").exists() {
            let _ = fs::rename(&old, &paths.root);
        }
    }

    pub fn load() -> Data {
        let paths = Paths::resolve();
        Self::migrate_old_folder(&paths);
        for dir in [&paths.root, &paths.inbox, &paths.backups, &paths.models] {
            let _ = fs::create_dir_all(dir);
        }
        let _ = fs::create_dir_all(paths.inbox.join("imported"));
        let _ = fs::create_dir_all(paths.inbox.join("failed"));
        // Keep a copy of the import spec next to the data so AI tools / you can find it.
        let _ = fs::write(paths.root.join("IMPORT_FORMAT.md"), IMPORT_FORMAT);

        let mut settings: Settings = read_json(&paths.settings).ok().flatten().unwrap_or_default();
        crate::categories::normalize(&mut settings.categories);
        crate::categories::apply(&settings.categories, &settings.colors);
        let (body, body_error) = match read_json(&paths.body) {
            Ok(Some(b)) => (b, None),
            // first run with body.json: carry over the limit from settings.json
            Ok(None) => (crate::body::Body { fallback_daily_limit: settings.daily_calorie_limit, ..Default::default() }, None),
            Err(e) => (crate::body::Body::default(), Some(e)),
        };
        let sync = read_json(&paths.sync_state).ok().flatten().unwrap_or_default();
        let gcache = read_json(&paths.gcache).ok().flatten().unwrap_or_default();
        let mut data = Data {
            google_connected: paths.token.exists(),
            paths,
            items: Vec::new(),
            calories: Vec::new(),
            weights: Vec::new(),
            journal: Vec::new(),
            foods: Default::default(),
            exercises: Default::default(),
            workouts: Default::default(),
            workout_log: Default::default(),
            settings,
            body,
            body_error,
            sync,
            gcache,
            syncing: false,
            status: String::new(),
            load_error: None,
            items_mtime: None,
            calories_mtime: None,
            weight_mtime: None,
            journal_mtime: None,
            foods_mtime: None,
            exercises_mtime: None,
            workouts_mtime: None,
            workout_log_mtime: None,
        };
        if !data.paths.settings.exists() {
            data.save_settings();
        }
        if !data.paths.body.exists() {
            data.save_body();
        }
        data.reload_all();
        crate::backup::daily(&data.paths);
        data
    }

    /// (Re)reads items.json and calories.json from disk.
    /// Reloads a part after a backup was restored over its file.
    pub fn reload_after_restore(&mut self, part: &str) {
        if part == "body" {
            if let Ok(Some(b)) = read_json(&self.paths.body) {
                self.body = b;
            }
        }
        self.reload_all();
    }

    pub fn reload_all(&mut self) {
        self.items_mtime = mtime(&self.paths.items);
        self.calories_mtime = mtime(&self.paths.calories);
        self.weight_mtime = mtime(&self.paths.weight);
        self.journal_mtime = mtime(&self.paths.journal);
        self.foods_mtime = mtime(&self.paths.foods);
        let mut errors = Vec::new();
        self.exercises_mtime = mtime(&self.paths.exercises);
        match read_json::<crate::workouts::ExerciseFile>(&self.paths.exercises) {
            Ok(f) => {
                self.exercises = f.unwrap_or_default();
                let removed = self.exercises.removed.clone();
                if crate::workouts::merge_builtins(&mut self.exercises.exercises, &removed) && errors.is_empty() {
                    self.exercises.version = 1;
                    if write_json(&self.paths.exercises, &self.exercises).is_ok() {
                        self.exercises_mtime = mtime(&self.paths.exercises);
                    }
                }
            }
            Err(e) => errors.push(format!("exercises.json could not be read: {e}")),
        }
        self.workout_log_mtime = mtime(&self.paths.workout_log);
        match read_json::<crate::workouts::WorkoutLog>(&self.paths.workout_log) {
            Ok(f) => self.workout_log = f.unwrap_or_default(),
            Err(e) => errors.push(format!("workout_log.json could not be read: {e}")),
        }
        self.workouts_mtime = mtime(&self.paths.workouts);
        match read_json::<crate::workouts::WorkoutFile>(&self.paths.workouts) {
            Ok(f) => {
                self.workouts = f.unwrap_or_default();
                let removed = self.workouts.removed.clone();
                if crate::workouts::merge_starters(&mut self.workouts.workouts, &removed) && errors.is_empty() {
                    self.workouts.version = 1;
                    if write_json(&self.paths.workouts, &self.workouts).is_ok() {
                        self.workouts_mtime = mtime(&self.paths.workouts);
                    }
                }
            }
            Err(e) => errors.push(format!("workouts.json could not be read: {e}")),
        }
        match read_json::<crate::foods::Library>(&self.paths.foods) {
            Ok(f) => self.foods = f.unwrap_or_default(),
            Err(e) => errors.push(format!("foods.json could not be read: {e}")),
        }
        match read_json::<JournalFile>(&self.paths.journal) {
            Ok(f) => {
                self.journal = f.map(|f| f.entries).unwrap_or_default();
                self.journal.sort_by_key(crate::journal::sort_key);
            }
            Err(e) => errors.push(format!("journal.json could not be read: {e}")),
        }
        match read_json::<ItemsFile>(&self.paths.items) {
            Ok(f) => self.items = f.map(|f| f.items).unwrap_or_default(),
            Err(e) => errors.push(format!("items.json could not be read: {e}")),
        }
        match read_json::<WeightFile>(&self.paths.weight) {
            Ok(f) => {
                self.weights = f.map(|f| f.entries).unwrap_or_default();
                self.weights.sort_by_key(|w| w.date);
            }
            Err(e) => errors.push(format!("weight.json could not be read: {e}")),
        }
        match read_json::<CaloriesFile>(&self.paths.calories) {
            Ok(f) => self.calories = f.map(|f| f.entries).unwrap_or_default(),
            Err(e) => errors.push(format!("calories.json could not be read: {e}")),
        }
        self.load_error = (!errors.is_empty()).then(|| {
            format!(
                "{}. Fix the file (or restore one from the backups folder) and press Reload. Changes are not saved until then.",
                errors.join("; ")
            )
        });
    }

    /// True if items.json or calories.json was modified by something other than this app.
    pub fn files_changed_on_disk(&self) -> bool {
        let changed = |p: &Path, known: Option<SystemTime>| {
            let now = mtime(p);
            now.is_some() && now != known
        };
        changed(&self.paths.items, self.items_mtime)
            || changed(&self.paths.calories, self.calories_mtime)
            || changed(&self.paths.weight, self.weight_mtime)
            || changed(&self.paths.journal, self.journal_mtime)
            || changed(&self.paths.foods, self.foods_mtime)
            || changed(&self.paths.exercises, self.exercises_mtime)
            || changed(&self.paths.workouts, self.workouts_mtime)
            || changed(&self.paths.workout_log, self.workout_log_mtime)
    }

    pub fn save_journal(&mut self) {
        if self.load_error.is_some() {
            return;
        }
        self.journal.sort_by_key(crate::journal::sort_key);
        let file = JournalFile { version: 1, entries: self.journal.clone() };
        match write_json(&self.paths.journal, &file) {
            Ok(()) => self.journal_mtime = mtime(&self.paths.journal),
            Err(e) => self.status = format!("Saving the journal failed: {e}"),
        }
    }

    pub fn save_exercises(&mut self) {
        if self.load_error.is_some() {
            return;
        }
        self.exercises.version = 1;
        match write_json(&self.paths.exercises, &self.exercises) {
            Ok(()) => self.exercises_mtime = mtime(&self.paths.exercises),
            Err(e) => self.status = format!("Saving exercises failed: {e}"),
        }
    }

    pub fn save_workouts(&mut self) {
        if self.load_error.is_some() {
            return;
        }
        self.workouts.version = 1;
        match write_json(&self.paths.workouts, &self.workouts) {
            Ok(()) => self.workouts_mtime = mtime(&self.paths.workouts),
            Err(e) => self.status = format!("Saving workouts failed: {e}"),
        }
    }

    /// Deletes a journal entry; its recording is moved to journal-audio/deleted, not destroyed.
    pub fn delete_journal_entry(&mut self, id: &str) -> bool {
        let Some(pos) = self.journal.iter().position(|e| e.id == id) else { return false };
        let e = self.journal.remove(pos);
        if let Some(file) = e.audio {
            let deleted = self.paths.journal_audio.join("deleted");
            let _ = std::fs::create_dir_all(&deleted);
            let _ = std::fs::rename(self.paths.journal_audio.join(&file), deleted.join(&file));
        }
        self.save_journal();
        true
    }

    /// Saves a finished (or ended early) workout; a completed one ticks off its linked task
    /// for the day. Returns a status message.
    pub fn record_workout(&mut self, session: crate::workouts::Session, task: Option<&str>) -> String {
        let (name, day, completed) = (session.workout.clone(), session.date, session.completed);
        self.workout_log.sessions.push(session);
        self.save_workout_log();
        let mut msg = format!("Workout saved: {name}");
        if completed {
            if let Some(i) = task.and_then(|id| self.item_index(id)) {
                if !self.items[i].is_done_on(day) {
                    self.items[i].toggle_done(day);
                    self.save_items();
                    msg = format!("Workout saved · ✔ {}", self.items[i].title);
                }
            }
        }
        msg
    }

    pub fn save_workout_log(&mut self) {
        if self.load_error.is_some() {
            return;
        }
        self.workout_log.version = 1;
        match write_json(&self.paths.workout_log, &self.workout_log) {
            Ok(()) => self.workout_log_mtime = mtime(&self.paths.workout_log),
            Err(e) => self.status = format!("Saving the workout log failed: {e}"),
        }
    }

    pub fn save_foods(&mut self) {
        if self.load_error.is_some() {
            return;
        }
        self.foods.version = 1;
        match write_json(&self.paths.foods, &self.foods) {
            Ok(()) => self.foods_mtime = mtime(&self.paths.foods),
            Err(e) => self.status = format!("Saving foods failed: {e}"),
        }
    }

    /// A calorie entry for an amount of a library food or meal (not yet added).
    pub fn food_entry(p: &crate::foods::Portionable, amount: crate::foods::Amount, date: chrono::NaiveDate, source: &str) -> CalorieEntry {
        let n = p.nutrition(amount);
        let mut e = CalorieEntry::new(date, p.kcal(amount), format!("{} · {}", p.name, p.describe(amount)), source);
        e.food = Some(p.name.clone());
        e.grams = Some((p.grams(amount) * 10.0).round() / 10.0);
        let r = |v: Option<f32>| v.map(|g| (g * 10.0).round() / 10.0);
        (e.protein, e.carbs, e.fat) = (r(n.protein), r(n.carbs), r(n.fat));
        e
    }

    /// Logs an amount of a library food for today; returns the status message.
    pub fn log_food(&mut self, p: &crate::foods::Portionable, amount: crate::foods::Amount) -> String {
        let today = Local::now().date_naive();
        let e = Self::food_entry(p, amount, today, "library");
        let msg = format!("+{} kcal · {} · {} kcal today", e.kcal, e.description, self.calories_on(today) + e.kcal);
        self.calories.push(e);
        self.save_calories();
        self.status = msg.clone();
        msg
    }

    pub fn journal_on(&self, day: chrono::NaiveDate) -> usize {
        self.journal.iter().filter(|e| e.date == day && e.kind == "note").count()
    }

    pub fn save_weight(&mut self) {
        if self.load_error.is_some() {
            return;
        }
        self.weights.sort_by_key(|w| w.date);
        let file = WeightFile { version: 1, entries: self.weights.clone() };
        match write_json(&self.paths.weight, &file) {
            Ok(()) => self.weight_mtime = mtime(&self.paths.weight),
            Err(e) => self.status = format!("Saving weight failed: {e}"),
        }
    }

    /// Sets (or with `None`, removes) the weight for `date`.
    pub fn set_weight(&mut self, date: chrono::NaiveDate, kg: Option<f32>) {
        self.weights.retain(|w| w.date != date);
        if let Some(kg) = kg {
            self.weights.push(WeightEntry { date, kg: (kg * 10.0).round() / 10.0 });
        }
        self.save_weight();
    }

    pub fn weight_on(&self, date: chrono::NaiveDate) -> Option<f32> {
        self.weights.iter().find(|w| w.date == date).map(|w| w.kg)
    }

    /// Most recent entry strictly before `date`.
    pub fn weight_before(&self, date: chrono::NaiveDate) -> Option<&WeightEntry> {
        self.weights.iter().rev().find(|w| w.date < date)
    }

    pub fn save_calories(&mut self) {
        if self.load_error.is_some() {
            return;
        }
        self.calories.sort_by_key(|c| (c.date, c.time));
        let file = CaloriesFile { version: 1, entries: self.calories.clone() };
        match write_json(&self.paths.calories, &file) {
            Ok(()) => self.calories_mtime = mtime(&self.paths.calories),
            Err(e) => self.status = format!("Saving calories failed: {e}"),
        }
    }

    pub fn calories_on(&self, day: chrono::NaiveDate) -> u32 {
        self.calories.iter().filter(|c| c.date == day).map(|c| c.kcal).sum()
    }

    /// Daily limit × days in the month containing `day`, if a limit is set.
    pub fn month_budget(&self, day: chrono::NaiveDate) -> Option<u32> {
        let limit = self.daily_limit();
        (limit > 0).then(|| limit * days_in_month(day))
    }

    /// Logged kcal per day of the month containing `day` (index 0 = the 1st); `None` for
    /// days with no entries at all.
    fn month_days(&self, day: chrono::NaiveDate) -> Vec<Option<u32>> {
        use chrono::Datelike;
        let mut days = vec![None; days_in_month(day) as usize];
        for c in &self.calories {
            if c.date.year() == day.year() && c.date.month() == day.month() {
                let slot = &mut days[c.date.day0() as usize];
                *slot = Some(slot.unwrap_or(0) + c.kcal);
            }
        }
        days
    }

    /// How the month containing `day` counts against its budget, as of `today`. Past days
    /// with nothing logged count as the daily limit: "no food entered", not "ate nothing".
    pub fn month_tally(&self, day: chrono::NaiveDate, today: chrono::NaiveDate) -> MonthTally {
        use chrono::Datelike;
        let limit = self.daily_limit();
        let mut t = MonthTally::default();
        for (i, kcal) in self.month_days(day).into_iter().enumerate() {
            let date = day.with_day(i as u32 + 1).unwrap();
            match kcal {
                Some(k) => {
                    t.logged += k;
                    t.counted += k;
                }
                None if date < today && limit > 0 => {
                    t.unlogged_past_days += 1;
                    t.counted += limit;
                }
                None => {}
            }
        }
        t
    }

    /// Recommended calories for `day`: what's left of the month's budget after the earlier
    /// days of the month, spread evenly over the days left (including `day`). Eating too much
    /// one day lowers the following days. Earlier days with nothing logged count as exactly
    /// the daily limit. Fixed for the day; it doesn't change as you log food on `day`.
    pub fn recommended_for(&self, day: chrono::NaiveDate) -> Option<u32> {
        use chrono::Datelike;
        let budget = self.month_budget(day)? as i64;
        let before = &self.month_days(day)[..day.day0() as usize];
        let days_left = (days_in_month(day) - day.day0()) as i64;
        Some(spread_budget(budget, counted(before, self.daily_limit()), days_left))
    }

    pub fn save_items(&mut self) {
        if self.load_error.is_some() {
            return;
        }
        let file = ItemsFile { version: 1, items: self.items.clone() };
        match write_json(&self.paths.items, &file) {
            Ok(()) => self.items_mtime = mtime(&self.paths.items),
            Err(e) => self.status = format!("Saving failed: {e}"),
        }
    }

    pub fn save_settings(&mut self) {
        if let Err(e) = write_json(&self.paths.settings, &self.settings) {
            self.status = format!("Saving settings failed: {e}");
        }
    }

    /// Moves items between categories: renamed kinds to their new name, deleted kinds to
    /// "task". Saves items if any changed; returns how many did.
    pub fn move_kinds(&mut self, renamed: &std::collections::BTreeMap<String, String>, deleted: &std::collections::HashSet<String>) -> usize {
        let mut moved = 0;
        for item in &mut self.items {
            let to = renamed.get(&item.kind).cloned().or_else(|| deleted.contains(&item.kind).then(|| crate::categories::DEFAULT_KIND.to_string()));
            if let Some(to) = to {
                item.kind = to;
                item.touch();
                moved += 1;
            }
        }
        if moved > 0 {
            self.save_items();
        }
        moved
    }

    pub fn save_body(&mut self) {
        if let Some(e) = &self.body_error {
            self.status = format!("body.json could not be read ({e}). Fix it and restart; changes aren't saved until then.");
            return;
        }
        if let Err(e) = write_json(&self.paths.body, &self.body) {
            self.status = format!("Saving body.json failed: {e}");
        }
    }

    /// Weight the body calculations use: the 7-day average at the latest weigh-in.
    pub fn current_weight(&self) -> Option<f32> {
        crate::body::current_weight(&self.weights)
    }

    /// Profile numbers and today's plan, if the profile is complete and a weight is logged.
    pub fn body_plan(&self, today: chrono::NaiveDate) -> Option<(crate::body::Numbers, crate::body::Plan)> {
        let n = self.body.numbers(self.current_weight()?, today)?;
        let p = self.body.plan(&n, today);
        Some((n, p))
    }

    /// The daily calorie limit (0 = none): from the plan when the Body profile is complete,
    /// otherwise the fallback limit.
    pub fn daily_limit(&self) -> u32 {
        match self.body_plan(Local::now().date_naive()) {
            Some((_, p)) => p.daily,
            None => self.body.fallback_daily_limit,
        }
    }

    /// True when the daily limit comes from the Body plan.
    pub fn limit_from_plan(&self) -> bool {
        self.body_plan(Local::now().date_naive()).is_some()
    }

    /// kcal logged per day.
    pub fn daily_kcal(&self) -> std::collections::BTreeMap<chrono::NaiveDate, u32> {
        let mut m = std::collections::BTreeMap::new();
        for c in &self.calories {
            *m.entry(c.date).or_insert(0) += c.kcal;
        }
        m
    }

    pub fn save_sync(&mut self) {
        let _ = write_json(&self.paths.sync_state, &self.sync);
        let _ = write_json(&self.paths.gcache, &self.gcache);
    }

    pub fn item_index(&self, id: &str) -> Option<usize> {
        self.items.iter().position(|i| i.id == id)
    }

    pub fn delete_item(&mut self, id: &str) {
        if let Some(idx) = self.item_index(id) {
            let item = self.items.remove(idx);
            if let Some(gid) = item.google_id {
                self.sync.pending_deletes.push(gid);
                self.save_sync();
            }
        }
    }


}

#[derive(Default, Debug, PartialEq)]
pub struct MonthTally {
    /// kcal actually logged this month.
    pub logged: u32,
    /// Past days with no entries, counted at the daily limit.
    pub unlogged_past_days: u32,
    /// logged + unlogged_past_days × limit: what counts against the budget.
    pub counted: u32,
}

/// Sum of days' kcal, counting days with nothing logged as the daily limit.
pub fn counted(days: &[Option<u32>], limit: u32) -> i64 {
    days.iter().map(|d| d.unwrap_or(limit) as i64).sum()
}

/// What's left of a budget, split evenly over the remaining days (never below 0).
pub fn spread_budget(budget: i64, eaten_before: i64, days_left: i64) -> u32 {
    ((budget - eaten_before) / days_left.max(1)).max(0) as u32
}

pub fn days_in_month(day: chrono::NaiveDate) -> u32 {
    use chrono::Datelike;
    let first = day.with_day(1).unwrap();
    let next = if day.month() == 12 {
        chrono::NaiveDate::from_ymd_opt(day.year() + 1, 1, 1)
    } else {
        chrono::NaiveDate::from_ymd_opt(day.year(), day.month() + 1, 1)
    }
    .unwrap();
    (next - first).num_days() as u32
}

fn mtime(p: &Path) -> Option<SystemTime> {
    fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Ok(None) if the file doesn't exist.
pub fn read_json<T: DeserializeOwned>(p: &Path) -> Result<Option<T>, String> {
    match fs::read_to_string(p) {
        // Some editors save UTF-8 with a byte-order mark, which serde_json rejects.
        Ok(s) => serde_json::from_str(s.trim_start_matches('\u{feff}')).map(Some).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Writes pretty JSON via a temp file + rename so a crash never leaves a half-written file.
pub fn write_json<T: Serialize>(p: &Path, value: &T) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let tmp = p.with_extension("json.tmp");
    fs::write(&tmp, json).map_err(|e| e.to_string())?;
    fs::rename(&tmp, p).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overeating_lowers_the_following_days() {
        // 31-day month, 2000/day limit, nothing logged until the 29th (3000 kcal).
        // Unlogged days count as 2000, so on the 30th (2 days left): (62000 - 59000) / 2 = 1500.
        let budget = 2000 * 31;
        let mut before = vec![None; 28];
        before.push(Some(3000));
        assert_eq!(counted(&before, 2000), 59000);
        assert_eq!(spread_budget(budget, counted(&before, 2000), 2), 1500);
        // A logged day of 0 kcal is still 0, not the limit.
        assert_eq!(counted(&[Some(0), None], 2000), 2000);
        // Way over budget never goes negative.
        assert_eq!(spread_budget(budget, 70000, 2), 0);
        let d = chrono::NaiveDate::from_ymd_opt(2026, 10, 10).unwrap();
        assert_eq!(days_in_month(d), 31);
        assert_eq!(days_in_month(chrono::NaiveDate::from_ymd_opt(2028, 2, 3).unwrap()), 29);
        assert_eq!(days_in_month(chrono::NaiveDate::from_ymd_opt(2026, 12, 31).unwrap()), 31);
    }
}
