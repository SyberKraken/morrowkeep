//! Workouts tab. Step 1 of WORKOUTS_PLAN.md: the exercise library (list with search and
//! equipment/muscle filters, info page with a muscle map, editor). Workouts, the player and
//! history come in the next steps.

use super::theme;
use crate::store::Data;
use crate::workouts::{self, Exercise, Item, EQUIPMENT, KINDS, LEVELS, MUSCLES};
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke, Ui, Vec2};

#[derive(Clone, Copy, PartialEq)]
enum Section {
    Exercises,
    Workouts,
    History,
}

pub struct WorkoutsUi {
    section: Section,
    search: String,
    equipment: Vec<String>,
    muscle: Option<String>,
    selected: Option<String>,
    editing: bool,
    confirm_delete: bool,
    /// Selected workout, and the exercise search for adding to it.
    workout: Option<String>,
    add_search: String,
    /// The workout being done, full window.
    player: Option<super::player::Player>,
}

impl Default for WorkoutsUi {
    fn default() -> Self {
        WorkoutsUi { section: Section::Exercises, search: String::new(), equipment: Vec::new(), muscle: None, selected: None, editing: false, confirm_delete: false, workout: None, add_search: String::new(), player: None }
    }
}

pub fn workout_color() -> Color32 {
    theme::th().data.workout
}

fn chip(ui: &mut Ui, on: bool, text: &str) -> bool {
    ui.selectable_label(on, RichText::new(text).size(12.5)).clicked()
}

