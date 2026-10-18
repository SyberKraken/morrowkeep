//! The window: top bar, quick-add bar, and the Today / Calendar views.

mod board;
mod body;
mod charts;
mod streaks;
pub mod theme;
mod dialogs;
mod foods;
mod journal;
mod palettes;
mod settings;
mod views;
mod workouts;

use crate::model::{Item, Repeat};
use crate::{claude, import};
use crate::parse::{parse_command, parse_quick, Command};
use crate::store::Data;
use crate::tray;
use crate::worker::{Job, UiHandle};
use chrono::{Local, NaiveDate, NaiveTime};
use dialogs::{Editor, ImportDlg};
use eframe::egui::{self, Color32, Id, RichText};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq)]
enum View {
    Today,
    Calendar,
    Charts,
    Body,
    Foods,
    Workouts,
    Journal,
    Settings,
}

#[derive(Clone, Copy, PartialEq)]
pub enum CalMode {
    Month,
    Week,
}

/// Things a click asks for, applied after drawing to keep borrows simple.
pub enum Action {
    Toggle(String, NaiveDate),
    Edit(String),
    NewAt(NaiveDate, Option<NaiveTime>),
    SelectDay(NaiveDate),
    MoveToToday(String),
    AddCalories(u32, String),
    /// Today's weight in kg; None removes it.
    SetWeight(Option<f32>),
    DeleteCalories(String),
    Journal(journal::Cmd),
    OpenJournal,
    /// Log an amount of a library food or meal (by id).
    LogFood(String, crate::foods::Amount),
}

/// Text fields of the calorie card on the Today view.
#[derive(Default)]
pub struct CalorieInput {
    pub kcal: String,
    pub what: String,
    pub weight: String,
    /// "From your foods" search on the calorie card.
    pub food: String,
}

/// A category's color: yours from Settings, else the theme's.
pub fn kind_color(kind: &str) -> Color32 {
    crate::categories::color(kind).unwrap_or_else(|| theme::th().kind(kind))
}

// Colors that identify a kind of data. All come from the current theme (see theme.rs).
pub fn google_color() -> Color32 {
    theme::th().data.google
}
pub fn claude_color() -> Color32 {
    theme::th().data.claude
}
pub fn calorie_color() -> Color32 {
    crate::categories::calorie_color().unwrap_or_else(|| theme::th().data.calories)
}
pub fn weight_color() -> Color32 {
    crate::categories::weight_color().unwrap_or_else(|| theme::th().data.weight)
}
/// Muted ink for "nothing logged" notes.
pub fn not_logged_color() -> Color32 {
    theme::th().data.not_logged
}
pub fn gold_color() -> Color32 {
    theme::th().data.gold
}
/// egui id of the main entry bar.
const QUICK_ID: &str = "quick_entry";

#[cfg(feature = "voice")]
enum Voice {
    Idle,
    Recording(crate::voice::Recorder, std::time::Instant),
    Transcribing(mpsc::Receiver<Result<String, String>>),
}

pub struct App {
    shared: Arc<Mutex<Data>>,
    jobs: Sender<Job>,
    view: View,
    cal_mode: CalMode,
    /// Day the calendar is showing (any day within the month/week).
    cal_anchor: NaiveDate,
    selected: NaiveDate,
    week_scrolled: bool,
    quick: String,
    focus_quick: bool,
    editor: Option<Editor>,
    import: Option<ImportDlg>,
    /// The Settings tab (kept while switching tabs, so unsaved changes survive).
    settings: Option<settings::SettingsPage>,
    /// Running Claude request: (what was asked, result channel, start time, came from the
    /// calorie card's food box).
    claude_job: Option<(String, mpsc::Receiver<Result<String, String>>, std::time::Instant, bool)>,
    cal_input: CalorieInput,
    /// Theme version last applied to egui's visuals.
    theme_version: u64,
    applied_font: String,
    /// Window level last applied (None = not yet).
    on_top: Option<bool>,
    /// Status message being shown as a toast, and when it appeared.
    toast: Option<(String, std::time::Instant)>,
    last_status: String,
    /// Entry bar mode for this session (starts from the default in Settings).
    claude_session: bool,
    chart_period: charts::Period,
    body_ui: body::BodyUi,
    journal: journal::JournalUi,
    foods_ui: foods::FoodsUi,
    workouts_ui: workouts::WorkoutsUi,
    board: board::BoardState,
    /// Claude's answer waiting for Apply / Discard: (what was asked, import JSON).
    claude_reply: Option<(String, String)>,
    claude_error: Option<String>,
    #[cfg(feature = "voice")]
    voice: Voice,
}

