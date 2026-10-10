//! Completion history as streaks.
//!
//! Each recurring task gets a row of nodes (one per occurrence: daily tasks every day, weekly
//! tasks once a week). Consecutive completed occurrences form a streak, drawn in a style that
//! depends on the task's kind:
//!
//! | kind                 | style                                                       |
//! |----------------------|-------------------------------------------------------------|
//! | workout              | ribbon: thickens as the streak grows                        |
//! | meal                 | comet: the live streak is a bright head, older days fade    |
//! | chore                | arcs: each day hops to the next, hops grow with the streak  |
//! | habit                | vine: a winding stem that sprouts leaves at milestones      |
//! | task, event, other   | constellation: zig-zag with a 4-point star every 7th day    |
//!
//! Missed occurrences are hollow grey rings; today, while not yet done, is a pending ring.
//! One-off tasks get a final row of small stars on the day they were finished.
//! [`day_stars`] draws the same vocabulary small, for calendar month cells.

use super::kind_color;
use crate::model::{occurrences, Item};
use crate::store::Data;
use chrono::{Duration, Local, NaiveDate};
use eframe::egui::{
    self, epaint::{CubicBezierShape, QuadraticBezierShape}, Align2, Color32, FontId, Id, Painter, Pos2, Rect, RichText,
    Sense, Shape, Stroke, Ui, Vec2,
};

const ROW_H: f32 = 46.0;
const LABEL_W: f32 = 190.0;
/// Narrowest spacing between days; longer periods are trimmed to fit.
const MIN_STEP: f32 = 13.0;

#[derive(Clone, Copy, PartialEq)]
enum State {
    Done,
    Missed,
    /// Today, not done yet: doesn't break the streak.
    Pending,
}

struct Node {
    day: NaiveDate,
    pos: Pos2,
    state: State,
    /// Streak length at this node (1 = first day of a streak); 0 if not done.
    run: u32,
}

#[derive(Clone, Copy)]
enum Style {
    Ribbon,
    Comet,
    Arcs,
    Vine,
    Constellation,
}

fn style_for(kind: &str) -> Style {
    match kind {
        "workout" => Style::Ribbon,
        "meal" => Style::Comet,
        "chore" => Style::Arcs,
        "habit" => Style::Vine,
        _ => Style::Constellation,
    }
}

fn with_alpha(c: Color32, a: f32) -> Color32 {
    c.linear_multiply(a.clamp(0.0, 1.0))
}

/// Mixes a color toward white (for leaves and highlights).
fn lighten(c: Color32, t: f32) -> Color32 {
    let l = |v: u8| (v as f32 + (255.0 - v as f32) * t) as u8;
    Color32::from_rgb(l(c.r()), l(c.g()), l(c.b()))
}

/// Occurrence states of a repeating item from its start (at most two years back) up to `to`,
/// streak lengths filled in. Streaks are counted over the whole history so they're true no
/// matter how short the period on screen is; callers drop what's before their window.
fn history(item: &Item, to: NaiveDate, today: NaiveDate) -> Vec<(NaiveDate, State, u32)> {
    let (Some(rule), Some(start)) = (item.repeat_rule(), item.date) else { return Vec::new() };
    let last = item.until.map_or(to, |u| u.min(to)).min(today);
    let mut out = Vec::new();
    let mut run = 0;
    let mut d = start.max(today - Duration::days(730));
    while d <= last {
        if rule.occurs_on(start, d) {
            let state = if item.done_dates.contains(&d) {
                State::Done
            } else if d < today {
                State::Missed
            } else {
                State::Pending
            };
            match state {
                State::Done => run += 1,
                State::Missed => run = 0,
                State::Pending => {}
            }
            out.push((d, state, if state == State::Done { run } else { 0 }));
        }
        d += Duration::days(1);
    }
    out
}

/// Current streak (ending today or at the last occurrence) and best streak in the history.
fn streak_stats(h: &[(NaiveDate, State, u32)]) -> (u32, u32) {
    let best = h.iter().map(|(_, _, r)| *r).max().unwrap_or(0);
    let current = h.iter().rev().find(|(_, s, _)| *s != State::Pending).map_or(0, |(_, _, r)| *r);
    (current, best)
}