impl WorkoutsUi {
    pub fn page(&mut self, ctx: &egui::Context, data: &mut Data) {
        if let Some(p) = &mut self.player {
            let mut keep = true;
            egui::CentralPanel::default().show(ctx, |ui| keep = p.show(ui, data));
            if !keep {
                self.player = None;
            }
            return;
        }
        egui::SidePanel::left("workouts_side").resizable(false).exact_width(300.0).show(ctx, |ui| self.sidebar(ui, data));
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.set_max_width(860.0);
                ui.add_space(10.0);
                match self.section {
                    Section::Exercises => self.exercise_page(ui, data),
                    Section::Workouts => self.workout_page(ui, data),
                    Section::History => super::streaks::empty_state(ui, "History comes with the workout player", "Finished workouts will be listed here."),
                }
            });
        });
    }

    fn sidebar(&mut self, ui: &mut Ui, data: &mut Data) {
        let t = theme::th();
        ui.add_space(10.0);
        ui.heading("Workouts");
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            for (s, label) in [(Section::Exercises, "Exercises"), (Section::Workouts, "Workouts"), (Section::History, "History")] {
                ui.selectable_value(&mut self.section, s, label);
            }
        });
        ui.separator();
        if self.section == Section::Workouts {
            return self.workouts_sidebar(ui, data);
        }
        if self.section != Section::Exercises {
            return;
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search exercises…").desired_width(ui.available_width() - 70.0));
            if ui.button("+ New").on_hover_text("Add your own exercise").clicked() {
                let e = Exercise { id: format!("my-{}", uuid::Uuid::new_v4().simple()), name: "New exercise".into(), equipment: vec!["bodyweight".into()], level: "beginner".into(), kind: "reps".into(), ..Default::default() };
                self.selected = Some(e.id.clone());
                self.editing = true;
                data.exercises.exercises.push(e);
                data.save_exercises();
            }
        });
        ui.horizontal_wrapped(|ui| {
            for (key, label) in [("dumbbells", "Dumbbells"), ("bodyweight", "Bodyweight"), ("bench", "Bench")] {
                let on = self.equipment.iter().any(|e| e == key);
                if chip(ui, on, label) {
                    if on {
                        self.equipment.retain(|e| e != key);
                    } else {
                        self.equipment.push(key.into());
                    }
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            for m in ["chest", "shoulders", "upper back", "lats", "biceps", "triceps", "core", "glutes", "quads", "hamstrings", "full body"] {
                let on = self.muscle.as_deref() == Some(m);
                if chip(ui, on, m) {
                    self.muscle = if on { None } else { Some(m.into()) };
                }
            }
        });
        ui.separator();
        let mut list: Vec<&Exercise> = data
            .exercises
            .exercises
            .iter()
            .filter(|e| e.matches(&self.search))
            .filter(|e| self.equipment.iter().all(|q| e.needs(q)))
            .filter(|e| self.muscle.as_deref().is_none_or(|m| e.works(m)))
            .collect();
        list.sort_by_key(|e| e.name.to_lowercase());
        ui.label(RichText::new(format!("{} exercise{}", list.len(), if list.len() == 1 { "" } else { "s" })).size(12.0).color(t.text.muted));
        let mut pick = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                for e in list {
                    let sel = self.selected.as_deref() == Some(e.id.as_str());
                    let r = ui.add(egui::SelectableLabel::new(sel, &e.name));
                    ui.painter().text(r.rect.right_center() - Vec2::new(6.0, 0.0), egui::Align2::RIGHT_CENTER, e.equipment_short(), egui::FontId::proportional(11.5), t.text.muted);
                    if r.clicked() {
                        pick = Some(e.id.clone());
                    }
                }
            });
        });
        if let Some(id) = pick {
            self.selected = Some(id);
            self.editing = false;
            self.confirm_delete = false;
        }
    }

    fn exercise_page(&mut self, ui: &mut Ui, data: &mut Data) {
        let Some(i) = self.selected.as_ref().and_then(|id| data.exercises.exercises.iter().position(|e| e.id == *id)) else {
            ui.label(RichText::new("Exercise library").font(theme::bold(22.0)));
            ui.label(RichText::new("Home exercises for dumbbells, bodyweight and a flat bench. Pick one on the left for how to do it, good form and the muscles it works; filter by equipment or muscle.").color(theme::th().text.muted));
            return;
        };
        if self.editing {
            self.editor(ui, data, i);
        } else {
            self.info(ui, data, i);
        }
    }

    fn info(&mut self, ui: &mut Ui, data: &mut Data, i: usize) {
        let t = theme::th();
        let e = data.exercises.exercises[i].clone();
        let mut delete = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new(&e.name).font(theme::bold(22.0)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.confirm_delete {
                    if ui.small_button("No").clicked() {
                        self.confirm_delete = false;
                    }
                    if ui.small_button("Yes, delete").clicked() {
                        delete = true;
                    }
                    ui.label(RichText::new("Delete this exercise?").color(t.status.warning));
                } else {
                    if ui.small_button("🗑").on_hover_text("Delete").clicked() {
                        self.confirm_delete = true;
                    }
                    if ui.button("Edit").clicked() {
                        self.editing = true;
                    }
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
            let tag = |ui: &mut Ui, text: String, strong: bool| {
                let c = workout_color();
                egui::Frame::default()
                    .fill(if strong { c.linear_multiply(0.25) } else { t.widget.fill })
                    .corner_radius(10.0)
                    .inner_margin(egui::Margin::symmetric(8, 3))
                    .show(ui, |ui| ui.label(RichText::new(text).size(12.5)));
            };
            for m in &e.muscles_main {
                tag(ui, m.clone(), true);
            }
            for m in &e.muscles_helping {
                tag(ui, m.clone(), false);
            }
            for q in &e.equipment {
                tag(ui, workouts::equipment_label(q).to_string(), false);
            }
            tag(ui, workouts::level_label(&e.level).to_string(), false);
            tag(ui, workouts::kind_label(&e.kind).to_string(), false);
        });
        ui.add_space(8.0);
        ui.horizontal_top(|ui| {
            let text_w = (ui.available_width() - 200.0).max(300.0);
            ui.vertical(|ui| {
                ui.set_width(text_w);
                theme::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let section = |ui: &mut Ui, title: &str, lines: &[String], numbered: bool| {
                        if lines.is_empty() {
                            return;
                        }
                        ui.label(RichText::new(title).size(12.0).color(t.text.muted));
                        for (k, l) in lines.iter().enumerate() {
                            let text = if numbered { format!("{}. {l}", k + 1) } else { format!("•  {l}") };
                            ui.add(egui::Label::new(text).wrap());
                        }
                        ui.add_space(6.0);
                    };
                    section(ui, "HOW TO DO IT", &e.steps, true);
                    section(ui, "GOOD FORM", &e.form, false);
                    section(ui, "COMMON MISTAKES", &e.mistakes, false);
                    if !e.easier.is_empty() || !e.harder.is_empty() {
                        ui.label(RichText::new("EASIER · HARDER").size(12.0).color(t.text.muted));
                        ui.label(format!("{}  ·  {}", if e.easier.is_empty() { "–" } else { &e.easier }, if e.harder.is_empty() { "–" } else { &e.harder }));
                    }
                    if !e.notes.is_empty() {
                        ui.add_space(6.0);
                        ui.label(RichText::new(&e.notes).italics());
                    }
                });
            });
            theme::card().show(ui, |ui| {
                ui.vertical(|ui| {
                muscle_map(ui, &e);
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
                    ui.painter().rect_filled(r, 2.0, workout_color());
                    ui.label(RichText::new("main").size(11.5));
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
                    ui.painter().rect_filled(r, 2.0, workout_color().linear_multiply(0.22));
                    ui.label(RichText::new("helping").size(11.5));
                });
                });
            });
        });
        if delete {
            let removed = data.exercises.exercises.remove(i);
            if removed.builtin {
                data.exercises.removed.push(removed.id);
            }
            data.save_exercises();
            self.selected = None;
            self.confirm_delete = false;
        }
    }

    fn editor(&mut self, ui: &mut Ui, data: &mut Data, i: usize) {
        let t = theme::th();
        let mut changed = false;
        let e = &mut data.exercises.exercises[i];
        ui.horizontal(|ui| {
            ui.label(RichText::new("Edit exercise").font(theme::bold(20.0)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(RichText::new("Done").strong()).clicked() {
                    self.editing = false;
                }
            });
        });
        egui::Grid::new("ex_edit").num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
            ui.label("Name");
            changed |= ui.add(egui::TextEdit::singleline(&mut e.name).desired_width(360.0)).changed();
            ui.end_row();
            ui.label("Equipment");
            ui.horizontal_wrapped(|ui| {
                for (key, label) in EQUIPMENT {
                    let mut on = e.equipment.iter().any(|q| q == key);
                    if ui.checkbox(&mut on, *label).changed() {
                        if on {
                            e.equipment.push(key.to_string());
                        } else {
                            e.equipment.retain(|q| q != key);
                        }
                        changed = true;
                    }
                }
            });
            ui.end_row();
            for (label, main) in [("Main muscles", true), ("Helping muscles", false)] {
                ui.label(label);
                ui.horizontal_wrapped(|ui| {
                    for m in MUSCLES {
                        let list = if main { &mut e.muscles_main } else { &mut e.muscles_helping };
                        let on = list.iter().any(|x| x == m);
                        if chip(ui, on, m) {
                            if on {
                                list.retain(|x| x != m);
                            } else {
                                list.push(m.to_string());
                            }
                            changed = true;
                        }
                    }
                });
                ui.end_row();
            }
            ui.label("Level");
            egui::ComboBox::from_id_salt("ex_level").selected_text(workouts::level_label(&e.level).to_string()).show_ui(ui, |ui| {
                for l in LEVELS {
                    changed |= ui.selectable_value(&mut e.level, l.to_string(), workouts::level_label(l)).changed();
                }
            });
            ui.end_row();
            ui.label("Counted in");
            egui::ComboBox::from_id_salt("ex_kind").selected_text(workouts::kind_label(&e.kind).to_string()).show_ui(ui, |ui| {
                for (k, label) in KINDS {
                    changed |= ui.selectable_value(&mut e.kind, k.to_string(), *label).changed();
                }
            });
            ui.end_row();
            for (label, list) in [("Steps", &mut e.steps), ("Good form", &mut e.form), ("Mistakes", &mut e.mistakes)] {
                ui.label(label);
                let mut text = list.join("\n");
                if ui.add(egui::TextEdit::multiline(&mut text).desired_rows(3).desired_width(480.0).hint_text("one per line")).changed() {
                    *list = text.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
                    changed = true;
                }
                ui.end_row();
            }
            ui.label("Easier");
            changed |= ui.add(egui::TextEdit::singleline(&mut e.easier).desired_width(360.0)).changed();
            ui.end_row();
            ui.label("Harder");
            changed |= ui.add(egui::TextEdit::singleline(&mut e.harder).desired_width(360.0)).changed();
            ui.end_row();
            ui.label("Notes");
            changed |= ui.add(egui::TextEdit::multiline(&mut e.notes).desired_rows(2).desired_width(480.0)).changed();
            ui.end_row();
        });
        if e.builtin {
            ui.label(RichText::new("Built-in exercise: once edited, it keeps your version instead of updates.").size(12.0).color(t.text.muted));
        }
        if changed {
            e.edited = true;
            data.save_exercises();
        }
    }
}

