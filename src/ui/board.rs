//! The "quest board": today's anytime tasks as a grid of large tiles.
//!
//! - The grid is fixed (tasks keep their order) so you build muscle memory; 3–5 columns.
//! - Color shows state: pending tiles are dark with a colored outline and icon; completed
//!   tiles fill with their color and get a checkmark, so the board fills up as you work.
//! - Completing a tile flips it over, it stays a moment, then moves to the completion tray
//!   (click a tray chip to undo). The last tile is always "add a task".
//! - A constellation in the header lights a star per finished task, plus two fixed stars
//!   for "calories logged today" and "weight logged today".

use super::{weight_color, kind_color, Action, calorie_color, google_color};
use crate::model::Item;
use crate::store::Data;
use chrono::NaiveDate;
use eframe::egui::{
    self, text::LayoutJob, Align2, Color32, CornerRadius, FontId, Id, Pos2, Rect, RichText, Sense, Stroke,
    StrokeKind, TextFormat, Ui, Vec2,
};
use std::collections::HashMap;
use std::time::Instant;

/// Seconds the flip animation takes (before the theme's motion speed).
const FLIP: f32 = 0.4;
/// Seconds a star takes to ignite, and a tile's sparkle burst lasts.
const IGNITE: f32 = 0.8;
const SPARKLE: f32 = 0.75;
/// Seconds a completed tile stays on the board before moving to the tray.
const LINGER: f32 = 2.2;
const GAP: f32 = 10.0;
const MIN_TILE_W: f32 = 170.0;

/// Tile height for a given tile width: tiles keep their shape as the window resizes.
fn tile_height(tile_w: f32) -> f32 {
    (tile_w * 0.55).clamp(84.0, 190.0)
}

/// Per-tile animation state, kept by the app between frames.
#[derive(Default)]
pub struct BoardState {
    /// Item id → when it was last toggled from the board or tray.
    toggled: HashMap<String, Instant>,
    /// Tile order last shown, and where the grid was: while the pointer is over the board the
    /// layout is frozen, so tiles never move under the cursor.
    layout: Vec<String>,
    grid: Option<Rect>,
    /// Star key ("kcal", "kg" or item id) → lit last frame, and when stars lit up.
    star_lit: HashMap<String, bool>,
    ignited: HashMap<String, Instant>,
}

/// A category's icon (from Settings > Categories).
pub fn kind_icon(kind: &str) -> String {
    crate::categories::icon(kind)
}

fn wrapped(text: &str, size: f32, color: Color32, width: f32, rows: usize) -> LayoutJob {
    let mut job = LayoutJob::single_section(text.to_string(), TextFormat::simple(FontId::proportional(size), color));
    job.wrap.max_width = width.max(1.0);
    job.wrap.max_rows = rows;
    job.wrap.break_anywhere = false;
    job.wrap.overflow_character = Some('…');
    job
}

