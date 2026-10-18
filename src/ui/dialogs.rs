//! Item editor and import dialog.

use crate::import;
use crate::model::{Item, Repeat};
use crate::parse::{parse_date, parse_duration, parse_time};
use crate::store::Data;
use chrono::{Local, NaiveDate, NaiveTime};
use eframe::egui::{self, Align2, Color32, RichText};

fn err_color() -> Color32 {
    super::theme::th().status.danger
}
fn ok_color() -> Color32 {
    super::theme::th().status.success
}

fn modal(title: &str) -> egui::Window<'_> {
    egui::Window::new(title)
        .collapsible(false)
        .resizable(true)
        .pivot(Align2::CENTER_CENTER)
        .default_pos(egui::pos2(560.0, 380.0))
}

/// Shows how a free-text field was understood (✔ value / ⚠ message).
fn hint(ui: &mut egui::Ui, res: Result<Option<String>, String>) {
    match res {
        Ok(Some(s)) => {
            ui.label(RichText::new(format!("✔ {s}")).size(12.0).color(ok_color()));
        }
        Ok(None) => {}
        Err(e) => {
            ui.label(RichText::new(format!("⚠ {e}")).size(12.0).color(err_color()));
        }
    }
}

// ---------------------------------------------------------------- editor

pub struct Editor {
    id: Option<String>,
    title: String,
    kind: String,
    date: String,
    time: String,
    duration: String,
    repeat: String,
    until: String,
    notes: String,
    tags: String,
    sync_google: bool,
    done: bool,
    error: Option<String>,
    confirm_delete: bool,
}

impl Editor {
    pub fn new_at(day: Option<NaiveDate>, time: Option<NaiveTime>) -> Self {
        Editor {
            id: None,
            title: String::new(),
            kind: "task".into(),
            date: day.map(|d| d.to_string()).unwrap_or_default(),
            time: time.map(|t| t.format("%H:%M").to_string()).unwrap_or_default(),
            duration: if time.is_some() { "60".into() } else { String::new() },
            repeat: String::new(),
            until: String::new(),
            notes: String::new(),
            tags: String::new(),
            sync_google: true,
            done: false,
            error: None,
            confirm_delete: false,
        }
    }

    pub fn from_item(i: &Item) -> Self {
        Editor {
            id: Some(i.id.clone()),
            title: i.title.clone(),
            kind: i.kind.clone(),
            date: i.date.map(|d| d.to_string()).unwrap_or_default(),
            time: i.time.map(|t| t.format("%H:%M").to_string()).unwrap_or_default(),
            duration: i.duration_min.map(|m| m.to_string()).unwrap_or_default(),
            repeat: i.repeat.clone().unwrap_or_default(),
            until: i.until.map(|d| d.to_string()).unwrap_or_default(),
            notes: i.notes.clone(),
            tags: i.tags.join(", "),
            sync_google: i.sync_google,
            done: i.done,
            error: None,
            confirm_delete: false,
        }
    }

    fn parsed_date(s: &str) -> Result<Option<NaiveDate>, String> {
        let s = s.trim();
        if s.is_empty() {
            return Ok(None);
        }
        parse_date(s, Local::now().date_naive()).map(Some).ok_or_else(|| format!("can't read date \"{s}\""))
    }

