//! "From your phone": what the phone sent while the PC was off, run here in order
//! (`crate::phone_queue`). A 📱 button in the top bar shows how many are waiting; the panel lists
//! the steps with what became of each, and Skip / Retry / Undo.

use super::theme;
use crate::phone_queue::{self, Status};
use crate::store::Data;
use chrono::Local;
use eframe::egui::{self, RichText, Ui};

/// The top bar button: "📱 3" while steps wait or run, "📱" for a while after; nothing otherwise.
pub fn button(ui: &mut Ui, data: &Data, open: &mut bool) {
    if !data.settings.phone_enabled && !*open {
        return;
    }
    let steps = phone_queue::all(&data.paths);
    let busy = steps.iter().filter(|s| !s.status.finished()).count();
    let recent = steps.iter().any(|s| s.done_at.is_some_and(|t| Local::now() - t < chrono::Duration::hours(12)));
    if busy == 0 && !recent && !*open {
        return;
    }
    let label = if busy > 0 { format!("📱 {busy}") } else { "📱".into() };
    if ui
        .add(egui::Button::new(label).selected(*open))
        .on_hover_text("From your phone: what you did on the phone while the PC was off, run here in order")
        .clicked()
    {
        *open = !*open;
    }
}

/// The side panel.
pub fn show(ui: &mut Ui, data: &mut Data, open: &mut bool, confirm_undo: &mut Option<String>) {
    let t = theme::th();
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("From your phone").font(theme::bold(18.0)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("✖").on_hover_text("Close").clicked() {
                *open = false;
            }
        });
    });
    ui.label(
        RichText::new("What you did on the phone while the PC was off, run here one by one in the order you did it. Claude's answers are applied straight away; Undo puts back what a step changed.")
            .weak()
            .size(12.5),
    );
    ui.add_space(6.0);
    let steps = phone_queue::all(&data.paths);
    if steps.is_empty() {
        ui.label(RichText::new("Nothing from the phone yet.").weak());
        return;
    }
    let devices: Vec<(String, String)> = data.settings.phone_devices.iter().map(|d| (d.id.clone(), d.name.clone())).collect();
    let mut action: Option<(&'static str, String)> = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        // newest first
        for s in steps.iter().rev() {
            theme::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (icon, color) = match s.status {
                        Status::Waiting => ("⏳", t.text.muted),
                        Status::Running => ("⟳", t.data.claude),
                        Status::Applied => ("✔", t.status.success),
                        Status::Failed => ("✖", t.status.danger),
                        Status::Skipped => ("⤼", t.text.muted),
                    };
                    if s.status == Status::Running {
                        ui.spinner();
                    } else {
                        ui.label(RichText::new(icon).color(color).strong());
                    }
                    ui.add(egui::Label::new(RichText::new(s.step.describe()).strong()).wrap());
                });
                let phone = devices.iter().find(|(id, _)| *id == s.device).map_or("phone", |(_, n)| n.as_str());
                let when = s.at.with_timezone(&Local).format("%a %b %-d, %H:%M");
                let for_day = if s.date != s.at.with_timezone(&Local).date_naive() { format!(" · for {}", s.date.format("%a %b %-d")) } else { String::new() };
                ui.label(RichText::new(format!("{phone} · {when}{for_day}")).weak().size(12.0));
                if !s.message.is_empty() {
                    let c = if s.status == Status::Failed { t.status.danger } else { t.text.primary };
                    ui.add(egui::Label::new(RichText::new(&s.message).color(c).size(12.5)).wrap());
                }
                ui.horizontal_wrapped(|ui| match s.status {
                    Status::Waiting => {
                        if ui.small_button("Skip").on_hover_text("Don't run this step").clicked() {
                            action = Some(("skip", s.id.clone()));
                        }
                    }
                    Status::Failed | Status::Skipped => {
                        if ui.small_button("Retry").on_hover_text("Run this step again").clicked() {
                            action = Some(("retry", s.id.clone()));
                        }
                    }
                    Status::Applied if !s.undo.is_empty() => {
                        if confirm_undo.as_deref() == Some(s.id.as_str()) {
                            ui.label(RichText::new("Put back the parts this changed? Later changes to them are undone too.").color(t.status.warning).size(12.0));
                            if ui.small_button("Yes, undo").clicked() {
                                action = Some(("undo", s.id.clone()));
                            }
                            if ui.small_button("No").clicked() {
                                *confirm_undo = None;
                            }
                        } else if ui.small_button("Undo").clicked() {
                            *confirm_undo = Some(s.id.clone());
                        }
                    }
                    _ => {}
                });
            });
            ui.add_space(4.0);
        }
    });
    match action {
        Some(("skip", id)) => phone_queue::set_waiting_or_skipped(&data.paths, &id, true),
        Some(("retry", id)) => phone_queue::set_waiting_or_skipped(&data.paths, &id, false),
        Some(("undo", id)) => {
            *confirm_undo = None;
            data.status = match phone_queue::undo(data, &id) {
                Ok(m) => format!("📱 {m}"),
                Err(e) => format!("📱 Couldn't undo: {e}"),
            };
        }
        _ => {}
    }
}
