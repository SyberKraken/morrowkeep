//! Built-in themes inspired by popular color schemes. Each is generated from the scheme's
//! own palette (surfaces, text, accent and seven named colors), so every theme uses the same
//! rules for which color goes where. They're written to the themes folder like the others and
//! can be copied and edited there.

use super::theme::{Charts, DataColors, Motion, Shape, Sky, StatusColors, Surfaces, TextColors, Theme, WidgetColors};
use eframe::egui::Color32;

struct Palette {
    name: &'static str,
    about: &'static str,
    dark: bool,
    /// Main background, cards/dialogs, and raised controls.
    page: &'static str,
    card: &'static str,
    raised: &'static str,
    fg: &'static str,
    muted: &'static str,
    /// Selection, links and focus.
    accent: &'static str,
    /// Text drawn on colored fills (chips, filled tiles).
    on_color: &'static str,
    red: &'static str,
    orange: &'static str,
    yellow: &'static str,
    green: &'static str,
    cyan: &'static str,
    blue: &'static str,
    purple: &'static str,
}

fn h(s: &str) -> Color32 {
    crate::categories::parse_hex(s).unwrap_or(Color32::GRAY)
}

/// `a` moved toward `b` by `t` (0..1).
fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

fn with_alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

fn build(p: &Palette) -> Theme {
    let (page, card, raised, fg, muted, accent) = (h(p.page), h(p.card), h(p.raised), h(p.fg), h(p.muted), h(p.accent));
    let (red, orange, yellow, green, cyan, blue, purple) = (h(p.red), h(p.orange), h(p.yellow), h(p.green), h(p.cyan), h(p.blue), h(p.purple));
    let dark = p.dark;
    let border = mix(raised, fg, if dark { 0.10 } else { 0.12 });
    Theme {
        name: p.name.into(),
        about: p.about.into(),
        dark,
        surface: Surfaces {
            page,
            window: card,
            card,
            card_border: mix(card, fg, if dark { 0.10 } else { 0.13 }),
            input: if dark { mix(page, Color32::BLACK, 0.25) } else { mix(card, Color32::WHITE, 0.5) },
            weekend: mix(page, card, 0.5),
            shadow: Color32::from_black_alpha(if dark { 60 } else { 22 }),
        },
        text: TextColors { primary: fg, muted, on_color: h(p.on_color) },
        widget: WidgetColors {
            fill: raised,
            fill_hover: mix(raised, fg, 0.08),
            fill_active: mix(raised, fg, 0.16),
            border,
            border_hover: mix(border, fg, 0.25),
            selection: mix(card, accent, if dark { 0.45 } else { 0.28 }),
            selection_border: accent,
            link: accent,
        },
        status: StatusColors { success: green, danger: red, warning: orange, danger_background: mix(page, red, if dark { 0.3 } else { 0.15 }) },
        kinds: [("workout", green), ("meal", orange), ("chore", yellow), ("task", blue), ("event", cyan), ("habit", purple), ("other", muted)]
            .into_iter()
            .map(|(k, c)| (k.to_string(), c))
            .collect(),
        data: DataColors {
            brand: green,
            today: green,
            calories: orange,
            weight: blue,
            claude: mix(orange, red, 0.35),
            google: muted,
            not_logged: muted,
            gold: yellow,
            now_line: red,
            chart_limit_line: muted,
            goal: cyan,
            trend: mix(blue, fg, 0.4),
            journal: mix(purple, red, 0.45),
        },
        sky: if dark { Sky { star: with_alpha(fg, 110), density: 0.7 } } else { Sky { star: with_alpha(muted, 80), density: 0.5 } },
        shape: Shape::default(),
        charts: Charts::default(),
        motion: Motion::default(),
    }
}