/// Front and back outline with the worked muscles colored (main strong, helping light).
fn muscle_map(ui: &mut Ui, e: &Exercise) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(170.0, 160.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    let t = theme::th();
    let outline = Stroke::new(1.2, t.text.muted.linear_multiply(0.7));
    let main = workout_color();
    let help = main.linear_multiply(0.22);
    let level = |m: &str| -> Option<Color32> {
        if e.muscles_main.iter().any(|x| x == m || x == "full body") {
            Some(main)
        } else if e.muscles_helping.iter().any(|x| x == m || x == "full body") {
            Some(help)
        } else {
            None
        }
    };
    for (k, back) in [(0, false), (1, true)] {
        let o = rect.min + Vec2::new(10.0 + k as f32 * 85.0, 6.0);
        let r = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(o + Vec2::new(x0, y0), o + Vec2::new(x1, y1));
        let fill = |m: &str, rects: &[Rect]| {
            if let Some(c) = level(m) {
                for rr in rects {
                    p.rect_filled(*rr, 3.0, c);
                }
            }
        };
        if back {
            fill("upper back", &[r(19.0, 24.0, 41.0, 34.0)]);
            fill("rear delts", &[r(12.0, 22.0, 19.0, 30.0), r(41.0, 22.0, 48.0, 30.0)]);
            fill("lats", &[r(18.0, 34.0, 24.0, 52.0), r(36.0, 34.0, 42.0, 52.0)]);
            fill("triceps", &[r(8.0, 31.0, 14.0, 50.0), r(46.0, 31.0, 52.0, 50.0)]);
            fill("lower back", &[r(24.0, 50.0, 36.0, 62.0)]);
            fill("glutes", &[r(20.0, 62.0, 40.0, 74.0)]);
            fill("hamstrings", &[r(20.0, 76.0, 29.0, 100.0), r(31.0, 76.0, 40.0, 100.0)]);
            fill("calves", &[r(21.0, 104.0, 28.0, 126.0), r(32.0, 104.0, 39.0, 126.0)]);
        } else {
            fill("shoulders", &[r(12.0, 22.0, 19.0, 30.0), r(41.0, 22.0, 48.0, 30.0)]);
            fill("chest", &[r(19.0, 24.0, 41.0, 37.0)]);
            fill("biceps", &[r(8.0, 31.0, 14.0, 50.0), r(46.0, 31.0, 52.0, 50.0)]);
            fill("core", &[r(23.0, 39.0, 37.0, 62.0)]);
            fill("obliques", &[r(19.0, 41.0, 23.0, 62.0), r(37.0, 41.0, 41.0, 62.0)]);
            fill("hip flexors", &[r(22.0, 62.0, 38.0, 68.0)]);
            fill("quads", &[r(20.0, 70.0, 29.0, 100.0), r(31.0, 70.0, 40.0, 100.0)]);
        }
        fill("forearms", &[r(6.0, 52.0, 12.0, 68.0), r(48.0, 52.0, 54.0, 68.0)]);
        // outline: head, torso, arms, legs
        p.circle_stroke(o + Vec2::new(30.0, 10.0), 8.0, outline);
        p.rect_stroke(r(17.0, 20.0, 43.0, 66.0), 7.0, outline, egui::StrokeKind::Middle);
        for (x0, x1) in [(6.0, 15.0), (45.0, 54.0)] {
            p.rect_stroke(r(x0, 22.0, x1, 70.0), 4.0, outline, egui::StrokeKind::Middle);
        }
        for (x0, x1) in [(19.0, 29.5), (30.5, 41.0)] {
            p.rect_stroke(r(x0, 66.0, x1, 132.0), 4.0, outline, egui::StrokeKind::Middle);
        }
        p.text(o + Vec2::new(30.0, 146.0), egui::Align2::CENTER_CENTER, if back { "back" } else { "front" }, egui::FontId::proportional(11.0), t.text.muted);
    }
    let _ = Pos2::ZERO;
}

