//! Settings tab: a section list on the left, the section on the right, and Save / Revert.
//! Everything is edited on a draft and saved together (Ctrl+S works too).

use crate::categories::{self, Category, StreakStyle};
use crate::store::{Data, Settings};
use crate::worker::Job;
use eframe::egui::{self, Color32, RichText, Ui};
use std::collections::{BTreeMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};

fn err_color() -> Color32 {
    super::theme::th().status.danger
}
fn ok_color() -> Color32 {
    super::theme::th().status.success
}

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

#[derive(Clone, Copy, PartialEq)]
pub enum Section {
    General,
    Appearance,
    Categories,
    Input,
    Reminders,
    Google,
}

impl Section {
    const ALL: [Section; 6] = [Section::General, Section::Appearance, Section::Categories, Section::Input, Section::Reminders, Section::Google];

    fn label(self) -> &'static str {
        match self {
            Section::General => "General",
            Section::Appearance => "Appearance",
            Section::Categories => "Categories",
            Section::Input => "Input & voice",
            Section::Reminders => "Reminders",
            Section::Google => "Google Calendar",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Section::General => "📂",
            Section::Appearance => "🎨",
            Section::Categories => "⭐",
            Section::Input => "🎤",
            Section::Reminders => "🔔",
            Section::Google => "📅",
        }
    }
}

pub struct SettingsPage {
    draft: Settings,
    /// The saved settings the draft started from.
    base: Settings,
    pull: String,
    pub section: Section,
    /// For each draft category, the name it had when loaded (None = added here); used to
    /// rename or move items on Save.
    origins: Vec<Option<String>>,
    /// Category waiting for delete confirmation.
    confirm_delete: Option<usize>,
    autostart: bool,
    connecting: Option<Receiver<Result<(), String>>>,
    message: Option<(String, bool)>,
    claude_test: Option<Receiver<Result<String, String>>>,
    claude_msg: Option<(String, bool)>,
    reminder_msg: Option<(String, bool)>,
    #[cfg(feature = "voice")]
    download: Option<(std::sync::Arc<std::sync::Mutex<f32>>, Receiver<Result<(), String>>)>,
}

