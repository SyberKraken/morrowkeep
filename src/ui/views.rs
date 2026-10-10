//! Today view, day list, month grid and week timeline.

use super::board::{anytime_board, BoardState};
use super::{weight_color, kind_color, schedule_text, Action, CalMode, CalorieInput, calorie_color, claude_color, google_color, not_logged_color};
use crate::model::occurrences;
use crate::store::Data;
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveTime, Timelike};
use eframe::egui::{
    self, text::LayoutJob, Align2, Color32, FontId, Id, Painter, Pos2, Rect, RichText, Sense, Stroke,
    StrokeKind, TextFormat, Ui, Vec2,
};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy)]
enum Entry {
    Item(usize),
    Ext(usize),
}

/// (timed entries sorted by start, untimed entries) for one day.
fn entries_for(data: &Data, day: NaiveDate) -> (Vec<Entry>, Vec<Entry>) {
    let mut timed = Vec::new();
    let mut untimed = Vec::new();
    for (_, idx) in occurrences(&data.items, day, day) {
        if data.items[idx].time.is_some() {
            timed.push((data.items[idx].time.unwrap(), Entry::Item(idx)));
        } else {
            untimed.push(Entry::Item(idx));
        }
    }
    for (i, ev) in data.gcache.iter().enumerate() {
        if ev.on_day(day) {
            if ev.all_day {
                untimed.push(Entry::Ext(i));
            } else {
                let t = if ev.start.date() < day { NaiveTime::MIN } else { ev.start.time() };
                timed.push((t, Entry::Ext(i)));
            }
        }
    }
    timed.sort_by_key(|(t, _)| *t);
    (timed.into_iter().map(|(_, e)| e).collect(), untimed)
}

fn kind_chip(ui: &mut Ui, kind: &str) {
    let color = kind_color(kind);
    ui.label(RichText::new(kind).size(11.5).color(color).strong());
}

fn entry_row(ui: &mut Ui, data: &Data, e: Entry, day: NaiveDate, actions: &mut Vec<Action>) {
    match e {
        Entry::Item(idx) => {
            let item = &data.items[idx];
            let done = item.is_done_on(day);
            ui.horizontal(|ui| {
                let mut d = done;
                if ui.checkbox(&mut d, "").changed() {
                    actions.push(Action::Toggle(item.id.clone(), day));
                }
                let time = item.time.map(|t| t.format("%H:%M").to_string()).unwrap_or_default();
                ui.add_sized([40.0, 18.0], egui::Label::new(RichText::new(time).monospace().weak()));
                let (r, _) = ui.allocate_exact_size(Vec2::new(4.0, 18.0), Sense::hover());
                ui.painter().rect_filled(r, 2.0, kind_color(&item.kind));
                // Right-hand details first, so the title truncates into whatever space is left.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("✏").on_hover_text("Edit").clicked() {
                        actions.push(Action::Edit(item.id.clone()));
                    }
                    kind_chip(ui, &item.kind);
                    let sched = schedule_text(item);
                    if !sched.is_empty() && ui.available_width() > 320.0 {
                        ui.label(RichText::new(sched).size(12.0).weak());
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        let mut title = RichText::new(&item.title).size(15.0);
                        if done {
                            title = title.strikethrough().weak();
                        }
                        let resp = ui.add(egui::Label::new(title).sense(Sense::click()).truncate());
                        if resp.clicked() {
                            actions.push(Action::Edit(item.id.clone()));
                        }
                        let tip = match (item.notes.is_empty(), schedule_text(item)) {
                            (true, s) if s.is_empty() => "Click to edit".to_string(),
                            (true, s) => s,
                            (false, s) => format!("{s}

{}", item.notes).trim().to_string(),
                        };
                        resp.on_hover_text(tip);
                    });
                });
            });
        }
        Entry::Ext(i) => {
            let ev = &data.gcache[i];
            ui.horizontal(|ui| {
                ui.add_space(26.0);
                let time = if ev.all_day { String::new() } else { ev.start.format("%H:%M").to_string() };
                ui.add_sized([40.0, 18.0], egui::Label::new(RichText::new(time).monospace().weak()));
                let (r, _) = ui.allocate_exact_size(Vec2::new(4.0, 18.0), Sense::hover());
                ui.painter().rect_filled(r, 2.0, google_color());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("Google").size(11.5).color(google_color()));
                    if !ev.all_day && ui.available_width() > 320.0 {
                        ui.label(RichText::new(format!("{}–{}", ev.start.format("%H:%M"), ev.end.format("%H:%M"))).size(12.0).weak());
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(RichText::new(&ev.title).size(15.0).color(google_color())).truncate());
                    });
                });
            });
        }
    }
}

fn section(ui: &mut Ui, title: &str, count: usize) {
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).strong().size(13.0).color(ui.visuals().weak_text_color()));
        if count > 0 {
            ui.label(RichText::new(count.to_string()).size(12.0).weak());
        }
    });
    ui.separator();
}

