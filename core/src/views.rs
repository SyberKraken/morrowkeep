//! What the phone's screens show, as JSON, worked out from a `Snapshot` of your data. The PC's
//! phone server uses these on its data, and the phone uses the very same code (WebAssembly) on
//! its own copy with the waiting steps applied, so the screens look the same offline.
//!
//! A few things only the PC knows come along in `Pc`: the theme, category colors and icons,
//! whether Claude Code and notifications are set up, and which recordings are being transcribed.

use crate::journal::JournalEntry;
use crate::model::Item;
use crate::steps::Snapshot;
use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

/// A category as the phone draws it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct KindStyle {
    pub name: String,
    pub icon: String,
    pub color: String,
}

/// What only the PC knows (sent with each snapshot).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Pc {
    pub theme: Value,
    pub categories: Vec<KindStyle>,
    /// For kinds that aren't one of your categories.
    pub other: KindStyle,
    pub claude: bool,
    pub notifications: bool,
    /// Journal entries whose recording is being transcribed right now.
    pub transcribing: Vec<String>,
}

impl Pc {
    fn kind(&self, kind: &str) -> (&str, &str) {
        let k = self.categories.iter().find(|c| c.name == kind).unwrap_or(&self.other);
        (&k.icon, &k.color)
    }
}

/// An f32 rounded to `places` decimals as f64, so JSON shows 81.8 and not 81.80000305175781.
pub fn dec(x: f32, places: i32) -> f64 {
    let k = 10f64.powi(places);
    (x as f64 * k).round() / k
}

fn hm(t: Option<chrono::NaiveTime>) -> Option<String> {
    t.map(|t| t.format("%H:%M").to_string())
}

impl Snapshot {
    pub fn calories_on(&self, day: NaiveDate) -> u32 {
        self.calories.iter().filter(|c| c.date == day).map(|c| c.kcal).sum()
    }
    pub fn weight_on(&self, day: NaiveDate) -> Option<f32> {
        self.weights.iter().find(|w| w.date == day).map(|w| w.kg)
    }
    /// Most recent weigh-in strictly before `day`.
    pub fn weight_before(&self, day: NaiveDate) -> Option<&crate::model::WeightEntry> {
        self.weights.iter().rev().find(|w| w.date < day)
    }
    pub fn journal_on(&self, day: NaiveDate) -> usize {
        self.journal.iter().filter(|e| e.date == day && e.kind == "note").count()
    }
    pub fn current_weight(&self) -> Option<f32> {
        crate::body::current_weight(&self.weights)
    }
    pub fn daily_limit(&self, today: NaiveDate) -> u32 {
        crate::budget::daily_limit(&self.body, &self.weights, today).0
    }
    pub fn recommended_for(&self, day: NaiveDate, today: NaiveDate) -> Option<u32> {
        crate::budget::recommended_for(&self.calories, self.daily_limit(today), day)
    }
}

/// A task for the phone, as it is on `day`.
pub fn item_json(it: &Item, day: NaiveDate, pc: &Pc) -> Value {
    let (icon, color) = pc.kind(&it.kind);
    json!({"id": it.id, "title": it.title, "date": it.date.map(|x| x.to_string()), "day": day.to_string(), "time": hm(it.time),
           "duration": it.duration_min, "done": it.is_done_on(day), "kind": it.kind, "color": color,
           "icon": icon, "repeat": it.repeat, "notes": it.notes})
}

/// A journal entry for the phone.
pub fn entry_json(e: &JournalEntry, pc: &Pc) -> Value {
    json!({"id": e.id, "date": e.date.to_string(), "time": hm(e.time), "text": e.text,
           "transcript": e.transcript, "mood": e.mood, "mood_label": e.mood.map(crate::journal::mood_label), "pinned": e.pinned,
           "audio_secs": e.audio_secs, "transcribing": pc.transcribing.contains(&e.id), "kind": e.kind, "words": e.words()})
}