impl App {
    pub fn new(cc: &eframe::CreationContext, shared: Arc<Mutex<Data>>, jobs: Sender<Job>, ui_handle: UiHandle) -> Self {
        setup_style(&cc.egui_ctx, &shared.lock().unwrap().settings.font);
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(h) = cc.window_handle() {
                if let RawWindowHandle::Win32(w) = h.as_raw() {
                    tray::HWND.store(w.hwnd.get(), std::sync::atomic::Ordering::Relaxed);
                }
            }
        }
        *ui_handle.lock().unwrap() = Some(cc.egui_ctx.clone());
        let today = Local::now().date_naive();
        let start_in_claude = shared.lock().unwrap().settings.default_input_mode == "claude";
        let applied_font = shared.lock().unwrap().settings.font.clone();
        App {
            shared,
            jobs,
            view: View::Today,
            cal_mode: CalMode::Month,
            cal_anchor: today,
            selected: today,
            week_scrolled: false,
            quick: String::new(),
            focus_quick: false,
            editor: None,
            import: None,
            settings: None,
            claude_job: None,
            cal_input: CalorieInput::default(),
            theme_version: theme::version(),
            applied_font,
            on_top: None,
            toast: None,
            last_status: String::new(),
            claude_session: start_in_claude,
            chart_period: charts::Period::Month,
            body_ui: body::BodyUi::default(),
            journal: journal::JournalUi::default(),
            foods_ui: foods::FoodsUi::default(),
            workouts_ui: workouts::WorkoutsUi::default(),
            board: board::BoardState::default(),
            claude_reply: None,
            claude_error: None,
            #[cfg(feature = "voice")]
            voice: Voice::Idle,
        }
    }

    fn claude_mode(&self) -> bool {
        self.claude_session
    }

    /// Sends a request to Claude Code in the background. `food`: it's a description of
    /// something eaten, from the calorie card.
    fn ask_claude(&mut self, data: &mut Data, ctx: &egui::Context, request: String, food: bool) {
        let request = request.trim().to_string();
        if request.is_empty() || self.claude_job.is_some() {
            return;
        }
        let Some(cli) = claude::find_cli(&data.settings) else {
            data.status = "Claude Code not found. Set its path in Settings > Input mode.".into();
            return;
        };
        let prompt = if food {
            claude::build_prompt(data, &format!("Log the calories for what I ate/drank (estimate high): {request}"))
        } else {
            claude::build_prompt(data, &request)
        };
        let (model, dir) = (data.settings.claude_model.clone(), data.paths.root.clone());
        let (tx, rx) = mpsc::channel();
        let c = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(claude::ask(&cli, &model, &prompt, &dir));
            c.request_repaint();
        });
        self.claude_job = Some((request, rx, std::time::Instant::now(), food));
        self.claude_reply = None;
        self.claude_error = None;
        self.quick.clear();
    }

    /// Puts a failed/cancelled request back where it was typed.
    fn restore_request(&mut self, request: String, food: bool) {
        // food requests go back as "ate …" so Enter retries the estimate
        self.quick = if food { format!("ate {request}") } else { request };
    }

    /// Spinner while Claude works, then a preview of its changes with Apply / Discard.
    fn claude_panel(&mut self, ui: &mut egui::Ui, data: &mut Data) {
        if let Some((request, rx, started, food)) = &self.claude_job {
            let (request, food) = (request.clone(), *food);
            match rx.try_recv() {
                Ok(Ok(text)) => {
                    self.claude_reply = Some((request, text));
                    self.claude_job = None;
                }
                Ok(Err(e)) => {
                    // put the request back so it isn't lost
                    self.restore_request(request, food);
                    self.claude_error = Some(e);
                    self.claude_job = None;
                }
                Err(_) => {
                    let mut cancel = false;
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new(format!("Claude is working on \"{request}\"… {}s", started.elapsed().as_secs())).weak());
                        cancel = ui.small_button("Cancel").clicked();
                    });
                    if cancel {
                        self.restore_request(request, food);
                        self.claude_job = None;
                    }
                    ui.ctx().request_repaint_after(Duration::from_millis(500));
                    return;
                }
            }
        }
        if let Some(e) = &self.claude_error {
            let mut dismiss = false;
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("⚠ {e}")).color(theme::th().status.danger));
                dismiss = ui.small_button("Dismiss").clicked();
            });
            if dismiss {
                self.claude_error = None;
            }
        }
        let Some((request, text)) = &self.claude_reply else { return };
        let plan = import::plan(text, &data.items, &data.calories, &data.settings, &data.foods, Local::now().date_naive());
        let mut close = false;
        let mut discard = false;
        let mut edit = None;
        egui::Frame::default()
            .fill(ui.visuals().faint_bg_color)
            .stroke(egui::Stroke::new(1.0, claude_color()))
            .inner_margin(8.0)
            .corner_radius(6.0)
            .show(ui, |ui| {
                ui.label(RichText::new(format!("Claude · \"{request}\"")).color(claude_color()).strong());
                if let Some(m) = &plan.message {
                    ui.label(RichText::new(m).italics());
                }
                for e in &plan.errors {
                    ui.label(RichText::new(format!("⚠ {e}")).color(theme::th().status.danger));
                }
                if plan.errors.is_empty() {
                    egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                        for op in &plan.ops {
                            op_line(ui, &op.describe());
                        }
                    });
                }
                ui.horizontal(|ui| {
                    let ok = plan.errors.is_empty() && !plan.ops.is_empty();
                    let n = plan.ops.len();
                    if ok && ui.button(RichText::new(format!("Apply {n} change(s)")).strong()).clicked() {
                        close = true;
                    }
                    if ui.button("Edit in Import").on_hover_text("Open Claude's JSON in the Import window to tweak it").clicked() {
                        edit = Some(text.clone());
                    }
                    discard = ui.button("Discard").clicked();
                });
            });
        if discard {
            self.claude_reply = None;
        }
        if close {
            let ops = plan
                .ops
                .into_iter()
                .map(|op| match op {
                    import::Op::LogCalories(mut c) => {
                        c.source = "claude".into();
                        import::Op::LogCalories(c)
                    }
                    other => other,
                })
                .collect();
            let summary = import::apply(data, ops);
            data.status = format!("Claude: {summary}");
            self.claude_reply = None;
        }
        if let Some(text) = edit {
            self.import = Some(ImportDlg::with_text(text));
            self.claude_reply = None;
        }
    }

    /// Day that quick-add uses when the text has no date.
    fn default_day(&self) -> NaiveDate {
        match self.view {
            View::Today | View::Charts | View::Body | View::Foods | View::Workouts | View::Journal | View::Settings => Local::now().date_naive(),
            View::Calendar => self.selected,
        }
    }

    /// Today's task best matching `query` for "done …": exact title, then title containing the
    /// words, among today's occurrences first, then any undone one-off.
    fn find_task(data: &Data, query: &str) -> Option<usize> {
        let today = Local::now().date_naive();
        let q = query.trim().to_lowercase();
        let todays: Vec<usize> = crate::model::occurrences(&data.items, today, today).into_iter().map(|(_, i)| i).collect();
        let others: Vec<usize> = (0..data.items.len()).filter(|i| data.items[*i].repeat.is_none() && !data.items[*i].done).collect();
        let matches = |i: &usize, exact: bool| {
            let t = data.items[*i].title.to_lowercase();
            if exact {
                t == q
            } else {
                q.split_whitespace().all(|w| t.contains(w))
            }
        };
        for exact in [true, false] {
            if let Some(i) = todays.iter().find(|i| matches(i, exact) && !data.items[**i].is_done_on(today)) {
                return Some(*i);
            }
            if let Some(i) = others.iter().find(|i| matches(i, exact)) {
                return Some(*i);
            }
        }
        None
    }

    /// Calories, weight, "done …" and food entries typed into the bar (Local mode).
    /// Returns true if the text was one of those rather than a task.
    fn run_command(&mut self, data: &mut Data, ctx: &egui::Context, text: &str) -> bool {
        // "1 serving of kvarg", "200 g chili": foods from the library are logged exactly
        if let Some((p, amount)) = crate::foods::parse_log(text, &data.foods) {
            data.log_food(&p, amount);
            self.quick.clear();
            return true;
        }
        let Some(cmd) = parse_command(text) else { return false };
        let today = Local::now().date_naive();
        match cmd {
            Command::Calories { kcal, what } => self.apply(data, Action::AddCalories(kcal, what)),
            Command::Weight(kg) if (20.0..=400.0).contains(&kg) => self.apply(data, Action::SetWeight(Some(kg))),
            Command::Weight(kg) => {
                data.status = format!("{kg} kg doesn't look like a body weight");
                return true;
            }
            Command::Goal { kg, by } => {
                let date = match &by {
                    Some(text) => match crate::parse::parse_date(text, today).filter(|d| *d > today) {
                        Some(d) => d,
                        None => {
                            data.status = format!("Couldn't read \"{text}\" as a future date");
                            return true;
                        }
                    },
                    None => crate::body::default_target_date(data.current_weight().unwrap_or(kg), kg, today),
                };
                if let Err(e) = body::set_goal(data, kg, date) {
                    data.status = e;
                    return true;
                }
                self.body_ui.reset();
            }
            Command::Journal(text) => journal::save_text(data, text, None),
            Command::Done(q) => match Self::find_task(data, &q) {
                Some(i) => {
                    let id = data.items[i].id.clone();
                    let title = data.items[i].title.clone();
                    if !data.items[i].is_done_on(today) {
                        self.apply(data, Action::Toggle(id, today));
                    }
                    data.status = format!("✔ {title}");
                }
                None => {
                    data.status = format!("No task matching \"{q}\" today");
                    return true;
                }
            },
            Command::Food(what) => {
                self.ask_claude(data, ctx, what, true);
            }
        }
        self.quick.clear();
        true
    }

    fn quick_add(&mut self, data: &mut Data, ctx: &egui::Context) {
        let text = self.quick.trim().to_string();
        if text.is_empty() {
            return;
        }
        if self.run_command(data, ctx, &text) {
            return;
        }
        let p = parse_quick(&text, Local::now().date_naive());
        if p.title.is_empty() {
            data.status = "Couldn't find a title in that".into();
            return;
        }
        let mut item = Item::new(p.title);
        if let Some(k) = p.kind {
            item.kind = k;
        }
        item.date = if p.someday { None } else { p.date.or(Some(self.default_day())) };
        item.time = p.time;
        item.duration_min = p.duration_min;
        item.repeat = p.repeat;
        item.until = p.until;
        data.status = format!("Added \"{}\"", item.title);
        data.items.push(item);
        data.save_items();
        self.quick.clear();
    }

    fn apply(&mut self, data: &mut Data, action: Action) {
        match action {
            Action::AddCalories(kcal, what) => {
                let today = Local::now().date_naive();
                data.calories.push(crate::model::CalorieEntry::new(today, kcal, what, "manual"));
                data.save_calories();
                data.status = format!("+{kcal} kcal · {} kcal today", data.calories_on(today));
            }
            Action::SetWeight(kg) => {
                let today = Local::now().date_naive();
                data.set_weight(today, kg);
                data.status = match kg {
                    Some(kg) => format!("Weight {kg:.1} kg saved"),
                    None => "Today's weight removed".into(),
                };
                // first weigh-in at or under the goal
                if let (Some(kg), Some(g)) = (kg, data.body.goal.as_mut()) {
                    if g.reached_on.is_none() && g.start_kg > g.target_kg && kg <= g.target_kg {
                        g.reached_on = Some(today);
                        let target = g.target_kg;
                        data.save_body();
                        data.status = format!("🎉 Goal reached: {target:.1} kg! The plan switches to maintenance.");
                    }
                }
            }
            Action::Journal(cmd) => self.journal.apply(cmd, data),
            Action::OpenJournal => self.view = View::Journal,
            Action::LogFood(id, amount) => {
                if let Some(p) = data.foods.by_id(&id) {
                    data.log_food(&p, amount);
                }
            }
            Action::DeleteCalories(id) => {
                data.calories.retain(|c| c.id != id);
                data.save_calories();
            }
            Action::Toggle(id, day) => {
                if let Some(i) = data.item_index(&id) {
                    data.items[i].toggle_done(day);
                    data.save_items();
                }
            }
            Action::Edit(id) => {
                if let Some(i) = data.item_index(&id) {
                    self.editor = Some(Editor::from_item(&data.items[i]));
                }
            }
            Action::NewAt(day, time) => self.editor = Some(Editor::new_at(Some(day), time)),
            Action::SelectDay(d) => self.selected = d,
            Action::MoveToToday(id) => {
                if let Some(i) = data.item_index(&id) {
                    data.items[i].date = Some(Local::now().date_naive());
                    data.items[i].touch();
                    data.save_items();
                }
            }
        }
    }

    /// The current toast: a pill at the bottom that slides in and fades out. Problems stay
    /// longer. Click to dismiss.
    fn show_toast(&mut self, ctx: &egui::Context) {
        let Some((msg, since)) = self.toast.clone() else { return };
        let lower = msg.to_lowercase();
        let problem = ["fail", "error", "couldn't", "not ", "no task", "doesn't", "isn't"].iter().any(|w| lower.contains(w));
        let life = if problem { 7.0 } else { 3.5 };
        let age = since.elapsed().as_secs_f32();
        if age >= life {
            self.toast = None;
            return;
        }
        let t = theme::th();
        let slide = theme::anim(0.22);
        let fade_out = theme::anim(0.45);
        let enter = if slide > 0.0 { (age / slide).min(1.0) } else { 1.0 };
        let leave = if fade_out > 0.0 { ((life - age) / fade_out).min(1.0) } else { 1.0 };
        let alpha = enter.min(leave);
        let lift = (1.0 - enter) * 18.0;
        let accent = if problem { t.status.danger } else { t.status.success };
        let resp = egui::Area::new(Id::new("toast"))
            .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -22.0 + lift])
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                ui.multiply_opacity(alpha);
                egui::Frame::default()
                    .fill(t.surface.window)
                    .stroke(egui::Stroke::new(1.0, t.surface.card_border))
                    .corner_radius(egui::CornerRadius::same(20))
                    .inner_margin(egui::Margin::symmetric(16, 9))
                    .shadow(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: t.surface.shadow })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (dot, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                            ui.painter().circle_filled(dot.center(), 4.0, accent);
                            ui.label(RichText::new(&msg).color(t.text.primary));
                        });
                    });
            })
            .response;
        if resp.interact(egui::Sense::click()).clicked() {
            self.toast = None;
            return;
        }
        // repaint smoothly while sliding/fading, otherwise just when it should disappear
        if age < slide || life - age < fade_out {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_secs_f32((life - fade_out - age).max(0.0)));
        }
    }

    /// Switches to the Settings tab (optionally at a section).
    fn open_settings(&mut self, data: &Data, section: Option<settings::Section>) {
        let page = self.settings.get_or_insert_with(|| settings::SettingsPage::open(data));
        if let Some(s) = section {
            page.section = s;
        }
        self.view = View::Settings;
    }

    fn top_bar(&mut self, ui: &mut egui::Ui, data: &mut Data) {
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            logo_mark(ui);
            ui.label(RichText::new("Morrowkeep").font(theme::bold(18.0)).color(theme::th().data.brand));
            ui.add_space(14.0);
            let order = View::ordered(&data.settings.tab_order);
            if let Some(new) = tab_bar(ui, &mut self.view, &order) {
                data.settings.tab_order = new.iter().map(|v| v.key().to_string()).collect();
                data.save_settings();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let unsaved = self.settings.as_ref().is_some_and(|s| s.dirty(data));
                let label = if unsaved { "⚙ Settings •" } else { "⚙ Settings" };
                if ui
                    .add(egui::Button::new(label).selected(self.view == View::Settings))
                    .on_hover_text(if unsaved { "Settings (unsaved changes)" } else { "Settings" })
                    .clicked()
                {
                    self.open_settings(data, None);
                }
                // reminders only on this PC for now (overrides "phone when I'm away")
                if data.settings.phone_enabled {
                    let phone_ready = crate::phone::has_push(data);
                    let tip = if phone_ready {
                        "Checked: reminders only show on this PC. Unchecked: they go to your phone when you've been away from the PC for a while (Settings > Reminders)."
                    } else {
                        "Reminders only show on this PC. Turn on notifications in the phone app to get them on your phone too."
                    };
                    if ui.checkbox(&mut data.settings.pc_only, "PC only").on_hover_text(tip).changed() {
                        data.save_settings();
                        data.status = if data.settings.pc_only { "Reminders: this PC only".into() } else { "Reminders: phone when you're away from the PC".into() };
                    }
                }
                if ui.button("📥 Import").on_hover_text("Paste JSON from your AI tools").clicked() {
                    self.import = Some(ImportDlg::default());
                }
                if data.syncing {
                    ui.spinner();
                } else if data.google_connected {
                    if ui.button("Sync now").on_hover_text("Sync with Google Calendar now").clicked() {
                        let _ = self.jobs.send(Job::SyncNow);
                    }
                }
            });
        });
    }

    fn quick_bar(&mut self, ui: &mut egui::Ui, data: &mut Data) {
        ui.horizontal(|ui| {
            #[cfg(feature = "voice")]
            self.voice_button(ui, data);
            let claude_mode = self.claude_mode();
            let (chip, chip_color) = if claude_mode { ("Claude", claude_color()) } else { ("Local", theme::th().text.muted) };
            if ui
                .add(egui::Button::new(RichText::new(chip).color(chip_color).strong()))
                .on_hover_text(if claude_mode {
                    "Requests go to Claude Code using your Claude plan. Click or press Tab in the bar for the free offline parser."
                } else {
                    "Free offline parser. Click or press Tab in the bar to send requests to Claude (your plan) instead."
                })
                .clicked()
            {
                self.claude_session = !claude_mode;
            }
            let add_w = 70.0;
            let hint = if claude_mode {
                "Just start typing… \"plan 3 gym sessions this week\", \"I had a burrito for lunch\", \"I did my run\", \"I weigh 82.4\""
            } else {
                "Just start typing… \"gym tomorrow 6pm #workout\", \"450 kcal pasta\", \"82.4 kg\", \"done vacuum\", \"ate a burrito\""
            };
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.quick)
                    .id(Id::new(QUICK_ID))
                    // keep focus on Tab: Tab switches Local/Claude instead of moving on
                    .lock_focus(true)
                    .hint_text(hint)
                    .desired_width(ui.available_width() - add_w)
                    .font(egui::TextStyle::Body),
            );
            if self.focus_quick {
                resp.request_focus();
                self.focus_quick = false;
                // put the caret after what was typed
                let id = Id::new(QUICK_ID);
                if let Some(mut state) = egui::text_edit::TextEditState::load(ui.ctx(), id) {
                    let end = egui::text::CCursor::new(self.quick.chars().count());
                    state.cursor.set_char_range(Some(egui::text_selection::CCursorRange::one(end)));
                    state.store(ui.ctx(), id);
                }
            }
            if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Tab)) {
                self.claude_session = !self.claude_session;
            }
            // Esc clears the bar (egui already drops focus on Esc, so catch it as focus is lost)
            if (resp.has_focus() || resp.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.quick.clear();
                resp.surrender_focus();
            }
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let label = if claude_mode { "Ask" } else { "Add" };
            let can = !self.quick.trim().is_empty() && self.claude_job.is_none();
            if ui.add_enabled(can, egui::Button::new(label)).clicked() || (enter && can) {
                if claude_mode {
                    let request = self.quick.clone();
                    self.ask_claude(data, ui.ctx(), request, false);
                } else {
                    self.quick_add(data, ui.ctx());
                }
                resp.request_focus();
            }
        });
        self.claude_panel(ui, data);
        // live preview of how the text was understood
        if !self.claude_mode() && !self.quick.trim().is_empty() {
            if let Some((p, amount)) = crate::foods::parse_log(&self.quick, &data.foods) {
                ui.label(RichText::new(format!("» log {} · {} = {} kcal (from your foods)", p.name, p.describe(amount), p.kcal(amount))).weak().italics());
                return;
            } else if let Some(cmd) = parse_command(&self.quick) {
                let text = match cmd {
                    Command::Calories { kcal, what } if what.is_empty() => format!("» log {kcal} kcal"),
                    Command::Calories { kcal, what } => format!("» log {kcal} kcal · {what}"),
                    Command::Weight(kg) => format!("» weight {kg:.1} kg today"),
                    Command::Journal(text) => format!("» journal entry: {text}"),
                    Command::Goal { kg, by: Some(by) } => format!("» goal {kg:.1} kg by {by}"),
                    Command::Goal { kg, by: None } => format!("» goal {kg:.1} kg (at 0.5 kg/week)"),
                    Command::Done(q) => match Self::find_task(data, &q) {
                        Some(i) => format!("» tick off \"{}\"", data.items[i].title),
                        None => format!("» no task matching \"{q}\" today"),
                    },
                    Command::Food(what) => format!("» estimate calories with Claude: \"{what}\""),
                };
                ui.label(RichText::new(text).weak().italics());
                return;
            }
            let p = parse_quick(&self.quick, Local::now().date_naive());
            let date = if p.someday { None } else { p.date.or(Some(self.default_day())) };
            let mut parts = vec![format!("[{}]", p.kind.as_deref().unwrap_or("task")), format!("\"{}\"", p.title)];
            parts.push(match date {
                Some(d) => d.format("%a %b %-d").to_string(),
                None => "backlog (no date)".into(),
            });
            if let Some(t) = p.time {
                parts.push(t.format("%H:%M").to_string());
            }
            if let Some(m) = p.duration_min {
                parts.push(format!("{m} min"));
            }
            if let Some(r) = &p.repeat {
                parts.push(format!("🔁 {r}"));
            }
            if let Some(u) = p.until {
                parts.push(format!("until {}", u.format("%b %-d")));
            }
            ui.label(RichText::new(format!("» {}", parts.join("  ·  "))).weak().italics());
        }
    }

    #[cfg(feature = "voice")]
    fn voice_button(&mut self, ui: &mut egui::Ui, data: &mut Data) {
        let model = data.paths.models.join(&data.settings.whisper_model);
        match &self.voice {
            Voice::Idle => {
                if ui.button("🎤").on_hover_text("Dictate (click again to stop)").clicked() {
                    if !model.exists() {
                        data.status = "Voice model not downloaded yet. Download it under Settings > Input & voice.".into();
                        self.open_settings(data, Some(settings::Section::Input));
                    } else {
                        match crate::voice::Recorder::start() {
                            Ok(r) => self.voice = Voice::Recording(r, std::time::Instant::now()),
                            Err(e) => data.status = format!("Microphone: {e}"),
                        }
                    }
                }
            }
            Voice::Recording(_, started) => {
                let secs = started.elapsed().as_secs();
                let pulse = (ui.input(|i| i.time) * 3.0).sin() * 0.5 + 0.5;
                let red = Color32::from_rgb(200 + (55.0 * pulse) as u8, 50, 50);
                if ui
                    .add(egui::Button::new(RichText::new(format!("■ Stop {secs}s")).color(theme::th().text.on_color)).fill(red))
                    .on_hover_text("Stop and transcribe")
                    .clicked()
                    || secs >= 60
                {
                    if let Voice::Recording(rec, _) = std::mem::replace(&mut self.voice, Voice::Idle) {
                        let audio = rec.finish();
                        let lang = data.settings.voice_language.clone();
                        let (tx, rx) = mpsc::channel();
                        let ctx = ui.ctx().clone();
                        std::thread::spawn(move || {
                            let _ = tx.send(crate::voice::transcribe(&model, &audio, &lang));
                            ctx.request_repaint();
                        });
                        self.voice = Voice::Transcribing(rx);
                    }
                }
                ui.ctx().request_repaint_after(Duration::from_millis(100));
            }
            Voice::Transcribing(rx) => {
                ui.spinner();
                if let Ok(res) = rx.try_recv() {
                    match res {
                        Ok(text) => {
                            if !self.quick.is_empty() {
                                self.quick.push(' ');
                            }
                            self.quick += &text;
                            self.focus_quick = true;
                            data.status = "Check the preview, then press Enter to add".into();
                        }
                        Err(e) => data.status = format!("Voice: {e}"),
                    }
                    self.voice = Voice::Idle;
                }
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let shared = self.shared.clone();
        let mut data = shared.lock().unwrap();
        // a font changed outside the Settings page (e.g. by Claude)
        if data.settings.font != self.applied_font {
            self.applied_font = data.settings.font.clone();
            theme::apply_fonts(ctx, &self.applied_font);
        }
        if theme::version() != self.theme_version {
            self.theme_version = theme::version();
            theme::apply_visuals(ctx);
        }
        // Food typed into a reminder notification: estimate it with Claude, shown for review.
        if let Some(food) = crate::notify::take_pending_food() {
            self.view = View::Today;
            self.ask_claude(&mut data, ctx, food, true);
        }
        let typed: String = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| if let egui::Event::Text(t) = e { Some(t.as_str()) } else { None })
                .collect()
        });
        let no_dialog = self.editor.is_none() && self.import.is_none();
        // Only when no text box is taking keyboard input (a focused button or tile doesn't count).
        if !typed.is_empty() && no_dialog && !ctx.wants_keyboard_input() {
            // on the Journal page, typing starts an entry instead
            if self.view == View::Journal {
                self.journal.type_into(&typed);
            } else {
                self.quick.push_str(&typed);
                self.focus_quick = true;
            }
        }
        self.journal.poll(ctx, &mut data);
        self.foods_ui.poll(&mut data);
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::K)) {
            self.focus_quick = true;
        }
        // F11: full screen on/off
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F11)) {
            let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!full));
        }
        // Always on top, from Settings (applied when it changes and when the window opens)
        if self.on_top != Some(data.settings.always_on_top) {
            self.on_top = Some(data.settings.always_on_top);
            let level = if data.settings.always_on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal };
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
        }

        let mut actions = Vec::new();
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(6.0);
            self.top_bar(ui, &mut data);
            ui.add_space(4.0);
            if let Some(err) = data.load_error.clone() {
                egui::Frame::default()
                    .fill(theme::th().status.danger_background)
                    .inner_margin(8.0)
                    .corner_radius(4.0)
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(err).color(theme::th().text.primary));
                            if ui.button("Reload").clicked() {
                                data.reload_all();
                            }
                        });
                    });
                ui.add_space(4.0);
            }
            self.quick_bar(ui, &mut data);
            ui.add_space(6.0);
        });

        match self.view {
            View::Today => {
                egui::SidePanel::right("today_side").resizable(true).default_width(370.0).min_width(300.0).max_width(560.0).show(ctx, |ui| {
                    views::today_side(ui, &data, &mut self.cal_input, &mut self.journal, &mut actions);
                });
                egui::CentralPanel::default().show(ctx, |ui| views::today(ui, &data, &mut self.board, &mut actions));
            }
            View::Charts => {
                egui::CentralPanel::default().show(ctx, |ui| charts::charts(ui, &data, &mut self.chart_period));
            }
            View::Body => {
                egui::CentralPanel::default().show(ctx, |ui| body::body(ui, &mut data, &mut self.body_ui));
            }
            View::Journal => self.journal.page(ctx, &mut data),
            View::Foods => self.foods_ui.page(ctx, &mut data),
            View::Workouts => self.workouts_ui.page(ctx, &mut data),
            View::Settings => {
                let page = self.settings.get_or_insert_with(|| settings::SettingsPage::open(&data));
                egui::SidePanel::left("settings_nav").resizable(false).exact_width(210.0).show(ctx, |ui| page.nav(ui, &data));
                egui::CentralPanel::default().show(ctx, |ui| page.show(ui, &mut data, &self.jobs));
            }
            View::Calendar => {
                egui::SidePanel::right("day_panel").resizable(true).default_width(330.0).min_width(240.0).show(ctx, |ui| {
                    views::day_panel(ui, &data, self.selected, &mut actions);
                });
                egui::CentralPanel::default().show(ctx, |ui| {
                    views::calendar_header(ui, &mut self.cal_mode, &mut self.cal_anchor, &mut self.selected);
                    ui.add_space(4.0);
                    match self.cal_mode {
                        CalMode::Month => views::month(ui, &data, self.cal_anchor, self.selected, &mut actions),
                        CalMode::Week => {
                            views::week(ui, &data, self.cal_anchor, self.selected, &mut self.week_scrolled, &mut actions)
                        }
                    }
                });
            }
        }

        for a in actions {
            self.apply(&mut data, a);
        }

        if let Some(mut ed) = self.editor.take() {
            if ed.show(ctx, &mut data) {
                self.editor = Some(ed);
            }
        }
        if let Some(mut dlg) = self.import.take() {
            if dlg.show(ctx, &mut data) {
                self.import = Some(dlg);
            }
        }

        // New status messages appear as a toast at the bottom (routine sync results don't).
        if data.status != self.last_status {
            self.last_status = data.status.clone();
            if !data.status.is_empty() && !data.status.starts_with("Synced") {
                self.toast = Some((data.status.clone(), std::time::Instant::now()));
            }
        }
        self.show_toast(ctx);

        // Keep the clock / "now" line / day rollover current.
        ctx.request_repaint_after(Duration::from_secs(30));
    }
}