/// Entries for a single day, for the calendar's side panel.
fn day_list(ui: &mut Ui, data: &Data, day: NaiveDate, actions: &mut Vec<Action>) {
    let (timed, untimed) = entries_for(data, day);
    let now = Local::now().naive_local();
    let is_today = day == now.date();

    section(ui, "SCHEDULE", timed.len());
    if timed.is_empty() {
        ui.label(RichText::new("Nothing scheduled").weak());
    }
    let mut now_drawn = !is_today;
    for e in &timed {
        let start = match *e {
            Entry::Item(i) => data.items[i].time.unwrap(),
            Entry::Ext(i) => data.gcache[i].start.time(),
        };
        if !now_drawn && start > now.time() {
            now_line(ui);
            now_drawn = true;
        }
        entry_row(ui, data, *e, day, actions);
    }
    if !now_drawn && !timed.is_empty() {
        now_line(ui);
    }

    section(ui, if is_today { "ANYTIME TODAY" } else { "ANYTIME" }, untimed.len());
    if untimed.is_empty() {
        ui.label(RichText::new("Nothing here").weak());
    }
    vertical_constellation(ui, data, day, &untimed, actions);
}

/// The day's anytime tasks as a vertical constellation: one star per task, stacked, with its
/// name beside it. Lit and joined when done; click to tick off, right-click to edit.
fn vertical_constellation(ui: &mut Ui, data: &Data, day: NaiveDate, entries: &[Entry], actions: &mut Vec<Action>) {
    let today = Local::now().date_naive();
    let row_h = 30.0;
    let x_off = 16.0;
    let faint = ui.visuals().weak_text_color();
    let mut prev: Option<(Pos2, bool, Color32)> = None;
    let mut all_done = true;
    let mut any = false;
    for e in entries {
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::click());
        let node = Pos2::new(rect.left() + x_off, rect.center().y);
        let painter = ui.painter();
        match *e {
            Entry::Item(idx) => {
                any = true;
                let item = &data.items[idx];
                let color = kind_color(&item.kind);
                let done = item.is_done_on(day);
                all_done &= done;
                if let Some((p, pd, _)) = prev {
                    let (w, c) = if pd && done { (2.5, color.linear_multiply(0.8)) } else { (1.0, faint.linear_multiply(0.4)) };
                    painter.line_segment([p, node], Stroke::new(w, c));
                }
                if resp.hovered() {
                    painter.rect_filled(rect, 4.0, ui.visuals().widgets.hovered.weak_bg_fill);
                }
                if done {
                    painter.circle_filled(node, 12.0, color.linear_multiply(0.15));
                    painter.circle_filled(node, 7.0, color);
                } else if day < today {
                    painter.circle_stroke(node, 5.5, Stroke::new(1.4, faint));
                } else {
                    painter.circle_stroke(node, 5.5, Stroke::new(1.6, color));
                }
                let ink = if done { ui.visuals().text_color() } else if day < today { faint } else { ui.visuals().text_color() };
                let galley = painter.layout_no_wrap(item.title.clone(), FontId::proportional(14.5), ink);
                let text_pos = Pos2::new(rect.left() + x_off + 18.0, rect.center().y - galley.size().y / 2.0);
                painter.galley(text_pos, galley, ink);
                painter.text(rect.right_center() - Vec2::new(6.0, 0.0), Align2::RIGHT_CENTER, &item.kind, FontId::proportional(11.5), color);
                if resp.clicked() {
                    actions.push(Action::Toggle(item.id.clone(), day));
                } else if resp.secondary_clicked() {
                    actions.push(Action::Edit(item.id.clone()));
                }
                resp.on_hover_text(if done { "Click to un-tick · right-click to edit" } else { "Click to tick off · right-click to edit" });
                prev = Some((node, done, color));
            }
            Entry::Ext(i) => {
                // all-day Google event: not a task, shown as a small grey diamond
                let ev = &data.gcache[i];
                four_diamond(painter, node, 5.0, google_color());
                painter.text(Pos2::new(rect.left() + x_off + 18.0, rect.center().y), Align2::LEFT_CENTER, &ev.title, FontId::proportional(14.0), google_color());
            }
        }
    }
    if any && all_done {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::hover());
        let node = Pos2::new(rect.left() + x_off, rect.center().y);
        super::streaks::gold_star(ui.painter(), node, 10.0);
        ui.painter().text(Pos2::new(rect.left() + x_off + 18.0, rect.center().y), Align2::LEFT_CENTER, "All done", FontId::proportional(14.0), super::gold_color());
    }
}

fn four_diamond(p: &egui::Painter, c: Pos2, r: f32, color: Color32) {
    p.add(egui::Shape::convex_polygon(
        vec![c + Vec2::new(0.0, -r), c + Vec2::new(r, 0.0), c + Vec2::new(0.0, r), c + Vec2::new(-r, 0.0)],
        color,
        Stroke::NONE,
    ));
}

fn now_line(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 10.0), Sense::hover());
    let red = super::theme::th().data.now_line;
    ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(1.5, red));
    ui.painter().circle_filled(r.left_center(), 4.0, red);
    ui.painter().text(
        r.right_center() - Vec2::new(4.0, 0.0),
        Align2::RIGHT_BOTTOM,
        format!("now {}", Local::now().format("%H:%M")),
        FontId::proportional(11.0),
        red,
    );
}

