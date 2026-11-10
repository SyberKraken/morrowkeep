//! The category ("kind") names in use, so the parser can recognise `#study` and friends. The
//! desktop app sets them from Settings > Categories; the phone sets them from its snapshot.

use std::sync::RwLock;

static KINDS: RwLock<Vec<String>> = RwLock::new(Vec::new());

/// The built-in categories, used until `set` is called.
pub const DEFAULTS: &[&str] = &["task", "chore", "meal", "workout", "event", "habit"];

/// Replaces the known category names.
pub fn set(names: Vec<String>) {
    *KINDS.write().unwrap() = names;
}

/// Whether `name` is one of your categories.
pub fn is_known(name: &str) -> bool {
    let kinds = KINDS.read().unwrap();
    if kinds.is_empty() {
        DEFAULTS.contains(&name)
    } else {
        kinds.iter().any(|k| k == name)
    }
}
