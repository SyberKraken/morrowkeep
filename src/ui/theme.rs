//! Look and feel: fonts (switchable in Settings), the night-sky palette, card styling and the
//! faint star field behind the Habits constellation.

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Painter, Pos2, Rect, Shadow, Stroke, Visuals};
use std::sync::Arc;

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

// Night-sky palette: deep navy surfaces, slightly lifted cards, cool text.
pub const PAGE: Color32 = Color32::from_rgb(14, 18, 30);
pub const CARD: Color32 = Color32::from_rgb(22, 28, 44);
pub const CARD_BORDER: Color32 = Color32::from_rgb(38, 46, 68);
const INPUT: Color32 = Color32::from_rgb(10, 13, 22);
const TEXT: Color32 = Color32::from_rgb(220, 226, 238);
const ACCENT: Color32 = Color32::from_rgb(70, 92, 170);

pub fn night_visuals() -> Visuals {
    let mut v = Visuals::dark();
    v.panel_fill = PAGE;
    v.window_fill = Color32::from_rgb(20, 25, 40);
    v.faint_bg_color = CARD;
    v.extreme_bg_color = INPUT;
    v.code_bg_color = CARD;
    v.window_stroke = Stroke::new(1.0, CARD_BORDER);
    v.window_shadow = Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(110) };
    v.popup_shadow = Shadow { offset: [0, 4], blur: 12, spread: 0, color: Color32::from_black_alpha(90) };
    v.window_corner_radius = CornerRadius::same(10);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = PAGE;
    w.noninteractive.weak_bg_fill = PAGE;
    w.noninteractive.bg_stroke = Stroke::new(1.0, CARD_BORDER);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    // buttons need to read as buttons on top of cards, so they get a fill and a hairline
    w.inactive.bg_fill = Color32::from_rgb(40, 50, 74);
    w.inactive.weak_bg_fill = Color32::from_rgb(38, 47, 70);
    w.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(54, 65, 94));
    w.inactive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(200, 208, 224));
    w.hovered.bg_fill = Color32::from_rgb(52, 64, 94);
    w.hovered.weak_bg_fill = Color32::from_rgb(50, 62, 92);
    w.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(72, 86, 122));
    w.hovered.fg_stroke = Stroke::new(1.5, Color32::WHITE);
    w.active.bg_fill = Color32::from_rgb(62, 76, 112);
    w.active.weak_bg_fill = Color32::from_rgb(60, 73, 108);
    w.active.fg_stroke = Stroke::new(2.0, Color32::WHITE);
    w.open = w.hovered;
    for ws in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        ws.corner_radius = CornerRadius::same(6);
    }
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = Stroke::new(1.0, Color32::from_rgb(170, 188, 255));
    v.hyperlink_color = Color32::from_rgb(140, 170, 255);
    v
}

/// A lifted card: card fill, hairline border, soft shadow. Color goes on the content (numbers,
/// icons, stars), not on the card itself.
pub fn card() -> egui::Frame {
    egui::Frame::default()
        .fill(CARD)
        .stroke(Stroke::new(1.0, CARD_BORDER))
        .inner_margin(Margin::same(14))
        .corner_radius(CornerRadius::same(12))
        .shadow(Shadow { offset: [0, 3], blur: 12, spread: 0, color: Color32::from_black_alpha(60) })
}

/// A sparse field of faint stars across `rect`. Positions come from a fixed sequence (scaled to
/// the rect), so it's static: drawn once per frame like any other shape, never animated.
pub fn star_field(p: &Painter, rect: Rect, density: f32) {
    let count = ((rect.width() * rect.height()) * density / 1000.0) as usize;
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
        let a = (40.0 + bright * 90.0) as u8;
        p.circle_filled(pos, r, Color32::from_rgba_unmultiplied(205, 215, 255, a));
    }
}