/// Entries logged on `day`, each with a remove button.
fn calorie_list(ui: &mut Ui, data: &Data, day: NaiveDate, actions: &mut Vec<Action>) {
    for c in data.calories.iter().filter(|c| c.date == day) {
        ui.horizontal(|ui| {
            let time = c.time.map(|t| t.format("%H:%M").to_string()).unwrap_or_default();
            ui.add_sized([40.0, 18.0], egui::Label::new(RichText::new(time).monospace().weak()));
            ui.add_sized([70.0, 18.0], egui::Label::new(RichText::new(format!("{} kcal", c.kcal)).strong().color(calorie_color())));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("✖").on_hover_text("Remove this entry").clicked() {
                    actions.push(Action::DeleteCalories(c.id.clone()));
                }
                if c.source == "claude" {
                    ui.label(RichText::new("Claude").size(11.0).color(claude_color()));
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let what = if c.description.is_empty() { "(no description)" } else { c.description.as_str() };
                    ui.add(egui::Label::new(RichText::new(what).size(13.0)).truncate()).on_hover_text(what);
                });
            });
        });
    }
}

/// Big daily total, today's budget, manual entry and quick-add buttons. Food descriptions
/// for Claude go through the entry bar ("ate …"). Laid out for the narrow right-hand panel.
fn calorie_card(ui: &mut Ui, data: &Data, input: &mut CalorieInput, actions: &mut Vec<Action>) {
    let today = Local::now().date_naive();
    let total = data.calories_on(today);
    super::theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                // counts up/down to the new total instead of jumping
                let shown = super::theme::animate(ui.ctx(), "kcal_total", total as f32, 0.6).round() as i64;
                ui.label(RichText::new(shown.to_string()).font(super::theme::bold(46.0)).color(calorie_color()));
                ui.vertical(|ui| {
                    ui.add_space(14.0);
                    ui.label(RichText::new("kcal today").size(16.0));
                });
            });
            if let Some(rec) = data.recommended_for(today) {
                let left = rec as i64 - total as i64;
                let (text, color) = if rec == 0 {
                    ("This month's budget is used up".to_string(), super::theme::th().status.danger)
                } else if left >= 0 {
                    (format!("{left} kcal left today"), calorie_color())
                } else {
                    (format!("{} kcal over today", -left), super::theme::th().status.danger)
                };
                let frac = if rec == 0 { 1.0 } else { (total as f32 / rec as f32).min(1.0) };
                ui.label(RichText::new(text).size(18.0).strong().color(color));
                ui.add(egui::ProgressBar::new(frac).desired_width(ui.available_width()).fill(color.linear_multiply(0.8)));
                ui.label(RichText::new(format!("Recommended today: {rec} kcal")).weak().size(12.5))
                    .on_hover_text("What's left of this month's calorie budget, spread over the days left in the month. Going over one day lowers the next days.");
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let k = ui.add(egui::TextEdit::singleline(&mut input.kcal).desired_width(64.0).hint_text("kcal"));
                let kcal: Option<u32> = input.kcal.trim().parse().ok().filter(|n| *n > 0 && *n <= 20000);
                let enter = k.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                // Button first (right-aligned), then the text field takes exactly what's left.
                // Sizing the field from available width before placing the button would
                // overshoot and make the resizable panel creep wider every frame.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(kcal.is_some(), egui::Button::new(RichText::new("+ Add").strong())).clicked() || (enter && kcal.is_some()) {
                        actions.push(Action::AddCalories(kcal.unwrap(), input.what.trim().to_string()));
                        input.kcal.clear();
                        input.what.clear();
                    }
                    ui.add(egui::TextEdit::singleline(&mut input.what).desired_width(ui.available_width()).hint_text("what was it? (optional)"));
                });
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Quick:").weak());
                for n in [100, 250, 500] {
                    if ui.button(format!("+{n}")).on_hover_text(format!("Log {n} kcal right now")).clicked() {
                        actions.push(Action::AddCalories(n, input.what.trim().to_string()));
                        input.what.clear();
                    }
                }
            });
            if data.calories.iter().any(|c| c.date == today) {
                ui.add_space(6.0);
                calorie_list(ui, data, today, actions);
            }
        });
}

