//! Doing a workout: one step at a time with a big circle timer. Every step with a time counts
//! down and moves on by itself; a set with only reps waits for "Done set". Beeps for the last
//! 3 seconds and whenever it moves on. At the end the session is saved to workout_log.json.

use super::theme;
use crate::store::Data;
use crate::workouts::{Exercise, Session, SetDone, Step, Workout};
use chrono::{DateTime, Local};
use eframe::egui::{self, Pos2, RichText, Shape, Stroke, Ui, Vec2};
use std::time::{Duration, Instant};

pub struct Player {
    workout_id: String,
    /// Task to tick off when the workout is completed.
    task: Option<String>,
    name: String,
    steps: Vec<Step>,
    idx: usize,
    step_started: Instant,
    paused_at: Option<Instant>,
    /// Time paused during the current step.
    paused_for: Duration,
    started: Instant,
    started_clock: DateTime<Local>,
    done: Vec<SetDone>,
    /// Reps and kg for the current set (start from the plan, can be changed).
    reps: Option<u32>,
    kg: Option<f32>,
    /// Last countdown second that beeped (3, 2, 1).
    last_tick: Option<u32>,
    /// Some(completed) once over; the session is saved then.
    finished: Option<bool>,
    /// Ran off the end: save on the next frame (where the data is at hand).
    pending_save: bool,
    confirm_end: bool,
    /// The how-to side panel is open.
    pub show_how_to: bool,
}

fn beep(freq: u32, ms: u32) {
    std::thread::spawn(move || unsafe {
        windows_sys::Win32::System::Diagnostics::Debug::Beep(freq, ms);
    });
}

fn mmss(s: u32) -> String {
    format!("{}:{:02}", s / 60, s % 60)
}

impl Player {
    pub fn new(w: &Workout, lib: &[Exercise]) -> Self {
        let steps = w.steps(lib);
        let mut p = Player {
            workout_id: w.id.clone(),
            task: w.task.clone(),
            name: w.name.clone(),
            steps,
            idx: 0,
            step_started: Instant::now(),
            paused_at: None,
            paused_for: Duration::ZERO,
            started: Instant::now(),
            started_clock: Local::now(),
            done: Vec::new(),
            reps: None,
            kg: None,
            last_tick: None,
            finished: None,
            pending_save: false,
            confirm_end: false,
            show_how_to: true,
        };
        p.enter_step();
        beep(880, 180);
        p
    }

    fn enter_step(&mut self) {
        self.step_started = Instant::now();
        self.paused_for = Duration::ZERO;
        self.paused_at = None;
        self.last_tick = None;
        if let Some(Step::Set { reps, kg, .. }) = self.steps.get(self.idx) {
            self.reps = *reps;
            self.kg = *kg;
        }
    }

    /// Seconds spent on the current step (not counting pauses).
    fn elapsed(&self) -> f32 {
        let end = self.paused_at.unwrap_or_else(Instant::now);
        (end.duration_since(self.step_started).saturating_sub(self.paused_for)).as_secs_f32()
    }

    /// Finishes the current step (recording a set) and moves on.
    fn next(&mut self) {
        if let Some(Step::Set { exercise, name, set, .. }) = self.steps.get(self.idx) {
            self.done.push(SetDone { exercise: exercise.clone(), name: name.clone(), set: *set, reps: self.reps, seconds: self.elapsed().round() as u32, kg: self.kg });
        }
        self.idx += 1;
        if self.idx >= self.steps.len() {
            self.finished = Some(true);
            self.pending_save = true;
            beep(1320, 450);
        } else {
            beep(1320, 220);
            self.enter_step();
        }
    }

    fn back(&mut self) {
        if self.idx > 0 {
            self.idx -= 1;
            // the set being redone isn't counted twice
            if let Some(Step::Set { exercise, set, .. }) = self.steps.get(self.idx) {
                if let Some(pos) = self.done.iter().rposition(|d| d.exercise == *exercise && d.set == *set) {
                    self.done.remove(pos);
                }
            }
        }
        self.enter_step();
    }

    fn toggle_pause(&mut self) {
        match self.paused_at.take() {
            Some(t) => self.paused_for += t.elapsed(),
            None => self.paused_at = Some(Instant::now()),
        }
    }

