//! Journal tab, and the quick journal card on the Habits page.
//!
//! Typed entries save straight away. A voice entry saves its recording as a WAV file at once
//! and is transcribed in the background with Whisper; the transcript appears in the entry
//! when it's done (on any tab). The page lists days newest first; the mini calendar or
//! "Show older days" browses back, and search covers every entry.

use super::{claude_color, theme, Action};
use crate::journal::{self, JournalEntry, MOODS};
use crate::store::Data;
use chrono::{Datelike, Duration, Local, NaiveDate};
use eframe::egui::{self, Color32, RichText, Sense, Ui, Vec2};
use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver};

/// Days listed before "Show older days".
const DAYS_PER_PAGE: usize = 14;
/// Longest recording, in seconds.
const MAX_REC_SECS: u64 = 300;

/// What the quick card on the Habits page asks for.
pub enum Cmd {
    SaveQuick,
    ToggleRecordQuick,
}

pub struct JournalUi {
    draft: String,
    mood: Option<u8>,
    prompt: usize,
    focus_draft: bool,
    quick: String,
    quick_mood: Option<u8>,
    search: String,
    only_pinned: bool,
    only_voice: bool,
    /// Browse from this day backwards (None = from today).
    anchor: Option<NaiveDate>,
    days_shown: usize,
    month: NaiveDate,
    /// Entry being edited: (id, text, transcript).
    editing: Option<(String, String, String)>,
    confirm_delete: Option<String>,
    /// Recording in progress: (recorder, started, from the quick card).
    #[cfg(feature = "voice")]
    rec: Option<(crate::voice::Recorder, std::time::Instant, bool)>,
    /// Transcriptions running in the background: (entry id, result).
    transcribing: Vec<(String, Receiver<Result<String, String>>)>,
    reflecting: Option<Receiver<Result<String, String>>>,
    ctx: Option<egui::Context>,
}

impl Default for JournalUi {
    fn default() -> Self {
        let today = Local::now().date_naive();
        JournalUi {
            draft: String::new(),
            mood: None,
            prompt: today.ordinal0() as usize % journal::PROMPTS.len(),
            focus_draft: false,
            quick: String::new(),
            quick_mood: None,
            search: String::new(),
            only_pinned: false,
            only_voice: false,
            anchor: None,
            days_shown: DAYS_PER_PAGE,
            month: today.with_day(1).unwrap(),
            editing: None,
            confirm_delete: None,
            #[cfg(feature = "voice")]
            rec: None,
            transcribing: Vec::new(),
            reflecting: None,
            ctx: None,
        }
    }
}

pub fn journal_color() -> Color32 {
    theme::th().data.journal
}

/// rough → great as danger → warning → success.
pub fn mood_color(m: u8) -> Color32 {
    let t = theme::th();
    let mix = |a: Color32, b: Color32, f: f32| {
        let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * f) as u8;
        Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
    };
    match m {
        1 => t.status.danger,
        2 => mix(t.status.danger, t.status.warning, 0.6),
        3 => t.status.warning,
        4 => mix(t.status.warning, t.status.success, 0.6),
        _ => t.status.success,
    }
}

/// Five dots; click one to pick a mood, click it again to clear.
fn mood_picker(ui: &mut Ui, mood: &mut Option<u8>) {
    ui.label(RichText::new("Mood").color(theme::th().text.muted).size(12.5));
    for m in 1..=5u8 {
        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
        let c = mood_color(m);
        let selected = *mood == Some(m);
        let p = ui.painter();
        if selected {
            p.circle_filled(rect.center(), 7.5, c);
        } else {
            p.circle_stroke(rect.center(), 6.0, egui::Stroke::new(if resp.hovered() { 2.0 } else { 1.3 }, c.linear_multiply(0.8)));
        }
        if resp.on_hover_text(MOODS[(m - 1) as usize]).clicked() {
            *mood = if selected { None } else { Some(m) };
        }
    }
    if let Some(m) = mood {
        ui.label(RichText::new(journal::mood_label(*m)).size(12.5).color(mood_color(*m)));
    }
}