/// One number per day: today's weight, with the change since the last weigh-in.
fn weight_row(ui: &mut Ui, data: &Data, input: &mut CalorieInput, actions: &mut Vec<Action>) {
    let today = Local::now().date_naive();
    let today_kg = data.weight_on(today);
    let previous = data.weight_before(today);
    super::theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("Weight").size(16.0).strong());
                if today_kg.is_some() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("✖").on_hover_text("Remove today's weight").clicked() {
                            actions.push(Action::SetWeight(None));
                        }
                    });
                }
            });
            match (today_kg, previous) {
                (Some(k), p) => {
                    ui.horizontal(|ui| {
                        let shown = super::theme::animate(ui.ctx(), "weight_today", k, 0.6);
                        ui.label(RichText::new(format!("{shown:.1} kg")).font(super::theme::bold(28.0)).color(weight_color()));
                        if let Some(p) = p {
                            ui.label(RichText::new(format!("{:+.1} kg since {}", k - p.kg, p.date.format("%b %-d"))).weak());
                        }
                    });
                }
                (None, Some(p)) => {
                    ui.label(RichText::new(format!("Not weighed today · last {:.1} kg on {}", p.kg, p.date.format("%b %-d"))).color(not_logged_color()));
                }
                (None, None) => {
                    ui.label(RichText::new("Not weighed today").color(not_logged_color()));
                }
            }
            ui.horizontal(|ui| {
                let hint = today_kg.or(previous.map(|p| p.kg)).map(|k| format!("{k:.1}")).unwrap_or_else(|| "kg".into());
                let r = ui.add(egui::TextEdit::singleline(&mut input.weight).desired_width(70.0).hint_text(hint));
                ui.label("kg");
                // accept "82,4" as well as "82.4"
                let kg: Option<f32> = input.weight.trim().replace(',', ".").parse().ok().filter(|k| (20.0..=400.0).contains(k));
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let label = if today_kg.is_some() { "Update" } else { "Save" };
                if ui.add_enabled(kg.is_some(), egui::Button::new(label)).clicked() || (enter && kg.is_some()) {
                    actions.push(Action::SetWeight(kg));
                    input.weight.clear();
                }
            });
        });
}

/// Backlog in the side panel: undated items plus overdue one-off items from earlier days,
/// each movable onto today's board.
fn backlog_list(ui: &mut Ui, data: &Data, actions: &mut Vec<Action>) {
    let today = Local::now().date_naive();
    let mut overdue: Vec<usize> = (0..data.items.len())
        .filter(|i| {
            let it = &data.items[*i];
            it.repeat.is_none() && !it.done && it.date.is_some_and(|d| d < today)
        })
        .collect();
    overdue.sort_by_key(|i| data.items[*i].date);
    let undated: Vec<usize> = (0..data.items.len()).filter(|i| data.items[*i].date.is_none() && !data.items[*i].done).collect();
    section(ui, "BACKLOG", overdue.len() + undated.len());
    if overdue.is_empty() && undated.is_empty() {
        ui.label(RichText::new("Nothing waiting. Overdue calendar items and things without a date show up here.").weak().size(12.5));
        return;
    }
    for i in overdue.into_iter().chain(undated) {
        let item = &data.items[i];
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::new(4.0, 18.0), Sense::hover());
            ui.painter().rect_filled(r, 2.0, kind_color(&item.kind));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Do today").on_hover_text("Move onto today's board").clicked() {
                    actions.push(Action::MoveToToday(item.id.clone()));
                }
                if let Some(d) = item.date {
                    ui.label(RichText::new(d.format("%b %-d").to_string()).size(12.0).color(super::theme::th().status.danger))
                        .on_hover_text("Overdue");
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let resp = ui.add(egui::Label::new(RichText::new(&item.title)).sense(Sense::click()).truncate());
                    if resp.on_hover_text("Click to edit").clicked() {
                        actions.push(Action::Edit(item.id.clone()));
                    }
                });
            });
        });
    }
}

/// Right-hand panel of the Habits page: calorie and weight entry, today's date, backlog.
pub fn today_side(ui: &mut Ui, data: &Data, cal: &mut CalorieInput, actions: &mut Vec<Action>) {
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(10.0);
        calorie_card(ui, data, cal, actions);
        ui.add_space(10.0);
        weight_row(ui, data, cal, actions);
        ui.add_space(8.0);
        ui.label(RichText::new(Local::now().format("%A, %B %-d").to_string()).weak());
        backlog_list(ui, data, actions);
        ui.add_space(10.0);
    });
}

/// Main area of the Habits page: the constellation and the board of today's anytime items.
/// Fills the window; the tiles scale with its width.
pub fn today(ui: &mut Ui, data: &Data, board: &mut BoardState, actions: &mut Vec<Action>) {
    let today = Local::now().date_naive();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(8.0);
        let (_, untimed) = entries_for(data, today);
        let board_ids: Vec<usize> = untimed.iter().filter_map(|e| if let Entry::Item(i) = e { Some(*i) } else { None }).collect();
        let all_day: Vec<String> =
            untimed.iter().filter_map(|e| if let Entry::Ext(i) = e { Some(data.gcache[*i].title.clone()) } else { None }).collect();
        anytime_board(ui, data, today, &board_ids, &all_day, board, actions);
        ui.add_space(20.0);
    });
}

pub fn day_panel(ui: &mut Ui, data: &Data, day: NaiveDate, actions: &mut Vec<Action>) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.heading(day.format("%a, %b %-d").to_string());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("+ New").on_hover_text("New item on this day").clicked() {
                actions.push(Action::NewAt(day, None));
            }
        });
    });
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let kcal = data.calories_on(day);
        if kcal > 0 {
            section(ui, &format!("CALORIES · {kcal} kcal"), 0);
            calorie_list(ui, data, day, actions);
        } else if day < Local::now().date_naive() {
            section(ui, "CALORIES", 0);
            let limit = data.settings.daily_calorie_limit;
            let note = if limit > 0 {
                format!("No food logged. Counted as your daily limit ({limit} kcal).")
            } else {
                "No food logged".to_string()
            };
            ui.label(RichText::new(note).italics().color(not_logged_color()));
        }
        day_list(ui, data, day, actions);
    });
}