fn ring(p: &Painter, c: Pos2, r: f32, color: Color32) {
    p.circle_stroke(c, r, Stroke::new(1.3, color));
}

/// 4-point star made of two crossing diamonds (each convex, so egui can fill them).
pub fn four_point_star(p: &Painter, c: Pos2, r: f32, color: Color32) {
    let t = r * 0.28;
    p.add(Shape::convex_polygon(
        vec![c + Vec2::new(0.0, -r), c + Vec2::new(t, 0.0), c + Vec2::new(0.0, r), c + Vec2::new(-t, 0.0)],
        color,
        Stroke::NONE,
    ));
    p.add(Shape::convex_polygon(
        vec![c + Vec2::new(-r, 0.0), c + Vec2::new(0.0, -t), c + Vec2::new(r, 0.0), c + Vec2::new(0.0, t)],
        color,
        Stroke::NONE,
    ));
}

/// Lens-shaped leaf from `base` toward `dir` (convex).
fn leaf(p: &Painter, base: Pos2, dir: Vec2, len: f32, width: f32, color: Color32) {
    let d = dir.normalized();
    let n = Vec2::new(-d.y, d.x);
    let pts: Vec<Pos2> = (0..=10)
        .map(|i| {
            let t = i as f32 / 10.0;
            base + d * len * t + n * width * (std::f32::consts::PI * t).sin()
        })
        .chain((1..10).rev().map(|i| {
            let t = i as f32 / 10.0;
            base + d * len * t - n * width * 0.35 * (std::f32::consts::PI * t).sin()
        }))
        .collect();
    p.add(Shape::convex_polygon(pts, color, Stroke::NONE));
}

fn is_milestone(run: u32) -> bool {
    matches!(run, 3 | 7 | 14 | 21 | 30) || (run > 30 && run % 30 == 0)
}

