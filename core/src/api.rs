//! What the phone calls (through WebAssembly, see `wasm.rs`): JSON in, JSON out, so the phone's
//! JavaScript needs no generated bindings. Every call carries the phone's time, a bit of
//! randomness for new ids, and your category names:
//!
//! `{"fn": "plan_entry", "now_ms": 1762790400000, "tz_min": 60, "seed": 12345, "kinds": [..], ...}`
//!
//! Functions:
//! - `plan_entry` {text, day, today, snapshot} → {steps} | {claude} | {error}
//! - `project` {snapshot, steps, today} → {snapshot, results: [{ok}|{error}|{pending}]}
//! - `occurrences` {items, from, to} → [[date, index], …]
//! - `library` {foods} → every ingredient and meal with its numbers per serving and per 100 g
//! - `trend` {weights} → [[date, kg], …] (7-day average)
//! - `journal_streak` {journal, today} → days in a row
//! - `food_log` {food, servings?, grams?, date, time?} → a FoodLog step with a new id
//!
//! The phone's copy is kept here between calls, so screens don't resend it:
//! - `load` {snapshot, pc} → keeps the copy from the PC
//! - `set_steps` {steps, today} → the copy with the waiting steps applied; {results}
//! - `view` {name, today, …} → a screen (see `views.rs`): state {day}, agenda {days}, foods,
//!   workouts, workout {id}, progress {days}, journal {q, pinned, voice, before}
//! - `plan_entry_here` {text, day, today} → like plan_entry, on the kept copy
//! - `make_step` {kind, …} → a step with new ids, built here so its format is exactly the PC's:
//!   calories {kcal, description, date, time}, journal {text, mood, date, time, audio_secs?},
//!   food {food}, meal {meal}, session {session}
//! - `new_id` → a fresh id

use crate::foods::{Amount, Food, Library};
use crate::steps::{self, Planned, Snapshot, Step};
use crate::views::{self, Pc};
use chrono::{NaiveDate, NaiveTime};
use serde_json::{json, Value};
use std::cell::RefCell;

/// The phone's copy: as the PC sent it, and with the waiting steps applied.
struct Kept {
    base: Snapshot,
    pc: Pc,
    projected: Snapshot,
}

thread_local! {
    static KEPT: RefCell<Option<Kept>> = const { RefCell::new(None) };
}

fn with_kept<T>(f: impl FnOnce(&mut Kept) -> Result<T, String>) -> Result<T, String> {
    KEPT.with(|k| match k.borrow_mut().as_mut() {
        Some(kept) => f(kept),
        None => Err("no data loaded yet".into()),
    })
}

/// Name the label-placeholder foods get on the phone until the PC has read the label.
pub const LABEL_WAITING: &str = "📷 Label (waiting for Claude)";

pub fn call(input: &str) -> String {
    match dispatch(input) {
        Ok(v) => v.to_string(),
        Err(e) => json!({ "error": e }).to_string(),
    }
}

fn arg<T: serde::de::DeserializeOwned>(v: &Value, k: &str) -> Result<T, String> {
    serde_json::from_value(v.get(k).cloned().unwrap_or(Value::Null)).map_err(|e| format!("{k}: {e}"))
}