// ---------------------------------------------------------------- workouts (step 2)

/// An optional whole number with "–" for none.
fn opt_u32(ui: &mut Ui, v: &mut Option<u32>, max: u32, suffix: &str, tip: &str) -> bool {
    let mut x = v.map_or(-1.0, |n| n as f64);
    let suffix = suffix.to_string();
    let r = ui
        .add(
            egui::DragValue::new(&mut x)
                .range(-1.0..=max as f64)
                .speed(0.2)
                .custom_formatter(move |n, _| if n < 0.0 { "–".into() } else { format!("{n:.0}{suffix}") })
                .custom_parser(|s| {
                    let s = s.trim().trim_end_matches(|c: char| c.is_alphabetic() || c == ' ');
                    if s.is_empty() || s == "-" || s == "–" { Some(-1.0) } else { s.parse().ok() }
                }),
        )
        .on_hover_text(tip);
    if r.changed() {
        *v = (x >= 0.5).then_some(x.round() as u32);
        true
    } else {
        false
    }
}

fn opt_kg(ui: &mut Ui, v: &mut Option<f32>) -> bool {
    let mut x = v.unwrap_or(-0.5);
    let r = ui
        .add(
            egui::DragValue::new(&mut x)
                .range(-0.5..=100.0)
                .speed(0.25)
                .custom_formatter(|n, _| if n < 0.0 { "– kg".into() } else { format!("{n:.1} kg") })
                .custom_parser(|s| {
                    let s = s.trim().trim_end_matches("kg").trim();
                    if s.is_empty() || s == "-" || s == "–" { Some(-0.5) } else { s.replace(',', ".").parse().ok() }
                }),
        )
        .on_hover_text("kg per dumbbell (two 15 kg dumbbells = 15)");
    if r.changed() {
        *v = (x > 0.0).then_some((x * 4.0).round() / 4.0);
        true
    } else {
        false
    }
}