/// The Today screen (and the header, theme and categories every screen uses), for `day`.
pub fn state(d: &Snapshot, pc: &Pc, day: NaiveDate, today: NaiveDate) -> Value {
    // tasks: scheduled first, then anytime (same order as the Habits board)
    let mut occ: Vec<usize> = crate::model::occurrences(&d.items, day, day).into_iter().map(|(_, i)| i).collect();
    occ.sort_by_key(|i| (d.items[*i].time.is_none(), d.items[*i].time));
    let tasks: Vec<Value> = occ
        .iter()
        .map(|i| {
            let it = &d.items[*i];
            let (icon, color) = pc.kind(&it.kind);
            json!({"id": it.id, "title": it.title, "time": hm(it.time), "done": it.is_done_on(day), "color": color, "icon": icon, "repeat": it.repeat.is_some()})
        })
        .collect();
    let entries: Vec<Value> =
        d.calories.iter().filter(|c| c.date == day).map(|c| json!({"id": c.id, "time": hm(c.time), "kcal": c.kcal, "description": c.description, "food": c.food})).collect();
    let last = d.weight_before(day).map(|w| json!({"kg": dec(w.kg, 1), "date": w.date.format("%b %-d").to_string()}));
    let goal = d.body.goal.as_ref().and_then(|g| {
        let p = crate::body::progress(g, &d.weights, today)?;
        (g.start_kg > g.target_kg).then(|| {
            if p.to_go <= 0.0 {
                format!("Goal {:.1} kg reached", g.target_kg)
            } else {
                format!("{:.1} kg to go to {:.1} kg · {}", p.to_go, g.target_kg, p.status())
            }
        })
    });
    let mut recent: Vec<&JournalEntry> = d.journal.iter().filter(|e| e.date >= today - Duration::days(2)).collect();
    recent.sort_by_key(|e| std::cmp::Reverse(crate::journal::sort_key(e)));
    let favorite = |id: &str| d.foods.food(id).map(|f| f.favorite).or_else(|| d.foods.meals.iter().find(|m| m.id == id).map(|m| m.favorite)).unwrap_or(false);
    let foods: Vec<Value> = d
        .foods
        .all()
        .iter()
        .map(|p| {
            json!({"id": p.id, "name": p.name, "meal": p.is_meal, "kcal_serving": p.kcal(crate::foods::Amount::Servings(1.0)),
                   "serving_name": p.serving_name, "serving_g": dec(p.serving_g, 1), "kcal_100g": dec(p.per100.kcal, 1), "favorite": favorite(&p.id),
                   "waiting": p.name == crate::api::LABEL_WAITING})
        })
        .collect();
    let mut recent_foods: Vec<String> = Vec::new();
    let mut logged: Vec<&crate::model::CalorieEntry> = d.calories.iter().filter(|c| c.food.is_some()).collect();
    logged.sort_by_key(|c| std::cmp::Reverse((c.date, c.time)));
    for c in logged {
        if let Some(p) = c.food.as_deref().and_then(|n| d.foods.find(n)) {
            if !recent_foods.contains(&p.id) {
                recent_foods.push(p.id);
            }
        }
        if recent_foods.len() >= 6 {
            break;
        }
    }
    let prompts = crate::journal::PROMPTS;
    json!({
        "date": today.to_string(),
        "date_label": today.format("%a %-d %b").to_string(),
        "theme": pc.theme,
        "day": day.to_string(),
        "day_label": day.format("%A %-d %b").to_string(),
        "calories": {"total": d.calories_on(day), "limit": d.daily_limit(today), "recommended": d.recommended_for(day, today), "entries": entries},
        "weight": {"today": d.weight_on(day).map(|k| dec(k, 1)), "last": last, "goal": goal},
        "journal": {"today": d.journal_on(day), "streak": crate::journal::streak(&d.journal, today),
                    "prompt": prompts[today.format("%j").to_string().parse::<usize>().unwrap_or(0) % prompts.len()],
                    "recent": recent.iter().map(|e| entry_json(e, pc)).collect::<Vec<_>>()},
        "tasks": tasks,
        "foods": foods,
        "recent_foods": recent_foods,
        "claude": pc.claude,
        "categories": pc.categories,
        "notifications": pc.notifications,
    })
}