fn dispatch(input: &str) -> Result<Value, String> {
    let v: Value = serde_json::from_str(input).map_err(|e| format!("not JSON: {e}"))?;
    #[cfg(target_arch = "wasm32")]
    crate::clock::set(v["now_ms"].as_i64().unwrap_or(0), v["tz_min"].as_i64().unwrap_or(0) as i32, v["seed"].as_u64().unwrap_or(0));
    if let Some(k) = v.get("kinds").and_then(Value::as_array) {
        crate::kinds::set(k.iter().filter_map(Value::as_str).map(str::to_string).collect());
    }
    match v["fn"].as_str().unwrap_or("") {
        "plan_entry" => {
            let snap: Snapshot = arg(&v, "snapshot")?;
            let (day, today): (NaiveDate, NaiveDate) = (arg(&v, "day")?, arg(&v, "today")?);
            let text: String = arg(&v, "text")?;
            Ok(match steps::plan_entry(&text, day, today, &snap.view()) {
                Planned::Steps(s) => json!({ "steps": s }),
                Planned::Claude(f) => json!({ "claude": f }),
                Planned::Error(e) => json!({ "error": e }),
            })
        }
        "project" => {
            let mut snap: Snapshot = arg(&v, "snapshot")?;
            let list: Vec<Step> = arg(&v, "steps")?;
            let today: NaiveDate = arg(&v, "today")?;
            let results: Vec<Value> = list.iter().map(|st| project_one(&mut snap, st, today)).collect();
            Ok(json!({ "snapshot": snap, "results": results }))
        }
        "occurrences" => {
            let items: Vec<crate::model::Item> = arg(&v, "items")?;
            let (from, to): (NaiveDate, NaiveDate) = (arg(&v, "from")?, arg(&v, "to")?);
            Ok(json!(crate::model::occurrences(&items, from, to)))
        }
        "library" => {
            let lib: Library = arg(&v, "foods")?;
            Ok(json!(library(&lib)))
        }
        "trend" => {
            let weights: Vec<crate::model::WeightEntry> = arg(&v, "weights")?;
            Ok(json!(crate::body::trend_series(&weights)))
        }
        "journal_streak" => {
            let journal: Vec<crate::journal::JournalEntry> = arg(&v, "journal")?;
            Ok(json!(crate::journal::streak(&journal, arg(&v, "today")?)))
        }
        "food_log" => {
            let food: String = arg(&v, "food")?;
            let date: NaiveDate = arg(&v, "date")?;
            let time: Option<NaiveTime> = v["time"].as_str().and_then(|t| NaiveTime::parse_from_str(t, "%H:%M").or_else(|_| NaiveTime::parse_from_str(t, "%H:%M:%S")).ok());
            Ok(json!(Step::FoodLog { id: crate::clock::new_id(), food, servings: arg(&v, "servings")?, grams: arg(&v, "grams")?, date, time }))
        }
        "new_id" => Ok(json!(crate::clock::new_id())),
        "make_step" => {
            let date: Option<NaiveDate> = v.get("date").and_then(|x| serde_json::from_value(x.clone()).ok());
            let time: Option<NaiveTime> = v["time"].as_str().and_then(|t| NaiveTime::parse_from_str(t, "%H:%M").ok());
            let step = match v["kind"].as_str().unwrap_or("") {
                "calories" => {
                    let kcal = v["kcal"].as_f64().filter(|k| (1.0..=20000.0).contains(k)).ok_or("Enter calories between 1 and 20000")?.ceil() as u32;
                    let mut e = crate::model::CalorieEntry::new(date.ok_or("date")?, kcal, v["description"].as_str().unwrap_or("").trim().to_string(), "phone");
                    e.time = time;
                    Step::CaloriesAdd { entry: e }
                }
                "journal" => {
                    let mut e = crate::journal::JournalEntry::new(v["text"].as_str().unwrap_or("").trim().to_string());
                    e.date = date.ok_or("date")?;
                    e.time = time;
                    e.mood = v["mood"].as_u64().filter(|m| (1..=5).contains(m)).map(|m| m as u8);
                    match v["audio_secs"].as_f64() {
                        Some(secs) => {
                            e.audio_secs = Some(secs as f32);
                            Step::JournalAudio { entry: e }
                        }
                        None if e.text.is_empty() => return Err("Write something first".into()),
                        None => Step::JournalAdd { entry: e },
                    }
                }
                "food" => {
                    let mut f: Food = arg(&v, "food")?;
                    if f.id.is_empty() {
                        f.id = crate::clock::new_id();
                    }
                    Step::FoodSave { food: f }
                }
                "meal" => {
                    let mut m: crate::foods::Meal = arg(&v, "meal")?;
                    if m.id.is_empty() {
                        m.id = crate::clock::new_id();
                    }
                    Step::MealSave { meal: m }
                }
                "session" => {
                    let mut s: crate::workouts::Session = arg(&v, "session")?;
                    if s.id.is_empty() {
                        s.id = crate::clock::new_id();
                    }
                    Step::WorkoutSession { session: s }
                }
                other => return Err(format!("unknown kind \"{other}\"")),
            };
            Ok(json!(step))
        }
        "load" => {
            let base: Snapshot = arg(&v, "snapshot")?;
            let pc: Pc = arg(&v, "pc")?;
            let projected = base.clone();
            KEPT.with(|k| *k.borrow_mut() = Some(Kept { base, pc, projected }));
            Ok(json!({ "ok": true }))
        }
        "set_steps" => {
            let list: Vec<Step> = arg(&v, "steps")?;
            let today: NaiveDate = arg(&v, "today")?;
            with_kept(|k| {
                k.projected = k.base.clone();
                let results: Vec<Value> = list.iter().map(|st| project_one(&mut k.projected, st, today)).collect();
                Ok(json!({ "results": results }))
            })
        }
        "plan_entry_here" => {
            let (day, today): (NaiveDate, NaiveDate) = (arg(&v, "day")?, arg(&v, "today")?);
            let text: String = arg(&v, "text")?;
            with_kept(|k| {
                Ok(match steps::plan_entry(&text, day, today, &k.projected.view()) {
                    Planned::Steps(s) => json!({ "steps": s }),
                    Planned::Claude(f) => json!({ "claude": f }),
                    Planned::Error(e) => json!({ "error": e }),
                })
            })
        }
        "view" => {
            let today: NaiveDate = arg(&v, "today")?;
            let name: String = arg(&v, "name")?;
            with_kept(|k| {
                let (d, pc) = (&k.projected, &k.pc);
                Ok(match name.as_str() {
                    "state" => views::state(d, pc, v.get("day").and_then(|x| serde_json::from_value(x.clone()).ok()).unwrap_or(today), today),
                    "agenda" => views::agenda(d, pc, today, v["days"].as_i64().unwrap_or(14)),
                    "foods" => views::foods(d),
                    "workouts" => views::workouts(d),
                    "workout" => views::workout(d, v["id"].as_str().unwrap_or(""))?,
                    "progress" => views::progress(d, pc, today, v["days"].as_i64().unwrap_or(30)),
                    "journal" => {
                        let before: Option<NaiveDate> = v.get("before").and_then(|x| serde_json::from_value(x.clone()).ok());
                        views::journal(d, pc, today, v["q"].as_str().unwrap_or(""), v["pinned"].as_bool().unwrap_or(false), v["voice"].as_bool().unwrap_or(false), before)
                    }
                    other => return Err(format!("unknown view \"{other}\"")),
                })
            })
        }
        other => Err(format!("unknown function \"{other}\"")),
    }
}

