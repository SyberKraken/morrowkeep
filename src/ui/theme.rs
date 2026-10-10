//! Look and feel: themes (colors and shapes, loaded from JSON files), fonts and the faint star
//! field behind the Habits constellation.
//!
//! # Themes
//! A theme is a JSON file in `<data folder>\themes\` (e.g. `night-sky.json`). Every visual color
//! in the app comes from the current theme. **New visual elements should add a field here
//! rather than hard-coding a color.** Every field has a default (the Night sky values), so
//! theme files only need the keys they change, and older theme files keep working when new
//! fields are added. Colors are `"#RRGGBB"` or `"#RRGGBBAA"`.
//!
//! The current theme is a global (`th()`), swapped atomically; the worker reloads it when the
//! file changes on disk, and the UI re-applies egui's visuals when `version()` changes.

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Painter, Pos2, Rect, Shadow, Stroke, Visuals};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

// ---------------------------------------------------------------- colors in JSON

/// Serializes a `Color32` as "#RRGGBB" (or "#RRGGBBAA" when not opaque).
mod hex {
    use eframe::egui::Color32;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn to_string(c: Color32) -> String {
        let [r, g, b, a] = c.to_srgba_unmultiplied();
        if a == 255 {
            format!("#{r:02X}{g:02X}{b:02X}")
        } else {
            format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
        }
    }

    pub fn parse(s: &str) -> Option<Color32> {
        let h = s.trim().trim_start_matches('#');
        let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
        match h.len() {
            6 => Some(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?)),
            8 => Some(Color32::from_rgba_unmultiplied(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            _ => None,
        }
    }

    pub fn serialize<S: Serializer>(c: &Color32, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&to_string(*c))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Color32, D::Error> {
        let s = String::deserialize(d)?;
        parse(&s).ok_or_else(|| serde::de::Error::custom(format!("\"{s}\" is not a color (use #RRGGBB or #RRGGBBAA)")))
    }
}

mod hex_map {
    use eframe::egui::Color32;
    use serde::{Deserialize, Deserializer, Serializer};
    use std::collections::BTreeMap;

