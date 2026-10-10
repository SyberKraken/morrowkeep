//! Body tab: profile, the numbers derived from it, the goal weight plan, the daily calorie
//! limit it produces (or the fallback limit) and the burn measured from your own logs.

use crate::body::{self, Activity, Goal, Mode, Sex};
use crate::store::Data;
use chrono::{Local, NaiveDate};
use eframe::egui::{self, RichText, Ui};

/// Text fields that need a draft while typing (dates and the target weight).
#[derive(Default)]
pub struct BodyUi {
    birth: String,
    target_kg: String,
    target_date: String,
    /// Drafts were filled from the data.
    loaded: bool,
}

impl BodyUi {
    fn load(&mut self, data: &Data) {
        self.birth = data.body.birth_date.map(|d| d.to_string()).unwrap_or_default();
        match &data.body.goal {
            Some(g) => {
                self.target_kg = format!("{:.1}", g.target_kg);
                self.target_date = g.target_date.to_string();
            }
            None => {
                self.target_kg.clear();
                self.target_date.clear();
            }
        }
        self.loaded = true;
    }

    /// Reload the drafts next frame (after the goal changed elsewhere, e.g. the entry bar).
    pub fn reset(&mut self) {
        self.loaded = false;
    }
}

/// Sets (or updates) the goal. An active goal keeps its start so the ideal path stays put;
/// a reached goal is archived and a new one starts from today's weight.
pub fn set_goal(data: &mut Data, target_kg: f32, target_date: NaiveDate) -> Result<(), String> {
    let today = Local::now().date_naive();
    let now = data.current_weight().ok_or("Log your weight first, so the goal has a starting point")?;
    if !(30.0..=300.0).contains(&target_kg) {
        return Err(format!("{target_kg} kg doesn't look like a goal weight"));
    }
    match &mut data.body.goal {
        Some(g) if g.reached_on.is_none() => {
            g.target_kg = target_kg;
            g.target_date = target_date;
        }
        slot => {
            if let Some(old) = slot.take() {
                data.body.past_goals.push(old);
            }
            *slot = Some(Goal { target_kg, target_date, set_on: today, start_kg: now, reached_on: None });
        }
    }
    data.save_body();
    data.status = format!("Goal set: {target_kg:.1} kg by {}", target_date.format("%b %-d, %Y"));
    Ok(())
}

fn clear_goal(data: &mut Data) {
    if let Some(g) = data.body.goal.take() {
        data.body.past_goals.push(g);
        data.save_body();
        data.status = "Goal cleared".into();
    }
}

fn row(ui: &mut Ui, label: &str, value: String, hint: &str) {
    ui.label(RichText::new(label).color(super::theme::th().text.muted));
    let r = ui.label(RichText::new(value).strong());
    if !hint.is_empty() {
        r.on_hover_text(hint);
    }
    ui.end_row();
}

fn kcal(k: f32) -> String {
    format!("{} kcal", ((k / 10.0).round() * 10.0) as i64)
}

pub fn body(ui: &mut Ui, data: &mut Data, st: &mut BodyUi) {
    if !st.loaded {
        st.load(data);
    }
    let today = Local::now().date_naive();
    let t = super::theme::th();
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.heading("Body");
        ui.add_space(12.0);
        ui.label(RichText::new("Estimates from the standard formulas. A guide, not medical advice.").weak().size(12.5));
    });
    ui.add_space(6.0);
    if let Some(e) = &data.body_error {
        ui.label(RichText::new(format!("body.json could not be read: {e}. Fix it and restart; nothing here is saved until then.")).color(t.status.danger));
    }
    let weight = data.current_weight();
    let plan = data.body_plan(today);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.columns(2, |cols| {
            profile_card(&mut cols[0], data, st, weight, today);
            numbers_card(&mut cols[1], data, weight, today);
        });
        ui.add_space(10.0);
        goal_card(ui, data, st, today);
        ui.add_space(10.0);
        limit_card(ui, data, plan.as_ref().map(|(_, p)| p.daily));
        ui.add_space(10.0);
        measured_card(ui, data, plan.as_ref().map(|(n, _)| n.tdee), today);
        ui.add_space(20.0);
    });
}