/// One preview line (or several) of an import; "#RRGGBB" colors get a swatch in front.
pub fn op_line(ui: &mut egui::Ui, text: &str) {
    for line in text.lines() {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let mut buf = String::new();
            for word in line.split_inclusive(' ') {
                if let Some(c) = word.get(..7).and_then(crate::categories::parse_hex) {
                    if !buf.is_empty() {
                        ui.label(RichText::new(std::mem::take(&mut buf)).monospace().size(12.5));
                    }
                    let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 12.0), egui::Sense::hover());
                    ui.painter().rect_filled(egui::Rect::from_center_size(r.center() - egui::vec2(2.0, 0.0), egui::vec2(11.0, 11.0)), 2.0, c);
                }
                buf.push_str(word);
            }
            if !buf.is_empty() {
                ui.label(RichText::new(buf).monospace().size(12.5));
            }
        });
    }
}

/// The logo: a tiny constellation (four stars, joined) in the brand color.
fn logo_mark(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(30.0, 24.0), egui::Sense::hover());
    let c = theme::th().data.brand;
    let p = ui.painter();
    let pts = [(3.0, 17.0), (11.0, 8.0), (19.0, 14.0), (27.0, 5.0)].map(|(x, y)| rect.left_top() + egui::vec2(x, y));
    for w in pts.windows(2) {
        p.line_segment([w[0], w[1]], egui::Stroke::new(1.6, c.linear_multiply(0.8)));
    }
    for (i, pt) in pts.iter().enumerate() {
        let r = if i == 3 { 3.6 } else { 2.6 };
        p.circle_filled(*pt, r + 2.5, c.linear_multiply(0.2));
        p.circle_filled(*pt, r, c);
    }
}

