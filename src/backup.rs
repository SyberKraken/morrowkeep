//! Backups of each data file, and restoring them. Daily copies go to `backups/<part>-YYYY-MM-DD.json`
//! (two weeks of each); snapshots (made by hand, or automatically before a restore) go to
//! `backups/snapshots/<part>-YYYYMMDD-HHMMSS-<why>.json`. Restoring copies a backup over the
//! live file after snapshotting the live one, so a restore can always be undone.

use crate::store::Paths;
use chrono::{Local, NaiveDate, NaiveDateTime};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

/// The parts that can be backed up and restored: (file prefix, what it holds).
pub const PARTS: &[(&str, &str)] = &[
    ("items", "Tasks and habits"),
    ("calories", "Calorie log"),
    ("weight", "Weight log"),
    ("body", "Body profile and goal"),
    ("foods", "Food library"),
    ("journal", "Journal"),
    ("exercises", "Exercise library"),
    ("workouts", "Workouts"),
    ("workout_log", "Workout history"),
];

const DAILY_KEEP: usize = 14;
const SNAPSHOT_KEEP: usize = 20;

pub fn live_path(paths: &Paths, part: &str) -> PathBuf {
    match part {
        "items" => paths.items.clone(),
        "calories" => paths.calories.clone(),
        "weight" => paths.weight.clone(),
        "body" => paths.body.clone(),
        "foods" => paths.foods.clone(),
        "journal" => paths.journal.clone(),
        "exercises" => paths.exercises.clone(),
        "workouts" => paths.workouts.clone(),
        _ => paths.workout_log.clone(),
    }
}

fn snapshots_dir(paths: &Paths) -> PathBuf {
    paths.backups.join("snapshots")
}

/// Today's copy of each part, once a day; keeps the last two weeks.
pub fn daily(paths: &Paths) {
    let today = Local::now().format("%Y-%m-%d").to_string();
    for (part, _) in PARTS {
        let file = live_path(paths, part);
        if !file.exists() {
            continue;
        }
        let target = paths.backups.join(format!("{part}-{today}.json"));
        if !target.exists() {
            let _ = fs::copy(&file, &target);
        }
        let mut dated: Vec<PathBuf> = files_in(&paths.backups).into_iter().filter(|p| daily_date(p, part).is_some()).collect();
        dated.sort();
        for f in dated.iter().take(dated.len().saturating_sub(DAILY_KEEP)) {
            let _ = fs::remove_file(f);
        }
    }
}

/// Copies a part's live file into the snapshots folder; `why` ends up in the name.
pub fn snapshot(paths: &Paths, part: &str, why: &str) -> Result<PathBuf, String> {
    let file = live_path(paths, part);
    if !file.exists() {
        return Err("there's nothing saved yet".into());
    }
    let dir = snapshots_dir(paths);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let target = dir.join(format!("{part}-{}-{why}.json", Local::now().format("%Y%m%d-%H%M%S")));
    fs::copy(&file, &target).map_err(|e| e.to_string())?;
    let mut snaps: Vec<PathBuf> = files_in(&dir).into_iter().filter(|p| snapshot_time(p, part).is_some()).collect();
    snaps.sort();
    for f in snaps.iter().take(snaps.len().saturating_sub(SNAPSHOT_KEEP)) {
        let _ = fs::remove_file(f);
    }
    Ok(target)
}

/// Puts a backup back: the current file is snapshotted first ("before-restore"), then the
/// backup is checked to be readable JSON of the right kind and copied over it. The caller
/// reloads the data afterwards.
pub fn restore(paths: &Paths, part: &str, backup: &Path) -> Result<(), String> {
    let text = fs::read_to_string(backup).map_err(|e| format!("couldn't read the backup: {e}"))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("the backup isn't valid JSON: {e}"))?;
    if summary(part, &v).is_none() {
        return Err("that file isn't a backup of this part".into());
    }
    if live_path(paths, part).exists() {
        snapshot(paths, part, "before-restore")?;
    }
    fs::copy(backup, live_path(paths, part)).map(|_| ()).map_err(|e| e.to_string())
}

/// A backup file of one part.
#[derive(Clone, Debug)]
pub struct Backup {
    pub path: PathBuf,
    /// When it was taken (daily copies: that day).
    pub when: NaiveDateTime,
    /// "daily", "manual", "before-restore", …
    pub why: String,
    /// What's in it: "14 ingredients, 1 meal".
    pub summary: String,
}

impl Backup {
    pub fn label(&self) -> String {
        let when = if self.why == "daily" { self.when.format("%a %b %-d").to_string() } else { self.when.format("%a %b %-d, %H:%M").to_string() };
        let why = match self.why.as_str() {
            "daily" => String::new(),
            "before-restore" => " · before a restore".into(),
            "manual" => " · backed up by hand".into(),
            w => format!(" · {}", w.replace('-', " ")),
        };
        format!("{when}{why} · {}", self.summary)
    }
}