fn profile_card(ui: &mut Ui, data: &mut Data, st: &mut BodyUi, weight: Option<f32>, today: NaiveDate) {
    let t = super::theme::th();
    super::theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Profile").size(17.0).strong());
        ui.add_space(4.0);
        let mut changed = false;
        egui::Grid::new("profile").num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
            ui.label("Sex");
            let text = match data.body.sex {
                Some(Sex::Male) => "Male",
                Some(Sex::Female) => "Female",
                None => "Not set",
            };
            egui::ComboBox::from_id_salt("sex").selected_text(text).show_ui(ui, |ui| {
                changed |= ui.selectable_value(&mut data.body.sex, Some(Sex::Male), "Male").changed();
                changed |= ui.selectable_value(&mut data.body.sex, Some(Sex::Female), "Female").changed();
            })
            .response
            .on_hover_text("Used by the resting-burn formula");
            ui.end_row();

            ui.label("Born");
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut st.birth).desired_width(110.0).hint_text("YYYY-MM-DD"));
                let parsed = NaiveDate::parse_from_str(st.birth.trim(), "%Y-%m-%d").ok().filter(|d| *d < today);
                if r.changed() {
                    if let Some(d) = parsed {
                        data.body.birth_date = Some(d);
                        changed = true;
                    } else if st.birth.trim().is_empty() && data.body.birth_date.is_some() {
                        data.body.birth_date = None;
                        changed = true;
                    }
                }
                match data.body.age_on(today) {
                    Some(a) if parsed.is_some() => ui.label(RichText::new(format!("{a} years")).weak()),
                    _ if !st.birth.trim().is_empty() => ui.label(RichText::new("use YYYY-MM-DD").color(t.status.warning)),
                    _ => ui.label(""),
                };
            });
            ui.end_row();

            ui.label("Height");
            let mut h = data.body.height_cm.unwrap_or(0.0);
            if ui.add(egui::DragValue::new(&mut h).range(0.0..=250.0).speed(0.5).custom_formatter(|v, _| if v == 0.0 { "not set".into() } else { format!("{v:.0} cm") }).custom_parser(|t| t.trim().trim_end_matches("cm").trim().replace(',', ".").parse().ok())).changed() {
                data.body.height_cm = (h >= 100.0).then_some(h);
                changed = true;
            }
            ui.end_row();

            ui.label("Activity");
            ui.horizontal(|ui| {
                let a = data.body.activity;
                egui::ComboBox::from_id_salt("activity").selected_text(a.label()).width(150.0).show_ui(ui, |ui| {
                    for act in Activity::ALL {
                        changed |= ui
                            .selectable_value(&mut data.body.activity, act, act.label())
                            .on_hover_text(format!("{} · ×{}", act.hint(), act.factor()))
                            .changed();
                    }
                });
                ui.label(RichText::new(data.body.activity.hint()).weak().size(12.5));
            });
            ui.end_row();

            ui.label("Body fat");
            ui.horizontal(|ui| {
                let mut known = data.body.body_fat_pct.is_some();
                if ui.checkbox(&mut known, "known").on_hover_text("If you know it (smart scale, DEXA…), resting burn uses your lean mass instead").changed() {
                    data.body.body_fat_pct = known.then_some(20.0);
                    changed = true;
                }
                if let Some(bf) = &mut data.body.body_fat_pct {
                    changed |= ui.add(egui::DragValue::new(bf).range(3.0..=70.0).speed(0.2).suffix(" %")).changed();
                } else {
                    ui.label(RichText::new("optional").weak().size(12.5));
                }
            });
            ui.end_row();

            ui.label("Weight");
            match weight {
                Some(w) => {
                    ui.label(RichText::new(format!("{w:.1} kg")).strong().color(super::weight_color()))
                        .on_hover_text("Average of your weigh-ins over the last 7 days before the latest one; smooths out day-to-day water swings");
                }
                None => {
                    ui.label(RichText::new("log a weight first").color(super::not_logged_color()));
                }
            }
            ui.end_row();
        });
        if changed {
            data.save_body();
        }
    });
}

fn numbers_card(ui: &mut Ui, data: &Data, weight: Option<f32>, today: NaiveDate) {
    super::theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Your numbers").size(17.0).strong());
        ui.add_space(4.0);
        let Some(n) = weight.and_then(|w| data.body.numbers(w, today)) else {
            let missing = data.body.missing(weight);
            ui.label(RichText::new(format!("Add {} to see your numbers.", missing.join(", "))).color(super::not_logged_color()));
            ui.add_space(4.0);
            ui.label(RichText::new("Until then the fallback daily limit below is used.").weak().size(12.5));
            return;
        };
        egui::Grid::new("numbers").num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
            row(ui, "Resting burn (BMR)", kcal(n.bmr), &format!("{}: what your body burns at complete rest", n.formula));
            row(
                ui,
                "Daily burn (TDEE)",
                kcal(n.tdee),
                &format!("Resting burn × {} ({}): roughly what you burn on a normal day, i.e. maintenance", data.body.activity.factor(), data.body.activity.label()),
            );
            row(ui, "BMI", format!("{:.1}", n.bmi), "Weight ÷ height². 18.5–25 is the \"healthy\" range; it can't tell muscle from fat.");
            row(ui, "Healthy range for you", format!("{:.0}–{:.0} kg", n.healthy_kg.0, n.healthy_kg.1), "Weights for BMI 18.5–25 at your height");
        });
        ui.add_space(2.0);
        ui.label(
            RichText::new(format!("{} · BMR × {} for {}", n.formula, data.body.activity.factor(), data.body.activity.label().to_lowercase()))
                .weak()
                .size(12.0),
        );
    });
}