    /// Seconds left in the whole workout (rep-only sets estimated at 3 s/rep).
    fn left_total(&self) -> u32 {
        let current = self.steps.get(self.idx).and_then(|s| s.duration()).map_or(0, |d| (d as f32 - self.elapsed()).max(0.0) as u32);
        let rest: u32 = self.steps.iter().skip(self.idx + 1).map(|s| s.duration().unwrap_or(match s {
            Step::Set { reps, per_side, .. } => reps.unwrap_or(10) * crate::workouts::SECS_PER_REP * if *per_side { 2 } else { 1 },
            _ => 0,
        })).sum();
        current + rest
    }

    fn finish(&mut self, data: &mut Data, completed: bool) {
        self.finished = Some(completed);
        if self.done.is_empty() {
            return;
        }
        let session = Session {
            id: uuid::Uuid::new_v4().to_string(),
            date: self.started_clock.date_naive(),
            start: self.started_clock.format("%H:%M").to_string(),
            duration_s: self.started.elapsed().as_secs() as u32,
            workout_id: self.workout_id.clone(),
            workout: self.name.clone(),
            sets: self.done.clone(),
            completed,
        };
        data.workout_log.sessions.push(session);
        data.save_workout_log();
        data.status = format!("Workout saved: {}", self.name);
        // a finished workout ticks off its task for today
        if completed {
            if let Some(i) = self.task.as_ref().and_then(|id| data.item_index(id)) {
                let today = Local::now().date_naive();
                if !data.items[i].is_done_on(today) {
                    data.items[i].toggle_done(today);
                    data.save_items();
                    data.status = format!("Workout saved · ✔ {}", data.items[i].title);
                }
            }
        }
    }

    /// Draws the player. Returns false when it should close.
    /// The exercise to explain: the current one, or during a rest the next one (true = upcoming).
    pub fn how_to_exercise(&self) -> Option<(String, bool)> {
        self.steps[self.idx..].iter().enumerate().find_map(|(k, s)| match s {
            Step::Set { exercise, .. } => Some((exercise.clone(), k > 0)),
            Step::Rest { .. } => None,
        })
    }

    pub fn is_finished(&self) -> bool {
        self.finished.is_some()
    }