fn mmss(secs: f32) -> String {
    let s = secs.round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

fn day_title(d: NaiveDate, today: NaiveDate) -> String {
    match (today - d).num_days() {
        0 => format!("Today · {}", d.format("%A, %B %-d")),
        1 => format!("Yesterday · {}", d.format("%A, %B %-d")),
        _ if d.year() == today.year() => d.format("%A, %B %-d").to_string(),
        _ => d.format("%A, %B %-d, %Y").to_string(),
    }
}

impl JournalUi {
    /// Runs every frame on every tab: finishes background transcriptions and reflections.
    pub fn poll(&mut self, ctx: &egui::Context, data: &mut Data) {
        self.ctx = Some(ctx.clone());
        let mut done = Vec::new();
        self.transcribing.retain(|(id, rx)| match rx.try_recv() {
            Ok(r) => {
                done.push((id.clone(), r));
                false
            }
            Err(mpsc::TryRecvError::Empty) => true,
            Err(mpsc::TryRecvError::Disconnected) => false,
        });
        for (id, r) in done {
            if let Some(e) = data.journal.iter_mut().find(|e| e.id == id) {
                match r {
                    Ok(text) => {
                        e.transcript = Some(journal::clean_transcript(&text));
                        data.status = "Recording transcribed".into();
                    }
                    Err(err) => data.status = format!("Transcription failed: {err}"),
                }
                data.save_journal();
            }
        }
        if let Some(rx) = &self.reflecting {
            match rx.try_recv() {
                Ok(Ok(text)) => {
                    let mut e = JournalEntry::new(text.trim());
                    e.kind = "reflection".into();
                    data.journal.push(e);
                    data.save_journal();
                    data.status = "Claude's reflection added to today's journal".into();
                    self.reflecting = None;
                    self.anchor = None;
                }
                Ok(Err(err)) => {
                    data.status = format!("Reflection failed: {err}");
                    self.reflecting = None;
                }
                Err(_) => {}
            }
        }
        #[cfg(feature = "voice")]
        if let Some((_, started, _)) = &self.rec {
            if started.elapsed().as_secs() >= MAX_REC_SECS {
                self.stop_recording(data);
            }
        }
        if self.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }

    fn busy(&self) -> bool {
        #[cfg(feature = "voice")]
        if self.rec.is_some() {
            return true;
        }
        !self.transcribing.is_empty() || self.reflecting.is_some()
    }

    #[cfg(feature = "voice")]
    fn recording(&self) -> Option<(u64, bool)> {
        self.rec.as_ref().map(|(_, t, quick)| (t.elapsed().as_secs(), *quick))
    }
    #[cfg(not(feature = "voice"))]
    fn recording(&self) -> Option<(u64, bool)> {
        None
    }

    pub fn apply(&mut self, cmd: Cmd, data: &mut Data) {
        match cmd {
            Cmd::SaveQuick => {
                let text = std::mem::take(&mut self.quick);
                let mood = self.quick_mood.take();
                save_text(data, text, mood);
            }
            Cmd::ToggleRecordQuick => self.toggle_recording(data, true),
        }
    }

    fn toggle_recording(&mut self, data: &mut Data, quick: bool) {
        #[cfg(feature = "voice")]
        {
            if self.rec.is_some() {
                self.stop_recording(data);
            } else {
                match crate::voice::Recorder::start() {
                    Ok(r) => self.rec = Some((r, std::time::Instant::now(), quick)),
                    Err(e) => data.status = format!("Microphone: {e}"),
                }
            }
        }
        #[cfg(not(feature = "voice"))]
        {
            let _ = quick;
            data.status = "This build has no voice support".into();
        }
    }

    /// Saves the recording (with any text typed alongside it) and starts transcribing it.
    #[cfg(feature = "voice")]
    fn stop_recording(&mut self, data: &mut Data) {
        let Some((rec, _, quick)) = self.rec.take() else { return };
        let samples = rec.finish();
        if samples.len() < 8000 {
            data.status = "Recording too short, nothing saved".into();
            return;
        }
        let (text, mood) = if quick {
            (std::mem::take(&mut self.quick), self.quick_mood.take())
        } else {
            (std::mem::take(&mut self.draft), self.mood.take())
        };
        let mut e = JournalEntry::new(text.trim());
        e.mood = mood;
        let file = format!("{}.wav", Local::now().format("%Y-%m-%d_%H%M%S"));
        if let Err(err) = journal::write_wav(&data.paths.journal_audio.join(&file), &samples) {
            data.status = format!("Couldn't save the recording: {err}");
            return;
        }
        e.audio = Some(file);
        e.audio_secs = Some(samples.len() as f32 / 16_000.0);
        let id = e.id.clone();
        data.journal.push(e);
        data.save_journal();
        data.status = "Recording saved, transcribing…".into();
        self.transcribe(data, id, samples);
    }

    /// Transcribes in the background, if a voice model is downloaded.
    #[cfg(feature = "voice")]
    fn transcribe(&mut self, data: &mut Data, id: String, samples: Vec<f32>) {
        let model = data.paths.models.join(&data.settings.whisper_model);
        if !model.exists() {
            data.status = "Recording saved. Download a voice model (Settings > Input & voice) to transcribe it.".into();
            return;
        }
        let lang = data.settings.voice_language.clone();
        let (tx, rx) = mpsc::channel();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(crate::voice::transcribe(&model, &samples, &lang));
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
        self.transcribing.push((id, rx));
    }

    fn start_reflection(&mut self, data: &mut Data) {
        let today = Local::now().date_naive();
        let Some(prompt) = journal::reflection_prompt(&data.journal, today) else {
            data.status = "Nothing written in the last 7 days to reflect on".into();
            return;
        };
        let Some(cli) = crate::claude::find_cli(&data.settings) else {
            data.status = "Claude Code not found. Set its path in Settings > Input & voice.".into();
            return;
        };
        let (tx, rx) = mpsc::channel();
        let (model, dir, ctx) = (data.settings.claude_model.clone(), data.paths.root.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(crate::claude::ask(&cli, &model, &prompt, &dir));
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
        self.reflecting = Some(rx);
        data.status = "Asking Claude for a reflection on your week…".into();
    }

    /// Text typed anywhere on the Journal page goes into the composer.
    pub fn type_into(&mut self, text: &str) {
        self.draft.push_str(text);
        self.focus_draft = true;
    }

    // ------------------------------------------------------------------ Habits card

    /// The quick journal card on the Habits page.
    pub fn quick_card(&mut self, ui: &mut Ui, data: &Data, actions: &mut Vec<Action>) {
        let today = Local::now().date_naive();
        let n = data.journal_on(today);
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("Journal").size(16.0).strong().color(journal_color()));
                let note = match n {
                    0 => "nothing yet today".to_string(),
                    1 => "1 entry today".to_string(),
                    n => format!("{n} entries today"),
                };
                ui.label(RichText::new(note).weak().size(12.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Open ›").on_hover_text("All entries, search and voice notes").clicked() {
                        actions.push(Action::OpenJournal);
                    }
                });
            });
            let r = ui.add(
                egui::TextEdit::multiline(&mut self.quick)
                    .desired_rows(2)
                    .desired_width(ui.available_width())
                    .hint_text("A line about today… (#tags work)"),
            );
            let ctrl_enter = r.has_focus() && ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Enter));
            ui.horizontal(|ui| {
                mood_picker(ui, &mut self.quick_mood);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let can_save = !self.quick.trim().is_empty();
                    if ui.add_enabled(can_save, egui::Button::new(RichText::new("Save").strong())).on_hover_text("Ctrl+Enter").clicked() || (ctrl_enter && can_save) {
                        actions.push(Action::Journal(Cmd::SaveQuick));
                    }
                    if cfg!(feature = "voice") {
                        match self.recording() {
                            Some((secs, true)) => {
                                if ui.add(egui::Button::new(RichText::new(format!("■ {}", mmss(secs as f32))).color(theme::th().text.on_color)).fill(theme::th().status.danger)).on_hover_text("Stop and save").clicked() {
                                    actions.push(Action::Journal(Cmd::ToggleRecordQuick));
                                }
                            }
                            Some(_) => {
                                ui.add_enabled(false, egui::Button::new("🎤")).on_disabled_hover_text("Recording on the Journal page");
                            }
                            None => {
                                if ui.button("🎤").on_hover_text("Record a voice entry (saved and transcribed)").clicked() {
                                    actions.push(Action::Journal(Cmd::ToggleRecordQuick));
                                }
                            }
                        }
                    }
                });
            });
        });
    }

    // ------------------------------------------------------------------ Journal tab

    pub fn page(&mut self, ctx: &egui::Context, data: &mut Data) {
        egui::SidePanel::left("journal_side").resizable(false).exact_width(280.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| self.sidebar(ui, data));
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.set_max_width(860.0);
                ui.add_space(10.0);
                self.composer(ui, data);
                ui.add_space(14.0);
                self.entries(ui, data);
                ui.add_space(30.0);
            });
        });
    }

    fn sidebar(&mut self, ui: &mut Ui, data: &mut Data) {
        let t = theme::th();
        let today = Local::now().date_naive();
        ui.add_space(10.0);
        ui.heading("Journal");
        // stats
        let streak = journal::streak(&data.journal, today);
        let month_entries: Vec<&JournalEntry> = data.journal.iter().filter(|e| e.kind == "note" && e.date.year() == today.year() && e.date.month() == today.month()).collect();
        let words: usize = month_entries.iter().map(|e| e.words()).sum();
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("🔥 {streak}")).strong().color(journal_color()));
            ui.label(RichText::new(if streak == 1 { "day in a row" } else { "days in a row" }).weak());
        });
        ui.label(RichText::new(format!("{} entries · {words} words this month", month_entries.len())).weak().size(12.5));
        ui.add_space(8.0);

        // search and filters
        ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search entries…").desired_width(f32::INFINITY));
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.only_pinned, "Pinned");
            ui.checkbox(&mut self.only_voice, "Voice");
            if (!self.search.is_empty() || self.only_pinned || self.only_voice) && ui.small_button("Clear").clicked() {
                self.search.clear();
                self.only_pinned = false;
                self.only_voice = false;
            }
        });
        // most used tags
        let mut tags: BTreeMap<String, usize> = BTreeMap::new();
        for e in &data.journal {
            for tag in e.tags() {
                *tags.entry(tag).or_default() += 1;
            }
        }
        if !tags.is_empty() {
            let mut top: Vec<(String, usize)> = tags.into_iter().collect();
            top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            ui.horizontal_wrapped(|ui| {
                for (tag, n) in top.into_iter().take(12) {
                    if ui.small_button(format!("#{tag} {n}")).on_hover_text("Search this tag").clicked() {
                        self.search = format!("#{tag}");
                    }
                }
            });
        }
        ui.add_space(10.0);
        self.mini_calendar(ui, data, today);

        // on this day
        let earlier: Vec<(String, &JournalEntry)> = [(30, "A month ago"), (91, "3 months ago"), (365, "A year ago")]
            .iter()
            .flat_map(|(days, label)| {
                let d = today - Duration::days(*days);
                data.journal.iter().filter(move |e| e.date == d && e.kind == "note").map(move |e| (label.to_string(), e))
            })
            .take(3)
            .collect();
        if !earlier.is_empty() {
            ui.add_space(10.0);
            ui.label(RichText::new("ON THIS DAY").size(11.5).color(t.text.muted));
            for (label, e) in earlier {
                let first = e.full_text().lines().next().unwrap_or("").chars().take(60).collect::<String>();
                if ui.add(egui::Label::new(RichText::new(format!("{label}: {first}")).size(12.5)).sense(Sense::click())).on_hover_text("Show that day").clicked() {
                    self.anchor = Some(e.date);
                    self.days_shown = DAYS_PER_PAGE;
                }
            }
        }

        ui.add_space(14.0);
        ui.label(RichText::new("MORE").size(11.5).color(t.text.muted));
        if self.reflecting.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Claude is reflecting…");
            });
        } else if ui
            .button(RichText::new("✨ Reflect on my week").color(claude_color()))
            .on_hover_text("Sends your last 7 days of entries to Claude and adds its short reflection to today. Uses a little of your Claude plan.")
            .clicked()
        {
            self.start_reflection(data);
        }
        if ui.button("Export to Markdown").on_hover_text("Writes the whole journal to journal-export.md and opens it").clicked() {
            let path = data.paths.root.join("journal-export.md");
            match std::fs::write(&path, journal::to_markdown(&data.journal)) {
                Ok(()) => {
                    let _ = open::that(&path);
                    data.status = "Journal exported to journal-export.md".into();
                }
                Err(e) => data.status = format!("Export failed: {e}"),
            }
        }
        if ui.button("📂 Recordings folder").clicked() {
            let _ = std::fs::create_dir_all(&data.paths.journal_audio);
            let _ = open::that(&data.paths.journal_audio);
        }
    }

    fn mini_calendar(&mut self, ui: &mut Ui, data: &Data, today: NaiveDate) {
        let t = theme::th();
        ui.horizontal(|ui| {
            if ui.small_button("◀").clicked() {
                self.month = (self.month - Duration::days(1)).with_day(1).unwrap();
            }
            ui.label(RichText::new(self.month.format("%B %Y").to_string()).strong());
            if ui.small_button("▶").clicked() {
                self.month = (self.month + Duration::days(32)).with_day(1).unwrap();
            }
        });
        let counts: BTreeMap<NaiveDate, usize> = data.journal.iter().filter(|e| e.kind == "note").fold(BTreeMap::new(), |mut m, e| {
            *m.entry(e.date).or_default() += 1;
            m
        });
        let cell = ((ui.available_width() - 4.0) / 7.0).min(36.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(cell * 7.0, cell * 6.0 + 16.0), Sense::hover());
        let p = ui.painter_at(rect);
        for (i, d) in ["M", "T", "W", "T", "F", "S", "S"].iter().enumerate() {
            p.text(egui::pos2(rect.left() + cell * (i as f32 + 0.5), rect.top() + 7.0), egui::Align2::CENTER_CENTER, *d, egui::FontId::proportional(10.5), t.text.muted);
        }
        let first = self.month;
        let start = first - Duration::days(first.weekday().num_days_from_monday() as i64);
        for k in 0..42 {
            let d = start + Duration::days(k);
            let r = egui::Rect::from_min_size(egui::pos2(rect.left() + cell * (k % 7) as f32, rect.top() + 16.0 + cell * (k / 7) as f32), Vec2::splat(cell)).shrink(2.0);
            let id = ui.id().with(("jcal", d));
            let resp = ui.interact(r, id, Sense::click());
            let in_month = d.month() == first.month();
            let selected = self.anchor == Some(d);
            if selected {
                p.rect_filled(r, 6.0, t.widget.selection);
            } else if resp.hovered() {
                p.rect_filled(r, 6.0, t.widget.fill_hover);
            }
            if d == today {
                p.rect_stroke(r, 6.0, egui::Stroke::new(1.5, t.data.today), egui::StrokeKind::Inside);
            }
            let ink = if !in_month { t.text.muted.linear_multiply(0.5) } else if d > today { t.text.muted } else { t.text.primary };
            p.text(r.center() - Vec2::new(0.0, 3.0), egui::Align2::CENTER_CENTER, d.day().to_string(), egui::FontId::proportional(12.0), ink);
            if let Some(n) = counts.get(&d) {
                let c = journal_color();
                let dots = (*n).min(3);
                for j in 0..dots {
                    let x = r.center().x + (j as f32 - (dots as f32 - 1.0) / 2.0) * 5.0;
                    p.circle_filled(egui::pos2(x, r.bottom() - 4.0), 1.8, if in_month { c } else { c.linear_multiply(0.4) });
                }
            }
            if resp.clicked() && d <= today {
                self.anchor = if d == today { None } else { Some(d) };
                self.days_shown = DAYS_PER_PAGE;
            }
        }
    }

    fn composer(&mut self, ui: &mut Ui, data: &mut Data) {
        let t = theme::th();
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(journal::PROMPTS[self.prompt % journal::PROMPTS.len()]).italics().color(t.text.muted));
            let r = ui.add(
                egui::TextEdit::multiline(&mut self.draft)
                    .desired_rows(4)
                    .desired_width(ui.available_width())
                    .hint_text("Write about your day… Use #tags to group entries. Ctrl+Enter saves."),
            );
            if self.focus_draft {
                r.request_focus();
                // put the caret after what was typed before the box had focus
                if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), r.id) {
                    let end = egui::text::CCursor::new(self.draft.chars().count());
                    state.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
                    state.store(ui.ctx(), r.id);
                }
                self.focus_draft = false;
            }
            let ctrl_enter = r.has_focus() && ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Enter));
            ui.horizontal(|ui| {
                // first in the row, so it never moves when the prompt or mood changes
                if ui.small_button("Another prompt").clicked() {
                    self.prompt = (self.prompt + 1) % journal::PROMPTS.len();
                }
                ui.add_space(10.0);
                mood_picker(ui, &mut self.mood);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let can_save = !self.draft.trim().is_empty() && self.recording().is_none();
                    if ui.add_enabled(can_save, egui::Button::new(RichText::new("Save entry").strong())).on_hover_text("Ctrl+Enter").clicked() || (ctrl_enter && can_save) {
                        save_text(data, std::mem::take(&mut self.draft), self.mood.take());
                        self.anchor = None;
                    }
                    if cfg!(feature = "voice") {
                        match self.recording() {
                            Some((secs, false)) => {
                                let b = egui::Button::new(RichText::new(format!("■ Stop  {}", mmss(secs as f32))).color(t.text.on_color)).fill(t.status.danger);
                                if ui.add(b).on_hover_text("Stop, save the recording and transcribe it").clicked() {
                                    self.toggle_recording(data, false);
                                }
                                ui.label(RichText::new(format!("Recording… (max {} min) Anything typed above is saved with it.", MAX_REC_SECS / 60)).color(t.status.danger).size(12.5));
                            }
                            Some(_) => {
                                ui.add_enabled(false, egui::Button::new("🎤 Record")).on_disabled_hover_text("Already recording on the Habits page");
                            }
                            None => {
                                if ui.button("🎤 Record").on_hover_text("Record a voice entry. The audio is kept, and Whisper transcribes it on this PC.").clicked() {
                                    self.toggle_recording(data, false);
                                }
                            }
                        }
                    }
                });
            });
        });
    }

    fn entries(&mut self, ui: &mut Ui, data: &mut Data) {
        let t = theme::th();
        let today = Local::now().date_naive();
        let searching = !self.search.trim().is_empty() || self.only_pinned || self.only_voice;
        if let Some(a) = self.anchor {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Showing {} and earlier", a.format("%B %-d, %Y"))).color(t.text.muted));
                if ui.small_button("Back to today").clicked() {
                    self.anchor = None;
                    self.days_shown = DAYS_PER_PAGE;
                }
            });
            ui.add_space(6.0);
        }
        let until = self.anchor.unwrap_or(today);
        let query = self.search.trim().to_lowercase();
        let mut by_day: BTreeMap<NaiveDate, Vec<usize>> = BTreeMap::new();
        for (i, e) in data.journal.iter().enumerate() {
            if e.date > until && !searching {
                continue;
            }
            if self.only_pinned && !e.pinned {
                continue;
            }
            if self.only_voice && e.audio.is_none() {
                continue;
            }
            if !query.is_empty() && !e.matches(&query) {
                continue;
            }
            by_day.entry(e.date).or_default().push(i);
        }
        if by_day.is_empty() {
            let (title, body) = if searching {
                ("Nothing matches", "Try other words, or clear the filters.")
            } else {
                ("Your journal is empty", "Write a few lines above, or record a voice note. Every day you write adds a star to the Habits constellation.")
            };
            super::streaks::empty_state(ui, title, body);
            return;
        }
        if searching {
            let n: usize = by_day.values().map(|v| v.len()).sum();
            ui.label(RichText::new(format!("{n} matching entr{}", if n == 1 { "y" } else { "ies" })).color(t.text.muted));
        }
        let total_days = by_day.len();
        let shown = if searching { total_days } else { self.days_shown.min(total_days) };
        let mut action: Option<EntryAction> = None;
        for (day, idxs) in by_day.iter().rev().take(shown) {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(day_title(*day, today)).size(16.0).strong());
                let notes = idxs.iter().filter(|i| data.journal[**i].kind == "note").count();
                if notes > 1 {
                    ui.label(RichText::new(format!("{notes} entries")).weak().size(12.5));
                }
            });
            ui.add_space(2.0);
            for i in idxs.iter().rev() {
                if let Some(a) = self.entry_card(ui, data, *i) {
                    action = Some(a);
                }
                ui.add_space(6.0);
            }
        }
        if shown < total_days {
            ui.add_space(8.0);
            if ui.button(format!("Show older days ({} more)", total_days - shown)).clicked() {
                self.days_shown += DAYS_PER_PAGE;
            }
        }
        if let Some(a) = action {
            self.run(a, data);
        }
    }

    fn entry_card(&mut self, ui: &mut Ui, data: &Data, i: usize) -> Option<EntryAction> {
        let t = theme::th();
        let e = &data.journal[i];
        let reflection = e.kind == "reflection";
        let mut action = None;
        let frame = theme::card().stroke(egui::Stroke::new(1.0, if reflection { claude_color().linear_multiply(0.7) } else { t.surface.card_border }));
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(journal::time_label(e)).color(t.text.muted).monospace())
                    .on_hover_text(if e.time.is_some() { "When it was written" } else { "Added afterwards for this day" });
                if reflection {
                    ui.label(RichText::new("✨ Reflection by Claude").color(claude_color()).size(12.5));
                }
                if let Some(m) = e.mood {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 4.5, mood_color(m));
                    ui.label(RichText::new(journal::mood_label(m)).size(12.5).color(mood_color(m)));
                }
                if e.edited.is_some() {
                    ui.label(RichText::new("edited").weak().size(11.5));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.confirm_delete.as_deref() == Some(e.id.as_str()) {
                        if ui.small_button("No").clicked() {
                            self.confirm_delete = None;
                        }
                        if ui.small_button("Yes, delete").clicked() {
                            action = Some(EntryAction::Delete(e.id.clone()));
                        }
                        ui.label(RichText::new("Delete this entry?").color(t.status.warning).size(12.5));
                    } else {
                        if ui.small_button("🗑").on_hover_text("Delete").clicked() {
                            self.confirm_delete = Some(e.id.clone());
                        }
                        if self.editing.is_none() && ui.small_button("✏").on_hover_text("Edit").clicked() {
                            self.editing = Some((e.id.clone(), e.text.clone(), e.transcript.clone().unwrap_or_default()));
                        }
                        let star = if e.pinned { RichText::new("★").color(t.data.gold) } else { RichText::new("☆").color(t.text.muted) };
                        if ui.small_button(star).on_hover_text(if e.pinned { "Unpin" } else { "Pin (find it under Pinned)" }).clicked() {
                            action = Some(EntryAction::TogglePin(e.id.clone()));
                        }
                    }
                });
            });
            let editing_this = self.editing.as_ref().is_some_and(|(id, ..)| *id == e.id);
            if editing_this {
                let (_, text, transcript) = self.editing.as_mut().unwrap();
                ui.add(egui::TextEdit::multiline(text).desired_rows(3).desired_width(ui.available_width()).hint_text("Text"));
                if e.audio.is_some() {
                    ui.label(RichText::new("Transcript").size(12.0).color(t.text.muted));
                    ui.add(egui::TextEdit::multiline(transcript).desired_rows(2).desired_width(ui.available_width()));
                }
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("Save").strong()).clicked() {
                        let (id, text, tr) = self.editing.take().unwrap();
                        action = Some(EntryAction::Edit(id, text, tr));
                    }
                    if ui.button("Cancel").clicked() {
                        self.editing = None;
                    }
                });
            } else if !e.text.trim().is_empty() {
                ui.add(egui::Label::new(RichText::new(e.text.trim()).size(14.5)).wrap().selectable(true));
            }
            if let Some(file) = &e.audio {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("🎤 {}", e.audio_secs.map(mmss).unwrap_or_default())).color(journal_color()));
                    let path = data.paths.journal_audio.join(file);
                    if path.exists() {
                        if ui.small_button("▶ Play").on_hover_text("Opens the recording in your audio player").clicked() {
                            let _ = open::that(&path);
                        }
                    } else {
                        ui.label(RichText::new("recording file missing").color(t.status.warning).size(12.0));
                    }
                    let busy = self.transcribing.iter().any(|(id, _)| *id == e.id);
                    if busy {
                        ui.spinner();
                        ui.label(RichText::new("transcribing…").weak().size(12.5));
                    } else if e.transcript.is_none() && path.exists() && cfg!(feature = "voice") {
                        if ui.small_button("Transcribe").on_hover_text("Needs a voice model (Settings > Input & voice)").clicked() {
                            action = Some(EntryAction::Transcribe(e.id.clone()));
                        }
                    }
                });
                if let Some(tr) = e.transcript.as_ref().filter(|_| !editing_this) {
                    let shown = if tr.trim().is_empty() { "(no speech recognized)" } else { tr.trim() };
                    ui.add(egui::Label::new(RichText::new(shown).italics().size(14.0)).wrap().selectable(true));
                }
            }
            let tags = e.tags();
            if !tags.is_empty() && !editing_this {
                ui.horizontal_wrapped(|ui| {
                    for tag in tags {
                        if ui.add(egui::Button::new(RichText::new(format!("#{tag}")).size(12.0).color(journal_color())).frame(false)).on_hover_text("Search this tag").clicked() {
                            action = Some(EntryAction::Search(format!("#{tag}")));
                        }
                    }
                });
            }
        });
        action
    }

    fn run(&mut self, a: EntryAction, data: &mut Data) {
        match a {
            EntryAction::Delete(id) => {
                if data.delete_journal_entry(&id) {
                    data.status = "Journal entry deleted".into();
                }
                self.confirm_delete = None;
            }
            EntryAction::Edit(id, text, tr) => {
                if let Some(e) = data.journal.iter_mut().find(|e| e.id == id) {
                    e.text = text.trim().to_string();
                    if e.audio.is_some() {
                        e.transcript = Some(tr.trim().to_string());
                    }
                    e.edited = Some(Local::now());
                    data.save_journal();
                }
            }
            EntryAction::TogglePin(id) => {
                if let Some(e) = data.journal.iter_mut().find(|e| e.id == id) {
                    e.pinned = !e.pinned;
                    data.save_journal();
                }
            }
            EntryAction::Search(q) => {
                self.search = q;
            }
            EntryAction::Transcribe(id) => {
                #[cfg(feature = "voice")]
                if let Some(e) = data.journal.iter().find(|e| e.id == id) {
                    if let Some(file) = &e.audio {
                        match journal::read_wav(&data.paths.journal_audio.join(file)) {
                            Ok(samples) => {
                                self.transcribe(data, id.clone(), samples);
                                if !self.transcribing.is_empty() {
                                    data.status = "Transcribing…".into();
                                }
                            }
                            Err(err) => data.status = format!("Couldn't read the recording: {err}"),
                        }
                    }
                }
                #[cfg(not(feature = "voice"))]
                let _ = id;
            }
        }
    }
}

enum EntryAction {
    Delete(String),
    Edit(String, String, String),
    TogglePin(String),
    Search(String),
    Transcribe(String),
}

/// Saves a typed entry for now.
pub fn save_text(data: &mut Data, text: String, mood: Option<u8>) {
    let text = text.trim().to_string();
    if text.is_empty() {
        return;
    }
    let mut e = JournalEntry::new(text);
    e.mood = mood;
    data.journal.push(e);
    data.save_journal();
    data.status = "Journal entry saved".into();
}