fn goal_card(ui: &mut Ui, data: &mut Data, st: &mut BodyUi, today: NaiveDate) {
    let t = super::theme::th();
    super::theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new("Goal").size(17.0).strong().color(t.data.goal));
            ui.label(RichText::new("lose weight or hold one; a target above your weight means maintenance for that weight").weak().size(12.5));
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Target");
            ui.add(egui::TextEdit::singleline(&mut st.target_kg).desired_width(56.0).hint_text("kg"));
            ui.label("kg   by");
            ui.add(egui::TextEdit::singleline(&mut st.target_date).desired_width(110.0).hint_text("YYYY-MM-DD"));
            let kg: Option<f32> = st.target_kg.trim().replace(',', ".").parse().ok();
            let date = crate::parse::parse_date(&st.target_date, today).filter(|d| *d > today);
            let ok = kg.is_some() && date.is_some();
            let label = if data.body.goal.as_ref().is_some_and(|g| g.reached_on.is_none()) { "Update goal" } else { "Set goal" };
            if ui.add_enabled(ok, egui::Button::new(RichText::new(label).strong())).clicked() {
                if let Err(e) = set_goal(data, kg.unwrap(), date.unwrap()) {
                    data.status = e;
                }
                st.reset();
            }
            if data.body.goal.is_some() && ui.button("Clear").on_hover_text("Stop this goal (it's kept in the history)").clicked() {
                clear_goal(data);
                st.reset();
            }
            if !st.target_date.trim().is_empty() && date.is_none() {
                ui.label(RichText::new("date: e.g. 2027-02-01, \"feb 1\" or \"in 12 weeks\"").color(t.status.warning).size(12.5));
            } else if let Some(d) = date {
                let weeks = (d - today).num_days() as f32 / 7.0;
                ui.label(RichText::new(format!("{weeks:.0} weeks")).weak());
            }
        });
        ui.add_space(6.0);
        // Pace picker for a target below the current weight.
        let kg_draft: Option<f32> = st.target_kg.trim().replace(',', ".").parse().ok();
        if let (Some(kg), Some((n, _))) = (kg_draft, data.body_plan(today)) {
            if let Some(o) = data.body.date_options(&n, kg, today) {
                let saved = data.body.goal.as_ref().filter(|g| g.reached_on.is_none()).map(|g| (g.target_kg, g.target_date));
                pace_picker(ui, st, &o, &n, today, saved);
                ui.add_space(8.0);
            }
        }

        let Some((n, plan)) = data.body_plan(today) else {
            ui.label(RichText::new("The plan appears once your profile is complete.").color(super::not_logged_color()));
            return;
        };
        if data.body.goal.is_some() {
            ui.label(RichText::new("CURRENT PLAN").size(11.5).color(t.text.muted));
        }
        match (&data.body.goal, plan.mode) {
            (None, _) => {
                ui.label(format!("No goal set, so your limit is maintenance: {}/day.", kcal(plan.maintenance)));
            }
            (Some(g), Mode::Maintain) => {
                let msg = if n.weight_kg <= g.target_kg && g.start_kg > g.target_kg {
                    format!("Goal reached 🎉  Holding {:.1} kg: maintenance {}/day.", g.target_kg, kcal(plan.maintenance))
                } else {
                    format!("You're at or under {:.1} kg, so the plan is maintenance for that weight: {}/day.", g.target_kg, kcal(plan.maintenance))
                };
                ui.label(RichText::new(msg).strong().color(t.data.goal));
            }
            (Some(_), Mode::Lose) => {
                let weeks = plan.days_left as f32 / 7.0;
                ui.label(format!(
                    "−{:.1} kg in {:.0} weeks: −{:.2} kg a week, a {}/day deficit",
                    plan.kg_left, weeks, plan.kg_per_week, kcal(plan.deficit)
                ));
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Eat ≈ {} kcal/day", plan.daily)).size(20.0).strong().color(super::calorie_color()));
                    ui.label(RichText::new(format!("maintenance {} − deficit {}", kcal(plan.maintenance), kcal(plan.deficit))).weak());
                });
                let warn = |ui: &mut Ui, text: String| ui.label(RichText::new(format!("⚠ {text}")).color(t.status.warning));
                if plan.date_passed {
                    warn(ui, format!("The target date has passed; continuing at the safe pace (−{:.2} kg/week).", plan.kg_per_week));
                } else if plan.too_fast {
                    ui.horizontal(|ui| {
                        warn(ui, format!("Faster than recommended (at most a {}/day deficit for you).", kcal(plan.max_deficit)));
                        if let Some(sd) = plan.safe_date {
                            if ui.button(format!("Use {} instead", sd.format("%b %-d, %Y"))).on_hover_text("The earliest date at the safe pace").clicked() {
                                if let Some(g) = data.body.goal.as_ref() {
                                    let _ = set_goal(data, g.target_kg, sd);
                                    st.reset();
                                }
                            }
                        }
                    });
                }
                if plan.floored {
                    warn(ui, format!("The deficit would put you under {} kcal/day, so the limit stays at {}. You'll get there later than planned.", plan.floor, plan.floor));
                }
            }
        }
        if plan.target_underweight {
            ui.label(RichText::new("⚠ That target is under BMI 18.5 for your height.").color(t.status.warning));
        }
        if let Some(g) = data.body.goal.clone() {
            if let Some(p) = body::progress(&g, &data.weights, today) {
                if g.start_kg > g.target_kg {
                    ui.add_space(4.0);
                    let mut text = format!("{:.1} of {:.1} kg · {}", p.lost.max(0.0), g.start_kg - g.target_kg, p.status());
                    if p.to_go > 0.0 && p.status() != "on track" {
                        text += &format!(" ({:+.1} kg vs the plan)", p.vs_path);
                    }
                    if let Some(eta) = p.eta {
                        text += &format!(" · at this pace: {}", eta.format("%b %-d"));
                    }
                    ui.add(egui::ProgressBar::new(p.fraction).desired_width(ui.available_width().min(560.0)).desired_height(8.0).fill(t.data.goal.linear_multiply(0.85)));
                    ui.label(RichText::new(text).strong().color(t.data.goal));
                    ui.label(RichText::new(format!("Started at {:.1} kg on {}", g.start_kg, g.set_on.format("%b %-d, %Y"))).weak().size(12.0));
                }
            }
        }
        if !data.body.past_goals.is_empty() {
            ui.add_space(4.0);
            egui::CollapsingHeader::new(RichText::new(format!("Earlier goals ({})", data.body.past_goals.len())).weak()).id_salt("past_goals").show(ui, |ui| {
                for g in data.body.past_goals.iter().rev() {
                    let reached = g.reached_on.map_or("not reached".to_string(), |d| format!("reached {}", d.format("%b %-d, %Y")));
                    ui.label(RichText::new(format!("{:.1} to {:.1} kg, set {} · {reached}", g.start_kg, g.target_kg, g.set_on.format("%b %-d, %Y"))).size(12.5));
                }
            });
        }
    });
}