pub fn calendar_header(ui: &mut Ui, mode: &mut CalMode, anchor: &mut NaiveDate, selected: &mut NaiveDate) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let step = |d: NaiveDate, fwd: bool, mode: CalMode| match mode {
            CalMode::Week => d + Duration::days(if fwd { 7 } else { -7 }),
            CalMode::Month => {
                let first = d.with_day(1).unwrap();
                if fwd {
                    (first + Duration::days(32)).with_day(1).unwrap()
                } else {
                    (first - Duration::days(1)).with_day(1).unwrap()
                }
            }
        };
        if ui.button("◀").clicked() {
            *anchor = step(*anchor, false, *mode);
            *selected = *anchor;
        }
        if ui.button("Today").clicked() {
            *anchor = Local::now().date_naive();
            *selected = *anchor;
        }
        if ui.button("▶").clicked() {
            *anchor = step(*anchor, true, *mode);
            *selected = *anchor;
        }
        let title = match mode {
            CalMode::Month => anchor.format("%B %Y").to_string(),
            CalMode::Week => {
                let ws = week_start(*anchor);
                format!("Week {} · {} – {}", ws.iso_week().week(), ws.format("%b %-d"), (ws + Duration::days(6)).format("%b %-d, %Y"))
            }
        };
        ui.heading(title);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.selectable_value(mode, CalMode::Week, "Week");
            ui.selectable_value(mode, CalMode::Month, "Month");
        });
    });
}

fn week_start(d: NaiveDate) -> NaiveDate {
    d - Duration::days(d.weekday().num_days_from_monday() as i64)
}