impl View {
    /// The main tabs (Settings has its own button), in their default order.
    const TABS: [View; 7] = [View::Today, View::Calendar, View::Charts, View::Body, View::Foods, View::Workouts, View::Journal];

    fn key(self) -> &'static str {
        match self {
            View::Today => "habits",
            View::Calendar => "calendar",
            View::Charts => "charts",
            View::Body => "body",
            View::Foods => "foods",
            View::Workouts => "workouts",
            View::Journal => "journal",
            View::Settings => "settings",
        }
    }

    fn tab(self) -> (&'static str, &'static str) {
        match self {
            View::Today => ("✔", "Habits"),
            View::Calendar => ("📅", "Calendar"),
            View::Charts => ("📈", "Charts"),
            View::Body => ("⚖", "Body"),
            View::Foods => ("🍴", "Foods"),
            View::Workouts => ("💪", "Workouts"),
            View::Journal => ("📖", "Journal"),
            View::Settings => ("⚙", "Settings"),
        }
    }

    /// The tabs in the saved order; tabs missing from it (e.g. new ones) go at the end.
    fn ordered(saved: &[String]) -> Vec<View> {
        let mut v: Vec<View> = saved.iter().filter_map(|k| View::TABS.iter().copied().find(|t| t.key() == k)).collect();
        v.dedup();
        for t in View::TABS {
            if !v.contains(&t) {
                v.push(t);
            }
        }
        v
    }
}