impl SettingsPage {
    pub fn open(data: &Data) -> Self {
        SettingsPage {
            draft: data.settings.clone(),
            base: data.settings.clone(),
            pull: data.settings.pull_calendars.join(", "),
            section: Section::General,
            origins: data.settings.categories.iter().map(|c| Some(c.name.clone())).collect(),
            confirm_delete: None,
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

    fn revert(&mut self, data: &Data) {
        let section = self.section;
        *self = SettingsPage { section, ..SettingsPage::open(data) };
    }

    /// The draft as it would be saved.
    fn effective(&self) -> Settings {
        let mut s = self.draft.clone();
        s.pull_calendars = self.pull.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        s
    }

    pub fn dirty(&self, _data: &Data) -> bool {
        self.effective() != self.base
    }

    /// Settings saved elsewhere (e.g. by Claude) while this page was open: picked up if
    /// nothing is being edited here, otherwise flagged.
    fn changed_elsewhere(&mut self, data: &Data) -> bool {
        if data.settings == self.base {
            return false;
        }
        if self.effective() == self.base {
            self.revert(data);
            return false;
        }
        true
    }

    /// Why the draft can't be saved, if it can't.
    fn problem(&self) -> Option<String> {
        let mut seen = HashSet::new();
        for c in &self.draft.categories {
            if c.name.is_empty() {
                return Some("A category needs a name".into());
            }
            if !seen.insert(c.name.as_str()) {
                return Some(format!("Two categories are called \"{}\"", c.name));
            }
        }
        None
    }

    fn commit(&mut self, data: &mut Data, ctx: &egui::Context) {
        if self.problem().is_some() {
            return;
        }
        let mut new = self.effective();
        if data.settings.font != new.font {
            super::theme::apply_fonts(ctx, &new.font);
        }
        if data.settings.theme != new.theme {
            if let Err(e) = super::theme::activate(&data.paths.root, &new.theme) {
                data.status = format!("Theme error: {e}");
            }
        }
        // Renamed categories take their items along (and keep their color); items of deleted
        // categories become tasks.
        let mut renamed: BTreeMap<String, String> = BTreeMap::new();
        for (cat, origin) in new.categories.iter_mut().zip(&self.origins) {
            if let Some(o) = origin.as_ref().filter(|o| **o != cat.name) {
                renamed.insert(o.clone(), cat.name.clone());
                if cat.color.is_none() {
                    cat.color = Some(categories::to_hex(super::theme::th().kind(o)));
                }
            }
        }
        let kept: HashSet<&str> = self.origins.iter().flatten().map(|s| s.as_str()).collect();
        let deleted: HashSet<String> = data.settings.categories.iter().map(|c| c.name.clone()).filter(|n| !kept.contains(n.as_str())).collect();
        let moved = data.move_kinds(&renamed, &deleted);
        if data.settings != new {
            data.settings = new;
            data.save_settings();
            categories::apply(&data.settings.categories, &data.settings.colors);
            data.status = if moved > 0 { format!("Settings saved · {moved} item(s) updated") } else { "Settings saved".into() };
        }
        self.draft = data.settings.clone();
        self.base = data.settings.clone();
        self.origins = self.draft.categories.iter().map(|c| Some(c.name.clone())).collect();
        self.confirm_delete = None;
    }

    /// Left column: the sections.
    pub fn nav(&mut self, ui: &mut Ui, data: &Data) {
        let t = super::theme::th();
        ui.add_space(10.0);
        ui.heading("Settings");
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
            ui.spacing_mut().button_padding = egui::vec2(12.0, 7.0);
            for s in Section::ALL {
                let selected = self.section == s;
                let text = RichText::new(format!("{}   {}", s.icon(), s.label())).size(15.0).color(if selected { t.text.primary } else { t.text.muted });
                if ui.add(egui::SelectableLabel::new(selected, text)).clicked() {
                    self.section = s;
                }
            }
        });
        if self.dirty(data) {
            ui.add_space(12.0);
            ui.label(RichText::new("• Unsaved changes").color(t.status.warning).size(12.5));
        }
    }