/// Draws one row's nodes in the given style. `pulse` (0..1) animates a pending "today".
fn draw_row(p: &Painter, nodes: &[Node], color: Color32, style: Style, pulse: f32, faint: Color32) {
    // Consecutive done pairs inside a streak.
    let pairs = || nodes.windows(2).filter(|w| w[0].state == State::Done && w[1].state == State::Done);
    match style {
        Style::Ribbon => {
            let w = |run: u32| (0.8 + 0.3 * run as f32).min(4.5);
            for pr in pairs() {
                let (a, b) = (&pr[0], &pr[1]);
                p.add(Shape::convex_polygon(
                    vec![a.pos - Vec2::new(0.0, w(a.run)), b.pos - Vec2::new(0.0, w(b.run)), b.pos + Vec2::new(0.0, w(b.run)), a.pos + Vec2::new(0.0, w(a.run))],
                    with_alpha(color, 0.85),
                    Stroke::NONE,
                ));
            }
            for n in nodes.iter().filter(|n| n.state == State::Done) {
                p.circle_filled(n.pos, 2.2 + w(n.run) * 0.45, color);
            }
        }
        Style::Arcs => {
            for pr in pairs() {
                let (a, b) = (&pr[0], &pr[1]);
                let h = (4.0 + 2.6 * b.run as f32).min(ROW_H * 0.42);
                let ctrl = Pos2::new((a.pos.x + b.pos.x) / 2.0, a.pos.y - 2.0 * h);
                p.add(QuadraticBezierShape::from_points_stroke([a.pos, ctrl, b.pos], false, Color32::TRANSPARENT, Stroke::new(2.0, color)));
            }
            for n in nodes.iter().filter(|n| n.state == State::Done) {
                p.circle_filled(n.pos, 4.0, color);
            }
        }
        Style::Vine => {
            for (i, pr) in pairs().enumerate() {
                let (a, b) = (&pr[0], &pr[1]);
                let dx = b.pos.x - a.pos.x;
                let s = if i % 2 == 0 { 1.0 } else { -1.0 };
                p.add(CubicBezierShape::from_points_stroke(
                    [a.pos, Pos2::new(a.pos.x + dx / 3.0, a.pos.y + 9.0 * s), Pos2::new(a.pos.x + 2.0 * dx / 3.0, a.pos.y - 9.0 * s), b.pos],
                    false,
                    Color32::TRANSPARENT,
                    Stroke::new(2.0, color),
                ));
            }
            let leaf_color = lighten(color, 0.35);
            for (i, n) in nodes.iter().enumerate().filter(|(_, n)| n.state == State::Done) {
                if is_milestone(n.run) {
                    let up = if i % 2 == 0 { -1.0 } else { 1.0 };
                    leaf(p, n.pos, Vec2::new(1.0, 1.4 * up), 15.0, 5.5, leaf_color);
                    if n.run >= 7 {
                        leaf(p, n.pos, Vec2::new(-0.8, -1.4 * up), 12.0, 4.5, leaf_color);
                    }
                }
                p.circle_filled(n.pos, 3.5, color);
            }
        }
        Style::Constellation => {
            for pr in pairs() {
                p.line_segment([pr[0].pos, pr[1].pos], Stroke::new(1.8, color));
            }
            for n in nodes.iter().filter(|n| n.state == State::Done) {
                if n.run > 0 && n.run % 7 == 0 {
                    p.circle_filled(n.pos, 12.0, with_alpha(color, 0.2));
                    four_point_star(p, n.pos, 10.0, color);
                } else {
                    p.circle_filled(n.pos, 4.5, color);
                }
            }
        }
        Style::Comet => {
            // Every streak is a comet flying toward its last day. Older ones fade; the live
            // one (reaching the latest occurrence) is bright with a glowing head.
            let last_real = nodes.iter().rposition(|n| n.state != State::Pending);
            let total = nodes.len().max(1) as f32;
            let mut runs: Vec<(usize, usize)> = Vec::new();
            let mut i = 0;
            while i < nodes.len() {
                if nodes[i].state == State::Done {
                    let s = i;
                    while i + 1 < nodes.len() && nodes[i + 1].state == State::Done {
                        i += 1;
                    }
                    runs.push((s, i));
                }
                i += 1;
            }
            for (s, e) in runs {
                let live = Some(e) == last_real;
                let f = if live { 1.0 } else { 0.25 + 0.4 * (e as f32 / total) };
                let (tail, head) = (nodes[s].pos, nodes[e].pos);
                let hw = 1.5 + 4.0 * f;
                if e > s {
                    p.add(Shape::convex_polygon(
                        vec![tail, head - Vec2::new(0.0, hw), head + Vec2::new(0.0, hw)],
                        with_alpha(color, 0.35 * f),
                        Stroke::NONE,
                    ));
                }
                let len = (e - s + 1) as f32;
                for (k, n) in nodes[s..=e].iter().enumerate() {
                    let t = (k as f32 + 1.0) / len;
                    p.circle_filled(n.pos, 1.8 + 2.2 * t * f, with_alpha(color, (0.3 + 0.6 * t) * f.max(0.5)));
                }
                if live {
                    p.circle_filled(head, 13.0, with_alpha(color, 0.22));
                }
                p.circle_filled(head, 3.0 + 5.0 * f, with_alpha(color, 0.45 + 0.55 * f));
            }
        }
    }
    // Missed and pending nodes look the same in every style.
    for n in nodes {
        match n.state {
            State::Missed => ring(p, n.pos, 3.5, faint),
            State::Pending => {
                // gently pulsing ring: "do it today"
                p.circle_filled(n.pos, 6.0 + 4.0 * pulse, with_alpha(color, 0.12 + 0.12 * pulse));
                ring(p, n.pos, 5.0, color);
            }
            State::Done => {}
        }
    }
}

/// One-off tasks completed on each day, as small stacked stars.
fn one_offs_on(data: &Data, day: NaiveDate) -> Vec<&Item> {
    data.items.iter().filter(|i| i.completed_on() == Some(day)).collect()
}