    pub fn serialize<S: Serializer>(m: &BTreeMap<String, Color32>, s: S) -> Result<S::Ok, S::Error> {
        s.collect_map(m.iter().map(|(k, c)| (k, super::hex::to_string(*c))))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<BTreeMap<String, Color32>, D::Error> {
        let raw = BTreeMap::<String, String>::deserialize(d)?;
        raw.into_iter()
            .map(|(k, v)| match super::hex::parse(&v) {
                Some(c) => Ok((k.to_lowercase(), c)),
                None => Err(serde::de::Error::custom(format!("kinds.{k}: \"{v}\" is not a color"))),
            })
            .collect()
    }
}

// ---------------------------------------------------------------- the theme

/// Shorthand for an opaque color in the defaults below.
const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub name: String,
    /// Free-text description, ignored by the app.
    pub about: String,
    /// true = dark theme (light text on dark surfaces), false = light theme.
    pub dark: bool,
    pub surface: Surfaces,
    pub text: TextColors,
    pub widget: WidgetColors,
    /// Meaning colors: success (done, "all done", connected), danger (over budget, errors).
    pub status: StatusColors,
    /// Color per task kind. Any kind name works (e.g. a custom "study" kind); kinds without
    /// an entry use "other".
    #[serde(with = "hex_map")]
    pub kinds: BTreeMap<String, Color32>,
    /// Colors that identify a kind of data wherever it appears.
    pub data: DataColors,
    /// The star field behind the Habits constellation.
    pub sky: Sky,
    pub shape: Shape,
    pub charts: Charts,
    /// Animations: tile flips, igniting stars, sparkles, counting numbers, toasts.
    pub motion: Motion,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Charts {
    /// Opacity of the fill under the weight line (0 = no fill).
    pub line_fill_alpha: f32,
    /// Rounding of the calorie bars' tops, in pixels.
    pub bar_radius: u8,
}

impl Default for Charts {
    fn default() -> Self {
        Charts { line_fill_alpha: 0.05, bar_radius: 4 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Motion {
    /// false = no animations at all (everything jumps straight to its end state).
    pub enabled: bool,
    /// Speed multiplier: 2.0 = twice as fast, 0.5 = half speed.
    pub speed: f32,
}

impl Default for Motion {
    fn default() -> Self {
        Motion { enabled: true, speed: 1.0 }
    }
}

/// Smoothly animates a number toward `target` (e.g. a total that just changed); instant when
/// the theme turns motion off.
pub fn animate(ctx: &egui::Context, id: &str, target: f32, secs: f32) -> f32 {
    let t = anim(secs);
    if t <= 0.0 {
        target
    } else {
        ctx.animate_value_with_time(egui::Id::new(("anim", id)), target, t)
    }
}

/// Duration of an animation in seconds after the theme's motion settings (0 = instant).
pub fn anim(secs: f32) -> f32 {
    let m = th().motion.clone();
    if m.enabled && m.speed > 0.0 {
        secs / m.speed
    } else {
        0.0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Surfaces {
    /// Main background.
    #[serde(with = "hex")]
    pub page: Color32,
    /// Dialogs and popups.
    #[serde(with = "hex")]
    pub window: Color32,
    /// Cards and pending tiles, lifted above the page.
    #[serde(with = "hex")]
    pub card: Color32,
    #[serde(with = "hex")]
    pub card_border: Color32,
    /// Text fields.
    #[serde(with = "hex")]
    pub input: Color32,
    /// Weekend days in the calendar (a touch dimmer than cards).
    #[serde(with = "hex")]
    pub weekend: Color32,
    /// Card and window shadows (use an alpha, e.g. #0000003C).
    #[serde(with = "hex")]
    pub shadow: Color32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TextColors {
    #[serde(with = "hex")]
    pub primary: Color32,
    /// Supporting text, hints, unlit labels.
    #[serde(with = "hex")]
    pub muted: Color32,
    /// Text and icons on colored fills (finished tiles, chips).
    #[serde(with = "hex")]
    pub on_color: Color32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct WidgetColors {
    /// Buttons at rest.
    #[serde(with = "hex")]
    pub fill: Color32,
    #[serde(with = "hex")]
    pub fill_hover: Color32,
    #[serde(with = "hex")]
    pub fill_active: Color32,
    #[serde(with = "hex")]
    pub border: Color32,
    #[serde(with = "hex")]
    pub border_hover: Color32,
    /// Selected tab, text selection.
    #[serde(with = "hex")]
    pub selection: Color32,
    #[serde(with = "hex")]
    pub selection_border: Color32,
    #[serde(with = "hex")]
    pub link: Color32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct StatusColors {
    #[serde(with = "hex")]
    pub success: Color32,
    #[serde(with = "hex")]
    pub danger: Color32,
    /// Cautions that aren't errors (e.g. a goal pace that's faster than safe).
    #[serde(with = "hex")]
    pub warning: Color32,
    /// Background of error banners.
    #[serde(with = "hex")]
    pub danger_background: Color32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct DataColors {
    /// The Morrowkeep logo and name in the header.
    #[serde(with = "hex")]
    pub brand: Color32,
    /// Today's glowing outline in the calendar.
    #[serde(with = "hex")]
    pub today: Color32,
    #[serde(with = "hex")]
    pub calories: Color32,
    #[serde(with = "hex")]
    pub weight: Color32,
    /// Claude-related controls and labels.
    #[serde(with = "hex")]
    pub claude: Color32,
    /// Events pulled from Google Calendar.
    #[serde(with = "hex")]
    pub google: Color32,
    /// "Not logged" notes and other "nothing here" text.
    #[serde(with = "hex")]
    pub not_logged: Color32,
    /// "Everything done" stars and highlights.
    #[serde(with = "hex")]
    pub gold: Color32,
    /// The red "now" line in schedules and the week view.
    #[serde(with = "hex")]
    pub now_line: Color32,
    /// Dashed daily-limit line in the calorie chart.
    #[serde(with = "hex")]
    pub chart_limit_line: Color32,
    /// Goal weight: target line, target star and ideal path in the weight chart, goal progress.
    #[serde(with = "hex")]
    pub goal: Color32,
    /// The 7-day average weight line and its projection.
    #[serde(with = "hex")]
    pub trend: Color32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Sky {
    /// Star color; its alpha is the brightest a star gets.
    #[serde(with = "hex")]
    pub star: Color32,
    /// Stars per 1000 square pixels (0 = no star field).
    pub density: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Shape {
    pub card_radius: u8,
    pub tile_radius: u8,
    pub widget_radius: u8,
}

impl Default for Surfaces {
    fn default() -> Self {
        Surfaces {
            page: rgb(14, 18, 30),
            window: rgb(20, 25, 40),
            card: rgb(22, 28, 44),
            card_border: rgb(38, 46, 68),
            input: rgb(10, 13, 22),
            weekend: rgb(18, 23, 37),
            shadow: Color32::from_black_alpha(60),
        }
    }
}

impl Default for TextColors {
    fn default() -> Self {
        TextColors { primary: rgb(220, 226, 238), muted: rgb(140, 150, 170), on_color: Color32::WHITE }
    }
}

impl Default for WidgetColors {
    fn default() -> Self {
        WidgetColors {
            fill: rgb(38, 47, 70),
            fill_hover: rgb(50, 62, 92),
            fill_active: rgb(60, 73, 108),
            border: rgb(54, 65, 94),
            border_hover: rgb(72, 86, 122),
            selection: rgb(70, 92, 170),
            selection_border: rgb(170, 188, 255),
            link: rgb(140, 170, 255),
        }
    }
}

impl Default for StatusColors {
    fn default() -> Self {
        StatusColors { success: rgb(46, 160, 103), danger: rgb(220, 80, 60), warning: rgb(232, 165, 50), danger_background: rgb(120, 30, 30) }
    }
}

impl Default for DataColors {
    fn default() -> Self {
        DataColors {
            brand: rgb(46, 160, 103),
            today: rgb(46, 160, 103),
            calories: rgb(235, 120, 50),
            weight: rgb(70, 140, 230),
            claude: rgb(217, 119, 87),
            google: rgb(128, 128, 140),
            not_logged: rgb(140, 140, 150),
            gold: rgb(232, 182, 48),
            now_line: rgb(220, 60, 60),
            chart_limit_line: rgb(150, 150, 150),
            goal: rgb(80, 200, 160),
            trend: rgb(150, 190, 255),
        }
    }
}

impl Default for Sky {
    fn default() -> Self {
        Sky { star: Color32::from_rgba_unmultiplied(205, 215, 255, 130), density: 0.9 }
    }
}

impl Default for Shape {
    fn default() -> Self {
        Shape { card_radius: 12, tile_radius: 12, widget_radius: 6 }
    }
}

fn default_kinds() -> BTreeMap<String, Color32> {
    [
        ("workout", rgb(46, 160, 103)),
        ("meal", rgb(232, 140, 40)),
        ("chore", rgb(196, 162, 30)),
        ("task", rgb(66, 133, 244)),
        ("event", rgb(0, 150, 170)),
        ("habit", rgb(150, 90, 200)),
        ("other", rgb(120, 120, 130)),
    ]
    .into_iter()
    .map(|(k, c)| (k.to_string(), c))
    .collect()
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            name: "Night sky".into(),
            about: "Deep navy surfaces; color only on what matters. The default theme.".into(),
            dark: true,
            surface: Surfaces::default(),
            text: TextColors::default(),
            widget: WidgetColors::default(),
            status: StatusColors::default(),
            kinds: default_kinds(),
            data: DataColors::default(),
            sky: Sky::default(),
            shape: Shape::default(),
            charts: Charts::default(),
            motion: Motion::default(),
        }
    }
}

/// A light theme, shipped as a second example.
fn daylight() -> Theme {
    Theme {
        name: "Daylight".into(),
        about: "Light surfaces with deeper accent colors for contrast.".into(),
        dark: false,
        surface: Surfaces {
            page: rgb(240, 243, 248),
            window: rgb(250, 251, 253),
            card: rgb(255, 255, 255),
            card_border: rgb(221, 226, 236),
            input: rgb(246, 248, 251),
            weekend: rgb(246, 248, 251),
            shadow: Color32::from_black_alpha(22),
        },
        text: TextColors { primary: rgb(30, 36, 48), muted: rgb(106, 116, 136), on_color: Color32::WHITE },
        widget: WidgetColors {
            fill: rgb(232, 236, 244),
            fill_hover: rgb(220, 226, 238),
            fill_active: rgb(206, 214, 230),
            border: rgb(205, 212, 226),
            border_hover: rgb(160, 172, 200),
            selection: rgb(200, 214, 252),
            selection_border: rgb(40, 70, 170),
            link: rgb(50, 90, 200),
        },
        status: StatusColors { success: rgb(30, 140, 85), danger: rgb(200, 60, 40), warning: rgb(196, 125, 10), danger_background: rgb(250, 220, 215) },
        kinds: [
            ("workout", rgb(30, 140, 85)),
            ("meal", rgb(214, 110, 20)),
            ("chore", rgb(176, 140, 10)),
            ("task", rgb(40, 105, 220)),
            ("event", rgb(0, 130, 150)),
            ("habit", rgb(130, 70, 190)),
            ("other", rgb(110, 110, 120)),
        ]
        .into_iter()
        .map(|(k, c)| (k.to_string(), c))
        .collect(),
        data: DataColors {
            brand: rgb(30, 140, 85),
            today: rgb(30, 140, 85),
            calories: rgb(214, 100, 30),
            weight: rgb(40, 110, 210),
            claude: rgb(196, 98, 66),
            google: rgb(120, 120, 135),
            not_logged: rgb(130, 130, 142),
            gold: rgb(200, 150, 20),
            now_line: rgb(210, 50, 50),
            chart_limit_line: rgb(140, 140, 150),
            goal: rgb(20, 150, 110),
            trend: rgb(20, 60, 150),
        },
        sky: Sky { star: Color32::from_rgba_unmultiplied(90, 105, 150, 90), density: 0.6 },
        shape: Shape::default(),
        charts: Charts { line_fill_alpha: 0.05, bar_radius: 4 },
        motion: Motion::default(),
    }
}

/// Built-in themes written to the themes folder when missing: (file stem, theme).
fn builtin() -> Vec<(&'static str, Theme)> {
    let mut v = vec![("night-sky", Theme::default()), ("daylight", daylight())];
    v.extend(super::palettes::all());
    v
}

impl Theme {
    /// Color of a task kind (falls back to "other").
    pub fn kind(&self, kind: &str) -> Color32 {
        self.kinds.get(kind).or_else(|| self.kinds.get("other")).copied().unwrap_or(rgb(120, 120, 130))
    }

    /// Fills in any kinds the file didn't mention.
    fn complete(mut self) -> Self {
        let base = if self.dark { Theme::default().kinds } else { daylight().kinds };
        for (k, c) in base {
            self.kinds.entry(k).or_insert(c);
        }
        self
    }
}

// ---------------------------------------------------------------- the current theme

static CURRENT: OnceLock<RwLock<Arc<Theme>>> = OnceLock::new();
static VERSION: AtomicU64 = AtomicU64::new(1);

fn slot() -> &'static RwLock<Arc<Theme>> {
    CURRENT.get_or_init(|| RwLock::new(Arc::new(Theme::default())))
}

/// The current theme. Cheap (an `Arc` clone); read it wherever a color is needed.
pub fn th() -> Arc<Theme> {
    slot().read().unwrap().clone()
}

/// Bumped whenever the theme changes, so the UI knows to re-apply egui's visuals.
pub fn version() -> u64 {
    VERSION.load(Ordering::Relaxed)
}

pub fn set(theme: Theme) {
    *slot().write().unwrap() = Arc::new(theme);
    VERSION.fetch_add(1, Ordering::Relaxed);
}

pub fn themes_dir(data_root: &Path) -> PathBuf {
    data_root.join("themes")
}

pub fn theme_path(data_root: &Path, name: &str) -> PathBuf {
    themes_dir(data_root).join(format!("{name}.json"))
}

/// Writes the built-in themes to the themes folder if they aren't there, and adds any keys
/// introduced since a theme file was written (with their default values, so nothing changes
/// visually). That way every theme file shows every option that exists. Never changes values
/// that are already in a file.
pub fn install_builtins(data_root: &Path) {
    let dir = themes_dir(data_root);
    let _ = std::fs::create_dir_all(&dir);
    for (stem, theme) in builtin() {
        let path = dir.join(format!("{stem}.json"));
        if !path.exists() {
            if let Ok(json) = serde_json::to_string_pretty(&theme) {
                let _ = std::fs::write(path, json);
            }
        }
    }
    for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            let _ = add_missing_keys(&path);
        }
    }
}

/// Fills keys missing from a theme file from the matching built-in (dark: Night sky,
/// light: Daylight). Leaves files with errors alone.
fn add_missing_keys(path: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut file: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| e.to_string())?;
    let dark = file.get("dark").and_then(|d| d.as_bool()).unwrap_or(true);
    let base = serde_json::to_value(if dark { Theme::default() } else { daylight() }).map_err(|e| e.to_string())?;
    fn merge(into: &mut serde_json::Value, from: &serde_json::Value) -> bool {
        let (Some(into), Some(from)) = (into.as_object_mut(), from.as_object()) else { return false };
        let mut changed = false;
        for (k, v) in from {
            // per-kind colors are the user's own list; don't add kinds back
            if k == "kinds" && into.contains_key(k) {
                continue;
            }
            match into.get_mut(k) {
                None => {
                    into.insert(k.clone(), v.clone());
                    changed = true;
                }
                Some(existing) => changed |= merge(existing, v),
            }
        }
        changed
    }
    if merge(&mut file, &base) {
        let json = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Theme files in the themes folder: (file stem, display name).
pub fn list(data_root: &Path) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = std::fs::read_dir(themes_dir(data_root))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            (p.extension()? == "json").then(|| {
                let stem = p.file_stem()?.to_string_lossy().to_string();
                let name = load_file(&p).map(|t| t.name).unwrap_or_else(|_| format!("{stem} (has errors)"));
                Some((stem, name))
            })?
        })
        .collect();
    out.sort();
    out
}

pub fn load_file(path: &Path) -> Result<Theme, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str::<Theme>(text.trim_start_matches('\u{feff}'))
        .map(Theme::complete)
        .map_err(|e| format!("{}: {e}", path.file_name().unwrap_or_default().to_string_lossy()))
}

/// Loads and activates a theme by file stem. On error the current theme stays and the error
/// is returned (for the status line).
pub fn activate(data_root: &Path, name: &str) -> Result<(), String> {
    let theme = load_file(&theme_path(data_root, name))?;
    set(theme);
    Ok(())
}

// ---------------------------------------------------------------- applying it to egui

/// egui visuals built from the current theme.
pub fn visuals() -> Visuals {
    let t = th();
    let mut v = if t.dark { Visuals::dark() } else { Visuals::light() };
    let s = &t.surface;
    v.panel_fill = s.page;
    v.window_fill = s.window;
    v.faint_bg_color = s.card;
    v.extreme_bg_color = s.input;
    v.code_bg_color = s.card;
    v.window_stroke = Stroke::new(1.0, s.card_border);
    v.window_shadow = Shadow { offset: [0, 8], blur: 24, spread: 0, color: s.shadow };
    v.popup_shadow = Shadow { offset: [0, 4], blur: 12, spread: 0, color: s.shadow };
    v.window_corner_radius = CornerRadius::same(t.shape.card_radius.saturating_sub(2));

    let wc = &t.widget;
    let strong = if t.dark { Color32::WHITE } else { Color32::BLACK };
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = s.page;
    w.noninteractive.weak_bg_fill = s.page;
    w.noninteractive.bg_stroke = Stroke::new(1.0, s.card_border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, t.text.primary);
    // buttons need to read as buttons on top of cards, so they get a fill and a hairline
    w.inactive.bg_fill = wc.fill;
    w.inactive.weak_bg_fill = wc.fill;
    w.inactive.bg_stroke = Stroke::new(1.0, wc.border);
    w.inactive.fg_stroke = Stroke::new(1.0, t.text.primary);
    w.hovered.bg_fill = wc.fill_hover;
    w.hovered.weak_bg_fill = wc.fill_hover;
    w.hovered.bg_stroke = Stroke::new(1.0, wc.border_hover);
    w.hovered.fg_stroke = Stroke::new(1.5, strong);
    w.active.bg_fill = wc.fill_active;
    w.active.weak_bg_fill = wc.fill_active;
    w.active.fg_stroke = Stroke::new(2.0, strong);
    w.open = w.hovered;
    for ws in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        ws.corner_radius = CornerRadius::same(t.shape.widget_radius);
    }
    v.selection.bg_fill = wc.selection;
    v.selection.stroke = Stroke::new(1.0, wc.selection_border);
    v.hyperlink_color = wc.link;
    v
}

/// Applies the current theme to egui (call on startup and when `version()` changes).
pub fn apply_visuals(ctx: &egui::Context) {
    let t = th();
    let (pref, theme) = if t.dark {
        (egui::ThemePreference::Dark, egui::Theme::Dark)
    } else {
        (egui::ThemePreference::Light, egui::Theme::Light)
    };
    ctx.options_mut(|o| o.theme_preference = pref);
    ctx.set_visuals_of(theme, visuals());
}

/// A lifted card: card fill, hairline border, soft shadow. Color goes on the content (numbers,
/// icons, stars), not on the card itself.
pub fn card() -> egui::Frame {
    let t = th();
    egui::Frame::default()
        .fill(t.surface.card)
        .stroke(Stroke::new(1.0, t.surface.card_border))
        .inner_margin(Margin::same(14))
        .corner_radius(CornerRadius::same(t.shape.card_radius))
        .shadow(Shadow { offset: [0, 3], blur: 12, spread: 0, color: t.surface.shadow })
}

/// A sparse field of faint stars across `rect`, from the theme's `sky`. Positions come from a
/// fixed sequence (scaled to the rect), so it's static; never animated.
pub fn star_field(p: &Painter, rect: Rect) {
    let sky = th().sky.clone();
    let count = ((rect.width() * rect.height()) * sky.density / 1000.0).max(0.0) as usize;
    let [sr, sg, sb, sa] = sky.star.to_srgba_unmultiplied();
    let mut seed: u32 = 0x9E37_79B9;
    let mut next = || {
        // xorshift: deterministic, so the sky doesn't change between frames
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed as f32) / (u32::MAX as f32)
    };
    for _ in 0..count {
        let (x, y, size, bright) = (next(), next(), next(), next());
        let pos = Pos2::new(rect.left() + x * rect.width(), rect.top() + y * rect.height());
        let r = 0.5 + size * size * 1.1;
        let a = (sa as f32 * (0.3 + 0.7 * bright)) as u8;
        p.circle_filled(pos, r, Color32::from_rgba_unmultiplied(sr, sg, sb, a));
    }
}

// ---------------------------------------------------------------- fonts

/// Font choices offered in Settings: (setting value, label).
pub const FONTS: &[(&str, &str)] = &[
    ("inter", "Inter (default)"),
    ("nunito", "Nunito (rounder, friendlier)"),
    ("segoe", "Segoe UI (Windows system font)"),
    ("egui", "Classic (egui's built-in font)"),
];

const INTER_REGULAR: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.ttf");
const INTER_SEMIBOLD: &[u8] = include_bytes!("../../assets/fonts/Inter-SemiBold.ttf");
const NUNITO_REGULAR: &[u8] = include_bytes!("../../assets/fonts/Nunito-Regular.ttf");
const NUNITO_BOLD: &[u8] = include_bytes!("../../assets/fonts/Nunito-Bold.ttf");

/// Font family for big numbers and headings (the bold cut of the chosen font).
pub fn bold_family() -> FontFamily {
    FontFamily::Name("bold".into())
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, bold_family())
}

/// Installs the chosen font as the main proportional font. egui's own fonts stay behind it as
/// fallbacks, so emoji and symbols still render.
pub fn apply_fonts(ctx: &egui::Context, choice: &str) {
    let mut fonts = FontDefinitions::default();
    let windows_font = |file: &str| std::fs::read(format!(r"C:\Windows\Fonts\{file}")).ok();
    let pair: Option<(FontData, FontData)> = match choice {
        "egui" => None,
        "nunito" => Some((FontData::from_static(NUNITO_REGULAR), FontData::from_static(NUNITO_BOLD))),
        "segoe" => match (windows_font("segoeui.ttf"), windows_font("seguisb.ttf")) {
            (Some(r), Some(b)) => Some((FontData::from_owned(r), FontData::from_owned(b))),
            _ => Some((FontData::from_static(INTER_REGULAR), FontData::from_static(INTER_SEMIBOLD))),
        },
        _ => Some((FontData::from_static(INTER_REGULAR), FontData::from_static(INTER_SEMIBOLD))),
    };
    let fallbacks = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    match pair {
        Some((regular, bold)) => {
            fonts.font_data.insert("ui-regular".into(), Arc::new(regular));
            fonts.font_data.insert("ui-bold".into(), Arc::new(bold));
            let mut prop = vec!["ui-regular".to_string()];
            prop.extend(fallbacks.iter().cloned());
            fonts.families.insert(FontFamily::Proportional, prop);
            let mut b = vec!["ui-bold".to_string(), "ui-regular".to_string()];
            b.extend(fallbacks);
            fonts.families.insert(bold_family(), b);
        }
        None => {
            fonts.families.insert(bold_family(), fallbacks);
        }
    }
    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_theme_files_fill_in_defaults() {
        let t: Theme = serde_json::from_str(r##"{"name":"Mine","data":{"calories":"#FF0000"},"kinds":{"study":"#00FF00"}}"##).unwrap();
        let t = t.complete();
        assert_eq!(t.data.calories, Color32::from_rgb(255, 0, 0));
        assert_eq!(t.data.weight, DataColors::default().weight); // untouched default
        assert_eq!(t.kind("study"), Color32::from_rgb(0, 255, 0)); // custom kind
        assert_eq!(t.kind("workout"), default_kinds()["workout"]); // filled in
        assert_eq!(t.kind("unknown"), default_kinds()["other"]);
        assert!(serde_json::from_str::<Theme>(r#"{"text":{"primary":"red"}}"#).is_err());
        // round trip
        let json = serde_json::to_string(&Theme::default()).unwrap();
        let back: Theme = serde_json::from_str(&json).unwrap();
        assert_eq!(back.surface.page, Theme::default().surface.page);
        assert_eq!(hex::to_string(Color32::from_black_alpha(60)), "#0000003C");
    }

    #[test]
    fn old_theme_files_gain_new_keys_without_changing_values() {
        let dir = std::env::temp_dir().join(format!("mk-theme-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.json");
        std::fs::write(&path, r##"{"name":"Old","data":{"calories":"#FF0000"},"kinds":{"study":"#00FF00"}}"##).unwrap();
        add_missing_keys(&path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["data"]["calories"], "#FF0000"); // kept
        assert!(v["data"]["today"].is_string()); // added
        assert!(v["motion"]["speed"].is_number()); // added section
        assert!(v["kinds"].get("workout").is_none()); // own kind list left alone
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// WCAG contrast ratio between two colors.
    fn contrast(a: Color32, b: Color32) -> f32 {
        let lum = |c: Color32| {
            let ch = |v: u8| {
                let v = v as f32 / 255.0;
                if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
        };
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn builtin_themes_are_readable() {
        let themes = builtin();
        assert_eq!(themes.len(), 12);
        for (stem, t) in themes {
            for bg in [t.surface.page, t.surface.card] {
                assert!(contrast(t.text.primary, bg) >= 7.0, "{stem}: primary text {:.1}", contrast(t.text.primary, bg));
                assert!(contrast(t.text.muted, bg) >= 3.0, "{stem}: muted text {:.1}", contrast(t.text.muted, bg));
            }
            // the theme survives a round trip through its JSON file
            let back: Theme = serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
            assert_eq!(back.name, t.name);
        }
    }
}