    pub fn show(&mut self, ui: &mut Ui, data: &mut Data) -> bool {
        if self.pending_save {
            self.pending_save = false;
            self.finish(data, true);
        }
        if let Some(completed) = self.finished {
            return self.summary(ui, completed);
        }
        let t = theme::th();
        let lib = data.exercises.exercises.clone();
        let step = self.steps[self.idx].clone();
        let running = self.paused_at.is_none();

        // keyboard: Space pauses, Enter finishes the set
        if ui.input(|i| i.key_pressed(egui::Key::Space)) && !ui.ctx().wants_keyboard_input() {
            self.toggle_pause();
        }
        if ui.input(|i| i.key_pressed(egui::Key::H)) && !ui.ctx().wants_keyboard_input() {
            self.show_how_to = !self.show_how_to;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !ui.ctx().wants_keyboard_input() {
            self.next();
            ui.ctx().request_repaint();
            return true;
        }

        // time: auto-advance timed steps, beep the last 3 seconds
        let elapsed = self.elapsed();
        if let Some(d) = step.duration() {
            let left = d as f32 - elapsed;
            if left <= 0.0 && running {
                self.next();
                // draw the next step right away
                ui.ctx().request_repaint();
                return true;
            }
            let whole = left.ceil() as u32;
            if running && (1..=3).contains(&whole) && self.last_tick != Some(whole) {
                self.last_tick = Some(whole);
                beep(880, 120);
            }
        }
        if running {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }

        let is_rest = matches!(step, Step::Rest { .. });
        let color = if is_rest { t.data.rest } else { t.data.workout };
        ui.vertical_centered(|ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} · step {} of {} · {} left", self.name, self.idx + 1, self.steps.len(), mmss(self.left_total()))).color(t.text.muted));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.confirm_end {
                        if ui.small_button("Keep going").clicked() {
                            self.confirm_end = false;
                        }
                        if ui.small_button("End and save").clicked() {
                            self.finish(data, false);
                        }
                        ui.label(RichText::new("End the workout?").color(t.status.warning));
                    } else {
                        if ui.button("End workout").clicked() {
                            self.confirm_end = true;
                        }
                        if !self.show_how_to && ui.button("📖 How to").on_hover_text("Show how to do the exercise (H)").clicked() {
                            self.show_how_to = true;
                        }
                    }
                });
            });
            ui.add_space(8.0);
            let (title, sub) = match &step {
                Step::Set { name, set, sets, .. } => (name.clone(), format!("set {set} of {sets}")),
                Step::Rest { label, .. } => (label.clone(), self.upcoming_name(1).map_or("last one coming up".into(), |n| format!("next: {n}"))),
            };
            ui.label(RichText::new(title).font(theme::bold(30.0)).color(if is_rest { color } else { t.text.primary }));
            ui.label(RichText::new(sub).size(17.0).color(t.text.muted));
            ui.add_space(10.0);

            // the ring
            let size = (ui.available_height() * 0.5).clamp(220.0, 380.0);
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
            let p = ui.painter();
            let c = rect.center();
            let r = size / 2.0 - 16.0;
            p.circle_stroke(c, r, Stroke::new(18.0, t.widget.fill));
            let frac = match step.duration() {
                Some(d) => (1.0 - elapsed / d.max(1) as f32).clamp(0.0, 1.0),
                None => 1.0,
            };
            if frac > 0.0 {
                let n = (120.0 * frac).max(2.0) as usize;
                let pts: Vec<Pos2> = (0..=n)
                    .map(|k| {
                        let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * frac * k as f32 / n as f32;
                        c + Vec2::new(a.cos(), a.sin()) * r
                    })
                    .collect();
                let col = if running { color } else { color.linear_multiply(0.45) };
                p.add(Shape::line(pts, Stroke::new(18.0, col)));
            }
            let (big, small) = match (&step, step.duration()) {
                (_, Some(d)) => {
                    let left = (d as f32 - elapsed).max(0.0).ceil() as u32;
                    let detail = match &step {
                        Step::Set { reps: Some(n), kg, per_side, .. } => format!("{n} reps{}{}", if *per_side { " per side" } else { "" }, kg.map(|k| format!(" · {k} kg")).unwrap_or_default()),
                        Step::Set { kg: Some(k), .. } => format!("{k} kg"),
                        Step::Set { .. } => "keep going".into(),
                        Step::Rest { .. } => "rest".into(),
                    };
                    (if left >= 60 { mmss(left) } else { left.to_string() }, detail)
                }
                (Step::Set { per_side, .. }, None) => (
                    self.reps.map_or("–".into(), |n| n.to_string()),
                    format!("reps{}{} · {}", if *per_side { " per side" } else { "" }, self.kg.map(|k| format!(" · {k} kg")).unwrap_or_default(), mmss(elapsed as u32)),
                ),
                _ => (String::new(), String::new()),
            };
            p.text(c - Vec2::new(0.0, 12.0), egui::Align2::CENTER_CENTER, big, egui::FontId::new(size * 0.24, theme::bold_family()), t.text.primary);
            p.text(c + Vec2::new(0.0, size * 0.15), egui::Align2::CENTER_CENTER, small, egui::FontId::proportional(16.0), t.text.muted);
            if !running {
                p.text(c - Vec2::new(0.0, size * 0.2), egui::Align2::CENTER_CENTER, "paused", egui::FontId::proportional(15.0), t.status.warning);
            }
            ui.add_space(12.0);

            // controls
            ui.horizontal(|ui| {
                let w = 4.0 * 120.0 + 3.0 * 8.0;
                ui.add_space(((ui.available_width() - w) / 2.0).max(0.0));
                if ui.add_sized([120.0, 40.0], egui::Button::new("⏮ Back")).clicked() {
                    self.back();
                }
                if ui.add_sized([120.0, 40.0], egui::Button::new(if running { "⏸ Pause" } else { "▶ Resume" })).on_hover_text("Space").clicked() {
                    self.toggle_pause();
                }
                let label = match (&step, step.duration()) {
                    (Step::Set { .. }, None) => "✔ Done set",
                    (Step::Rest { .. }, _) => "Skip rest ⏭",
                    _ => "✔ Done",
                };
                if ui.add_sized([120.0, 40.0], egui::Button::new(RichText::new(label).strong()).fill(color.linear_multiply(0.35))).on_hover_text("Enter").clicked() {
                    self.next();
                }
                if ui.add_sized([120.0, 40.0], egui::Button::new("Skip ⏭")).on_hover_text("Skip without counting this set").clicked() {
                    self.idx += 1;
                    if self.idx >= self.steps.len() {
                        self.finish(data, true);
                    } else {
                        beep(1320, 220);
                        self.enter_step();
                    }
                }
            });

            // what was actually done in this set
            if let Step::Set { exercise, .. } = &step {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_space(((ui.available_width() - 330.0) / 2.0).max(0.0));
                    ui.label(RichText::new("This set:").color(t.text.muted));
                    let mut reps = self.reps.unwrap_or(0);
                    if ui.add(egui::DragValue::new(&mut reps).range(0..=500).custom_formatter(|n, _| if n == 0.0 { "– reps".into() } else { format!("{n:.0} reps") })).changed() {
                        self.reps = (reps > 0).then_some(reps);
                    }
                    if lib.iter().find(|e| e.id == *exercise).is_some_and(|e| e.needs("dumbbells")) {
                        let mut kg = self.kg.unwrap_or(0.0);
                        if ui.add(egui::DragValue::new(&mut kg).range(0.0..=100.0).speed(0.25).suffix(" kg")).on_hover_text("kg per dumbbell").changed() {
                            self.kg = (kg > 0.0).then_some(kg);
                        }
                    }
                });
                if let Some(tip) = lib.iter().find(|e| e.id == *exercise).and_then(|e| e.form.first()).filter(|_| !self.show_how_to) {
                    ui.add_space(6.0);
                    ui.label(RichText::new(tip).italics().color(t.text.muted));
                }
            }

            // what's next
            ui.add_space(12.0);
            let next: Vec<String> = (1..=3).filter_map(|k| self.upcoming_label(k)).collect();
            if !next.is_empty() {
                ui.label(RichText::new(format!("Then: {}", next.join("  ›  "))).color(t.text.muted));
            }
        });
        true
    }

    fn upcoming_name(&self, k: usize) -> Option<String> {
        self.steps.iter().skip(self.idx + k).find_map(|s| match s {
            Step::Set { name, set, .. } => Some(format!("{name} (set {set})")),
            _ => None,
        })
    }

    fn upcoming_label(&self, k: usize) -> Option<String> {
        self.steps.get(self.idx + k).map(|s| match s {
            Step::Set { name, set, sets, .. } => format!("{name} {set}/{sets}"),
            Step::Rest { seconds, label } => format!("{label} {}", if *seconds >= 60 { mmss(*seconds) } else { format!("{seconds} s") }),
        })
    }

    fn summary(&mut self, ui: &mut Ui, completed: bool) -> bool {
        let t = theme::th();
        let mut keep = true;
        ui.vertical_centered(|ui| {
            ui.add_space(30.0);
            ui.label(RichText::new(if completed { "Workout complete 🎉" } else { "Workout ended" }).font(theme::bold(30.0)).color(t.data.workout));
            ui.label(RichText::new(&self.name).size(18.0));
            ui.add_space(10.0);
            let secs = self.started.elapsed().as_secs() as u32;
            let volume: f32 = self.done.iter().map(|s| s.reps.unwrap_or(0) as f32 * s.kg.unwrap_or(0.0)).sum();
            let mut line = format!("{} · {} set{}", mmss(secs), self.done.len(), if self.done.len() == 1 { "" } else { "s" });
            if volume > 0.0 {
                line += &format!(" · {volume:.0} kg lifted (reps × kg per dumbbell)");
            }
            ui.label(RichText::new(line).size(16.0).color(t.text.muted));
            ui.add_space(10.0);
            for s in &self.done {
                let mut l = format!("{} · set {}", s.name, s.set);
                if let Some(r) = s.reps {
                    l += &format!(" · {r} reps");
                }
                if let Some(k) = s.kg {
                    l += &format!(" · {k} kg");
                }
                l += &format!(" · {}", mmss(s.seconds));
                ui.label(RichText::new(l).size(13.5));
            }
            ui.add_space(14.0);
            if !self.done.is_empty() {
                ui.label(RichText::new("Saved to your workout history.").color(t.status.success));
            }
            if ui.add_sized([160.0, 40.0], egui::Button::new(RichText::new("Close").strong())).clicked() {
                keep = false;
            }
        });
        keep
    }
}