/// The Plan tab: overdue and undated tasks, then the next `days` days.
pub fn agenda(d: &Snapshot, pc: &Pc, today: NaiveDate, days: i64) -> Value {
    let end = today + Duration::days(days.clamp(1, 60) - 1);
    let mut overdue: Vec<&Item> = d.items.iter().filter(|it| it.repeat.is_none() && !it.done && it.date.is_some_and(|x| x < today)).collect();
    overdue.sort_by_key(|it| it.date);
    let backlog: Vec<Value> = d.items.iter().filter(|it| it.date.is_none() && !it.done).map(|it| item_json(it, today, pc)).collect();
    let occ = crate::model::occurrences(&d.items, today, end);
    let mut list: Vec<Value> = Vec::new();
    let mut day = today;
    while day <= end {
        let mut its: Vec<&Item> = occ.iter().filter(|(x, _)| *x == day).map(|(_, i)| &d.items[*i]).collect();
        its.sort_by_key(|it| (it.time.is_none(), it.time));
        let label = match (day - today).num_days() {
            0 => "Today".to_string(),
            1 => "Tomorrow".to_string(),
            _ => day.format("%A %-d %b").to_string(),
        };
        list.push(json!({"date": day.to_string(), "label": label, "items": its.iter().map(|it| item_json(it, day, pc)).collect::<Vec<_>>()}));
        day += Duration::days(1);
    }
    json!({"overdue": overdue.iter().map(|it| item_json(it, it.date.unwrap_or(today), pc)).collect::<Vec<_>>(), "backlog": backlog, "days": list})
}

/// The food library: every ingredient, and meals with their computed numbers.
pub fn foods(d: &Snapshot) -> Value {
    let lib = &d.foods;
    let meals: Vec<Value> = lib
        .meals
        .iter()
        .map(|m| {
            let p = lib.meal_portionable(m);
            let (_, total_g) = lib.meal_totals(m);
            let mut v = serde_json::to_value(m).unwrap_or(Value::Null);
            v["kcal_serving"] = json!(p.kcal(crate::foods::Amount::Servings(1.0)));
            v["serving_g"] = json!(p.serving_g.round());
            v["kcal_100g"] = json!(p.per100.kcal.round());
            v["total_g"] = json!(total_g.round());
            v
        })
        .collect();
    json!({"foods": lib.foods, "meals": meals})
}

/// The Train tab: workouts and the last 30 sessions.
pub fn workouts(d: &Snapshot) -> Value {
    let lib = &d.exercises.exercises;
    let workouts: Vec<Value> = d
        .workouts
        .workouts
        .iter()
        .filter(|w| !w.items.is_empty())
        .map(|w| {
            json!({"id": w.id, "name": w.name, "notes": w.notes, "seconds": w.seconds(lib), "exercises": w.exercise_count(),
                   "equipment": w.equipment(lib).iter().map(|q| crate::workouts::equipment_label(q).to_lowercase()).collect::<Vec<_>>(),
                   "task": w.task.as_deref().and_then(|id| d.items.iter().find(|i| i.id == id)).map(|i| i.title.clone())})
        })
        .collect();
    let mut sessions: Vec<&crate::workouts::Session> = d.workout_log.sessions.iter().collect();
    sessions.sort_by(|a, b| (b.date, &b.start).cmp(&(a.date, &a.start)));
    let history: Vec<Value> = sessions
        .iter()
        .take(30)
        .map(|s| {
            let volume: f32 = s.sets.iter().map(|x| x.reps.unwrap_or(0) as f32 * x.kg.unwrap_or(0.0)).sum();
            json!({"id": s.id, "date": s.date.to_string(), "start": s.start, "duration_s": s.duration_s, "workout": s.workout,
                   "sets": s.sets, "completed": s.completed, "volume": volume.round()})
        })
        .collect();
    json!({"workouts": workouts, "history": history})
}