/// The Streaks section of the Charts tab.
pub fn streaks_section(ui: &mut Ui, data: &Data, period_days: i64) {
    let today = Local::now().date_naive();
    let width = ui.available_width();
    let plot_w = (width - LABEL_W - 16.0).max(100.0);
    // Trim long periods so nodes stay readable.
    let days = (period_days.min((plot_w / MIN_STEP) as i64)).max(7);
    let from = today - Duration::days(days - 1);
    let step = plot_w / days as f32;
    let x_of = |left: f32, d: NaiveDate| left + LABEL_W + step * ((d - from).num_days() as f32 + 0.5);

    let recurring: Vec<&Item> = data
        .items
        .iter()
        .filter(|i| i.repeat.is_some() && i.date.is_some_and(|d| d <= today) && i.until.is_none_or(|u| u >= from))
        .collect();

    ui.horizontal(|ui| {
        ui.label(RichText::new("Streaks").size(17.0).strong());
        let mut note = format!("last {days} days");
        if days < period_days {
            note += " (trimmed to fit; widen the window to see more)";
        }
        ui.label(RichText::new(note).weak());
    });
    ui.label(
        RichText::new("workout = ribbon · meal = comet · chore = arcs · habit = vine · other = constellation")
            .weak()
            .size(12.0),
    );
    ui.add_space(4.0);
    if recurring.is_empty() && data.items.iter().all(|i| i.completed_on().is_none_or(|d| d < from)) {
        empty_state(ui, "No streaks yet", "Give a task a repeat rule, like \"daily\" or \"every mon and thu\", and its streak starts here.");
        return;
    }

    // Date axis
    let (axis, _) = ui.allocate_exact_size(Vec2::new(width, 16.0), Sense::hover());
    let weak = ui.visuals().weak_text_color();
    let label_every = ((44.0 / step).ceil() as i64).max(1);
    let mut d = from;
    let mut i = 0;
    while d <= today {
        if i % label_every == 0 {
            ui.painter().text(Pos2::new(x_of(axis.left(), d), axis.center().y), Align2::CENTER_CENTER, d.format("%b %-d").to_string(), FontId::proportional(11.0), weak);
        }
        d += Duration::days(1);
        i += 1;
    }

    // Today's open nodes pulse while the pointer is over the window (and motion is on);
    // otherwise they hold still so an idle window doesn't repaint.
    let animate = super::theme::anim(1.0) > 0.0 && ui.ctx().pointer_hover_pos().is_some();
    let time = ui.input(|i| i.time) as f32;
    let pulse = if animate { (time * 2.2).sin() * 0.5 + 0.5 } else { 0.5 };
    let mut any_pending = false;
    let faint = with_alpha(ui.visuals().weak_text_color(), 0.9);
    let grid = ui.visuals().widgets.noninteractive.bg_stroke.color;

    for item in recurring {
        let full = history(item, today, today);
        let (current, best) = streak_stats(&full);
        let h: Vec<_> = full.into_iter().filter(|(d, _, _)| *d >= from).collect();
        let (row, resp) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::hover());
        let painter = ui.painter_at(row);
        let color = kind_color(&item.kind);
        let style = style_for(&item.kind);
        painter.line_segment([Pos2::new(row.left() + LABEL_W, row.bottom()), Pos2::new(row.right(), row.bottom())], Stroke::new(0.5, grid));
        // label column
        let title_galley = painter.layout(item.title.clone(), FontId::proportional(14.0), ui.visuals().text_color(), LABEL_W - 16.0);
        painter.galley(row.left_top() + Vec2::new(4.0, 6.0), title_galley, ui.visuals().text_color());
        let stats = if current > 0 { format!("🔥 {current}   best {best}") } else { format!("best {best}") };
        painter.text(row.left_top() + Vec2::new(4.0, ROW_H - 8.0), Align2::LEFT_BOTTOM, stats, FontId::proportional(11.5), if current > 0 { color } else { weak });

        let cy = row.center().y + if matches!(style, Style::Arcs) { 8.0 } else { 0.0 };
        let nodes: Vec<Node> = h
            .iter()
            .enumerate()
            .map(|(k, (day, state, run))| {
                let zig = if matches!(style, Style::Constellation) { if k % 2 == 0 { -5.0 } else { 5.0 } } else { 0.0 };
                Node { day: *day, pos: Pos2::new(x_of(row.left(), *day), cy + zig), state: *state, run: *run }
            })
            .collect();
        any_pending |= nodes.iter().any(|n| n.state == State::Pending);
        draw_row(&painter, &nodes, color, style, pulse, faint);

        if let Some(ptr) = resp.hover_pos() {
            if let Some(n) = nodes.iter().min_by(|a, b| (a.pos.x - ptr.x).abs().total_cmp(&(b.pos.x - ptr.x).abs())) {
                if (n.pos.x - ptr.x).abs() <= step.max(8.0) {
                    let what = match n.state {
                        State::Done => format!("done · streak day {}", n.run),
                        State::Missed => "missed".into(),
                        State::Pending => "not done yet today".into(),
                    };
                    egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), Id::new(("streak_tip", &item.id)), |ui| {
                        ui.label(format!("{} · {}\n{what}", item.title, n.day.format("%a %b %-d")));
                    });
                }
            }
        }
    }

    // One-off tasks: small stars on the day they were finished.
    let (row, resp) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::hover());
    let painter = ui.painter_at(row);
    painter.text(row.left_top() + Vec2::new(4.0, 6.0), Align2::LEFT_TOP, "One-off tasks", FontId::proportional(14.0), ui.visuals().text_color());
    let mut d = from;
    let mut total = 0;
    while d <= today {
        let done = one_offs_on(data, d);
        total += done.len();
        let x = x_of(row.left(), d);
        let n = done.len().min(4);
        for (k, item) in done.iter().take(4).enumerate() {
            let y = row.center().y + (k as f32 - (n as f32 - 1.0) / 2.0) * 8.0;
            four_point_star(&painter, Pos2::new(x, y), 4.5, kind_color(&item.kind));
        }
        if done.len() > 4 {
            painter.text(Pos2::new(x, row.bottom() - 2.0), Align2::CENTER_BOTTOM, "+", FontId::proportional(10.0), weak);
        }
        d += Duration::days(1);
    }
    painter.text(row.left_top() + Vec2::new(4.0, ROW_H - 8.0), Align2::LEFT_BOTTOM, format!("{total} finished"), FontId::proportional(11.5), weak);
    if let Some(ptr) = resp.hover_pos() {
        let k = ((ptr.x - row.left() - LABEL_W) / step).floor() as i64;
        if (0..days).contains(&k) {
            let day = from + Duration::days(k);
            let done = one_offs_on(data, day);
            if !done.is_empty() {
                let names: Vec<&str> = done.iter().map(|i| i.title.as_str()).collect();
                egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), Id::new("oneoff_tip"), |ui| {
                    ui.label(format!("{}\n{}", day.format("%a %b %-d"), names.join("\n")));
                });
            }
        }
    }

    if any_pending && animate {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(40));
    }
}