fn one_line(painter: &Painter, text: String, size: f32, color: Color32, width: f32) -> Arc<egui::Galley> {
    let mut job = LayoutJob::single_section(text, TextFormat::simple(FontId::proportional(size), color));
    job.wrap.max_width = width.max(1.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    painter.layout_job(job)
}

/// "October total: 45 300 / 62 000 kcal" with a bar against the monthly budget.
fn month_calorie_bar(ui: &mut Ui, data: &Data, anchor: NaiveDate) {
    let tally = data.month_tally(anchor, Local::now().date_naive());
    let total = tally.counted;
    let month = anchor.format("%B").to_string();
    ui.horizontal(|ui| {
        match data.month_budget(anchor) {
            Some(budget) => {
                let over = total > budget;
                let color = if over { super::theme::th().status.danger } else { calorie_color() };
                ui.label(RichText::new(format!("{month}: {total} / {budget} kcal")).strong().color(color));
                ui.add(
                    egui::ProgressBar::new((total as f32 / budget as f32).min(1.0))
                        .desired_width(ui.available_width().min(420.0))
                        .fill(color.linear_multiply(0.8))
                        .text(if over {
                            format!("{} over budget", total - budget)
                        } else {
                            format!("{} left this month", budget - total)
                        }),
                )
                .on_hover_text({
                    let limit = data.settings.daily_calorie_limit;
                    let mut t = format!("Budget = {limit} kcal daily limit × {} days", crate::store::days_in_month(anchor));
                    t += &format!("\nLogged: {} kcal", tally.logged);
                    if tally.unlogged_past_days > 0 {
                        t += &format!(
                            "\n+ {} past day(s) with nothing logged, counted at the limit ({} kcal)",
                            tally.unlogged_past_days,
                            tally.unlogged_past_days * limit
                        );
                    }
                    t
                });
                if tally.unlogged_past_days > 0 {
                    ui.label(RichText::new(format!("incl. {} unlogged day(s) at limit", tally.unlogged_past_days)).weak().size(12.0));
                }
            }
            None => {
                ui.label(RichText::new(format!("{month}: {total} kcal")).strong().color(calorie_color()));
                ui.label(RichText::new("Set a daily calorie limit in Settings to get a monthly budget").weak().size(12.0));
            }
        }
    });
    ui.add_space(4.0);
}

pub fn month(ui: &mut Ui, data: &Data, anchor: NaiveDate, selected: NaiveDate, actions: &mut Vec<Action>) {
    month_calorie_bar(ui, data, anchor);
    let first = anchor.with_day(1).unwrap();
    let grid_start = week_start(first);
    let grid_end = grid_start + Duration::days(41);
    let today = Local::now().date_naive();

    // Only tasks with a time are listed as text; everything else lives in the constellation.
    let mut by_day: HashMap<NaiveDate, Vec<(Option<NaiveTime>, String, Color32, bool)>> = HashMap::new();
    for (d, idx) in occurrences(&data.items, grid_start, grid_end) {
        let it = &data.items[idx];
        if it.time.is_some() {
            by_day.entry(d).or_default().push((it.time, it.title.clone(), kind_color(&it.kind), it.is_done_on(d)));
        }
    }
    for ev in &data.gcache {
        let d = ev.start.date();
        if !ev.all_day && d >= grid_start && d <= grid_end {
            by_day.entry(d).or_default().push((Some(ev.start.time()), ev.title.clone(), google_color(), false));
        }
    }
    for v in by_day.values_mut() {
        v.sort_by_key(|(t, ..)| t.unwrap_or(NaiveTime::MIN));
    }

    let visuals = ui.visuals().clone();
    let avail = ui.available_size();
    let header_h = 22.0;
    let (rect, _) = ui.allocate_exact_size(avail, Sense::hover());
    let cell_w = rect.width() / 7.0;
    let cell_h = ((rect.height() - header_h) / 6.0).max(60.0);
    let painter = ui.painter_at(rect);
    for (i, name) in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"].iter().enumerate() {
        painter.text(
            Pos2::new(rect.left() + cell_w * (i as f32 + 0.5), rect.top() + header_h / 2.0),
            Align2::CENTER_CENTER,
            *name,
            FontId::proportional(12.5),
            visuals.weak_text_color(),
        );
    }
    let line_h = 17.0;
    for n in 0..42 {
        let day = grid_start + Duration::days(n);
        let (row, col) = (n / 7, n % 7);
        let cell = Rect::from_min_size(
            Pos2::new(rect.left() + col as f32 * cell_w, rect.top() + header_h + row as f32 * cell_h),
            Vec2::new(cell_w, cell_h),
        )
        .shrink(1.5);
        let resp = ui.interact(cell, Id::new(("month_cell", day)), Sense::click());
        let in_month = day.month() == first.month();
        let bg = if day == selected {
            visuals.selection.bg_fill.linear_multiply(0.35)
        } else if resp.hovered() {
            visuals.widgets.hovered.weak_bg_fill
        } else if in_month && day.weekday().num_days_from_monday() >= 5 {
            super::theme::th().surface.weekend
        } else if in_month {
            visuals.faint_bg_color
        } else {
            visuals.extreme_bg_color
        };
        painter.rect_filled(cell, 6.0, bg);
        if day == today {
            // soft glow: a crisp inner line plus a few fading outer rings
            let c = super::theme::th().data.today;
            for i in 1..=4 {
                let a = 0.32 / i as f32;
                painter.rect_stroke(cell.expand(i as f32 * 1.6), 6.0 + i as f32 * 1.6, Stroke::new(1.6, c.linear_multiply(a)), StrokeKind::Outside);
            }
            painter.rect_stroke(cell, 6.0, Stroke::new(1.5, c), StrokeKind::Inside);
        }
        let num_color = if in_month { visuals.text_color() } else { visuals.weak_text_color() };
        painter.text(cell.left_top() + Vec2::new(6.0, 4.0), Align2::LEFT_TOP, day.day().to_string(), FontId::proportional(13.0), num_color);
        let kcal = data.calories_on(day);
        if kcal > 0 {
            painter.text(cell.right_top() + Vec2::new(-6.0, 5.0), Align2::RIGHT_TOP, format!("{kcal} kcal"), FontId::proportional(11.5), calorie_color());
        } else if day < today {
            painter.text(cell.right_top() + Vec2::new(-6.0, 5.0), Align2::RIGHT_TOP, "not logged", FontId::proportional(11.0), not_logged_color());
        }

        // Timed tasks as text (at most 2 lines), then the day's constellation fills the rest.
        let timed_lines = by_day.get(&day).map_or(0, |l| l.len().min(2));
        let stars_top = cell.top() + 22.0 + timed_lines as f32 * line_h;
        let stars = Rect::from_min_max(Pos2::new(cell.left() + 2.0, stars_top), Pos2::new(cell.right() - 2.0, cell.bottom() - 3.0));
        super::streaks::day_stars(&painter, stars, data, day, today, visuals.weak_text_color());
        if let Some(list) = by_day.get(&day) {
            let max_lines = 2;
            let shown = if list.len() > max_lines { max_lines - 1 } else { list.len() };
            let clip = painter.with_clip_rect(cell);
            for (k, (t, title, color, done)) in list.iter().take(shown).enumerate() {
                let y = cell.top() + 22.0 + k as f32 * line_h;
                let bar = Rect::from_min_size(Pos2::new(cell.left() + 4.0, y + 2.0), Vec2::new(3.0, line_h - 4.0));
                clip.rect_filled(bar, 1.0, *color);
                let label = match t {
                    Some(t) => format!("{} {title}", t.format("%H:%M")),
                    None => title.clone(),
                };
                let text_color = if *done { visuals.weak_text_color() } else { visuals.text_color() };
                let g = one_line(&clip, label, 12.0, text_color, cell.width() - 14.0);
                let pos = Pos2::new(cell.left() + 10.0, y + 1.0);
                if *done {
                    let w = g.size().x;
                    clip.line_segment([pos + Vec2::new(0.0, 7.0), pos + Vec2::new(w, 7.0)], Stroke::new(1.0, text_color));
                }
                clip.galley(pos, g, text_color);
            }
            if list.len() > shown {
                let y = cell.top() + 22.0 + shown as f32 * line_h;
                clip.text(Pos2::new(cell.left() + 10.0, y + 1.0), Align2::LEFT_TOP, format!("+{} more", list.len() - shown), FontId::proportional(11.5), visuals.weak_text_color());
            }
        }
        if resp.double_clicked() {
            actions.push(Action::NewAt(day, None));
        } else if resp.clicked() {
            actions.push(Action::SelectDay(day));
        }
    }
}