/// One workout, ready to play: its items, steps and how to do each exercise.
pub fn workout(d: &Snapshot, id: &str) -> Result<Value, String> {
    let lib = &d.exercises.exercises;
    let w = d.workouts.workouts.iter().find(|w| w.id == id).ok_or("That workout isn't there any more")?;
    let steps: Vec<Value> = w
        .steps(lib)
        .into_iter()
        .map(|s| match s {
            crate::workouts::Step::Set { exercise, name, set, sets, reps, seconds, kg, per_side } => {
                json!({"type": "set", "exercise": exercise, "name": name, "set": set, "sets": sets, "reps": reps, "seconds": seconds, "kg": kg, "per_side": per_side})
            }
            crate::workouts::Step::Rest { seconds, label } => json!({"type": "rest", "seconds": seconds, "label": label}),
        })
        .collect();
    let mut exercises = serde_json::Map::new();
    for item in &w.items {
        if let crate::workouts::Item::Exercise { exercise, .. } = item {
            if let Some(e) = lib.iter().find(|e| e.id == *exercise) {
                exercises.insert(
                    e.id.clone(),
                    json!({"name": e.name, "steps": e.steps, "form": e.form, "mistakes": e.mistakes, "easier": e.easier,
                    "harder": e.harder, "muscles_main": e.muscles_main, "muscles_helping": e.muscles_helping,
                    "equipment": e.equipment.iter().map(|q| crate::workouts::equipment_label(q)).collect::<Vec<_>>()}),
                );
            }
        }
    }
    let items: Vec<Value> = w
        .items
        .iter()
        .map(|it| match it {
            crate::workouts::Item::Exercise { exercise, sets, reps, seconds, kg, rest_s } => {
                let e = lib.iter().find(|e| e.id == *exercise);
                json!({"type": "exercise", "exercise": exercise, "name": e.map_or(exercise.as_str(), |e| e.name.as_str()), "sets": sets, "reps": reps,
                       "seconds": seconds, "kg": kg, "rest_s": rest_s, "per_side": e.is_some_and(|e| e.kind == "reps_per_side")})
            }
            crate::workouts::Item::Break { seconds, label } => json!({"type": "break", "seconds": seconds, "label": label}),
        })
        .collect();
    Ok(json!({"id": w.id, "name": w.name, "notes": w.notes, "task": w.task, "seconds": w.seconds(lib), "items": items, "steps": steps, "exercises": exercises}))
}

/// Progress: calories and weight over `days` days, the Body numbers and habit streaks.
pub fn progress(d: &Snapshot, pc: &Pc, today: NaiveDate, days: i64) -> Value {
    let from = today - Duration::days(days.clamp(7, 365) - 1);
    let mut calories = Vec::new();
    let mut day = from;
    while day <= today {
        calories.push(json!({"date": day.to_string(), "kcal": d.calories_on(day)}));
        day += Duration::days(1);
    }
    let weights: Vec<Value> = d.weights.iter().filter(|w| w.date >= from).map(|w| json!({"date": w.date.to_string(), "kg": dec(w.kg, 1)})).collect();
    let trend: Vec<Value> = crate::body::trend_series(&d.weights).into_iter().filter(|(x, _)| *x >= from).map(|(x, kg)| json!({"date": x.to_string(), "kg": dec(kg, 2)})).collect();
    let goal = d.body.goal.as_ref().map(|g| {
        json!({"target_kg": dec(g.target_kg, 1), "target_date": g.target_date.to_string(), "set_on": g.set_on.to_string(), "start_kg": dec(g.start_kg, 2),
               "reached_on": g.reached_on.map(|x| x.to_string()),
               "status": crate::body::progress(g, &d.weights, today).map(|p| if p.to_go <= 0.0 { format!("Goal {:.1} kg reached", g.target_kg) } else { format!("{:.1} kg to go · {}", p.to_go, p.status()) })})
    });
    let (limit, from_plan) = crate::budget::daily_limit(&d.body, &d.weights, today);
    let body = match crate::budget::body_plan(&d.body, &d.weights, today) {
        Some((n, p)) => json!({"complete": true, "weight_kg": dec(n.weight_kg, 1), "age": n.age, "bmr": n.bmr.round(), "formula": n.formula,
            "tdee": n.tdee.round(), "bmi": dec(n.bmi, 1), "healthy": [n.healthy_kg.0.round(), n.healthy_kg.1.round()],
            "daily": p.daily, "losing": p.mode == crate::body::Mode::Lose, "deficit": p.deficit.round(), "kg_per_week": dec(p.kg_per_week, 2),
            "days_left": p.days_left, "safe_date": p.safe_date.map(|x| x.to_string()), "too_fast": p.too_fast, "floored": p.floored}),
        None => json!({"complete": false, "missing": d.body.missing(d.current_weight()), "daily": limit}),
    };
    let pace = crate::body::recent_pace(&d.weights).map(|k| dec(k * 7.0, 2));
    // habit streaks: each repeating task's last 21 days (due / done)
    let start = today - Duration::days(20);
    let occ = crate::model::occurrences(&d.items, start, today);
    let mut habits: Vec<Value> = Vec::new();
    for (i, it) in d.items.iter().enumerate().filter(|(_, it)| it.repeat.is_some() && it.date.is_some_and(|x| x <= today)) {
        let due: Vec<NaiveDate> = occ.iter().filter(|(_, k)| *k == i).map(|(x, _)| *x).collect();
        if due.is_empty() {
            continue;
        }
        // current streak: done occurrences in a row, not broken by today if it's still open
        let mut streak = 0;
        for x in due.iter().rev() {
            if it.is_done_on(*x) {
                streak += 1;
            } else if *x == today {
                continue;
            } else {
                break;
            }
        }
        let cells: Vec<Value> = (0..21).map(|k| start + Duration::days(k)).map(|x| json!(if !due.contains(&x) { 0 } else if it.is_done_on(x) { 2 } else { 1 })).collect();
        let (icon, color) = pc.kind(&it.kind);
        habits.push(json!({"title": it.title, "icon": icon, "color": color,
                           "streak": streak, "done": due.iter().filter(|x| it.is_done_on(**x)).count(), "due": due.len(), "cells": cells}));
    }
    json!({"from": from.to_string(), "today": today.to_string(), "calories": calories, "limit": limit, "limit_from_plan": from_plan, "weights": weights, "trend": trend, "goal": goal,
              "body": body, "pace_kg_week": pace, "habits": habits, "journal_streak": crate::journal::streak(&d.journal, today)})
}

