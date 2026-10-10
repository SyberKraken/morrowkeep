//! Item editor, import dialog and settings window.

use crate::import;
use crate::model::{Item, Repeat, KINDS};
use crate::parse::{parse_date, parse_duration, parse_time};
use crate::store::{Data, Settings};
use crate::worker::Job;
use chrono::{Local, NaiveDate, NaiveTime};
use eframe::egui::{self, Align2, Color32, RichText};
use std::sync::mpsc::{self, Receiver, Sender};

const ERR: Color32 = Color32::from_rgb(220, 80, 60);
const OK: Color32 = Color32::from_rgb(46, 160, 103);

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
            ui.label(RichText::new(format!("✔ {s}")).size(12.0).color(OK));
        }
        Ok(None) => {}
        Err(e) => {
            ui.label(RichText::new(format!("⚠ {e}")).size(12.0).color(ERR));
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
                        for k in KINDS {
                            ui.selectable_value(&mut self.kind, k.to_string(), *k);
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
                ui.label(RichText::new(e).color(ERR));
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
                            if ui.button(RichText::new("Really delete").color(ERR)).clicked() {
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
            let plan = (!self.text.trim().is_empty()).then(|| import::plan(&self.text, &data.items, &data.calories, Local::now().date_naive()));
            if let Some(p) = &plan {
                egui::ScrollArea::vertical().max_height(180.0).id_salt("import_preview").show(ui, |ui| {
                    for e in &p.errors {
                        ui.label(RichText::new(format!("⚠ {e}")).color(ERR));
                    }
                    if p.errors.is_empty() {
                        for op in &p.ops {
                            ui.label(RichText::new(op.describe()).monospace().size(12.5));
                        }
                    }
                });
            }
            if let Some(r) = &self.result {
                ui.label(RichText::new(r).color(OK));
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

// ---------------------------------------------------------------- settings

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

fn reg(args: &[&str]) -> bool {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("reg")
        .args(args)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

pub struct SettingsDlg {
    draft: Settings,
    pull: String,
    autostart: bool,
    connecting: Option<Receiver<Result<(), String>>>,
    message: Option<(String, bool)>,
    claude_test: Option<Receiver<Result<String, String>>>,
    claude_msg: Option<(String, bool)>,
    reminder_msg: Option<(String, bool)>,
    #[cfg(feature = "voice")]
    download: Option<(std::sync::Arc<std::sync::Mutex<f32>>, Receiver<Result<(), String>>)>,
}

impl SettingsDlg {
    pub fn open(data: &Data) -> Self {
        SettingsDlg {
            draft: data.settings.clone(),
            pull: data.settings.pull_calendars.join(", "),
            autostart: reg(&["query", RUN_KEY, "/v", "Morrowkeep"]),
            connecting: None,
            message: None,
            claude_test: None,
            claude_msg: None,
            reminder_msg: None,
            #[cfg(feature = "voice")]
            download: None,
        }
    }

    fn commit(&mut self, data: &mut Data) {
        self.draft.pull_calendars = self.pull.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        if data.settings != self.draft {
            data.settings = self.draft.clone();
            data.save_settings();
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, data: &mut Data, jobs: &Sender<Job>) -> bool {
        let mut keep = true;
        if let Some(rx) = &self.connecting {
            match rx.try_recv() {
                Ok(Ok(())) => {
                    data.google_connected = true;
                    self.message = Some(("Connected! Syncing now…".into(), true));
                    let _ = jobs.send(Job::SyncNow);
                    self.connecting = None;
                }
                Ok(Err(e)) => {
                    self.message = Some((e, false));
                    self.connecting = None;
                }
                Err(_) => ctx.request_repaint_after(std::time::Duration::from_millis(300)),
            }
        }
        modal("Settings").default_width(560.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().max_height(560.0).show(ui, |ui| {
                ui.heading("Data");
                ui.horizontal(|ui| {
                    ui.label(RichText::new(data.paths.root.display().to_string()).monospace());
                    if ui.button("📂 Open").clicked() {
                        let _ = open::that(&data.paths.root);
                    }
                    if ui.button("Reload data files").clicked() {
                        data.reload_all();
                    }
                });
                ui.label(RichText::new("All your data is plain JSON in this folder. Set the MORROWKEEP_DIR environment variable to use another folder (e.g. one synced by OneDrive).").weak().size(12.0));

                self.input_section(ui, data);

                ui.add_space(8.0);
                ui.separator();
                ui.heading("Calories");
                ui.horizontal(|ui| {
                    ui.label("Daily calorie limit");
                    ui.add(egui::DragValue::new(&mut self.draft.daily_calorie_limit).range(0..=10000).speed(10).suffix(" kcal"));
                    ui.label(RichText::new("0 = off").weak().size(12.0));
                });
                ui.label(
                    RichText::new(
                        "Sets the monthly budget (limit × days in the month), shown as a bar in the calendar's month view. \
                         Today shows a recommended amount instead: what's left of the month's budget spread over the remaining days, \
                         so going over one day lowers the next days.",
                    )
                    .weak()
                    .size(12.0),
                );

                ui.add_space(8.0);
                ui.separator();
                ui.heading("Google Calendar");
                ui.label(RichText::new("Morrowkeep copies your dated items into its own Google calendar and shows events from the calendars below (read-only). Your local files stay the master copy.").weak().size(12.0));
                egui::Grid::new("g_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label("OAuth client ID");
                    ui.add(egui::TextEdit::singleline(&mut self.draft.google_client_id).desired_width(340.0));
                    ui.end_row();
                    ui.label("Client secret");
                    ui.add(egui::TextEdit::singleline(&mut self.draft.google_client_secret).password(true).desired_width(340.0));
                    ui.end_row();
                    ui.label("Morrowkeep calendar");
                    ui.add(egui::TextEdit::singleline(&mut self.draft.google_calendar_name).desired_width(200.0));
                    ui.end_row();
                    ui.label("Show events from");
                    ui.add(egui::TextEdit::singleline(&mut self.pull).desired_width(340.0).hint_text("primary, other-calendar-id@group.calendar.google.com"));
                    ui.end_row();
                    ui.label("Auto sync");
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.draft.auto_sync, "every");
                        ui.add(egui::DragValue::new(&mut self.draft.sync_interval_min).range(1..=240).suffix(" min"));
                    });
                    ui.end_row();
                });
                egui::CollapsingHeader::new("How do I get a client ID? (one-time, free)").show(ui, |ui| {
                    for step in [
                        "1. Go to console.cloud.google.com and create a project (any name).",
                        "2. APIs & Services > Library > enable \"Google Calendar API\".",
                        "3. APIs & Services > OAuth consent screen > External, fill in app name + your email, and add yourself under Test users.",
                        "4. Credentials > Create credentials > OAuth client ID > Application type \"Desktop app\".",
                        "5. Copy the client ID and secret here, then press Connect.",
                    ] {
                        ui.label(RichText::new(step).size(12.5));
                    }
                });
                ui.horizontal(|ui| {
                    if self.connecting.is_some() {
                        ui.spinner();
                        ui.label("Waiting for you to sign in in the browser…");
                    } else {
                        let label = if data.google_connected { "Reconnect" } else { "Connect Google account" };
                        if ui.button(label).clicked() {
                            self.commit(data);
                            let settings = data.settings.clone();
                            let token = data.paths.token.clone();
                            let (tx, rx) = mpsc::channel();
                            let c = ctx.clone();
                            std::thread::spawn(move || {
                                let _ = tx.send(crate::gcal::connect(&settings, &token));
                                c.request_repaint();
                            });
                            self.connecting = Some(rx);
                            self.message = None;
                        }
                        if data.google_connected {
                            ui.label(RichText::new("● connected").color(OK));
                            if ui.button("Disconnect").clicked() {
                                let _ = std::fs::remove_file(&data.paths.token);
                                data.google_connected = false;
                            }
                        }
                    }
                });
                if let Some((m, ok)) = &self.message {
                    ui.label(RichText::new(m).color(if *ok { OK } else { ERR }));
                }
                if let Some(t) = data.sync.last_sync {
                    ui.label(RichText::new(format!("Last sync: {}", t.format("%Y-%m-%d %H:%M"))).weak().size(12.0));
                }

                #[cfg(feature = "voice")]
                self.voice_section(ui, data);

                ui.add_space(8.0);
                ui.separator();
                ui.heading("Reminders");
                ui.checkbox(&mut self.draft.reminders_enabled, "Show Windows notifications (also while Morrowkeep is in the tray)");
                if self.draft.reminders_enabled {
                    ui.indent("reminder_opts", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("At most every");
                            ui.add(egui::DragValue::new(&mut self.draft.reminder_interval_min).range(5..=480).suffix(" min"));
                        });
                        ui.checkbox(&mut self.draft.remind_anytime, "Unfinished \"anytime\" items for today");
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.draft.remind_calories, "No calories logged after");
                            ui.add(egui::TextEdit::singleline(&mut self.draft.calorie_reminder_after).desired_width(50.0));
                        });
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.draft.remind_weight, "No weight logged after");
                            ui.add(egui::TextEdit::singleline(&mut self.draft.weight_reminder_after).desired_width(50.0));
                        });
                        ui.label(
                            RichText::new("Calorie and weight reminders have input boxes, so you can log right from the notification. Typing a food description instead of a number opens Morrowkeep and estimates it with Claude.")
                                .weak()
                                .size(12.0),
                        );
                        ui.horizontal(|ui| {
                            ui.label("Quiet hours");
                            ui.add(egui::TextEdit::singleline(&mut self.draft.quiet_start).desired_width(50.0));
                            ui.label("to");
                            ui.add(egui::TextEdit::singleline(&mut self.draft.quiet_end).desired_width(50.0));
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Send test reminder").clicked() {
                                // Always show both input boxes in the test, so they can be tried out.
                                let mut r = crate::notify::pending_reminder(data).unwrap_or_default();
                                if r.lines.is_empty() {
                                    r.lines.push("Test reminder. Try logging calories or weight below.".into());
                                }
                                r.ask_kcal = true;
                                r.ask_weight = true;
                                self.reminder_msg = Some(match crate::notify::show(&data.paths, &r) {
                                    Ok(()) => ("Test reminder sent".into(), true),
                                    Err(e) => (e, false),
                                });
                            }
                            ui.label(RichText::new("Notifications have Log, Open and Snooze 2h buttons.").weak().size(12.0));
                        });
                        if let Some((m, ok)) = &self.reminder_msg {
                            ui.label(RichText::new(m).color(if *ok { OK } else { ERR }));
                        }
                    });
                }

                ui.add_space(8.0);
                ui.separator();
                ui.heading("Window");
                ui.checkbox(&mut self.draft.close_to_tray, "Closing the window keeps Morrowkeep running in the tray");
                let before = self.autostart;
                ui.checkbox(&mut self.autostart, "Start with Windows (in the tray)");
                if self.autostart != before {
                    let ok = if self.autostart {
                        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
                        let value = format!("\"{exe}\" --hidden");
                        reg(&["add", RUN_KEY, "/v", "Morrowkeep", "/t", "REG_SZ", "/d", &value, "/f"])
                    } else {
                        reg(&["delete", RUN_KEY, "/v", "Morrowkeep", "/f"])
                    };
                    if !ok {
                        self.autostart = before;
                    }
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(RichText::new("Save").strong()).clicked() {
                    self.commit(data);
                    keep = false;
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    keep = false;
                }
            });
        });
        keep
    }

    fn input_section(&mut self, ui: &mut egui::Ui, data: &mut Data) {
        use crate::claude;
        ui.add_space(8.0);
        ui.separator();
        ui.heading("Input mode");
        ui.radio_value(&mut self.draft.input_mode, "local".to_string(), "Local parser: free and offline, understands dates, times, repeats and #kinds");
        ui.radio_value(
            &mut self.draft.input_mode,
            "claude".to_string(),
            "Claude: requests run through Claude Code signed in with your Claude account (Pro), counting toward your plan's usage limits instead of paid API tokens",
        );
        if self.draft.input_mode != "claude" {
            return;
        }
        ui.indent("claude_opts", |ui| {
            egui::Grid::new("claude_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Model");
                let current = claude::MODELS.iter().find(|(m, _)| *m == self.draft.claude_model).map_or(self.draft.claude_model.as_str(), |(_, d)| *d);
                egui::ComboBox::from_id_salt("claude_model").selected_text(current).width(300.0).show_ui(ui, |ui| {
                    for (m, desc) in claude::MODELS {
                        ui.selectable_value(&mut self.draft.claude_model, m.to_string(), *desc);
                    }
                });
                ui.end_row();
                ui.label("Claude Code");
                ui.add(egui::TextEdit::singleline(&mut self.draft.claude_path).desired_width(340.0).hint_text("leave empty to find it automatically"));
                ui.end_row();
            });
            let found = claude::find_cli(&self.draft);
            match &found {
                Some(p) => ui.label(RichText::new(format!("Using {}", p.display())).weak().size(12.0)),
                None => ui.label(RichText::new("Claude Code not found. Install it, or point to claude.exe above.").color(ERR).size(12.0)),
            };
            ui.horizontal(|ui| {
                if let Some(rx) = &self.claude_test {
                    ui.spinner();
                    ui.label("Testing…");
                    match rx.try_recv() {
                        Ok(Ok(answer)) => {
                            self.claude_msg = Some((format!("Works. Claude answered: {}", answer.trim().chars().take(80).collect::<String>()), true));
                            self.claude_test = None;
                        }
                        Ok(Err(e)) => {
                            self.claude_msg = Some((e, false));
                            self.claude_test = None;
                        }
                        Err(_) => ui.ctx().request_repaint_after(std::time::Duration::from_millis(300)),
                    }
                } else if let Some(cli) = found {
                    if ui.button("Test connection").on_hover_text("Sends one tiny request (uses a little of your limit)").clicked() {
                        let (tx, rx) = mpsc::channel();
                        let (model, dir, c) = (self.draft.claude_model.clone(), data.paths.root.clone(), ui.ctx().clone());
                        std::thread::spawn(move || {
                            let _ = tx.send(claude::ask(&cli, &model, "Reply with exactly: Morrowkeep connection OK", &dir));
                            c.request_repaint();
                        });
                        self.claude_test = Some(rx);
                        self.claude_msg = None;
                    }
                }
            });
            if let Some((m, ok)) = &self.claude_msg {
                ui.label(RichText::new(m).color(if *ok { OK } else { ERR }));
            }
            egui::CollapsingHeader::new("How do I sign Claude Code in? (one-time)").show(ui, |ui| {
                for step in [
                    "1. Open a terminal (PowerShell).",
                    "2. Run the Claude Code path shown above (in quotes), or just: claude",
                    "3. Type /login and choose \"Claude account with subscription\", then approve in the browser.",
                    "4. Close the terminal and press Test connection here.",
                    "Each request counts toward the same usage limits as claude.ai and Claude Code.",
                ] {
                    ui.label(RichText::new(step).size(12.5));
                }
            });
        });
    }

    #[cfg(feature = "voice")]
    fn voice_section(&mut self, ui: &mut egui::Ui, data: &mut Data) {
        use crate::voice::{download_model, MODELS};
        ui.add_space(8.0);
        ui.separator();
        ui.heading("Voice input");
        ui.label(RichText::new("Speech is transcribed on this PC with Whisper. Nothing is sent anywhere. The model is only loaded while transcribing.").weak().size(12.0));
        ui.horizontal(|ui| {
            ui.label("Model");
            let current = MODELS.iter().find(|(f, _)| *f == self.draft.whisper_model).map_or(self.draft.whisper_model.as_str(), |(_, d)| *d);
            egui::ComboBox::from_id_salt("model").selected_text(current).width(260.0).show_ui(ui, |ui| {
                for (file, desc) in MODELS {
                    ui.selectable_value(&mut self.draft.whisper_model, file.to_string(), *desc);
                }
            });
            ui.label("Language");
            ui.add(egui::TextEdit::singleline(&mut self.draft.voice_language).desired_width(50.0))
                .on_hover_text("en, sv, de, … or auto. Non-English needs a multilingual model.");
        });
        let path = data.paths.models.join(&self.draft.whisper_model);
        if let Some((progress, rx)) = &self.download {
            let p = *progress.lock().unwrap();
            ui.add(egui::ProgressBar::new(p).show_percentage());
            match rx.try_recv() {
                Ok(Ok(())) => self.download = None,
                Ok(Err(e)) => {
                    self.message = Some((format!("Download failed: {e}"), false));
                    self.download = None;
                }
                Err(_) => ui.ctx().request_repaint_after(std::time::Duration::from_millis(200)),
            }
        } else if path.exists() {
            ui.label(RichText::new("✔ model downloaded").color(OK));
        } else {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Model not downloaded.").color(ERR));
                if ui.button("Download from Hugging Face").on_hover_text(crate::voice::model_url(&self.draft.whisper_model)).clicked() {
                    let progress = std::sync::Arc::new(std::sync::Mutex::new(0.0f32));
                    let (tx, rx) = mpsc::channel();
                    let (p2, file, dir, c) = (progress.clone(), self.draft.whisper_model.clone(), data.paths.models.clone(), ui.ctx().clone());
                    std::thread::spawn(move || {
                        let r = download_model(&file, &dir, |f| *p2.lock().unwrap() = f).map(|_| ());
                        let _ = tx.send(r);
                        c.request_repaint();
                    });
                    self.download = Some((progress, rx));
                }
            });
        }
    }
}