    fn save(&self, data: &mut Data) -> Result<(), String> {
        let title = self.title.trim();
        if title.is_empty() {
            return Err("Title is required".into());
        }
        let date = Self::parsed_date(&self.date)?;
        let time = match self.time.trim() {
            "" => None,
            t => Some(parse_time(t).ok_or_else(|| format!("can't read time \"{t}\""))?),
        };
        let duration = match self.duration.trim() {
            "" => None,
            d => Some(parse_duration(d).ok_or_else(|| format!("can't read duration \"{d}\""))?),
        };
        let repeat = match self.repeat.trim() {
            "" => None,
            r => Some(Repeat::parse(r)?.describe()),
        };
        if repeat.is_some() && date.is_none() {
            return Err("A repeating item needs a start date".into());
        }
        let until = Self::parsed_date(&self.until)?;

        let mut item = match self.id.as_ref().and_then(|id| data.item_index(id)) {
            Some(i) => data.items[i].clone(),
            None => Item::new(""),
        };
        item.title = title.to_string();
        item.kind = self.kind.trim().to_lowercase();
        if item.kind.is_empty() {
            item.kind = "task".into();
        }
        item.date = date;
        item.time = time;
        item.duration_min = duration;
        if item.repeat != repeat {
            item.done_dates.clear();
        }
        item.repeat = repeat;
        item.until = until;
        item.notes = self.notes.trim_end().to_string();
        item.tags = self.tags.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
        item.sync_google = self.sync_google;
        if item.repeat.is_none() {
            item.set_done(self.done);
        }
        item.touch();
        match data.item_index(&item.id) {
            Some(i) => data.items[i] = item,
            None => data.items.push(item),
        }
        data.save_items();
        Ok(())
    }

    /// Returns false when the window should close.
    pub fn show(&mut self, ctx: &egui::Context, data: &mut Data) -> bool {
        let mut keep = true;
        let title = if self.id.is_some() { "Edit item" } else { "New item" };
        let today = Local::now().date_naive();
        modal(title).default_width(440.0).show(ctx, |ui| {
            egui::Grid::new("editor_grid").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label("Title");
                let r = ui.add(egui::TextEdit::singleline(&mut self.title).desired_width(300.0));
                if self.id.is_none() && self.title.is_empty() {
                    r.request_focus();
                }
                ui.end_row();

                ui.label("Kind");
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("kind").selected_text(&self.kind).show_ui(ui, |ui| {
                        for k in crate::categories::names() {
                            let label = format!("{} {k}", crate::categories::icon(&k));
                            ui.selectable_value(&mut self.kind, k, label);
                        }
                    });
                    ui.add(egui::TextEdit::singleline(&mut self.kind).desired_width(100.0).hint_text("or type one"));
                });
                ui.end_row();

                ui.label("Date");
                ui.vertical(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.date).desired_width(160.0).hint_text("YYYY-MM-DD, tomorrow, fri…"));
                    let res = Self::parsed_date(&self.date).map(|d| match d {
                        Some(d) if self.date.trim() != d.to_string() => Some(d.format("%a %b %-d, %Y").to_string()),
                        Some(_) => None,
                        None => Some("no date (backlog)".to_string()),
                    });
                    hint(ui, res);
                });
                ui.end_row();

                ui.label("Time");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.time).desired_width(70.0).hint_text("18:00"));
                    ui.label("Duration");
                    ui.add(egui::TextEdit::singleline(&mut self.duration).desired_width(70.0).hint_text("45m, 1h30"));
                });
                ui.end_row();

                ui.label("Repeat");
                ui.vertical(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.repeat).desired_width(300.0).hint_text("weekly mon,thu · daily · every 2 weeks"));
                    if !self.repeat.trim().is_empty() {
                        hint(ui, Repeat::parse(&self.repeat).map(|r| Some(r.describe())));
                    }
                });
                ui.end_row();

                if !self.repeat.trim().is_empty() {
                    ui.label("Until");
                    ui.add(egui::TextEdit::singleline(&mut self.until).desired_width(160.0).hint_text("optional end date"));
                    ui.end_row();
                }

                ui.label("Notes");
                ui.add(egui::TextEdit::multiline(&mut self.notes).desired_width(300.0).desired_rows(4));
                ui.end_row();

                ui.label("Tags");
                ui.add(egui::TextEdit::singleline(&mut self.tags).desired_width(300.0).hint_text("comma, separated"));
                ui.end_row();

                ui.label("");
                ui.vertical(|ui| {
                    if self.repeat.trim().is_empty() {
                        ui.checkbox(&mut self.done, "Done");
                    }
                    ui.checkbox(&mut self.sync_google, "Show in Google Calendar");
                });
                ui.end_row();
            });

            if let Some(e) = &self.error {
                ui.label(RichText::new(e).color(err_color()));
            }
            ui.separator();
            ui.horizontal(|ui| {
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.ctrl);
                if ui.button(RichText::new("Save").strong()).on_hover_text("Ctrl+Enter").clicked() || enter {
                    match self.save(data) {
                        Ok(()) => keep = false,
                        Err(e) => self.error = Some(e),
                    }
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    keep = false;
                }
                if let Some(id) = self.id.clone() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.confirm_delete {
                            if ui.button(RichText::new("Really delete").color(err_color())).clicked() {
                                data.delete_item(&id);
                                data.save_items();
                                keep = false;
                            }
                        } else if ui.button("🗑 Delete").clicked() {
                            self.confirm_delete = true;
                        }
                        if !self.repeat.trim().is_empty() {
                            if let Some(i) = data.item_index(&id) {
                                let done = data.items[i].is_done_on(today);
                                if ui.button(if done { "Undo today" } else { "✔ Done today" }).clicked() {
                                    data.items[i].toggle_done(today);
                                    data.save_items();
                                }
                            }
                        }
                    });
                }
            });
        });
        keep
    }
}