/// What a task looks like on a given day: done, missed (past, not done) or open
/// (today/upcoming, not done yet).
#[derive(Clone, Copy, PartialEq)]
pub enum DayState {
    Done,
    Missed,
    Open,
}

/// Every task due on `day` (plus one-offs finished that day), done ones first so the lit part
/// reads as one connected figure: (state, color, item index).
pub fn day_tasks(data: &Data, day: NaiveDate, today: NaiveDate) -> Vec<(DayState, Color32, usize)> {
    let mut v: Vec<(DayState, Color32, usize)> = Vec::new();
    for (_, idx) in occurrences(&data.items, day, day) {
        let it = &data.items[idx];
        let done = if it.repeat.is_some() { it.done_dates.contains(&day) } else { it.done };
        let state = if done {
            DayState::Done
        } else if day < today {
            DayState::Missed
        } else {
            DayState::Open
        };
        v.push((state, kind_color(&it.kind), idx));
    }
    for (idx, it) in data.items.iter().enumerate() {
        if it.completed_on() == Some(day) && it.date != Some(day) {
            v.push((DayState::Done, kind_color(&it.kind), idx));
        }
    }
    v.sort_by_key(|(s, _, _)| *s != DayState::Done);
    v
}

/// A day's tasks as a compact constellation filling `area` (calendar month cells): lit dots
/// joined up for done, grey rings for missed, faint colored rings for still open, and a gold
/// star when everything due was done. Wraps onto more rows when the cell is narrow.
pub fn day_stars(p: &Painter, area: Rect, data: &Data, day: NaiveDate, today: NaiveDate, faint: Color32) {
    let tasks = day_tasks(data, day, today);
    let n = tasks.len();
    if n == 0 || area.height() < 8.0 {
        return;
    }
    let all_done = tasks.iter().all(|(s, _, _)| *s == DayState::Done);
    let r = (area.height() * 0.16).clamp(2.6, 5.0);
    let step = r * 3.4;
    let usable_w = area.width() - 8.0 - if all_done { step * 1.5 } else { 0.0 };
    let per_row = ((usable_w / step).floor() as usize).max(1);
    let rows = n.div_ceil(per_row);
    let row_h = (area.height() / rows as f32).min(r * 5.0);
    let top = area.center().y - row_h * rows as f32 / 2.0;
    let pts: Vec<Pos2> = (0..n)
        .map(|k| {
            let (row, col) = (k / per_row, k % per_row);
            let zig = if col % 2 == 0 { -0.7 } else { 0.7 } * r;
            Pos2::new(area.left() + 4.0 + r + step * col as f32, top + row_h * (row as f32 + 0.5) + zig)
        })
        .collect();
    for k in 1..n {
        let same_row = k % per_row != 0;
        if same_row && tasks[k - 1].0 == DayState::Done && tasks[k].0 == DayState::Done {
            p.line_segment([pts[k - 1], pts[k]], Stroke::new((r * 0.35).max(1.0), with_alpha(tasks[k].1, 0.8)));
        }
    }
    for (k, (state, c, _)) in tasks.iter().enumerate() {
        match state {
            DayState::Done => {
                p.circle_filled(pts[k], r * 1.8, with_alpha(*c, 0.15));
                p.circle_filled(pts[k], r, *c);
            }
            DayState::Missed => {
                p.circle_stroke(pts[k], r * 0.8, Stroke::new(1.0, faint));
            }
            DayState::Open => {
                p.circle_stroke(pts[k], r * 0.8, Stroke::new(1.0, with_alpha(*c, 0.55)));
            }
        }
    }
    if all_done {
        let last = pts[n - 1];
        four_point_star(p, Pos2::new(last.x + step * 1.2, last.y), r * 2.0, super::gold_color());
    }
}