/// The main tabs as a segmented pill control with icons. Drag a tab sideways to move it;
/// returns the new order when a drag ends.
fn tab_bar(ui: &mut egui::Ui, view: &mut View, order: &[View]) -> Option<Vec<View>> {
    let t = theme::th();
    let font = egui::FontId::proportional(14.5);
    let (pad_x, h) = (14.0, 30.0);
    let label = |v: View| {
        let (icon, name) = v.tab();
        format!("{icon}  {name}")
    };
    let widths: Vec<f32> = order.iter().map(|v| ui.painter().layout_no_wrap(label(*v), font.clone(), t.text.primary).size().x + 2.0 * pad_x).collect();
    let total = egui::vec2(widths.iter().sum::<f32>() + 6.0, h);
    let (outer, _) = ui.allocate_exact_size(total, egui::Sense::hover());
    let radius = egui::CornerRadius::same((h / 2.0) as u8);
    let pill = egui::CornerRadius::same(((h - 6.0) / 2.0) as u8);
    ui.painter().rect_filled(outer, radius, t.surface.card);
    ui.painter().rect_stroke(outer, radius, egui::Stroke::new(1.0, t.surface.card_border), egui::StrokeKind::Inside);
    let mut rects = Vec::new();
    let mut x = outer.left() + 3.0;
    for w in &widths {
        rects.push(egui::Rect::from_min_size(egui::pos2(x, outer.top() + 3.0), egui::vec2(*w, h - 6.0)));
        x += w;
    }
    let mut dragging: Option<(usize, f32)> = None;
    let mut dropped: Option<(usize, f32)> = None;
    for (k, tab) in order.iter().enumerate() {
        let r = rects[k];
        let resp = ui.interact(r, Id::new(("tab", tab.key())), egui::Sense::click_and_drag());
        if resp.dragged() {
            if let Some(p) = resp.interact_pointer_pos() {
                dragging = Some((k, p.x));
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
        }
        if resp.drag_stopped() {
            if let Some(p) = ui.ctx().pointer_latest_pos() {
                dropped = Some((k, p.x));
            }
        }
        let selected = *view == *tab;
        let being_dragged = dragging.is_some_and(|(d, _)| d == k);
        if selected {
            ui.painter().rect_filled(r, pill, t.widget.selection.linear_multiply(if being_dragged { 0.4 } else { 1.0 }));
        } else if resp.hovered() || being_dragged {
            ui.painter().rect_filled(r, pill, t.widget.fill_hover);
        }
        let ink = if selected || resp.hovered() { t.text.primary } else { t.text.muted };
        let g = ui.painter().layout_no_wrap(label(*tab), font.clone(), ink);
        ui.painter().galley(r.center() - g.size() / 2.0, g, ink);
        if resp.clicked() {
            *view = *tab;
        }
    }
    // where a dragged tab would land: before the first tab whose middle is right of the pointer
    let slot = |px: f32| rects.iter().position(|r| px < r.center().x).unwrap_or(rects.len());
    if let Some((from, px)) = dragging {
        let to = slot(px);
        if to != from && to != from + 1 {
            let line_x = if to < rects.len() { rects[to].left() } else { rects[rects.len() - 1].right() };
            ui.painter().line_segment([egui::pos2(line_x, outer.top() + 4.0), egui::pos2(line_x, outer.bottom() - 4.0)], egui::Stroke::new(2.5, t.widget.selection_border));
        }
    }
    let (from, px) = dropped?;
    let mut to = slot(px);
    if to == from || to == from + 1 {
        return None;
    }
    let mut v = order.to_vec();
    let moved = v.remove(from);
    if to > from {
        to -= 1;
    }
    v.insert(to, moved);
    Some(v)
}

fn setup_style(ctx: &egui::Context, font: &str) {
    theme::apply_fonts(ctx, font);
    theme::apply_visuals(ctx);
    ctx.all_styles_mut(|style| {
        use egui::{FontFamily, FontId, TextStyle};
        style.text_styles.insert(TextStyle::Body, FontId::new(14.5, FontFamily::Proportional));
        style.text_styles.insert(TextStyle::Button, FontId::new(14.0, FontFamily::Proportional));
        style.text_styles.insert(TextStyle::Heading, FontId::new(22.0, theme::bold_family()));
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        style.spacing.interact_size.y = 24.0;
        // A blinking caret repaints continuously while a text box has focus.
        style.visuals.text_cursor.blink = false;
    });
}

/// Human description of an item's schedule, e.g. "18:00–19:00 · 🔁 weekly mon,thu".
pub fn schedule_text(item: &Item) -> String {
    let mut parts = Vec::new();
    if let (Some(s), Some(e)) = (item.time, item.end_time()) {
        parts.push(format!("{}–{}", s.format("%H:%M"), e.format("%H:%M")));
    }
    if let Some(r) = item.repeat.as_deref() {
        parts.push(format!("🔁 {}", Repeat::parse(r).map(|r| r.describe()).unwrap_or_else(|_| r.to_string())));
    }
    parts.join(" · ")
}