const PALETTES: &[(&str, Palette)] = &[
    ("dracula", Palette {
        name: "Dracula",
        about: "Inspired by the Dracula color scheme: purple-grey night with vivid pastels.",
        dark: true,
        page: "#21222C", card: "#282A36", raised: "#3A3D4E", fg: "#F8F8F2", muted: "#8A93C2", accent: "#BD93F9", on_color: "#21222C",
        red: "#FF5555", orange: "#FFB86C", yellow: "#F1FA8C", green: "#50FA7B", cyan: "#8BE9FD", blue: "#BD93F9", purple: "#FF79C6",
    }),
    ("nord", Palette {
        name: "Nord",
        about: "Inspired by Nord: arctic blue-grey with frosty accents and muted aurora colors.",
        dark: true,
        page: "#2E3440", card: "#3B4252", raised: "#434C5E", fg: "#ECEFF4", muted: "#A0AABD", accent: "#88C0D0", on_color: "#2E3440",
        red: "#BF616A", orange: "#D08770", yellow: "#EBCB8B", green: "#A3BE8C", cyan: "#8FBCBB", blue: "#81A1C1", purple: "#B48EAD",
    }),
    ("solarized-dark", Palette {
        name: "Solarized Dark",
        about: "Inspired by Solarized (dark): deep teal base with Solarized's balanced accents.",
        dark: true,
        page: "#002B36", card: "#073642", raised: "#0F4553", fg: "#EEE8D5", muted: "#839496", accent: "#268BD2", on_color: "#FDF6E3",
        red: "#DC322F", orange: "#CB4B16", yellow: "#B58900", green: "#859900", cyan: "#2AA198", blue: "#268BD2", purple: "#6C71C4",
    }),
    ("solarized-light", Palette {
        name: "Solarized Light",
        about: "Inspired by Solarized (light): warm paper tones with Solarized's accents.",
        dark: false,
        page: "#EEE8D5", card: "#FDF6E3", raised: "#E6DFCA", fg: "#073642", muted: "#657B83", accent: "#268BD2", on_color: "#FDF6E3",
        red: "#DC322F", orange: "#CB4B16", yellow: "#B58900", green: "#859900", cyan: "#2AA198", blue: "#268BD2", purple: "#6C71C4",
    }),
    ("gruvbox-dark", Palette {
        name: "Gruvbox Dark",
        about: "Inspired by Gruvbox: retro, warm and earthy, with soft contrast.",
        dark: true,
        page: "#1D2021", card: "#282828", raised: "#3C3836", fg: "#EBDBB2", muted: "#A89984", accent: "#FABD2F", on_color: "#1D2021",
        red: "#FB4934", orange: "#FE8019", yellow: "#FABD2F", green: "#B8BB26", cyan: "#8EC07C", blue: "#83A598", purple: "#D3869B",
    }),
    ("catppuccin-mocha", Palette {
        name: "Catppuccin Mocha",
        about: "Inspired by Catppuccin Mocha: cozy dark base with soothing pastels.",
        dark: true,
        page: "#181825", card: "#1E1E2E", raised: "#313244", fg: "#CDD6F4", muted: "#A6ADC8", accent: "#CBA6F7", on_color: "#1E1E2E",
        red: "#F38BA8", orange: "#FAB387", yellow: "#F9E2AF", green: "#A6E3A1", cyan: "#94E2D5", blue: "#89B4FA", purple: "#CBA6F7",
    }),
    ("catppuccin-latte", Palette {
        name: "Catppuccin Latte",
        about: "Inspired by Catppuccin Latte: soft light base with rich accents.",
        dark: false,
        page: "#E6E9EF", card: "#EFF1F5", raised: "#DCE0E8", fg: "#3B3E56", muted: "#62657C", accent: "#8839EF", on_color: "#FFFFFF",
        red: "#D20F39", orange: "#FE640B", yellow: "#DF8E1D", green: "#40A02B", cyan: "#179299", blue: "#1E66F5", purple: "#8839EF",
    }),
    ("tokyo-night", Palette {
        name: "Tokyo Night",
        about: "Inspired by Tokyo Night: city lights on a deep indigo night.",
        dark: true,
        page: "#16161E", card: "#1A1B26", raised: "#292E42", fg: "#C0CAF5", muted: "#7C84AC", accent: "#7AA2F7", on_color: "#16161E",
        red: "#F7768E", orange: "#FF9E64", yellow: "#E0AF68", green: "#9ECE6A", cyan: "#7DCFFF", blue: "#7AA2F7", purple: "#BB9AF7",
    }),
    ("one-dark", Palette {
        name: "One Dark",
        about: "Inspired by Atom's One Dark: neutral charcoal with clear, friendly accents.",
        dark: true,
        page: "#21252B", card: "#282C34", raised: "#363B46", fg: "#D2D6DD", muted: "#868C97", accent: "#61AFEF", on_color: "#21252B",
        red: "#E06C75", orange: "#D19A66", yellow: "#E5C07B", green: "#98C379", cyan: "#56B6C2", blue: "#61AFEF", purple: "#C678DD",
    }),
    ("rose-pine", Palette {
        name: "Rosé Pine",
        about: "Inspired by Rosé Pine: muted plum night with rose, gold and foam.",
        dark: true,
        page: "#191724", card: "#1F1D2E", raised: "#26233A", fg: "#E0DEF4", muted: "#908CAA", accent: "#C4A7E7", on_color: "#191724",
        red: "#EB6F92", orange: "#EA9A97", yellow: "#F6C177", green: "#9CCFD8", cyan: "#56949F", blue: "#3E8FB0", purple: "#C4A7E7",
    }),
];

/// The palette themes: (file stem, theme).
pub(super) fn all() -> Vec<(&'static str, Theme)> {
    PALETTES.iter().map(|(stem, p)| (*stem, build(p))).collect()
}