/// ASAP and Easy dates with a slider between them; moving it fills in the date field.
fn pace_picker(ui: &mut Ui, st: &mut BodyUi, o: &body::DateOptions, n: &body::Numbers, today: NaiveDate, saved: Option<(f32, NaiveDate)>) {
    let t = super::theme::th();
    let (lo, hi) = ((o.asap - today).num_days(), (o.easy - today).num_days());
    let draft = crate::parse::parse_date(&st.target_date, today).filter(|d| *d > today);
    if draft.is_none() && st.target_date.trim().is_empty() {
        // start in the middle
        st.target_date = (today + chrono::Duration::days((lo + hi) / 2)).to_string();
    }
    let mut days = draft.map_or((lo + hi) / 2, |d| (d - today).num_days()).clamp(lo, hi);
    let set = |st: &mut BodyUi, d: i64| st.target_date = (today + chrono::Duration::days(d)).to_string();
    ui.label(RichText::new("Pick a pace").strong());
    ui.horizontal(|ui| {
        let end = |ui: &mut Ui, title: &str, date: NaiveDate, deficit: f32, tip: &str| {
            ui.add(egui::Button::new(RichText::new(format!("{title}\n{}", date.format("%b %-d, %Y"))).size(13.0)))
            .on_hover_text(format!("{tip}\n{}/day deficit · −{:.2} kg a week", kcal(deficit), deficit * 7.0 / body::KCAL_PER_KG))
            .clicked()
        };
        if end(ui, "ASAP", o.asap, o.asap_deficit, "The largest recommended deficit for you") {
            set(st, lo);
        }
        ui.spacing_mut().slider_width = (ui.available_width() - 130.0).clamp(160.0, 520.0);
        if hi > lo && ui.add(egui::Slider::new(&mut days, lo..=hi).show_value(false)).changed() {
            set(st, days);
        }
        if end(ui, "Easy", o.easy, o.easy_deficit, "A small deficit most people hardly notice") {
            set(st, hi);
        }
    });
    let draft_kg: Option<f32> = st.target_kg.trim().replace(',', ".").parse().ok();
    let draft_date = crate::parse::parse_date(&st.target_date, today);
    let pending = match (saved, draft_kg, draft_date) {
        (Some((k, d)), Some(dk), Some(dd)) => (k - dk).abs() > 0.05 || d != dd,
        (None, Some(_), Some(_)) => true,
        _ => false,
    };
    // what the chosen date means (the plan below already shows a saved goal)
    if let Some(d) = draft_date.filter(|d| pending && *d > today) {
        let deficit = o.deficit_for(d, today);
        let eat = (n.tdee - deficit).max(0.0);
        let mut text = format!(
            "{} ({:.0} weeks): −{:.2} kg a week, a {}/day deficit, eat ≈ {}/day",
            d.format("%b %-d, %Y"),
            (d - today).num_days() as f32 / 7.0,
            deficit * 7.0 / body::KCAL_PER_KG,
            kcal(deficit),
            kcal(eat)
        );
        let color = if deficit > o.asap_deficit + 1.0 {
            text += " · faster than recommended";
            t.status.warning
        } else {
            t.text.muted
        };
        ui.label(RichText::new(text).color(color).size(13.0));
    }
    if pending {
        ui.label(RichText::new("Not saved yet: press the goal button above to use it.").color(t.status.warning).size(12.0));
    }
}