/// Draws one task tile. `flip` is the animation progress 0..1 (1 = settled); during the first
/// half the tile still shows its previous state, then turns over to the new one.
fn draw_tile(ui: &Ui, rect: Rect, item: &Item, done_now: bool, flip: f32, hovered: bool) {
    let visuals = ui.visuals();
    // everything inside scales with the tile (1.0 at the smallest size)
    let k = (rect.height() / 96.0).clamp(0.9, 1.8);
    let color = kind_color(&item.kind);
    let showing_done = if flip < 0.5 { !done_now } else { done_now };
    // horizontal squash for the flip; slight lift on hover
    let sx = if flip < 1.0 { (std::f32::consts::PI * flip).cos().abs().max(0.03) } else { 1.0 };
    let mut r = Rect::from_center_size(rect.center(), Vec2::new(rect.width() * sx, rect.height()));
    if hovered && flip >= 1.0 {
        r = r.translate(Vec2::new(0.0, -2.0));
    }
    let painter = ui.painter();
    let radius = CornerRadius::same(12);
    if showing_done {
        painter.rect_filled(r, radius, color);
    } else {
        // pending: a calm card; the kind color shows in the icon and, on hover, the border
        let fill = if hovered { super::theme::th().surface.card.lerp_to_gamma(color, 0.10) } else { super::theme::th().surface.card };
        painter.rect_filled(r, radius, fill);
        let border = if hovered { Stroke::new(1.5, color.linear_multiply(0.9)) } else { Stroke::new(1.0, super::theme::th().surface.card_border) };
        painter.rect_stroke(r, radius, border, StrokeKind::Inside);
    }
    if sx < 0.35 {
        return; // too thin mid-flip to show content
    }
    let clip = painter.with_clip_rect(r);
    let ink = if showing_done { super::theme::th().text.on_color } else { visuals.text_color() };
    let pad = 12.0 * k;
    // icon
    let icon_color = if showing_done { super::theme::th().text.on_color } else { color };
    clip.text(r.left_top() + Vec2::new(pad, pad - 2.0), Align2::LEFT_TOP, kind_icon(&item.kind), FontId::proportional(24.0 * k), icon_color);
    // checkmark / time and repeat hint, top-right
    if showing_done {
        clip.text(r.right_top() + Vec2::new(-pad, pad - 4.0), Align2::RIGHT_TOP, "✔", FontId::proportional(28.0 * k), super::theme::th().text.on_color);
    } else {
        let repeat = item.repeat.is_some() && item.kind != "habit";
        let hint = match (item.time, repeat) {
            (Some(t), true) => format!("🔁 {}", t.format("%H:%M")),
            (Some(t), false) => t.format("%H:%M").to_string(),
            (None, true) => "🔁".into(),
            (None, false) => String::new(),
        };
        if !hint.is_empty() {
            let (size, ink) = if item.time.is_some() { (14.0 * k, visuals.text_color()) } else { (13.0 * k, visuals.weak_text_color()) };
            clip.text(r.right_top() + Vec2::new(-pad, pad), Align2::RIGHT_TOP, hint, FontId::proportional(size), ink);
        }
    }
    // title, bottom-left, up to 2 lines
    let galley = clip.layout_job(wrapped(&item.title, 16.0 * k, ink, r.width() - 2.0 * pad, 2));
    let pos = Pos2::new(r.left() + pad, r.bottom() - pad - galley.size().y);
    clip.galley(pos, galley, ink);
}

/// A burst of small sparks flying out from a tile's center (`t` = 0..1 progress).
fn sparkle(ui: &Ui, rect: Rect, color: Color32, t: f32) {
    let p = ui.painter();
    let ease = 1.0 - (1.0 - t).powi(3); // fast out, slow settle
    let fade = 1.0 - t;
    let reach = rect.width().min(rect.height()) * 0.75;
    let light = super::theme::th().text.on_color;
    for i in 0..14 {
        // fixed, slightly irregular directions and distances
        let a = i as f32 / 14.0 * std::f32::consts::TAU + (i * 7 % 5) as f32 * 0.09;
        let dist = reach * (0.55 + (i * 13 % 7) as f32 / 14.0) * ease;
        let pos = rect.center() + Vec2::new(a.cos(), a.sin()) * dist;
        let c = if i % 3 == 0 { light } else { color };
        let r = 1.2 + 2.6 * fade * if i % 2 == 0 { 1.0 } else { 0.7 };
        if i % 4 == 0 {
            // a few tiny 4-point sparks
            let s = r * 1.8;
            p.line_segment([pos - Vec2::new(s, 0.0), pos + Vec2::new(s, 0.0)], Stroke::new(1.2, c.linear_multiply(fade)));
            p.line_segment([pos - Vec2::new(0.0, s), pos + Vec2::new(0.0, s)], Stroke::new(1.2, c.linear_multiply(fade)));
        } else {
            p.circle_filled(pos, r, c.linear_multiply(fade));
        }
    }
}

fn draw_add_tile(ui: &Ui, rect: Rect, hovered: bool) {
    let visuals = ui.visuals();
    let painter = ui.painter();
    let c = if hovered { visuals.strong_text_color() } else { visuals.weak_text_color() };
    // dashed outline
    let r = rect.shrink(1.0);
    let dash = |a: Pos2, b: Pos2| {
        let len = a.distance(b);
        let dir = (b - a) / len;
        let mut t = 0.0;
        while t < len {
            let e = (t + 8.0).min(len);
            painter.line_segment([a + dir * t, a + dir * e], Stroke::new(1.5, c));
            t += 14.0;
        }
    };
    let k = 12.0;
    dash(r.left_top() + Vec2::new(k, 0.0), r.right_top() - Vec2::new(k, 0.0));
    dash(r.left_bottom() + Vec2::new(k, 0.0), r.right_bottom() - Vec2::new(k, 0.0));
    dash(r.left_top() + Vec2::new(0.0, k), r.left_bottom() - Vec2::new(0.0, k));
    dash(r.right_top() + Vec2::new(0.0, k), r.right_bottom() - Vec2::new(0.0, k));
    if hovered {
        painter.rect_filled(r, CornerRadius::same(12), visuals.faint_bg_color);
    }
    let s = (rect.height() / 96.0).clamp(0.9, 1.8);
    painter.text(rect.center() - Vec2::new(0.0, 10.0 * s), Align2::CENTER_CENTER, "➕", FontId::proportional(24.0 * s), c);
    painter.text(rect.center() + Vec2::new(0.0, 18.0 * s), Align2::CENTER_CENTER, "Add task", FontId::proportional(14.0 * s), c);
}