    /// Right side: Save / Revert and the current section.
    pub fn show(&mut self, ui: &mut Ui, data: &mut Data, jobs: &Sender<Job>) {
        let ctx = ui.ctx().clone();
        self.poll_connect(&ctx, data, jobs);
        let elsewhere = self.changed_elsewhere(data);
        let dirty = self.dirty(data);
        let problem = self.problem();
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(self.section.label()).font(super::theme::bold(22.0)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let save = ui.add_enabled(dirty && problem.is_none(), egui::Button::new(RichText::new("Save").strong())).on_hover_text("Ctrl+S");
                if save.clicked() || (dirty && problem.is_none() && ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::S))) {
                    self.commit(data, &ctx);
                }
                if ui.add_enabled(dirty, egui::Button::new("Revert")).on_hover_text("Throw away unsaved changes").clicked() {
                    self.revert(data);
                }
                if let Some(p) = &problem {
                    ui.label(RichText::new(p).color(err_color()));
                } else if dirty {
                    ui.label(RichText::new("Unsaved changes").color(super::theme::th().status.warning));
                }
            });
        });
        if elsewhere {
            ui.label(RichText::new("Settings were changed elsewhere (e.g. by Claude) since you started editing. Save overwrites those changes; Revert picks them up.").color(super::theme::th().status.warning));
        }
        ui.separator();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.set_max_width(980.0);
            ui.add_space(4.0);
            match self.section {
                Section::General => self.general(ui, data),
                Section::Appearance => self.appearance(ui, data),
                Section::Categories => self.categories(ui, data),
                Section::Input => {
                    self.input(ui, data);
                    #[cfg(feature = "voice")]
                    self.voice(ui, data);
                }
                Section::Reminders => self.reminders(ui, data),
                Section::Google => self.google(ui, data, &ctx),
            }
            ui.add_space(24.0);
        });
    }

    fn poll_connect(&mut self, ctx: &egui::Context, data: &mut Data, jobs: &Sender<Job>) {
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
    }

    fn general(&mut self, ui: &mut Ui, data: &mut Data) {
        subheading(ui, "Data folder");
        ui.horizontal(|ui| {
            ui.label(RichText::new(data.paths.root.display().to_string()).monospace());
            if ui.button("📂 Open").clicked() {
                let _ = open::that(&data.paths.root);
            }
            if ui.button("Reload data files").clicked() {
                data.reload_all();
            }
        });
        note(ui, "All your data is plain JSON in this folder. Set the MORROWKEEP_DIR environment variable to use another folder (e.g. one synced by OneDrive).");

        subheading(ui, "Window");
        ui.checkbox(&mut self.draft.close_to_tray, "Closing the window keeps Morrowkeep running in the tray");
        ui.checkbox(&mut self.draft.always_on_top, "Always on top (stay above other apps)");
        note(ui, "Press F11 to switch full screen on and off.");
        let before = self.autostart;
        ui.checkbox(&mut self.autostart, "Start with Windows (in the tray)").on_hover_text("Applies right away");
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

        subheading(ui, "Calories");
        note(ui, "The daily calorie limit is on the Body tab: worked out from your profile and goal weight, or a fallback limit you set there.");
    }

    fn appearance(&mut self, ui: &mut Ui, data: &mut Data) {
        subheading(ui, "Theme");
        ui.horizontal(|ui| {
            let themes = super::theme::list(&data.paths.root);
            let current = themes.iter().find(|(stem, _)| *stem == self.draft.theme).map_or(self.draft.theme.clone(), |(_, n)| n.clone());
            egui::ComboBox::from_id_salt("theme").selected_text(current).width(220.0).show_ui(ui, |ui| {
                for (stem, name) in &themes {
                    ui.selectable_value(&mut self.draft.theme, stem.clone(), name);
                }
            });
            if ui.button("📂 Themes folder").on_hover_text("Copy a theme file there to make your own").clicked() {
                let _ = open::that(super::theme::themes_dir(&data.paths.root));
            }
        });
        note(ui, "Themes are JSON files in the themes folder. Copy one, change its colors, and pick it here. Edits to the active theme apply live as you save the file.");

        subheading(ui, "Font");
        let current = super::theme::FONTS.iter().find(|(v, _)| *v == self.draft.font).map_or(self.draft.font.as_str(), |(_, l)| *l);
        egui::ComboBox::from_id_salt("font").selected_text(current).width(260.0).show_ui(ui, |ui| {
            for (value, label) in super::theme::FONTS {
                ui.selectable_value(&mut self.draft.font, value.to_string(), *label);
            }
        });

        subheading(ui, "Calorie and weight colors");
        note(ui, "Your picks override the theme's. Category colors are under Categories.");
        let t = super::theme::th();
        egui::Grid::new("data_colors").num_columns(3).spacing([12.0, 8.0]).show(ui, |ui| {
            color_row(ui, "Calories", &mut self.draft.colors.calories, t.data.calories);
            ui.end_row();
            color_row(ui, "Weight", &mut self.draft.colors.weight, t.data.weight);
            ui.end_row();
        });
    }

    fn categories(&mut self, ui: &mut Ui, data: &mut Data) {
        note(
            ui,
            "Categories are the kinds your tasks have. Type #name in the entry bar to use one. Each has an icon, a color and the style its streaks are drawn in on the Charts tab. \
             Renaming a category moves its tasks along; deleting one turns its tasks into plain tasks. Nothing changes until you press Save.",
        );
        ui.add_space(6.0);
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for it in &data.items {
            *counts.entry(it.kind.as_str()).or_default() += 1;
        }
        let t = super::theme::th();
        let mut delete: Option<usize> = None;
        egui::Grid::new("categories").num_columns(7).spacing([14.0, 10.0]).striped(false).show(ui, |ui| {
            for h in ["Icon", "Name", "Color", "Streak style", "Preview", "Tasks", ""] {
                ui.label(RichText::new(h).size(12.0).color(t.text.muted));
            }
            ui.end_row();
            for i in 0..self.draft.categories.len() {
                let origin = self.origins[i].clone();
                let cat = &mut self.draft.categories[i];
                let theme_color = t.kind(origin.as_deref().unwrap_or(&cat.name));
                let color = cat.color.as_deref().and_then(categories::parse_hex).unwrap_or(theme_color);
                // icon
                ui.menu_button(RichText::new(&cat.icon).size(20.0), |ui| {
                    ui.set_max_width(260.0);
                    ui.horizontal_wrapped(|ui| {
                        for ic in categories::ICONS {
                            if ui.button(RichText::new(*ic).size(20.0)).clicked() {
                                cat.icon = ic.to_string();
                                ui.close_menu();
                            }
                        }
                    });
                });
                // name
                let locked = cat.name == categories::DEFAULT_KIND && origin.as_deref() == Some(categories::DEFAULT_KIND);
                let mut name = cat.name.clone();
                let r = ui.add_enabled_ui(!locked, |ui| ui.add_sized([140.0, 24.0], egui::TextEdit::singleline(&mut name).hint_text("name"))).inner;
                if locked {
                    r.on_disabled_hover_text("Every task falls back to \"task\", so it can't be renamed or deleted");
                } else if name != cat.name {
                    cat.name = categories::clean_name(&name);
                }
                // color
                ui.horizontal(|ui| {
                    let mut c = color;
                    if egui::color_picker::color_edit_button_srgba(ui, &mut c, egui::color_picker::Alpha::Opaque).changed() {
                        cat.color = Some(categories::to_hex(c));
                    }
                    if cat.color.is_some() {
                        if ui.small_button("↺").on_hover_text("Use the theme's color").clicked() {
                            cat.color = None;
                        }
                    } else {
                        ui.label(RichText::new("theme").size(11.5).color(t.text.muted));
                    }
                });
                // style
                egui::ComboBox::from_id_salt(("style", i)).selected_text(cat.style.label()).width(130.0).show_ui(ui, |ui| {
                    for s in StreakStyle::ALL {
                        ui.selectable_value(&mut cat.style, s, s.label()).on_hover_text(s.hint());
                    }
                });
                super::streaks::style_preview(ui, cat.style, color).on_hover_text(cat.style.hint());
                // tasks using it
                let n = origin.as_deref().and_then(|o| counts.get(o)).copied().unwrap_or(0);
                ui.label(RichText::new(n.to_string()).color(t.text.muted));
                // delete
                if locked {
                    ui.label("");
                } else if self.confirm_delete == Some(i) {
                    ui.horizontal(|ui| {
                        let what = if n > 0 { format!("Delete? {n} task(s) become \"task\"") } else { "Delete?".into() };
                        ui.label(RichText::new(what).color(t.status.warning).size(12.5));
                        if ui.small_button("Yes").clicked() {
                            delete = Some(i);
                        }
                        if ui.small_button("No").clicked() {
                            self.confirm_delete = None;
                        }
                    });
                } else if ui.small_button("🗑").on_hover_text("Delete this category").clicked() {
                    self.confirm_delete = Some(i);
                }
                ui.end_row();
            }
        });
        if let Some(i) = delete {
            self.draft.categories.remove(i);
            self.origins.remove(i);
            self.confirm_delete = None;
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("+ Add category").clicked() {
                let mut name = "new".to_string();
                let mut k = 2;
                while self.draft.categories.iter().any(|c| c.name == name) {
                    name = format!("new-{k}");
                    k += 1;
                }
                self.draft.categories.push(Category::new(&name, "⭐", StreakStyle::Constellation));
                self.origins.push(None);
            }
            // kinds your tasks use that aren't categories yet (e.g. from #tags or imports)
            let known: HashSet<&str> = self.origins.iter().flatten().map(|s| s.as_str()).chain(self.draft.categories.iter().map(|c| c.name.as_str())).collect();
            let loose: Vec<(&str, usize)> = counts.iter().filter(|(k, _)| !known.contains(**k)).map(|(k, n)| (*k, *n)).collect();
            if !loose.is_empty() {
                ui.add_space(12.0);
                ui.label(RichText::new("Your tasks also use:").color(t.text.muted));
                for (k, n) in loose {
                    if ui.button(format!("+ {k} ({n})")).on_hover_text("Add as a category").clicked() {
                        self.draft.categories.push(Category::new(k, "⭐", StreakStyle::Constellation));
                        // it already exists on items, so treat it as an existing name
                        self.origins.push(Some(k.to_string()));
                    }
                }
            }
        });
    }

    fn input(&mut self, ui: &mut Ui, data: &mut Data) {
        use crate::claude;
        subheading(ui, "Entry bar mode");
        note(ui, "Default mode when Morrowkeep opens. Press Tab in the entry bar (or click its mode chip) to switch for the current session.");
        ui.radio_value(&mut self.draft.default_input_mode, "local".to_string(), "Local parser: free and offline, understands dates, times, repeats and #categories");
        ui.radio_value(
            &mut self.draft.default_input_mode,
            "claude".to_string(),
            "Claude: requests run through Claude Code signed in with your Claude account (Pro), counting toward your plan's usage limits instead of paid API tokens",
        );
        subheading(ui, "Claude");
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
            None => ui.label(RichText::new("Claude Code not found. Install it, or point to claude.exe above.").color(err_color()).size(12.0)),
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
            ui.label(RichText::new(m).color(if *ok { ok_color() } else { err_color() }));
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
    }

    #[cfg(feature = "voice")]
    fn voice(&mut self, ui: &mut Ui, data: &mut Data) {
        use crate::voice::{download_model, MODELS};
        subheading(ui, "Voice input");
        note(ui, "Speech is transcribed on this PC with Whisper. Nothing is sent anywhere. The model is only loaded while transcribing.");
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
            ui.label(RichText::new("✔ model downloaded").color(ok_color()));
        } else {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Model not downloaded.").color(err_color()));
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
        if let Some((m, false)) = &self.message {
            ui.label(RichText::new(m).color(err_color()));
        }
    }

    fn reminders(&mut self, ui: &mut Ui, data: &mut Data) {
        ui.checkbox(&mut self.draft.reminders_enabled, "Show Windows notifications (also while Morrowkeep is in the tray)");
        if !self.draft.reminders_enabled {
            return;
        }
        ui.add_space(6.0);
        egui::Grid::new("reminders").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("At most every");
            ui.add(egui::DragValue::new(&mut self.draft.reminder_interval_min).range(5..=480).suffix(" min"));
            ui.end_row();
            ui.label("Remind about");
            ui.checkbox(&mut self.draft.remind_anytime, "Unfinished \"anytime\" tasks for today");
            ui.end_row();
            ui.label("");
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.draft.remind_calories, "No calories logged after");
                ui.add(egui::TextEdit::singleline(&mut self.draft.calorie_reminder_after).desired_width(50.0));
            });
            ui.end_row();
            ui.label("");
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.draft.remind_weight, "No weight logged after");
                ui.add(egui::TextEdit::singleline(&mut self.draft.weight_reminder_after).desired_width(50.0));
            });
            ui.end_row();
            ui.label("");
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.draft.remind_journal, "Nothing in the journal after");
                ui.add(egui::TextEdit::singleline(&mut self.draft.journal_reminder_after).desired_width(50.0));
            });
            ui.end_row();
            ui.label("Quiet hours");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.draft.quiet_start).desired_width(50.0));
                ui.label("to");
                ui.add(egui::TextEdit::singleline(&mut self.draft.quiet_end).desired_width(50.0));
            });
            ui.end_row();
        });
        note(ui, "Calorie, weight and journal reminders have input boxes, so you can log or write right from the notification. Typing a food description instead of a number opens Morrowkeep and estimates it with Claude.");
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("Send test reminder").clicked() {
                // Always show both input boxes in the test, so they can be tried out.
                let mut r = crate::notify::pending_reminder(data).unwrap_or_default();
                if r.lines.is_empty() {
                    r.lines.push("Test reminder. Try logging calories, weight or a journal line below.".into());
                }
                r.ask_kcal = true;
                r.ask_weight = true;
                r.ask_journal = true;
                self.reminder_msg = Some(match crate::notify::show(&data.paths, &r) {
                    Ok(()) => ("Test reminder sent".into(), true),
                    Err(e) => (e, false),
                });
            }
            ui.label(RichText::new("Notifications have Log, Open and Snooze 2h buttons.").weak().size(12.0));
        });
        if let Some((m, ok)) = &self.reminder_msg {
            ui.label(RichText::new(m).color(if *ok { ok_color() } else { err_color() }));
        }
    }

    fn google(&mut self, ui: &mut Ui, data: &mut Data, ctx: &egui::Context) {
        note(ui, "Morrowkeep copies your dated tasks into its own Google calendar and shows events from the calendars below (read-only). Your local files stay the master copy.");
        ui.add_space(6.0);
        egui::Grid::new("g_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("OAuth client ID");
            ui.add(egui::TextEdit::singleline(&mut self.draft.google_client_id).desired_width(380.0));
            ui.end_row();
            ui.label("Client secret");
            ui.add(egui::TextEdit::singleline(&mut self.draft.google_client_secret).password(true).desired_width(380.0));
            ui.end_row();
            ui.label("Morrowkeep calendar");
            ui.add(egui::TextEdit::singleline(&mut self.draft.google_calendar_name).desired_width(200.0));
            ui.end_row();
            ui.label("Show events from");
            ui.add(egui::TextEdit::singleline(&mut self.pull).desired_width(380.0).hint_text("primary, other-calendar-id@group.calendar.google.com"));
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
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if self.connecting.is_some() {
                ui.spinner();
                ui.label("Waiting for you to sign in in the browser…");
            } else {
                let label = if data.google_connected { "Reconnect" } else { "Connect Google account" };
                if ui.button(label).on_hover_text("Saves your settings first").clicked() && self.problem().is_none() {
                    self.commit(data, ctx);
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
                    ui.label(RichText::new("● connected").color(ok_color()));
                    if ui.button("Disconnect").clicked() {
                        let _ = std::fs::remove_file(&data.paths.token);
                        data.google_connected = false;
                    }
                }
            }
        });
        if let Some((m, ok)) = &self.message {
            ui.label(RichText::new(m).color(if *ok { ok_color() } else { err_color() }));
        }
        if let Some(t) = data.sync.last_sync {
            ui.label(RichText::new(format!("Last sync: {}", t.format("%Y-%m-%d %H:%M"))).weak().size(12.0));
        }
    }
}

fn subheading(ui: &mut Ui, text: &str) {
    ui.add_space(12.0);
    ui.label(RichText::new(text).size(16.5).strong());
    ui.add_space(2.0);
}

fn note(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).weak().size(12.5));
}

/// A color button for an override (none = the theme's `theme_color`), with a reset.
fn color_row(ui: &mut Ui, label: &str, value: &mut Option<String>, theme_color: Color32) {
    ui.label(label);
    let mut c = value.as_deref().and_then(categories::parse_hex).unwrap_or(theme_color);
    if egui::color_picker::color_edit_button_srgba(ui, &mut c, egui::color_picker::Alpha::Opaque).changed() {
        *value = Some(categories::to_hex(c));
    }
    if value.is_some() {
        if ui.small_button("↺ theme color").clicked() {
            *value = None;
        }
    } else {
        ui.label(RichText::new("from the theme").size(12.0).color(super::theme::th().text.muted));
    }
}
