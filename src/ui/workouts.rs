//! Workouts tab: the exercise library (list with search and equipment/muscle filters, info
//! page with a muscle map, editor), the workout builder, history, and Claude fill-in/build.
//! The player is in `player.rs`.

use super::theme;
use crate::store::Data;
use crate::workouts::{self, Exercise, Item, EQUIPMENT, KINDS, LEVELS, MUSCLES};
use chrono::Datelike;
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
    /// Selected session in History.
    session: Option<String>,
    /// Claude: filling in an exercise (id, reply); building a workout (request, reply, result).
    filling: Option<(String, std::sync::mpsc::Receiver<Result<String, String>>)>,
    build_text: String,
    build_request: String,
    building: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    built: Option<String>,
}

impl Default for WorkoutsUi {
    fn default() -> Self {
        WorkoutsUi { section: Section::Exercises, search: String::new(), equipment: Vec::new(), muscle: None, selected: None, editing: false, confirm_delete: false, workout: None, add_search: String::new(), player: None, session: None, filling: None, build_text: String::new(), build_request: String::new(), building: None, built: None }
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
        self.poll_claude(ctx, data);
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
                    Section::Workouts => {
                        self.built_preview(ui, data);
                        self.workout_page(ui, data);
                    }
                    Section::History => self.history_page(ui, data),
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
        if self.section == Section::History {
            return self.history_sidebar(ui, data);
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
        ui.add_space(8.0);
        exercise_history(ui, data, &e.id);
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
        let (eid, ename) = (e.id.clone(), e.name.clone());
        let mut fill = false;
        if self.filling.as_ref().is_some_and(|(id, _)| *id == eid) {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Claude is writing it up…");
            });
        } else if ui
            .button(RichText::new("✨ Fill in with Claude").color(t.data.claude))
            .on_hover_text("Claude writes the steps, form cues, muscles and equipment from the name (uses a little of your Claude plan)")
            .clicked()
        {
            fill = true;
        }
        if changed {
            e.edited = true;
            data.save_exercises();
        }
        if fill {
            self.start_fill(data, &eid, &ename);
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
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.build_text).hint_text("30 min upper body…").desired_width(ui.available_width() - 82.0));
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.add_enabled(self.building.is_none(), egui::Button::new(RichText::new("✨ Build").color(theme::th().data.claude))).on_hover_text("Claude builds a workout from your exercise library; you review it before saving").clicked() || enter)
                && self.building.is_none()
            {
                self.start_build(data);
            }
        });
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
        // tasks a workout can tick off: repeating ones, and dated ones from today on
        let today = chrono::Local::now().date_naive();
        let mut task_titles: Vec<(String, String)> = data
            .items
            .iter()
            .filter(|it| it.repeat.is_some() || (!it.done && it.date.is_some_and(|d| d >= today)))
            .map(|it| (it.id.clone(), if it.repeat.is_some() { format!("{} (repeating)", it.title) } else { it.title.clone() }))
            .collect();
        task_titles.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
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
            ui.horizontal(|ui| {
                ui.label(RichText::new("When finished, tick off").color(t.text.muted));
                let current = w.task.as_ref().and_then(|id| task_titles.iter().find(|(i, _)| i == id)).map_or("no task".to_string(), |(_, title)| title.clone());
                egui::ComboBox::from_id_salt("wk_task").selected_text(current).width(260.0).show_ui(ui, |ui| {
                    changed |= ui.selectable_value(&mut w.task, None, "no task").changed();
                    for (id, title) in &task_titles {
                        changed |= ui.selectable_value(&mut w.task, Some(id.clone()), title).changed();
                    }
                });
            });
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

// ---------------------------------------------------------------- history (step 4)