/// Every backup of a part, newest first.
pub fn list(paths: &Paths, part: &str) -> Vec<Backup> {
    let mut out = Vec::new();
    for p in files_in(&paths.backups).into_iter().chain(files_in(&snapshots_dir(paths))) {
        let (when, why) = if let Some(d) = daily_date(&p, part) {
            (d.and_hms_opt(0, 0, 0).unwrap(), "daily".to_string())
        } else if let Some((t, why)) = snapshot_time(&p, part) {
            (t, why)
        } else if let Some(why) = other_backup(&p, part) {
            // older hand-made copies like "workouts-before-10kg-20261020-1246.json"
            let t = fs::metadata(&p).and_then(|m| m.modified()).map(|m| chrono::DateTime::<Local>::from(m).naive_local()).unwrap_or_default();
            (t, why)
        } else {
            continue;
        };
        let Some(summary) = fs::read_to_string(&p).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| summary(part, &v)) else { continue };
        out.push(Backup { path: p, when, why, summary });
    }
    out.sort_by(|a, b| b.when.cmp(&a.when));
    out
}

/// What a part's JSON holds, or None if it isn't that part.
pub fn summary(part: &str, v: &Value) -> Option<String> {
    let n = |k: &str| v.get(k).and_then(Value::as_array).map(Vec::len);
    let s = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    Some(match part {
        "items" => s(n("items")?, "task", "tasks"),
        "calories" => s(n("entries")?, "entry", "entries"),
        "weight" => s(n("entries")?, "weigh-in", "weigh-ins"),
        "journal" => s(n("entries")?, "entry", "entries"),
        "foods" => format!("{}, {}", s(n("foods")?, "ingredient", "ingredients"), s(n("meals").unwrap_or(0), "meal", "meals")),
        "exercises" => s(n("exercises")?, "exercise", "exercises"),
        "workouts" => s(n("workouts")?, "workout", "workouts"),
        "workout_log" => s(n("sessions")?, "session", "sessions"),
        "body" => {
            let goal = v.get("goal").and_then(|g| g.get("target_kg")).and_then(Value::as_f64);
            v.as_object()?;
            match goal {
                Some(kg) => format!("goal {kg:.1} kg"),
                None => "no goal".into(),
            }
        }
        _ => return None,
    })
}

fn files_in(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect()).unwrap_or_default()
}

fn stem_after<'a>(p: &'a Path, part: &str) -> Option<&'a str> {
    let name = p.file_name()?.to_str()?.strip_suffix(".json")?;
    name.strip_prefix(part)?.strip_prefix('-')
}

/// "foods-2026-10-24.json" → 2026-10-24
fn daily_date(p: &Path, part: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(stem_after(p, part)?, "%Y-%m-%d").ok()
}

/// "foods-20261026-143000-before-restore.json" → (time, "before-restore")
fn snapshot_time(p: &Path, part: &str) -> Option<(NaiveDateTime, String)> {
    let rest = stem_after(p, part)?;
    let t = NaiveDateTime::parse_from_str(rest.get(..15)?, "%Y%m%d-%H%M%S").ok()?;
    Some((t, rest.get(16..).unwrap_or("").to_string()))
}

/// Any other "<part>-something.json" in the backups folder.
fn other_backup(p: &Path, part: &str) -> Option<String> {
    let rest = stem_after(p, part)?;
    // "workout_log-…" isn't a "workouts" backup and vice versa, and dates are handled above
    (!rest.is_empty() && !rest.starts_with(|c: char| c.is_ascii_digit())).then(|| rest.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backups_are_listed_and_restored() {
        let root = std::env::temp_dir().join(format!("mk-backup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let paths = Paths::at(root.clone());
        fs::create_dir_all(&paths.backups).unwrap();
        fs::write(&paths.foods, r#"{"version":1,"foods":[{"id":"a"},{"id":"b"}],"meals":[]}"#).unwrap();
        fs::write(paths.backups.join("foods-2026-10-20.json"), r#"{"version":1,"foods":[{"id":"a"}],"meals":[{"id":"m"}]}"#).unwrap();
        fs::write(paths.backups.join("workouts-before-10kg-20261020-1246.json"), r#"{"version":1,"workouts":[]}"#).unwrap();
        fs::write(paths.backups.join("workout_log-2026-10-20.json"), r#"{"version":1,"sessions":[]}"#).unwrap();
        daily(&paths);

        let list = list(&paths, "foods");
        assert_eq!(list.len(), 2, "{list:?}"); // today's daily copy and Oct 20
        assert_eq!(list[1].summary, "1 ingredient, 1 meal");
        assert!(list[1].label().starts_with("Tue Oct 20 · 1 ingredient"), "{}", list[1].label());
        // the hand-made workouts copy is listed; workout_log files aren't workouts
        let w = super::list(&paths, "workouts");
        assert_eq!((w.len(), w[0].why.as_str()), (1, "before-10kg-20261020-1246"));

        // restoring keeps the current file as a snapshot, so it can be undone
        restore(&paths, "foods", &list[1].path).unwrap();
        assert!(fs::read_to_string(&paths.foods).unwrap().contains(r#""id":"m""#));
        let after = super::list(&paths, "foods");
        let before_restore = after.iter().find(|b| b.why == "before-restore").expect("snapshot");
        assert_eq!(before_restore.summary, "2 ingredients, 0 meals");
        // a file of another part is refused
        assert!(restore(&paths, "foods", &paths.backups.join("workout_log-2026-10-20.json")).is_err());
        let _ = fs::remove_dir_all(&root);
    }
}