/// The page's progress display: one star per task, lit in the task's color when done, with
/// lines joining neighbouring lit stars. Spreads across the available width.
fn constellation(ui: &mut Ui, data: &Data, ids: &[usize], day: NaiveDate, state: &mut BoardState) {
    // Two fixed trackers first (always in the same place), then one star per task.
    struct Star {
        lit: bool,
        color: Color32,
        label: Option<&'static str>,
        key: String,
    }
    let kcal_logged = data.calories_on(day) > 0;
    let weight_logged = data.weight_on(day).is_some();
    let mut stars = vec![
        Star { lit: kcal_logged, color: calorie_color(), label: Some("kcal"), key: "kcal".into() },
        Star { lit: weight_logged, color: weight_color(), label: Some("kg"), key: "kg".into() },
        Star { lit: data.journal_on(day) > 0, color: super::journal::journal_color(), label: Some("journal"), key: "journal".into() },
    ];
    stars.extend(ids.iter().map(|i| {
        let item = &data.items[*i];
        Star { lit: item.is_done_on(day), color: kind_color(&item.kind), label: None, key: item.id.clone() }
    }));
    let n = stars.len();
    let done = stars.iter().filter(|s| s.lit).count();

    // A star that was unlit last frame and is lit now ignites. Stars seen for the first time
    // (app start, new task) just appear, so opening the app doesn't set everything off.
    let now = Instant::now();
    let ignite_secs = super::theme::anim(IGNITE);
    for s in &stars {
        if s.lit && state.star_lit.get(&s.key) == Some(&false) && ignite_secs > 0.0 {
            state.ignited.insert(s.key.clone(), now);
        }
    }
    state.star_lit = stars.iter().map(|s| (s.key.clone(), s.lit)).collect();
    state.ignited.retain(|_, t| now.duration_since(*t).as_secs_f32() < ignite_secs);
    // 0..1 progress of each star's ignition (1 = settled)
    let progress: Vec<f32> = stars
        .iter()
        .map(|s| state.ignited.get(&s.key).map_or(1.0, |t| (now.duration_since(*t).as_secs_f32() / ignite_secs).min(1.0)))
        .collect();
    if progress.iter().any(|p| *p < 1.0) {
        ui.ctx().request_repaint();
    }
    let tasks_done = ids.iter().filter(|i| data.items[**i].is_done_on(day)).count();

    let width = ui.available_width();
    let height = 64.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let all_done = done == n;
    // trophy space on the right when everything is done
    let right_pad = if all_done { 190.0 } else { 20.0 };
    let step = ((width - 20.0 - right_pad) / n as f32).clamp(22.0, 64.0);
    // faint static sky behind the stars
    super::theme::star_field(ui.painter(), rect);
    let painter = ui.painter();
    let dim = ui.visuals().weak_text_color().linear_multiply(0.7);
    let cy = rect.center().y - 4.0; // leave room for the tracker labels underneath
    let pts: Vec<Pos2> = (0..n)
        .map(|i| {
            // zig-zag so it reads as a constellation rather than a progress bar
            Pos2::new(rect.left() + 14.0 + step * i as f32, cy + if i % 2 == 0 { -9.0 } else { 9.0 })
        })
        .collect();
    // faint path through all stars, brighter segments between lit neighbours
    for i in 1..n {
        let (a, b) = (&stars[i - 1], &stars[i]);
        painter.line_segment([pts[i - 1], pts[i]], Stroke::new(1.0, dim.linear_multiply(0.35)));
        if a.lit && b.lit {
            // a newly joined segment draws itself from the star that was already lit
            let grow = ((progress[i - 1].min(progress[i])) / 0.55).min(1.0);
            let (from, to) = if progress[i] < progress[i - 1] { (pts[i - 1], pts[i]) } else { (pts[i], pts[i - 1]) };
            let end = from + (to - from) * grow;
            painter.line_segment([from, end], Stroke::new(2.5, b.color.linear_multiply(0.8)));
        }
    }
    for (k, (p, star)) in pts.iter().zip(&stars).enumerate() {
        if star.lit {
            let t = progress[k];
            // pop: swells and settles; plus an expanding ring that fades
            let pop = 1.0 + 0.8 * (std::f32::consts::PI * t).sin() * (1.0 - t);
            if t < 1.0 {
                painter.circle_stroke(*p, 7.0 + 26.0 * t, Stroke::new(2.0 * (1.0 - t) + 0.5, star.color.linear_multiply(0.7 * (1.0 - t))));
            }
            painter.circle_filled(*p, 15.0 * pop, star.color.linear_multiply(0.12)); // outer glow
            painter.circle_filled(*p, 10.0 * pop, star.color.linear_multiply(0.3));
            painter.circle_filled(*p, 6.5 * pop, star.color);
        } else {
            painter.circle_stroke(*p, 5.5, Stroke::new(1.6, dim));
        }
        if let Some(label) = star.label {
            // identity in text, not color alone
            let ink = if star.lit { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
            painter.text(Pos2::new(p.x, rect.bottom() - 2.0), Align2::CENTER_BOTTOM, label, FontId::proportional(11.0), ink);
        }
    }
    if ids.is_empty() {
        // empty state: the constellation continues as dotted, unlit stars waiting to be added
        let start = Pos2::new(pts[n - 1].x + step, cy + if n % 2 == 0 { -9.0 } else { 9.0 });
        super::streaks::ghost_stars(painter, start, step, 4, 9.0);
        let x = start.x + step * 3.0 + 26.0;
        let t = super::theme::th();
        painter.text(Pos2::new(x, cy - 9.0), Align2::LEFT_CENTER, "Start your constellation", super::theme::bold(15.0), t.text.primary);
        painter.text(Pos2::new(x, cy + 10.0), Align2::LEFT_CENTER, "Add a habit or task for today. Each one becomes a star.", FontId::proportional(12.5), t.text.muted);
    } else if all_done {
        painter.text(
            Pos2::new(pts[n - 1].x + 30.0, cy),
            Align2::LEFT_CENTER,
            "🏆 All done today!",
            FontId::proportional(20.0),
            super::theme::th().status.success,
        );
    }
    let mark = |b: bool| if b { "✔" } else { "not yet" };
    resp.on_hover_text(format!(
        "{done} of {n} done today
Calories logged: {}
Weight logged: {}
Tasks: {tasks_done} of {}",
        mark(kcal_logged),
        mark(weight_logged),
        ids.len()
    ));
}

/// The Habits page board. `ids`: today's untimed item indices in display
/// order; `all_day`: titles of all-day Google events (shown as read-only chips).
pub fn anytime_board(ui: &mut Ui, data: &Data, day: NaiveDate, ids: &[usize], all_day: &[String], state: &mut BoardState, actions: &mut Vec<Action>) {
    let now = Instant::now();
    // forget finished animations, then snapshot how long ago each tile was toggled
    state.toggled.retain(|_, t| now.duration_since(*t).as_secs_f32() < LINGER + FLIP);
    let ages: HashMap<String, f32> = state.toggled.iter().map(|(k, t)| (k.clone(), now.duration_since(*t).as_secs_f32())).collect();
    let age = |id: &str| ages.get(id).copied();
    let mut toggled_now: Vec<String> = Vec::new();


    // Header: the constellation is the progress display
    ui.add_space(6.0);
    constellation(ui, data, ids, day, state);
    ui.add_space(8.0);

    if !all_day.is_empty() {
        ui.horizontal_wrapped(|ui| {
            for t in all_day {
                ui.label(RichText::new(format!("📅 {t}")).color(google_color()));
            }
        });
        ui.add_space(4.0);
    }

    // Tiles on the board: pending ones, plus completed ones still lingering after a toggle.
    let pointer_on_board = state
        .grid
        .zip(ui.ctx().pointer_hover_pos())
        .is_some_and(|(g, p)| g.expand(GAP).contains(p));
    let on_board: Vec<usize> = if pointer_on_board && !state.layout.is_empty() {
        // Frozen: keep the tiles shown last frame (finished ones stay, filled), plus any
        // newly added pending tasks at the end.
        let mut v: Vec<usize> = state
            .layout
            .iter()
            .filter_map(|id| ids.iter().copied().find(|i| data.items[*i].id == *id))
            .collect();
        for i in ids {
            if !v.contains(i) && !data.items[*i].is_done_on(day) {
                v.push(*i);
            }
        }
        v
    } else {
        ids.iter()
            .copied()
            .filter(|i| {
                let item = &data.items[*i];
                !item.is_done_on(day) || age(&item.id).is_some_and(|a| a < LINGER)
            })
            .collect()
    };
    state.layout = on_board.iter().map(|i| data.items[*i].id.clone()).collect();
    let tray: Vec<usize> = ids.iter().copied().filter(|i| !on_board.contains(i)).collect();

    let width = ui.available_width();
    let cols = (((width + GAP) / (MIN_TILE_W + GAP)).floor() as usize).clamp(3, 5);
    let tile_w = (width - GAP * (cols as f32 - 1.0)) / cols as f32;
    let tile_h = tile_height(tile_w);
    let slots = on_board.len() + 1; // + the "add" tile
    let rows = slots.div_ceil(cols);
    let (grid, _) = ui.allocate_exact_size(Vec2::new(width, rows as f32 * tile_h + (rows as f32 - 1.0) * GAP), Sense::hover());
    state.grid = Some(grid);
    let slot_rect = |i: usize| {
        let (r, c) = (i / cols, i % cols);
        Rect::from_min_size(
            Pos2::new(grid.left() + c as f32 * (tile_w + GAP), grid.top() + r as f32 * (tile_h + GAP)),
            Vec2::new(tile_w, tile_h),
        )
    };
    let mut animating = false;
    for (slot, idx) in on_board.iter().enumerate() {
        let item = &data.items[*idx];
        let rect = slot_rect(slot);
        let resp = ui
            .interact(rect, Id::new(("tile", &item.id)), Sense::click())
            .on_hover_text(if item.notes.is_empty() {
                "Click to complete · right-click to edit".to_string()
            } else {
                format!("{}\n\nClick to complete · right-click to edit", item.notes)
            });
        let done = item.is_done_on(day);
        let flip_secs = super::theme::anim(FLIP);
        let flip = match age(&item.id) {
            Some(a) if flip_secs > 0.0 => (a / flip_secs).min(1.0),
            _ => 1.0,
        };
        if age(&item.id).is_some() {
            animating = true;
        }
        draw_tile(ui, rect, item, done, flip, resp.hovered());
        if done {
            if let Some(a) = age(&item.id) {
                let start = flip_secs * 0.5;
                let len = super::theme::anim(SPARKLE);
                if len > 0.0 && a >= start && a < start + len {
                    sparkle(ui, rect, kind_color(&item.kind), (a - start) / len);
                }
            }
        }
        if resp.clicked() {
            actions.push(Action::Toggle(item.id.clone(), day));
            toggled_now.push(item.id.clone());
        } else if resp.secondary_clicked() {
            actions.push(Action::Edit(item.id.clone()));
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    let add_rect = slot_rect(on_board.len());
    let add = ui.interact(add_rect, Id::new("tile_add"), Sense::click()).on_hover_text("Add a task for today");
    draw_add_tile(ui, add_rect, add.hovered());
    if add.clicked() {
        actions.push(Action::NewAt(day, None));
    }
    if animating {
        ui.ctx().request_repaint();
    }

    // Completion tray
    if !tray.is_empty() {
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("DONE  {}", tray.len())).strong().size(12.5).color(ui.visuals().weak_text_color()));
            for idx in &tray {
                let item = &data.items[*idx];
                let c = kind_color(&item.kind);
                let chip = egui::Button::new(RichText::new(format!("✔ {}", item.title)).color(super::theme::th().text.on_color).size(13.0))
                    .fill(c.linear_multiply(0.85))
                    .corner_radius(12.0);
                if ui.add(chip).on_hover_text("Click to undo").clicked() {
                    actions.push(Action::Toggle(item.id.clone(), day));
                    toggled_now.push(item.id.clone());
                }
            }
        });
    }
    for id in toggled_now {
        state.toggled.insert(id, now);
    }
    if !state.toggled.is_empty() {
        ui.ctx().request_repaint();
    }
}