/// "Your history" for one exercise: last time, best, and progress over time.
fn exercise_history(ui: &mut Ui, data: &Data, exercise: &str) {
    let t = theme::th();
    let days = data.workout_log.exercise_days(exercise);
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("YOUR HISTORY").size(12.0).color(t.text.muted));
        let Some(last) = days.last() else {
            ui.label(RichText::new("Not done yet. Sets you do in a workout show up here.").color(t.text.muted));
            return;
        };
        let best_kg = days.iter().filter_map(|d| d.top_kg()).fold(None, |m: Option<f32>, k| Some(m.map_or(k, |m| m.max(k))));
        let best_reps = days.iter().filter_map(|d| d.top_reps()).max();
        let mut line = format!("Last: {} · {}", last.date.format("%a %b %-d"), last.summary());
        match (best_kg, best_reps) {
            (Some(k), _) => line += &format!("   ·   Best: {k} kg"),
            (None, Some(r)) => line += &format!("   ·   Best: {r} reps"),
            _ => {}
        }
        line += &format!("   ·   {} time{}", days.len(), if days.len() == 1 { "" } else { "s" });
        ui.label(line);
        // progress: top kg per day (or top reps for bodyweight work)
        let by_kg = best_kg.is_some();
        let pts: Vec<[f64; 2]> = days
            .iter()
            .filter_map(|d| {
                let y = if by_kg { d.top_kg().map(|k| k as f64) } else { d.top_reps().map(|r| r as f64) }?;
                Some([d.date.num_days_from_ce() as f64, y])
            })
            .collect();
        if pts.len() >= 2 {
            let unit = if by_kg { "kg" } else { "reps" };
            egui_plot::Plot::new(("ex_progress", exercise))
                .height(120.0)
                .allow_zoom(false)
                .allow_drag(false)
                .allow_scroll(false)
                .show_axes([false, true])
                .y_axis_formatter(move |m, _| format!("{:.0} {unit}", m.value))
                .label_formatter(|_, p| chrono::NaiveDate::from_num_days_from_ce_opt(p.x.round() as i32).map(|d| format!("{}\n{:.1}", d.format("%b %-d"), p.y)).unwrap_or_default())
                .show(ui, |plot| {
                    plot.line(egui_plot::Line::new(egui_plot::PlotPoints::new(pts.clone())).color(workout_color()).width(2.0));
                    plot.points(egui_plot::Points::new(egui_plot::PlotPoints::new(pts)).color(workout_color()).radius(3.5));
                });
        }
    });
}

impl WorkoutsUi {
    fn history_sidebar(&mut self, ui: &mut Ui, data: &Data) {
        let t = theme::th();
        let mut sessions: Vec<&workouts::Session> = data.workout_log.sessions.iter().collect();
        sessions.sort_by(|a, b| (b.date, &b.start).cmp(&(a.date, &a.start)));
        if sessions.is_empty() {
            ui.label(RichText::new("No workouts done yet.").color(t.text.muted));
            return;
        }
        let mut pick = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                for s in sessions {
                    let sel = self.session.as_deref() == Some(s.id.as_str());
                    let label = format!("{} · {} · {}", s.date.format("%a %b %-d"), s.workout, workouts::duration_label(s.duration_s));
                    let r = ui.add(egui::SelectableLabel::new(sel, label));
                    if r.clicked() {
                        pick = Some(s.id.clone());
                    }
                }
            });
        });
        if let Some(id) = pick {
            self.session = Some(id);
            self.confirm_delete = false;
        }
    }

    fn history_page(&mut self, ui: &mut Ui, data: &mut Data) {
        let t = theme::th();
        let Some(i) = self.session.as_ref().and_then(|id| data.workout_log.sessions.iter().position(|s| s.id == *id)) else {
            let n = data.workout_log.sessions.len();
            let week = data.workout_log.sessions.iter().filter(|s| s.date > chrono::Local::now().date_naive() - chrono::Duration::days(7)).count();
            ui.label(RichText::new("Workout history").font(theme::bold(22.0)));
            ui.label(RichText::new(format!("{n} workout{} in total · {week} in the last 7 days. Pick one on the left for its sets.", if n == 1 { "" } else { "s" })).color(t.text.muted));
            return;
        };
        let s = data.workout_log.sessions[i].clone();
        let mut delete = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new(&s.workout).font(theme::bold(22.0)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.confirm_delete {
                    if ui.small_button("No").clicked() {
                        self.confirm_delete = false;
                    }
                    if ui.small_button("Yes, delete").clicked() {
                        delete = true;
                    }
                    ui.label(RichText::new("Delete this session?").color(t.status.warning));
                } else if ui.small_button("🗑").on_hover_text("Delete").clicked() {
                    self.confirm_delete = true;
                }
            });
        });
        let volume: f32 = s.sets.iter().map(|x| x.reps.unwrap_or(0) as f32 * x.kg.unwrap_or(0.0)).sum();
        let mut line = format!("{} at {} · {} · {} sets", s.date.format("%A %b %-d"), s.start, workouts::duration_label(s.duration_s), s.sets.len());
        if volume > 0.0 {
            line += &format!(" · {volume:.0} kg lifted");
        }
        if !s.completed {
            line += " · ended early";
        }
        ui.label(RichText::new(line).color(t.text.muted));
        ui.add_space(8.0);
        // sets grouped by exercise, in order
        let mut groups: Vec<(String, Vec<&workouts::SetDone>)> = Vec::new();
        for x in &s.sets {
            match groups.last_mut() {
                Some((ex, v)) if *ex == x.exercise => v.push(x),
                _ => groups.push((x.exercise.clone(), vec![x])),
            }
        }
        for (_, sets) in groups {
            theme::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(&sets[0].name).strong());
                for x in sets {
                    let mut l = format!("set {}", x.set);
                    if let Some(r) = x.reps {
                        l += &format!(" · {r} reps");
                    }
                    if let Some(k) = x.kg {
                        l += &format!(" · {k} kg");
                    }
                    l += &format!(" · {}", workouts::duration_label(x.seconds));
                    ui.label(RichText::new(l).color(t.text.muted));
                }
            });
            ui.add_space(4.0);
        }
        if delete {
            data.workout_log.sessions.remove(i);
            data.save_workout_log();
            self.session = None;
            self.confirm_delete = false;
        }
    }
}

