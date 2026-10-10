//! Task categories (an item's `kind`) and your own color choices, kept in settings.json.
//! Each category has an icon, an optional color (none = the theme's color for that kind) and
//! the style its streaks are drawn in. A copy lives in a global so drawing code can look a
//! kind up without access to the data; `apply` refreshes it after settings change.

use eframe::egui::Color32;
use serde::{Deserialize, Serialize};
use std::sync::RwLock;

/// How a category's streaks are drawn on the Charts tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StreakStyle {
    /// A ribbon that thickens as the streak grows.
    Ribbon,
    /// Comets: the live streak is a bright head, older days fade.
    Comet,
    /// Each day hops to the next; hops grow with the streak.
    Arcs,
    /// A winding stem that sprouts leaves at milestones.
    Vine,
    /// Zig-zag with a 4-point star every 7th day.
    Constellation,
}

impl StreakStyle {
    pub const ALL: [StreakStyle; 5] = [StreakStyle::Ribbon, StreakStyle::Comet, StreakStyle::Arcs, StreakStyle::Vine, StreakStyle::Constellation];

    pub fn label(self) -> &'static str {
        match self {
            StreakStyle::Ribbon => "Ribbon",
            StreakStyle::Comet => "Comet",
            StreakStyle::Arcs => "Arcs",
            StreakStyle::Vine => "Vine",
            StreakStyle::Constellation => "Constellation",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            StreakStyle::Ribbon => "thickens as the streak grows",
            StreakStyle::Comet => "the live streak glows, older days fade",
            StreakStyle::Arcs => "hops from day to day, growing with the streak",
            StreakStyle::Vine => "a winding stem that sprouts leaves at milestones",
            StreakStyle::Constellation => "zig-zag with a star every 7th day",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Category {
    /// The kind name items use, e.g. "workout" (lowercase, no spaces; also the #tag).
    pub name: String,
    pub icon: String,
    /// "#RRGGBB"; none = the theme's color for this kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub style: StreakStyle,
}

impl Category {
    pub fn new(name: &str, icon: &str, style: StreakStyle) -> Self {
        Category { name: name.into(), icon: icon.into(), color: None, style }
    }
}

/// The kind every item falls back to; it can't be deleted.
pub const DEFAULT_KIND: &str = "task";

pub fn defaults() -> Vec<Category> {
    vec![
        Category::new("task", "📌", StreakStyle::Constellation),
        Category::new("chore", "🏠", StreakStyle::Arcs),
        Category::new("meal", "🍴", StreakStyle::Comet),
        Category::new("workout", "💪", StreakStyle::Ribbon),
        Category::new("event", "📅", StreakStyle::Constellation),
        Category::new("habit", "🔁", StreakStyle::Vine),
    ]
}

/// Icons offered in the category editor (all render with the bundled emoji fonts).
pub const ICONS: &[&str] = &[
    "📌", "🏠", "🍴", "💪", "📅", "🔁", "⭐", "📚", "💼", "🎵", "🎮", "💊", "🛒", "🚲", "🏃", "🐶", "🌱", "💤", "💧", "🔧", "✏", "💰", "❤", "☕",
];

/// Colors you chose over the theme's for the calorie and weight data.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calories: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<String>,
}

/// Makes sure the list is usable: the default kind exists, names are clean and unique.
pub fn normalize(cats: &mut Vec<Category>) {
    let mut seen = std::collections::HashSet::new();
    cats.retain_mut(|c| {
        c.name = clean_name(&c.name);
        !c.name.is_empty() && seen.insert(c.name.clone())
    });
    if !cats.iter().any(|c| c.name == DEFAULT_KIND) {
        cats.insert(0, defaults().remove(0));
    }
}

/// Lowercase, spaces to dashes, only letters/digits/dashes: usable as a #tag.
pub fn clean_name(s: &str) -> String {
    s.trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_whitespace() || c == '_' { '-' } else { c })
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect()
}

pub fn parse_hex(s: &str) -> Option<Color32> {
    let h = s.trim().strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

pub fn to_hex(c: Color32) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b())
}

struct Registry {
    cats: Vec<Category>,
    colors: ColorOverrides,
}

static REGISTRY: RwLock<Option<Registry>> = RwLock::new(None);

/// Makes these categories and colors the ones the app draws with.
pub fn apply(cats: &[Category], colors: &ColorOverrides) {
    *REGISTRY.write().unwrap() = Some(Registry { cats: cats.to_vec(), colors: colors.clone() });
}

fn with<T>(f: impl FnOnce(&Registry) -> T) -> T {
    let guard = REGISTRY.read().unwrap();
    match guard.as_ref() {
        Some(r) => f(r),
        None => f(&Registry { cats: defaults(), colors: ColorOverrides::default() }),
    }
}

pub fn get(kind: &str) -> Option<Category> {
    with(|r| r.cats.iter().find(|c| c.name == kind).cloned())
}

pub fn names() -> Vec<String> {
    with(|r| r.cats.iter().map(|c| c.name.clone()).collect())
}

/// Your color for a kind, if you picked one (otherwise the theme decides).
pub fn color(kind: &str) -> Option<Color32> {
    with(|r| r.cats.iter().find(|c| c.name == kind).and_then(|c| c.color.as_deref()).and_then(parse_hex))
}

pub fn icon(kind: &str) -> String {
    get(kind).map_or_else(|| "⭐".to_string(), |c| c.icon)
}

/// Unknown kinds are drawn as a constellation.
pub fn style(kind: &str) -> StreakStyle {
    get(kind).map_or(StreakStyle::Constellation, |c| c.style)
}

pub fn calorie_color() -> Option<Color32> {
    with(|r| r.colors.calories.as_deref().and_then(parse_hex))
}

pub fn weight_color() -> Option<Color32> {
    with(|r| r.colors.weight.as_deref().and_then(parse_hex))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_cleaned_and_task_kept() {
        assert_eq!(clean_name("  Deep Work_2 "), "deep-work-2");
        assert_eq!(clean_name("#gym!"), "gym");
        let mut cats = vec![Category::new("Study", "📚", StreakStyle::Vine), Category::new("study", "📚", StreakStyle::Arcs), Category::new("  ", "⭐", StreakStyle::Arcs)];
        normalize(&mut cats);
        assert_eq!(cats.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["task", "study"]);
        assert_eq!(cats[1].style, StreakStyle::Vine); // first one wins
        assert_eq!(parse_hex("#FF8000"), Some(Color32::from_rgb(255, 128, 0)));
        assert_eq!(to_hex(Color32::from_rgb(1, 2, 255)), "#0102FF");
        assert_eq!(parse_hex("red"), None);
    }
}