/// A dotted, unlit constellation: the "nothing here yet" picture used by empty states.
pub fn ghost_stars(p: &Painter, start: Pos2, step: f32, n: usize, amp: f32) {
    let c = super::theme::th().text.muted.linear_multiply(0.5);
    let pts: Vec<Pos2> = (0..n).map(|k| Pos2::new(start.x + step * k as f32, start.y + if k % 2 == 0 { -amp } else { amp })).collect();
    for w in pts.windows(2) {
        p.extend(Shape::dashed_line(&[w[0], w[1]], Stroke::new(1.0, c), 4.0, 4.0));
    }
    for pt in &pts {
        p.circle_stroke(*pt, 5.0, Stroke::new(1.3, c));
    }
}

/// Empty state: a ghost constellation with a short headline and an inviting line.
pub fn empty_state(ui: &mut Ui, title: &str, body: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 64.0), Sense::hover());
    let p = ui.painter();
    ghost_stars(p, Pos2::new(rect.left() + 14.0, rect.center().y), 30.0, 5, 8.0);
    let t = super::theme::th();
    let x = rect.left() + 14.0 + 30.0 * 4.0 + 28.0;
    p.text(Pos2::new(x, rect.center().y - 9.0), Align2::LEFT_CENTER, title, super::theme::bold(15.0), t.text.primary);
    p.text(Pos2::new(x, rect.center().y + 11.0), Align2::LEFT_CENTER, body, FontId::proportional(12.5), t.text.muted);
}

/// The gold "everything done" star.
pub fn gold_star(p: &Painter, c: Pos2, r: f32) {
    p.circle_filled(c, r * 1.3, with_alpha(super::gold_color(), 0.18));
    four_point_star(p, c, r, super::gold_color());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaks_count_beyond_the_visible_window() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 10).unwrap();
        let mut item = Item::new("Run");
        item.date = Some(today - Duration::days(20));
        item.repeat = Some("daily".into());
        // done the last 12 days before today, missed the day before that; today still open
        item.done_dates = (1..=12).map(|k| today - Duration::days(k)).collect();
        let full = history(&item, today, today);
        let (current, best) = streak_stats(&full);
        assert_eq!((current, best), (12, 12));
        // a 7-day window still shows the true streak length on its nodes
        let from = today - Duration::days(6);
        let window: Vec<_> = full.iter().filter(|(d, _, _)| *d >= from).collect();
        assert_eq!(window.first().unwrap().2, 7); // Oct 4 is day 7 of the streak
        assert!(window.last().unwrap().1 == State::Pending); // today, not done yet
        assert!(full.iter().any(|(_, s, _)| *s == State::Missed));
    }
}