// ---------------------------------------------------------------- Claude (step 5)

fn ask_claude_bg(data: &Data, prompt: String) -> Option<std::sync::mpsc::Receiver<Result<String, String>>> {
    let cli = crate::claude::find_cli(&data.settings)?;
    let (model, dir) = (data.settings.claude_model.clone(), data.paths.root.clone());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(crate::claude::ask(&cli, &model, &prompt, &dir));
    });
    Some(rx)
}

impl WorkoutsUi {
    /// Finishes background Claude requests (exercise fill-in, workout building).
    fn poll_claude(&mut self, ctx: &egui::Context, data: &mut Data) {
        if self.filling.is_some() || self.building.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(300));
        }
        if let Some((id, rx)) = &self.filling {
            if let Ok(res) = rx.try_recv() {
                let id = id.clone();
                self.filling = None;
                match res.and_then(|text| fill_from(&text)) {
                    Ok(v) => {
                        if let Some(e) = data.exercises.exercises.iter_mut().find(|e| e.id == id) {
                            let list = |k: &str, allowed: &dyn Fn(&str) -> bool| -> Vec<String> {
                                v[k].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).filter(|x| allowed(x)).map(str::to_string).collect()).unwrap_or_default()
                            };
                            let text = |k: &str| -> Vec<String> { v[k].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()).unwrap_or_default() };
                            let eq = list("equipment", &|x| EQUIPMENT.iter().any(|(k, _)| *k == x));
                            if !eq.is_empty() {
                                e.equipment = eq;
                            }
                            e.muscles_main = list("muscles_main", &|x| MUSCLES.contains(&x));
                            e.muscles_helping = list("muscles_helping", &|x| MUSCLES.contains(&x));
                            if let Some(l) = v["level"].as_str().filter(|l| LEVELS.contains(l)) {
                                e.level = l.into();
                            }
                            if let Some(k) = v["kind"].as_str().filter(|k| KINDS.iter().any(|(x, _)| x == k)) {
                                e.kind = k.into();
                            }
                            e.steps = text("steps");
                            e.form = text("form");
                            e.mistakes = text("mistakes");
                            e.easier = v["easier"].as_str().unwrap_or("").trim().to_string();
                            e.harder = v["harder"].as_str().unwrap_or("").trim().to_string();
                            e.edited = true;
                            let name = e.name.clone();
                            data.save_exercises();
                            data.status = format!("Claude filled in {name}. Check it over.");
                        }
                    }
                    Err(err) => data.status = format!("Claude couldn't fill it in: {err}"),
                }
            }
        }
        if let Some(rx) = &self.building {
            if let Ok(res) = rx.try_recv() {
                self.building = None;
                match res {
                    Ok(text) => self.built = Some(text),
                    Err(e) => data.status = format!("Claude: {e}"),
                }
            }
        }
    }

    fn start_fill(&mut self, data: &mut Data, id: &str, name: &str) {
        let muscles = MUSCLES.join(", ");
        let equipment = EQUIPMENT.iter().map(|(k, _)| *k).collect::<Vec<_>>().join(", ");
        let prompt = format!(
            "Describe the home exercise \"{name}\" for an exercise library (equipment available: dumbbells, a flat bench that doesn't recline, a mat). \
             Reply with ONLY this JSON, no other text: {{\"equipment\": [from: {equipment}], \"muscles_main\": [from: {muscles}], \"muscles_helping\": [same list], \
             \"level\": \"beginner\"|\"intermediate\"|\"advanced\", \"kind\": \"reps\"|\"reps_per_side\"|\"time\", \"steps\": [3-5 short steps], \"form\": [2-3 good-form cues], \
             \"mistakes\": [1-3 common mistakes], \"easier\": \"an easier version\", \"harder\": \"a harder version\"}}"
        );
        match ask_claude_bg(data, prompt) {
            Some(rx) => self.filling = Some((id.to_string(), rx)),
            None => data.status = "Claude Code wasn't found (Settings > Input & voice)".into(),
        }
    }

    fn start_build(&mut self, data: &mut Data) {
        let request = self.build_text.trim().to_string();
        if request.is_empty() {
            return;
        }
        let prompt = crate::claude::build_prompt(
            data,
            &format!(
                "Create a home workout and add it under \"workouts\" (only use exercise ids from my exercise library; I have dumbbells, a flat non-reclining bench and a mat; \
                 kg is per dumbbell; give reps or seconds for every exercise, rest_s between sets, and breaks where they make sense). The workout: {request}"
            ),
        );
        match ask_claude_bg(data, prompt) {
            Some(rx) => {
                self.building = Some(rx);
                self.built = None;
                self.build_request = request;
            }
            None => data.status = "Claude Code wasn't found (Settings > Input & voice)".into(),
        }
    }

    /// Claude's workout, for review: Save or Discard.
    fn built_preview(&mut self, ui: &mut Ui, data: &mut Data) {
        let t = theme::th();
        if self.building.is_some() {
            theme::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(format!("Claude is building \"{}\"…", self.build_request)).color(t.data.claude));
                });
            });
            ui.add_space(8.0);
            return;
        }
        let Some(text) = self.built.clone() else { return };
        let plan = crate::import::plan(&text, &data.items, &data.calories, &data.settings, &data.foods, &data.exercises.exercises, chrono::Local::now().date_naive());
        let mut close = false;
        let mut save = false;
        theme::card().stroke(Stroke::new(1.0, t.data.claude)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(format!("Claude · \"{}\"", self.build_request)).color(t.data.claude).strong());
            if let Some(m) = &plan.message {
                ui.label(RichText::new(m).italics());
            }
            for e in &plan.errors {
                ui.label(RichText::new(format!("⚠ {e}")).color(t.status.danger));
            }
            for op in &plan.ops {
                ui.label(RichText::new(op.describe()).monospace().size(12.5));
            }
            ui.horizontal(|ui| {
                if plan.errors.is_empty() && !plan.ops.is_empty() && ui.button(RichText::new("Save workout").strong()).clicked() {
                    save = true;
                }
                if ui.button("Discard").clicked() {
                    close = true;
                }
            });
        });
        ui.add_space(8.0);
        if save {
            let summary = crate::import::apply(data, plan.ops);
            data.status = format!("Claude: {summary}");
            self.workout = data.workouts.workouts.last().map(|w| w.id.clone());
            close = true;
        }
        if close {
            self.built = None;
        }
    }
}

/// The JSON object in Claude's reply.
fn fill_from(text: &str) -> Result<serde_json::Value, String> {
    let (a, b) = (text.find('{').ok_or("no JSON in the reply")?, text.rfind('}').ok_or("no JSON in the reply")?);
    serde_json::from_str(&text[a..=b]).map_err(|e| e.to_string())
}
