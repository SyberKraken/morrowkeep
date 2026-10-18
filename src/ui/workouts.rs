//! Workouts tab. Step 1 of WORKOUTS_PLAN.md: the exercise library (list with search and
//! equipment/muscle filters, info page with a muscle map, editor). Workouts, the player and
//! history come in the next steps.

use super::theme;
use crate::store::Data;
use crate::workouts::{self, Exercise, EQUIPMENT, KINDS, LEVELS, MUSCLES};
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
}

impl Default for WorkoutsUi {
    fn default() -> Self {
        WorkoutsUi { section: Section::Exercises, search: String::new(), equipment: Vec::new(), muscle: None, selected: None, editing: false, confirm_delete: false }
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
        egui::SidePanel::left("workouts_side").resizable(false).exact_width(300.0).show(ctx, |ui| self.sidebar(ui, data));
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.set_max_width(860.0);
                ui.add_space(10.0);
                match self.section {
                    Section::Exercises => self.exercise_page(ui, data),
                    Section::Workouts => super::streaks::empty_state(ui, "Workouts are coming next", "Build workouts from these exercises, with sets, reps or times, and breaks. Then do them with a big timer."),
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
