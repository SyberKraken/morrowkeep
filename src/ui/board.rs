//! The "quest board": today's anytime tasks as a grid of large tiles.
//!
//! - The grid is fixed (tasks keep their order) so you build muscle memory; 3–5 columns.
//! - Color shows state: pending tiles are dark with a colored outline and icon; completed
//!   tiles fill with their color and get a checkmark, so the board fills up as you work.
//! - Completing a tile flips it over, it stays a moment, then moves to the completion tray
//!   (click a tray chip to undo). The last tile is always "add a task".
//! - A constellation in the header lights a star per finished task, plus two fixed stars
//!   for "calories logged today" and "weight logged today".

use super::charts::WEIGHT_COLOR;
use super::{kind_color, Action, CALORIE_COLOR, GOOGLE_COLOR};
use crate::model::Item;
use crate::store::Data;
use chrono::NaiveDate;
use eframe::egui::{
    self, text::LayoutJob, Align2, Color32, CornerRadius, FontId, Id, Pos2, Rect, RichText, Sense, Stroke,
    StrokeKind, TextFormat, Ui, Vec2,
};
use std::collections::HashMap;
use std::time::Instant;

/// Seconds the flip animation takes.
const FLIP: f32 = 0.4;
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
}

pub fn kind_icon(kind: &str) -> &'static str {
    match kind {
        "workout" => "💪",
        "meal" => "🍴",
        "chore" => "🏠",
        "habit" => "🔁",
        "event" => "📅",
        "task" => "📌",
        _ => "⭐",
    }
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
        let fill = if hovered { color.linear_multiply(0.14) } else { visuals.faint_bg_color };
        painter.rect_filled(r, radius, fill);
        painter.rect_stroke(r, radius, Stroke::new(if hovered { 2.5 } else { 2.0 }, color.linear_multiply(0.85)), StrokeKind::Inside);
    }
    if sx < 0.35 {
        return; // too thin mid-flip to show content
    }
    let clip = painter.with_clip_rect(r);
    let ink = if showing_done { Color32::WHITE } else { visuals.text_color() };
    let pad = 12.0 * k;
    // icon
    let icon_color = if showing_done { Color32::WHITE } else { color };
    clip.text(r.left_top() + Vec2::new(pad, pad - 2.0), Align2::LEFT_TOP, kind_icon(&item.kind), FontId::proportional(24.0 * k), icon_color);
    // checkmark / repeat hint, top-right
    if showing_done {
        clip.text(r.right_top() + Vec2::new(-pad, pad - 4.0), Align2::RIGHT_TOP, "✔", FontId::proportional(28.0 * k), Color32::WHITE);
    } else if item.repeat.is_some() && item.kind != "habit" {
        clip.text(r.right_top() + Vec2::new(-pad, pad), Align2::RIGHT_TOP, "🔁", FontId::proportional(13.0 * k), visuals.weak_text_color());
    }
    // title, bottom-left, up to 2 lines
    let galley = clip.layout_job(wrapped(&item.title, 16.0 * k, ink, r.width() - 2.0 * pad, 2));
    let pos = Pos2::new(r.left() + pad, r.bottom() - pad - galley.size().y);
    clip.galley(pos, galley, ink);
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
fn constellation(ui: &mut Ui, data: &Data, ids: &[usize], day: NaiveDate) {
    // Two fixed trackers first (always in the same place), then one star per task.
    struct Star {
        lit: bool,
        color: Color32,
        label: Option<&'static str>,
    }
    let kcal_logged = data.calories_on(day) > 0;
    let weight_logged = data.weight_on(day).is_some();
    let mut stars = vec![
        Star { lit: kcal_logged, color: CALORIE_COLOR, label: Some("kcal") },
        Star { lit: weight_logged, color: WEIGHT_COLOR, label: Some("kg") },
    ];
    stars.extend(ids.iter().map(|i| {
        let item = &data.items[*i];
        Star { lit: item.is_done_on(day), color: kind_color(&item.kind), label: None }
    }));
    let n = stars.len();
    let done = stars.iter().filter(|s| s.lit).count();
    let tasks_done = ids.iter().filter(|i| data.items[**i].is_done_on(day)).count();

    let width = ui.available_width();
    let height = 64.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let all_done = done == n;
    // trophy space on the right when everything is done
    let right_pad = if all_done { 190.0 } else { 20.0 };
    let step = ((width - 20.0 - right_pad) / n as f32).clamp(22.0, 64.0);
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
        let (stroke_w, color) = if a.lit && b.lit { (2.5, b.color.linear_multiply(0.8)) } else { (1.0, dim.linear_multiply(0.35)) };
        painter.line_segment([pts[i - 1], pts[i]], Stroke::new(stroke_w, color));
    }
    for (p, star) in pts.iter().zip(&stars) {
        if star.lit {
            painter.circle_filled(*p, 15.0, star.color.linear_multiply(0.12)); // outer glow
            painter.circle_filled(*p, 10.0, star.color.linear_multiply(0.3));
            painter.circle_filled(*p, 6.5, star.color);
        } else {
            painter.circle_stroke(*p, 5.5, Stroke::new(1.6, dim));
        }
        if let Some(label) = star.label {
            // identity in text, not color alone
            let ink = if star.lit { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
            painter.text(Pos2::new(p.x, rect.bottom() - 2.0), Align2::CENTER_BOTTOM, label, FontId::proportional(11.0), ink);
        }
    }
    if all_done {
        painter.text(
            Pos2::new(pts[n - 1].x + 30.0, cy),
            Align2::LEFT_CENTER,
            "🏆 All done today!",
            FontId::proportional(20.0),
            Color32::from_rgb(46, 160, 103),
        );
    } else if ids.is_empty() {
        painter.text(
            Pos2::new(pts[n - 1].x + 30.0, cy),
            Align2::LEFT_CENTER,
            "No habits for today yet. Add one below.",
            FontId::proportional(15.0),
            ui.visuals().weak_text_color(),
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
    constellation(ui, data, ids, day);
    ui.add_space(8.0);

    if !all_day.is_empty() {
        ui.horizontal_wrapped(|ui| {
            for t in all_day {
                ui.label(RichText::new(format!("📅 {t}")).color(GOOGLE_COLOR));
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
        let flip = age(&item.id).map_or(1.0, |a| (a / FLIP).min(1.0));
        if age(&item.id).is_some() {
            animating = true;
        }
        draw_tile(ui, rect, item, done, flip, resp.hovered());
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
                let chip = egui::Button::new(RichText::new(format!("✔ {}", item.title)).color(Color32::WHITE).size(13.0))
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