impl WorkoutsUi {
    fn workouts_sidebar(&mut self, ui: &mut Ui, data: &mut Data) {
        let t = theme::th();
        if ui.button("+ New workout").clicked() {
            let w = workouts::Workout { id: format!("my-{}", uuid::Uuid::new_v4().simple()), name: "New workout".into(), ..Default::default() };
            self.workout = Some(w.id.clone());
            data.workouts.workouts.push(w);
            data.save_workouts();
        }
        ui.add_space(4.0);
        let lib = &data.exercises.exercises;
        let mut pick = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                for w in &data.workouts.workouts {
                    let sel = self.workout.as_deref() == Some(w.id.as_str());
                    let r = ui.add(egui::SelectableLabel::new(sel, &w.name));
                    ui.painter().text(r.rect.right_center() - Vec2::new(6.0, 0.0), egui::Align2::RIGHT_CENTER, workouts::duration_label(w.seconds(lib)), egui::FontId::proportional(11.5), t.text.muted);
                    if r.clicked() {
                        pick = Some(w.id.clone());
                    }
                }
            });
        });
        if let Some(id) = pick {
            self.workout = Some(id);
            self.confirm_delete = false;
            self.add_search.clear();
        }
    }

    fn workout_page(&mut self, ui: &mut Ui, data: &mut Data) {
        let t = theme::th();
        let Some(wi) = self.workout.as_ref().and_then(|id| data.workouts.workouts.iter().position(|w| w.id == *id)) else {
            ui.label(RichText::new("Workouts").font(theme::bold(22.0)));
            ui.label(RichText::new("Exercises in order, each with sets and reps or a time (kg per dumbbell), plus timed breaks. Pick one on the left, or make a new one.").color(t.text.muted));
            return;
        };
        let lib = data.exercises.exercises.clone();
        let mut changed = false;
        let mut delete = false;
        let mut duplicate = false;
        let mut start = false;
        let (mut swap, mut remove): (Option<(usize, usize)>, Option<usize>) = (None, None);
        {
            let w = &mut data.workouts.workouts[wi];
            ui.horizontal(|ui| {
                changed |= ui.add(egui::TextEdit::singleline(&mut w.name).font(egui::FontId::proportional(20.0)).desired_width(420.0)).changed();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.confirm_delete {
                        if ui.small_button("No").clicked() {
                            self.confirm_delete = false;
                        }
                        if ui.small_button("Yes, delete").clicked() {
                            delete = true;
                        }
                        ui.label(RichText::new("Delete this workout?").color(t.status.warning));
                    } else {
                        if ui.small_button("🗑").on_hover_text("Delete").clicked() {
                            self.confirm_delete = true;
                        }
                        if ui.small_button("Duplicate").clicked() {
                            duplicate = true;
                        }
                        let can = !w.items.is_empty();
                        if ui.add_enabled(can, egui::Button::new(RichText::new("▶ Start workout").strong()).fill(workout_color().linear_multiply(0.35))).clicked() {
                            start = true;
                        }
                    }
                });
            });
            let eq: Vec<String> = w.equipment(&lib).iter().map(|q| workouts::equipment_label(q).to_lowercase()).collect();
            ui.label(
                RichText::new(format!(
                    "≈ {} · {} exercise{} · {}",
                    workouts::duration_label(w.seconds(&lib)),
                    w.exercise_count(),
                    if w.exercise_count() == 1 { "" } else { "s" },
                    if eq.is_empty() { "no equipment".to_string() } else { eq.join(", ") }
                ))
                .color(t.text.muted),
            );
            changed |= ui.add(egui::TextEdit::multiline(&mut w.notes).desired_rows(1).desired_width(f32::INFINITY).hint_text("notes (optional)")).changed();
            ui.add_space(6.0);
            let n = w.items.len();
            for (k, item) in w.items.iter_mut().enumerate() {
                let is_break = matches!(item, Item::Break { .. });
                let frame = egui::Frame::default()
                    .fill(if is_break { t.surface.page } else { t.surface.card })
                    .stroke(Stroke::new(1.0, if is_break { theme::th().data.rest.linear_multiply(0.6) } else { t.surface.card_border }))
                    .corner_radius(8.0)
                    .inner_margin(egui::Margin::symmetric(10, 6));
                frame.show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{}", k + 1)).color(t.text.muted).monospace());
                        match item {
                            Item::Exercise { exercise, sets, reps, seconds, kg, rest_s } => {
                                let e = lib.iter().find(|e| e.id == *exercise);
                                let name = e.map_or("(deleted exercise)".to_string(), |e| e.name.clone());
                                let uses_db = e.is_some_and(|e| e.needs("dumbbells"));
                                let per_side = e.is_some_and(|e| e.kind == "reps_per_side");
                                ui.allocate_ui_with_layout(Vec2::new(220.0, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                    ui.set_min_width(220.0);
                                    ui.add(egui::Label::new(RichText::new(name).strong()).truncate());
                                });
                                changed |= ui
                                    .add(egui::DragValue::new(sets).range(1..=20).custom_formatter(|n, _| if n == 1.0 { "1 set".into() } else { format!("{n:.0} sets") }))
                                    .changed();
                                ui.label("×");
                                changed |= opt_u32(ui, reps, 200, if per_side { " reps/side" } else { " reps" }, "Reps per set (– for none)");
                                changed |= opt_u32(ui, seconds, 3600, " s", "Seconds per set. With a time, the set counts down and moves on by itself; with only reps, the player waits for Done set.");
                                if uses_db {
                                    changed |= opt_kg(ui, kg);
                                }
                                ui.label(RichText::new("rest").color(t.text.muted));
                                changed |= ui.add(egui::DragValue::new(rest_s).range(0..=600).speed(1.0).suffix(" s")).on_hover_text("Rest between sets").changed();
                            }
                            Item::Break { seconds, label } => {
                                ui.label(RichText::new("Break").color(theme::th().data.rest).strong());
                                changed |= ui.add(egui::DragValue::new(seconds).range(5..=1800).speed(1.0).suffix(" s")).changed();
                                changed |= ui.add(egui::TextEdit::singleline(label).desired_width(220.0).hint_text("label (optional)")).changed();
                            }
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("✖").on_hover_text("Remove").clicked() {
                                remove = Some(k);
                            }
                            if k + 1 < n && ui.small_button("⬇").on_hover_text("Move down").clicked() {
                                swap = Some((k, k + 1));
                            }
                            if k > 0 && ui.small_button("⬆").on_hover_text("Move up").clicked() {
                                swap = Some((k - 1, k));
                            }
                        });
                    });
                });
                ui.add_space(3.0);
            }
        }
        // adding items
        ui.add_space(4.0);
        let mut add: Option<Item> = None;
        ui.horizontal(|ui| {
            ui.label("Add");
            ui.add(egui::TextEdit::singleline(&mut self.add_search).hint_text("search exercises…").desired_width(240.0));
            if ui.button("+ Break").clicked() {
                add = Some(Item::Break { seconds: 60, label: String::new() });
            }
        });
        if !self.add_search.trim().is_empty() {
            ui.horizontal_wrapped(|ui| {
                for e in lib.iter().filter(|e| e.matches(&self.add_search)).take(10) {
                    if ui.button(format!("+ {}", e.name)).clicked() {
                        add = Some(match e.kind.as_str() {
                            "time" => Item::Exercise { exercise: e.id.clone(), sets: 2, reps: None, seconds: Some(30), kg: None, rest_s: 20 },
                            _ => Item::Exercise { exercise: e.id.clone(), sets: 3, reps: Some(10), seconds: None, kg: None, rest_s: 60 },
                        });
                    }
                }
            });
        }
        let w = &mut data.workouts.workouts[wi];
        if let Some(a) = add {
            w.items.push(a);
            self.add_search.clear();
            changed = true;
        }
        if let Some((a, b)) = swap {
            w.items.swap(a, b);
            changed = true;
        }
        if let Some(k) = remove {
            w.items.remove(k);
            changed = true;
        }
        if changed {
            w.edited = true;
            data.save_workouts();
        }
        if start {
            self.start_player(data, wi);
        }
        if duplicate {
            let mut copy = data.workouts.workouts[wi].clone();
            copy.id = format!("my-{}", uuid::Uuid::new_v4().simple());
            copy.name = format!("{} (copy)", copy.name);
            copy.builtin = false;
            copy.edited = false;
            self.workout = Some(copy.id.clone());
            data.workouts.workouts.push(copy);
            data.save_workouts();
        }
        if delete {
            let w = data.workouts.workouts.remove(wi);
            if w.builtin {
                data.workouts.removed.push(w.id);
            }
            data.save_workouts();
            self.workout = None;
            self.confirm_delete = false;
        }
    }
}

impl WorkoutsUi {
    fn start_player(&mut self, data: &mut Data, wi: usize) {
        self.player = Some(super::player::Player::new(&data.workouts.workouts[wi], &data.exercises.exercises));
    }
}