/// One waiting step on the phone's copy: what it did, or that it waits for the PC.
fn project_one(snap: &mut Snapshot, st: &Step, today: NaiveDate) -> Value {
    match st {
        Step::Claude { .. } => json!({ "pending": "Claude does this when your PC is on" }),
        Step::Reflect { .. } => json!({ "pending": "Claude writes your reflection when your PC is on" }),
        Step::Label { food_id } => {
            // a placeholder in the library, so it can be logged (as ? kcal) before it's read
            if snap.foods.food(food_id).is_none() {
                snap.foods.foods.push(Food {
                    id: food_id.clone(),
                    name: LABEL_WAITING.into(),
                    serving_g: 100.0,
                    serving_name: "portion".into(),
                    notes: "waiting for Claude to read the label".into(),
                    ..Default::default()
                });
            }
            json!({ "pending": "Claude reads the label when your PC is on" })
        }
        _ => match steps::apply(snap, st, today) {
            Ok(m) => json!({ "ok": m }),
            Err(e) => json!({ "error": e }),
        },
    }
}

/// Every ingredient and meal with its numbers, as the Foods screens show them.
fn library(lib: &Library) -> Vec<Value> {
    lib.all()
        .iter()
        .map(|p| {
            let per = |x: Option<f32>| x.map(|g| (g as f64 * 10.0).round() / 10.0);
            json!({
                "id": p.id, "name": p.name, "meal": p.is_meal, "serving_name": p.serving_name,
                "serving_g": (p.serving_g as f64).round(), "kcal_serving": p.kcal(Amount::Servings(1.0)),
                "kcal_100g": (p.per100.kcal as f64).round(), "protein_100g": per(p.per100.protein),
                "carbs_100g": per(p.per100.carbs), "fat_100g": per(p.per100.fat),
                "waiting": p.name == LABEL_WAITING,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call_v(v: Value) -> Value {
        serde_json::from_str(&call(&v.to_string())).unwrap()
    }

    #[test]
    fn the_phone_plans_and_projects_offline() {
        let today = "2026-11-10";
        let mut snap = json!({ "weights": [{ "date": "2026-11-09", "kg": 82.0 }] });
        let mut queue: Vec<Value> = Vec::new();
        // typed offline: a task for tomorrow, 200 kcal, and something for Claude
        for text in ["call mum tomorrow 7pm", "200 kcal"] {
            let r = call_v(json!({ "fn": "plan_entry", "text": text, "day": today, "today": today, "snapshot": snap }));
            queue.extend(r["steps"].as_array().unwrap().iter().cloned());
        }
        let r = call_v(json!({ "fn": "plan_entry", "text": "ate a burrito", "day": today, "today": today, "snapshot": snap }));
        assert_eq!(r["claude"], "a burrito");
        queue.push(json!({ "type": "claude", "text": "a burrito", "food": true }));
        // …and the new task ticked off, before any of it reached the PC
        let task_id = queue[0]["item"]["id"].as_str().unwrap().to_string();
        queue.push(json!({ "type": "task_toggle", "id": task_id, "day": "2026-11-11" }));
        // a label scan, logged straight away
        queue.push(json!({ "type": "label", "food_id": "L1" }));
        queue.push(json!({ "type": "food_log", "id": "c9", "food": "L1", "servings": 1.0, "grams": null, "date": today, "time": null }));

        let p = call_v(json!({ "fn": "project", "snapshot": snap, "steps": queue, "today": today }));
        let results = p["results"].as_array().unwrap();
        assert!(results[0]["ok"].as_str().unwrap().starts_with("Added \"Call mum\""));
        assert!(results[1]["ok"].as_str().unwrap().starts_with("+200 kcal"));
        assert!(results[2]["pending"].is_string());
        assert!(results[3]["ok"].as_str().unwrap().starts_with("✔ Call mum"));
        assert!(results[4]["pending"].is_string());
        assert!(results[5]["ok"].is_string());
        snap = p["snapshot"].clone();
        let kcal: u64 = snap["calories"].as_array().unwrap().iter().map(|c| c["kcal"].as_u64().unwrap()).sum();
        assert_eq!(kcal, 200); // the label food counts 0 until it's read
        assert_eq!(snap["items"][0]["done"], true);

        let lib = call_v(json!({ "fn": "library", "foods": snap["foods"] }));
        assert_eq!(lib[0]["waiting"], true);
        let occ = call_v(json!({ "fn": "occurrences", "items": snap["items"], "from": "2026-11-10", "to": "2026-11-12" }));
        assert_eq!(occ[0][0], "2026-11-11");
        assert!(call_v(json!({ "fn": "nope" }))["error"].is_string());
    }

    #[test]
    fn the_phone_keeps_its_copy_and_draws_screens_from_it() {
        let today = "2026-11-10";
        assert!(call_v(json!({ "fn": "view", "name": "state", "today": today }))["error"].is_string());
        let snap = json!({ "calories": [{ "id": "a", "date": today, "kcal": 500, "description": "lunch", "source": "phone" }] });
        call_v(json!({ "fn": "load", "snapshot": snap, "pc": { "claude": true } }));
        let st = call_v(json!({ "fn": "view", "name": "state", "today": today }));
        assert_eq!((st["calories"]["total"].as_u64(), st["claude"].as_bool()), (Some(500), Some(true)));
        // waiting steps show on the screens; replacing them starts again from the PC's copy
        let p = call_v(json!({ "fn": "plan_entry_here", "text": "200 kcal", "day": today, "today": today }));
        call_v(json!({ "fn": "set_steps", "steps": p["steps"], "today": today }));
        assert_eq!(call_v(json!({ "fn": "view", "name": "state", "today": today }))["calories"]["total"], 700);
        call_v(json!({ "fn": "set_steps", "steps": [], "today": today }));
        assert_eq!(call_v(json!({ "fn": "view", "name": "state", "today": today }))["calories"]["total"], 500);
        let ag = call_v(json!({ "fn": "view", "name": "agenda", "today": today, "days": 3 }));
        assert_eq!(ag["days"].as_array().unwrap().len(), 3);
    }
}