const HOUR_H: f32 = 44.0;
const GUTTER: f32 = 46.0;

pub fn week(ui: &mut Ui, data: &Data, anchor: NaiveDate, selected: NaiveDate, scrolled: &mut bool, actions: &mut Vec<Action>) {
    let ws = week_start(anchor);
    let we = ws + Duration::days(6);
    let today = Local::now().date_naive();
    let visuals = ui.visuals().clone();
    let width = ui.available_width();
    let col_w = (width - GUTTER) / 7.0;

    // Collect blocks per day.
    struct Block {
        start: f32, // minutes since midnight
        end: f32,
        title: String,
        color: Color32,
        done: bool,
        item_id: Option<String>,
    }
    let mut timed: Vec<Vec<Block>> = (0..7).map(|_| Vec::new()).collect();
    let mut allday: Vec<Vec<(String, Color32, Option<String>, bool)>> = (0..7).map(|_| Vec::new()).collect();
    for (d, idx) in occurrences(&data.items, ws, we) {
        let it = &data.items[idx];
        let col = (d - ws).num_days() as usize;
        let done = it.is_done_on(d);
        match it.time {
            Some(t) => {
                let s = (t.hour() * 60 + t.minute()) as f32;
                let e = (s + it.duration_min.unwrap_or(30) as f32).min(24.0 * 60.0);
                timed[col].push(Block { start: s, end: e.max(s + 20.0), title: it.title.clone(), color: kind_color(&it.kind), done, item_id: Some(it.id.clone()) });
            }
            None => allday[col].push((it.title.clone(), kind_color(&it.kind), Some(it.id.clone()), done)),
        }
    }
    for ev in &data.gcache {
        for col in 0..7 {
            let d = ws + Duration::days(col as i64);
            if !ev.on_day(d) {
                continue;
            }
            if ev.all_day {
                allday[col].push((ev.title.clone(), google_color(), None, false));
            } else {
                let s = if ev.start.date() < d { 0.0 } else { (ev.start.hour() * 60 + ev.start.minute()) as f32 };
                let e = if ev.end.date() > d { 24.0 * 60.0 } else { (ev.end.hour() * 60 + ev.end.minute()) as f32 };
                timed[col].push(Block { start: s, end: e.max(s + 20.0), title: ev.title.clone(), color: google_color(), done: false, item_id: None });
            }
        }
    }

    // Day header + all-day rows.
    let rows = allday.iter().map(|v| v.len()).max().unwrap_or(0);
    const DAY_H: f32 = 38.0;
    let head_h = DAY_H + 2.0 + rows as f32 * 19.0 + 4.0;
    let (head, _) = ui.allocate_exact_size(Vec2::new(width, head_h), Sense::hover());
    let painter = ui.painter_at(head);
    for col in 0..7 {
        let d = ws + Duration::days(col as i64);
        let x = head.left() + GUTTER + col as f32 * col_w;
        let hr = Rect::from_min_size(Pos2::new(x, head.top()), Vec2::new(col_w, DAY_H)).shrink(1.0);
        let resp = ui.interact(hr, Id::new(("week_head", d)), Sense::click());
        if d == selected {
            painter.rect_filled(hr, 4.0, visuals.selection.bg_fill.linear_multiply(0.35));
        }
        let color = if d == today { super::theme::th().status.success } else { visuals.text_color() };
        painter.text(hr.center_top() + Vec2::new(0.0, 11.0), Align2::CENTER_CENTER, d.format("%a %-d").to_string(), FontId::proportional(13.5), color);
        let kcal = data.calories_on(d);
        if kcal > 0 {
            painter.text(hr.center_top() + Vec2::new(0.0, 28.0), Align2::CENTER_CENTER, format!("{kcal} kcal"), FontId::proportional(11.0), calorie_color());
        } else if d < today {
            painter.text(hr.center_top() + Vec2::new(0.0, 28.0), Align2::CENTER_CENTER, "not logged", FontId::proportional(10.5), not_logged_color());
        }
        if resp.clicked() {
            actions.push(Action::SelectDay(d));
        }
        for (k, (title, c, id, done)) in allday[col].iter().enumerate() {
            let r = Rect::from_min_size(Pos2::new(x + 2.0, head.top() + DAY_H + 2.0 + k as f32 * 19.0), Vec2::new(col_w - 4.0, 17.0));
            painter.rect_filled(r, 3.0, c.linear_multiply(if *done { 0.25 } else { 0.55 }));
            let g = one_line(&painter, title.clone(), 12.0, super::theme::th().text.on_color, r.width() - 8.0);
            painter.galley(r.left_top() + Vec2::new(4.0, 1.5), g, super::theme::th().text.on_color);
            if let Some(id) = id {
                if ui.interact(r, Id::new(("allday", id, d)), Sense::click()).clicked() {
                    actions.push(Action::Edit(id.clone()));
                }
            }
        }
    }

    let mut scroll = egui::ScrollArea::vertical().auto_shrink([false, false]);
    if !*scrolled {
        scroll = scroll.vertical_scroll_offset(HOUR_H * 7.0);
        *scrolled = true;
    }
    scroll.show(ui, |ui| {
        let (grid, grid_resp) = ui.allocate_exact_size(Vec2::new(width, HOUR_H * 24.0), Sense::click());
        let painter = ui.painter_at(grid);
        for h in 0..24 {
            let y = grid.top() + h as f32 * HOUR_H;
            painter.line_segment([Pos2::new(grid.left() + GUTTER, y), Pos2::new(grid.right(), y)], Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color));
            painter.text(Pos2::new(grid.left() + GUTTER - 6.0, y + 2.0), Align2::RIGHT_TOP, format!("{h:02}:00"), FontId::monospace(11.0), visuals.weak_text_color());
        }
        // weekend columns a touch dimmer
        for col in 5..7 {
            let x = grid.left() + GUTTER + col as f32 * col_w;
            painter.rect_filled(Rect::from_min_size(Pos2::new(x, grid.top()), Vec2::new(col_w, grid.height())), 0.0, super::theme::th().surface.weekend.linear_multiply(0.6));
        }
        for col in 0..=7 {
            let x = grid.left() + GUTTER + col as f32 * col_w;
            painter.line_segment([Pos2::new(x, grid.top()), Pos2::new(x, grid.bottom())], Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color));
        }
        // Double-click empty space → new item at that time (rounded to 30 min).
        if grid_resp.double_clicked() {
            if let Some(p) = grid_resp.interact_pointer_pos() {
                let col = ((p.x - grid.left() - GUTTER) / col_w).floor();
                if (0.0..7.0).contains(&col) {
                    let mins = (((p.y - grid.top()) / HOUR_H * 60.0) / 30.0).floor() as u32 * 30;
                    let t = NaiveTime::from_hms_opt((mins / 60).min(23), mins % 60, 0);
                    actions.push(Action::NewAt(ws + Duration::days(col as i64), t));
                }
            }
        }

        for (col, blocks) in timed.iter_mut().enumerate() {
            blocks.sort_by(|a, b| a.start.total_cmp(&b.start));
            // Greedy lanes so overlapping blocks sit side by side.
            let mut lane_ends: Vec<f32> = Vec::new();
            let mut lanes = Vec::new();
            for b in blocks.iter() {
                let lane = match lane_ends.iter().position(|e| *e <= b.start) {
                    Some(l) => {
                        lane_ends[l] = b.end;
                        l
                    }
                    None => {
                        lane_ends.push(b.end);
                        lane_ends.len() - 1
                    }
                };
                lanes.push(lane);
            }
            let n_lanes = lane_ends.len().max(1) as f32;
            let x0 = grid.left() + GUTTER + col as f32 * col_w;
            let lane_w = (col_w - 4.0) / n_lanes;
            for (b, lane) in blocks.iter().zip(lanes) {
                let r = Rect::from_min_max(
                    Pos2::new(x0 + 2.0 + lane as f32 * lane_w, grid.top() + b.start / 60.0 * HOUR_H + 1.0),
                    Pos2::new(x0 + 2.0 + (lane + 1) as f32 * lane_w - 2.0, grid.top() + b.end / 60.0 * HOUR_H - 1.0),
                );
                let resp = b.item_id.as_ref().map(|id| ui.interact(r, Id::new(("block", id, col)), Sense::click()));
                let hovered = resp.as_ref().is_some_and(|r| r.hovered());
                let fill = b.color.linear_multiply(if b.done { 0.3 } else if hovered { 0.9 } else { 0.7 });
                painter.rect_filled(r, 4.0, fill);
                let clip = painter.with_clip_rect(r.intersect(grid));
                let h = (b.start / 60.0) as u32;
                let m = (b.start % 60.0) as u32;
                let label = format!("{h:02}:{m:02} {}", b.title);
                let mut job = LayoutJob::single_section(label, TextFormat::simple(FontId::proportional(12.0), super::theme::th().text.on_color));
                job.wrap.max_width = (r.width() - 6.0).max(1.0);
                job.wrap.max_rows = ((r.height() - 2.0) / 15.0).floor().max(1.0) as usize;
                job.wrap.overflow_character = Some('…');
                let g = clip.layout_job(job);
                clip.galley(r.left_top() + Vec2::new(4.0, 2.0), g, super::theme::th().text.on_color);
                if let (Some(resp), Some(id)) = (resp, &b.item_id) {
                    if resp.clicked() {
                        actions.push(Action::Edit(id.clone()));
                    }
                }
            }
        }

        // Current time line.
        if today >= ws && today <= we {
            let now = Local::now().time();
            let col = (today - ws).num_days() as f32;
            let y = grid.top() + (now.hour() * 60 + now.minute()) as f32 / 60.0 * HOUR_H;
            let x0 = grid.left() + GUTTER + col * col_w;
            let red = super::theme::th().data.now_line;
            painter.line_segment([Pos2::new(x0, y), Pos2::new(x0 + col_w, y)], Stroke::new(2.0, red));
            painter.circle_filled(Pos2::new(x0, y), 4.0, red);
        }
    });
}
