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
}