/// The journal, a page of days at a time (newest first, before `before`), or search results.
pub fn journal(d: &Snapshot, pc: &Pc, today: NaiveDate, query: &str, pinned: bool, voice: bool, before: Option<NaiveDate>) -> Value {
    let query = query.trim();
    let keep = |e: &&JournalEntry| (!pinned || e.pinned) && (!voice || e.audio.is_some()) && (query.is_empty() || e.matches(query));
    let mut list: Vec<&JournalEntry> = d.journal.iter().filter(keep).collect();
    list.sort_by_key(|e| std::cmp::Reverse(crate::journal::sort_key(e)));
    // browsing: 10 days with entries per page
    let filtered = pinned || voice || !query.is_empty();
    let before = before.unwrap_or(today + Duration::days(1));
    let mut days: Vec<NaiveDate> = list.iter().map(|e| e.date).filter(|x| *x < before).collect();
    days.dedup();
    let page = if filtered { 60 } else { 10 };
    let more = days.len() > page;
    days.truncate(page);
    let out: Vec<Value> = days
        .iter()
        .map(|day| json!({"date": day.to_string(), "entries": list.iter().filter(|e| e.date == *day).map(|e| entry_json(e, pc)).collect::<Vec<_>>()}))
        .collect();
    // most used tags, and what you wrote a month, 3 months and a year ago
    let mut tags: HashMap<String, usize> = HashMap::new();
    for e in &d.journal {
        for t in e.tags() {
            *tags.entry(t).or_default() += 1;
        }
    }
    let mut tags: Vec<(String, usize)> = tags.into_iter().collect();
    tags.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let earlier: Vec<Value> = [(30, "A month ago"), (91, "3 months ago"), (365, "A year ago")]
        .iter()
        .filter_map(|(n, label)| {
            let day = today - Duration::days(*n);
            let es: Vec<Value> = d.journal.iter().filter(|e| e.date == day).map(|e| entry_json(e, pc)).collect();
            (!es.is_empty()).then(|| json!({"label": label, "date": day.to_string(), "entries": es}))
        })
        .collect();
    json!({"days": out, "more": more, "tags": tags.iter().take(8).map(|(t, _)| t).collect::<Vec<_>>(), "on_this_day": earlier,
              "total": d.journal.len(), "streak": crate::journal::streak(&d.journal, today)})
}
