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
}
