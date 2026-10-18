//! Workouts: an exercise library (dumbbells, bodyweight, flat bench) kept as readable JSON so
//! exports and Claude know what each exercise does. See WORKOUTS_PLAN.md for the whole plan.

use serde::{Deserialize, Serialize};

const BUILTIN: &str = include_str!("workouts_builtin.json");

/// Muscle names used in the library (and by the muscle map).
pub const MUSCLES: &[&str] = &[
    "chest", "shoulders", "biceps", "forearms", "core", "obliques", "hip flexors", "quads", "upper back", "lats", "rear delts", "triceps", "lower back",
    "glutes", "hamstrings", "calves", "full body",
];

/// Equipment values: (stored name, label).
pub const EQUIPMENT: &[(&str, &str)] = &[("dumbbells", "Two dumbbells"), ("dumbbell", "One dumbbell"), ("bench", "Flat bench"), ("bodyweight", "Bodyweight"), ("mat", "Mat")];

pub const LEVELS: &[&str] = &["beginner", "intermediate", "advanced"];

/// How an exercise is counted.
pub const KINDS: &[(&str, &str)] = &[("reps", "Reps"), ("reps_per_side", "Reps per side"), ("time", "Time")];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Exercise {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub equipment: Vec<String>,
    #[serde(default)]
    pub muscles_main: Vec<String>,
    #[serde(default)]
    pub muscles_helping: Vec<String>,
    #[serde(default = "beginner")]
    pub level: String,
    /// "reps", "reps_per_side" or "time".
    #[serde(default = "reps")]
    pub kind: String,
    #[serde(default)]
    pub steps: Vec<String>,
    #[serde(default)]
    pub form: Vec<String>,
    #[serde(default)]
    pub mistakes: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub easier: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub harder: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Shipped with Morrowkeep (updated with new versions unless `edited`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub builtin: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub edited: bool,
}

fn beginner() -> String {
    "beginner".into()
}
fn reps() -> String {
    "reps".into()
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ExerciseFile {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub exercises: Vec<Exercise>,
    /// Built-in exercises you deleted (so they aren't added back).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<String>,
}

pub fn builtins() -> Vec<Exercise> {
    let mut v: Vec<Exercise> = serde_json::from_str(BUILTIN).expect("built-in exercises are valid JSON");
    for e in &mut v {
        e.builtin = true;
    }
    v
}

/// Adds built-in exercises that are missing and refreshes the ones you haven't edited (and
/// haven't deleted: deleted built-ins are remembered by id in `removed`). Returns true if
/// anything changed.
pub fn merge_builtins(list: &mut Vec<Exercise>, removed: &[String]) -> bool {
    let mut changed = false;
    for b in builtins() {
        if removed.contains(&b.id) {
            continue;
        }
        match list.iter_mut().find(|e| e.id == b.id) {
            None => {
                list.push(b);
                changed = true;
            }
            Some(e) if !e.edited && *e != b => {
                *e = b;
                changed = true;
            }
            _ => {}
        }
    }
    changed
}

impl Exercise {
    /// Short equipment label for lists: "DB · bench", "BW".
    pub fn equipment_short(&self) -> String {
        let parts: Vec<&str> = self
            .equipment
            .iter()
            .filter_map(|e| match e.as_str() {
                "dumbbells" | "dumbbell" => Some("DB"),
                "bench" => Some("bench"),
                "bodyweight" => Some("BW"),
                _ => None,
            })
            .collect();
        let mut seen = Vec::new();
        for p in parts {
            if !seen.contains(&p) {
                seen.push(p);
            }
        }
        seen.join(" · ")
    }

    pub fn works(&self, muscle: &str) -> bool {
        self.muscles_main.iter().chain(&self.muscles_helping).any(|m| m == muscle)
    }

    pub fn needs(&self, equipment: &str) -> bool {
        match equipment {
            // "dumbbells" filter also finds one-dumbbell exercises
            "dumbbells" => self.equipment.iter().any(|e| e == "dumbbells" || e == "dumbbell"),
            other => self.equipment.iter().any(|e| e == other),
        }
    }

    pub fn matches(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        q.is_empty() || std::iter::once(&self.name).chain(&self.aliases).any(|n| q.split_whitespace().all(|w| n.to_lowercase().contains(w)))
    }
}

pub fn level_label(l: &str) -> &str {
    match l {
        "beginner" => "Beginner",
        "intermediate" => "Intermediate",
        "advanced" => "Advanced",
        other => other,
    }
}

pub fn kind_label(k: &str) -> &str {
    KINDS.iter().find(|(v, _)| *v == k).map_or(k, |(_, l)| *l)
}

pub fn equipment_label(e: &str) -> &str {
    EQUIPMENT.iter().find(|(v, _)| *v == e).map_or(e, |(_, l)| *l)
}

// ---------------------------------------------------------------- workouts

const STARTER: &str = include_str!("workouts_starter.json");

/// One step of a workout: an exercise done for some sets, or a timed break.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Item {
    Exercise {
        /// Id of an exercise in the library.
        exercise: String,
        sets: u32,
        /// Reps per set (per side for "reps per side" exercises).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reps: Option<u32>,
        /// Seconds per set. With a time the set counts down and moves on by itself; with only
        /// reps the player waits for "Done set".
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seconds: Option<u32>,
        /// kg per dumbbell.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kg: Option<f32>,
        /// Rest between sets, in seconds.
        #[serde(default)]
        rest_s: u32,
    },
    Break {
        seconds: u32,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        label: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Workout {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    #[serde(default)]
    pub items: Vec<Item>,
    /// A task (item id) that gets ticked off for the day when this workout is finished.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub builtin: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub edited: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WorkoutFile {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub workouts: Vec<Workout>,
    /// Starter workouts you deleted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<String>,
}

pub fn starter_workouts() -> Vec<Workout> {
    let mut v: Vec<Workout> = serde_json::from_str(STARTER).expect("starter workouts are valid JSON");
    for w in &mut v {
        w.builtin = true;
    }
    v
}

/// Same rules as `merge_builtins`, for the starter workouts.
pub fn merge_starters(list: &mut Vec<Workout>, removed: &[String]) -> bool {
    let mut changed = false;
    for b in starter_workouts() {
        if removed.contains(&b.id) {
            continue;
        }
        match list.iter_mut().find(|w| w.id == b.id) {
            None => {
                list.push(b);
                changed = true;
            }
            Some(w) if !w.edited && *w != b => {
                *w = b;
                changed = true;
            }
            _ => {}
        }
    }
    changed
}

/// Seconds a rep takes, for estimates.
pub const SECS_PER_REP: u32 = 3;

impl Item {
    /// Rough length in seconds (timed sets as set, reps at 3 s each, both sides for
    /// per-side exercises, rests between sets).
    pub fn seconds(&self, lib: &[Exercise]) -> u32 {
        match self {
            Item::Break { seconds, .. } => *seconds,
            Item::Exercise { exercise, sets, reps, seconds, rest_s, .. } => {
                let sides = if lib.iter().find(|e| e.id == *exercise).is_some_and(|e| e.kind == "reps_per_side") { 2 } else { 1 };
                let per_set = seconds.unwrap_or(reps.unwrap_or(10) * SECS_PER_REP * sides);
                sets * per_set + sets.saturating_sub(1) * rest_s
            }
        }
    }
}

impl Workout {
    pub fn seconds(&self, lib: &[Exercise]) -> u32 {
        self.items.iter().map(|i| i.seconds(lib)).sum()
    }

    /// Equipment needed, without repeats ("two dumbbells" covers "one dumbbell").
    pub fn equipment(&self, lib: &[Exercise]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for i in &self.items {
            if let Item::Exercise { exercise, .. } = i {
                if let Some(e) = lib.iter().find(|e| e.id == *exercise) {
                    for q in &e.equipment {
                        if q != "bodyweight" && !out.contains(q) {
                            out.push(q.clone());
                        }
                    }
                }
            }
        }
        if out.iter().any(|q| q == "dumbbells") {
            out.retain(|q| q != "dumbbell");
        }
        out
    }

    pub fn exercise_count(&self) -> usize {
        self.items.iter().filter(|i| matches!(i, Item::Exercise { .. })).count()
    }
}

// ---------------------------------------------------------------- playing and history

/// One step of a workout as it's played: a set of an exercise, or a rest.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Set { exercise: String, name: String, set: u32, sets: u32, reps: Option<u32>, seconds: Option<u32>, kg: Option<f32>, per_side: bool },
    Rest { seconds: u32, label: String },
}

impl Step {
    /// Seconds the step's timer runs; None for a set with only reps (waits for "Done set").
    pub fn duration(&self) -> Option<u32> {
        match self {
            Step::Set { seconds, .. } => *seconds,
            Step::Rest { seconds, .. } => Some(*seconds),
        }
    }
}

impl Workout {
    /// The workout as steps: each set, the rest between sets, and breaks.
    pub fn steps(&self, lib: &[Exercise]) -> Vec<Step> {
        let mut out = Vec::new();
        for item in &self.items {
            match item {
                Item::Break { seconds, label } => out.push(Step::Rest { seconds: *seconds, label: if label.is_empty() { "Break".into() } else { label.clone() } }),
                Item::Exercise { exercise, sets, reps, seconds, kg, rest_s } => {
                    let e = lib.iter().find(|e| e.id == *exercise);
                    let name = e.map_or(exercise.clone(), |e| e.name.clone());
                    let per_side = e.is_some_and(|e| e.kind == "reps_per_side");
                    for set in 1..=(*sets).max(1) {
                        out.push(Step::Set { exercise: exercise.clone(), name: name.clone(), set, sets: *sets, reps: *reps, seconds: *seconds, kg: *kg, per_side });
                        if set < *sets && *rest_s > 0 {
                            out.push(Step::Rest { seconds: *rest_s, label: "Rest".into() });
                        }
                    }
                }
            }
        }
        out
    }
}

/// A set as it was done.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SetDone {
    pub exercise: String,
    pub name: String,
    pub set: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reps: Option<u32>,
    pub seconds: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kg: Option<f32>,
}

/// A finished (or ended early) workout.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub date: chrono::NaiveDate,
    /// "HH:MM" it started.
    pub start: String,
    pub duration_s: u32,
    pub workout_id: String,
    pub workout: String,
    pub sets: Vec<SetDone>,
    pub completed: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WorkoutLog {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub sessions: Vec<Session>,
}

/// One day's sets of an exercise, from the log.
#[derive(Clone, Debug, PartialEq)]
pub struct ExerciseDay {
    pub date: chrono::NaiveDate,
    pub sets: Vec<SetDone>,
}

impl ExerciseDay {
    pub fn top_kg(&self) -> Option<f32> {
        self.sets.iter().filter_map(|s| s.kg).fold(None, |m, k| Some(m.map_or(k, |m: f32| m.max(k))))
    }
    pub fn top_reps(&self) -> Option<u32> {
        self.sets.iter().filter_map(|s| s.reps).max()
    }
    /// "3 × 10 @ 12 kg", "2 × 40 s".
    pub fn summary(&self) -> String {
        let n = self.sets.len();
        match (self.top_reps(), self.top_kg()) {
            (Some(r), Some(k)) => format!("{n} × {r} @ {k} kg"),
            (Some(r), None) => format!("{n} × {r} reps"),
            _ => format!("{n} × {} s", self.sets.iter().map(|s| s.seconds).max().unwrap_or(0)),
        }
    }
}

impl WorkoutLog {
    /// Every day an exercise was done, oldest first.
    pub fn exercise_days(&self, exercise: &str) -> Vec<ExerciseDay> {
        let mut days: Vec<ExerciseDay> = Vec::new();
        let mut sessions: Vec<&Session> = self.sessions.iter().collect();
        sessions.sort_by(|a, b| (a.date, &a.start).cmp(&(b.date, &b.start)));
        for s in sessions {
            let sets: Vec<SetDone> = s.sets.iter().filter(|x| x.exercise == exercise).cloned().collect();
            if sets.is_empty() {
                continue;
            }
            match days.last_mut() {
                Some(d) if d.date == s.date => d.sets.extend(sets),
                _ => days.push(ExerciseDay { date: s.date, sets }),
            }
        }
        days
    }
}

/// "28 min", "45 s".
pub fn duration_label(secs: u32) -> String {
    if secs >= 60 {
        format!("{} min", (secs + 30) / 60)
    } else {
        format!("{secs} s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_library_is_complete_and_readable() {
        let b = builtins();
        assert!(b.len() >= 40, "{}", b.len());
        let mut ids = std::collections::HashSet::new();
        for e in &b {
            assert!(ids.insert(e.id.clone()), "duplicate id {}", e.id);
            assert!(!e.steps.is_empty() && !e.form.is_empty(), "{} needs steps and form", e.id);
            assert!(!e.muscles_main.is_empty(), "{} needs main muscles", e.id);
            for m in e.muscles_main.iter().chain(&e.muscles_helping) {
                assert!(MUSCLES.contains(&m.as_str()), "{}: unknown muscle {m}", e.id);
            }
            for q in &e.equipment {
                assert!(EQUIPMENT.iter().any(|(v, _)| v == q), "{}: unknown equipment {q}", e.id);
            }
            assert!(LEVELS.contains(&e.level.as_str()) && KINDS.iter().any(|(k, _)| *k == e.kind), "{}", e.id);
        }
        // merging: adds missing, refreshes unedited, keeps edited, skips removed
        let mut list = vec![Exercise { id: "push-up".into(), name: "My push-up".into(), edited: true, ..Default::default() }];
        assert!(merge_builtins(&mut list, &["plank".to_string()]));
        assert_eq!(list.iter().find(|e| e.id == "push-up").unwrap().name, "My push-up");
        assert!(!list.iter().any(|e| e.id == "plank"));
        assert!(!merge_builtins(&mut list, &["plank".to_string()]));
        assert_eq!(list.iter().find(|e| e.id == "db-bent-over-row").unwrap().equipment_short(), "DB");
    }

    #[test]
    fn starter_workouts_use_library_exercises() {
        let lib = builtins();
        let ws = starter_workouts();
        assert_eq!(ws.len(), 5);
        for w in &ws {
            for item in &w.items {
                if let Item::Exercise { exercise, sets, reps, seconds, .. } = item {
                    assert!(lib.iter().any(|e| e.id == *exercise), "{}: unknown exercise {exercise}", w.id);
                    assert!(*sets >= 1 && (reps.is_some() || seconds.is_some()), "{}: {exercise} needs reps or a time", w.id);
                }
            }
        }
        // quick workout is about 10 minutes; full body about half an hour
        let quick = ws.iter().find(|w| w.id == "starter-quick").unwrap();
        assert!((8 * 60..=13 * 60).contains(&quick.seconds(&lib)), "{}", quick.seconds(&lib));
        let full = ws.iter().find(|w| w.id == "starter-full-body").unwrap();
        assert!((20 * 60..=40 * 60).contains(&full.seconds(&lib)), "{}", full.seconds(&lib));
        assert_eq!(full.equipment(&lib), ["dumbbells", "bench", "mat"]);
        // one item's estimate: 3 × 10 reps (30 s) + 2 rests of 60 s
        let i = Item::Exercise { exercise: "db-curl".into(), sets: 3, reps: Some(10), seconds: None, kg: Some(8.0), rest_s: 60 };
        assert_eq!(i.seconds(&lib), 3 * 30 + 2 * 60);
        // stored readably
        let json = serde_json::to_string(&Item::Break { seconds: 60, label: String::new() }).unwrap();
        assert_eq!(json, r#"{"type":"break","seconds":60}"#);
    }

    #[test]
    fn workouts_become_steps() {
        let lib = builtins();
        let w = Workout {
            items: vec![
                Item::Exercise { exercise: "db-curl".into(), sets: 3, reps: Some(10), seconds: None, kg: Some(8.0), rest_s: 60 },
                Item::Break { seconds: 90, label: String::new() },
                Item::Exercise { exercise: "plank".into(), sets: 2, reps: None, seconds: Some(40), kg: None, rest_s: 0 },
            ],
            ..Default::default()
        };
        let steps = w.steps(&lib);
        // 3 sets + 2 rests, a break, 2 plank sets (no rest)
        assert_eq!(steps.len(), 8);
        assert_eq!(steps[0].duration(), None); // reps only: waits for Done set
        assert_eq!(steps[1], Step::Rest { seconds: 60, label: "Rest".into() });
        assert_eq!(steps[5], Step::Rest { seconds: 90, label: "Break".into() });
        assert_eq!(steps[6].duration(), Some(40));
    }

    #[test]
    fn exercise_history_from_the_log() {
        let d = |n| chrono::NaiveDate::from_ymd_opt(2026, 10, n).unwrap();
        let set = |ex: &str, reps, kg| SetDone { exercise: ex.into(), name: ex.into(), set: 1, reps: Some(reps), seconds: 30, kg: Some(kg) };
        let sess = |date, sets| Session { id: "x".into(), date, start: "18:00".into(), duration_s: 600, workout_id: "w".into(), workout: "W".into(), sets, completed: true };
        let log = WorkoutLog { version: 1, sessions: vec![sess(d(12), vec![set("row", 10, 12.0), set("row", 8, 14.0), set("curl", 12, 8.0)]), sess(d(10), vec![set("row", 10, 10.0)])] };
        let days = log.exercise_days("row");
        assert_eq!(days.len(), 2);
        assert_eq!(days[0].date, d(10)); // oldest first
        assert_eq!(days[1].top_kg(), Some(14.0));
        assert_eq!(days[1].summary(), "2 × 10 @ 14 kg");
        assert!(log.exercise_days("plank").is_empty());
    }
}