// ---------------------------------------------------------------- import

#[derive(Default)]
pub struct ImportDlg {
    text: String,
    result: Option<String>,
}

impl ImportDlg {
    pub fn with_text(text: String) -> Self {
        ImportDlg { text, result: None }
    }

    pub fn show(&mut self, ctx: &egui::Context, data: &mut Data) -> bool {
        let mut keep = true;
        modal("Import").default_width(620.0).show(ctx, |ui| {
            ui.label("Paste JSON from an AI chat (ChatGPT, Claude, Gemini…). To give the AI the format and your current plan, use the button below, paste it into the chat, then ask for what you want, e.g. \"plan 3 gym sessions and healthy dinners for next week\".");
            ui.horizontal(|ui| {
                if ui.button("📋 Copy AI instructions + my current plan").clicked() {
                    ctx.copy_text(import::ai_context(data, 14));
                    self.result = Some("Copied. Paste it into your AI chat, then paste its reply here.".into());
                }
                if ui.button("📂 Open inbox folder").on_hover_text("JSON files saved here are imported automatically").clicked() {
                    let _ = open::that(&data.paths.inbox);
                }
            });
            ui.add_space(4.0);
            egui::ScrollArea::vertical().max_height(260.0).id_salt("import_text").show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.text)
                        .code_editor()
                        .desired_rows(12)
                        .desired_width(f32::INFINITY)
                        .hint_text("{ \"morrowkeep_import\": 1, \"items\": [ … ] }"),
                );
            });
            let plan = (!self.text.trim().is_empty()).then(|| import::plan(&self.text, &data.items, &data.calories, &data.settings, &data.foods, &data.exercises.exercises, Local::now().date_naive()));
            if let Some(p) = &plan {
                egui::ScrollArea::vertical().max_height(180.0).id_salt("import_preview").show(ui, |ui| {
                    for e in &p.errors {
                        ui.label(RichText::new(format!("⚠ {e}")).color(err_color()));
                    }
                    if p.errors.is_empty() {
                        for op in &p.ops {
                            super::op_line(ui, &op.describe());
                        }
                    }
                });
            }
            if let Some(r) = &self.result {
                ui.label(RichText::new(r).color(ok_color()));
            }
            ui.separator();
            ui.horizontal(|ui| {
                let ready = plan.as_ref().is_some_and(|p| p.errors.is_empty() && !p.ops.is_empty());
                let n = plan.as_ref().map_or(0, |p| p.ops.len());
                if ui.add_enabled(ready, egui::Button::new(RichText::new(format!("Import {n} change(s)")).strong())).clicked() {
                    if let Some(p) = plan {
                        let summary = import::apply(data, p.ops);
                        data.status = format!("Imported: {summary}");
                        self.result = Some(format!("Imported: {summary}"));
                        self.text.clear();
                    }
                }
                if ui.button("Close").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    keep = false;
                }
            });
        });
        keep
    }
}