fn limit_card(ui: &mut Ui, data: &mut Data, from_plan: Option<u32>) {
    super::theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Daily calorie limit").size(17.0).strong());
        ui.add_space(2.0);
        match from_plan {
            Some(k) => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{k} kcal")).font(super::theme::bold(26.0)).color(super::calorie_color()));
                    ui.label(RichText::new("from your Body plan, updated as your weight changes").weak());
                });
            }
            None => {
                let k = data.body.fallback_daily_limit;
                ui.horizontal(|ui| {
                    let text = if k > 0 { format!("{k} kcal") } else { "none".into() };
                    ui.label(RichText::new(text).font(super::theme::bold(26.0)).color(super::calorie_color()));
                    ui.label(RichText::new("the fallback below, until the profile is complete").weak());
                });
            }
        }
        ui.horizontal(|ui| {
            ui.label("Fallback daily limit");
            if ui.add(egui::DragValue::new(&mut data.body.fallback_daily_limit).range(0..=10000).speed(10).suffix(" kcal")).changed() {
                data.save_body();
            }
            ui.label(RichText::new("used while the profile is incomplete · 0 = no limit").weak().size(12.5));
        });
        ui.label(
            RichText::new(
                "The limit sets the monthly budget (limit × days in the month). Habits shows a recommended amount: what's left of the \
                 month's budget spread over the remaining days, so going over one day lowers the next. Days with nothing logged count as the limit.",
            )
            .weak()
            .size(12.0),
        );
    });
}

fn measured_card(ui: &mut Ui, data: &Data, formula_tdee: Option<f32>, today: NaiveDate) {
    super::theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Measured burn").size(17.0).strong());
        ui.add_space(2.0);
        let daily = data.daily_kcal();
        match body::measured_burn(&daily, &data.weights, today) {
            Some((burn, days)) => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("≈ {}/day", kcal(burn))).font(super::theme::bold(22.0)).color(super::weight_color()));
                    if let Some(f) = formula_tdee {
                        ui.label(RichText::new(format!("formula: {} ({:+.0})", kcal(f), burn - f)).weak());
                    }
                });
                ui.label(
                    RichText::new(format!(
                        "From {days} days with food logged and your weight trend over the last 4 weeks. You count calories on the high side, so this reads a little low."
                    ))
                    .weak()
                    .size(12.5),
                );
            }
            None => {
                let from = today - chrono::Duration::days(28);
                let logged = daily.range(from..today).filter(|(_, k)| **k > 0).count();
                ui.label(
                    RichText::new(format!(
                        "Compares what you ate with how your weight actually changed. Needs 14+ days with food logged and weigh-ins spanning 2+ weeks in the last 4 weeks (now: {logged} days logged)."
                    ))
                    .color(super::not_logged_color()),
                );
            }
        }
    });
}
