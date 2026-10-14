//! Importing items from JSON (pasted, or dropped into the inbox folder). See IMPORT_FORMAT.md.

use crate::categories::{self, Category, StreakStyle};
use crate::foods::{Amount, Food, Library};
use crate::model::{CalorieEntry, Item, Repeat, WeightEntry};
use crate::parse::{parse_date, parse_duration, parse_time};
use crate::store::{Data, Settings, IMPORT_FORMAT};
use chrono::{Duration, Local, NaiveDate, Timelike};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashSet};
use std::fs;

pub enum Op {
    Add(Item),
    Update(Item),
    Delete { id: String, title: String },
    LogCalories(CalorieEntry),
    LogWeight(WeightEntry),
    DeleteCalories { id: String, label: String },
    Settings(Box<SettingsChange>),
    Journal(crate::journal::JournalEntry),
    /// Add a food to the library, or replace the one with the same name.
    SaveFood(Food),
}

impl Op {
    pub fn describe(&self) -> String {
        let when = |i: &Item| {
            let mut s = String::new();
            if let Some(d) = i.date {
                s += &format!(" · {}", d.format("%a %b %-d"));
            }
            if let Some(t) = i.time {
                s += &format!(" {}", t.format("%H:%M"));
            }
            if let Some(r) = &i.repeat {
                s += &format!(" · {r}");
            }
            s
        };
        match self {
            Op::Add(i) => format!("+ add [{}] {}{}", i.kind, i.title, when(i)),
            Op::Update(i) => format!("~ update [{}] {}{}", i.kind, i.title, when(i)),
            Op::Delete { title, .. } => format!("- delete {title}"),
            Op::LogCalories(c) => {
                let when = match c.time {
                    Some(t) => format!("{} {}", c.date.format("%a %b %-d"), t.format("%H:%M")),
                    None => c.date.format("%a %b %-d").to_string(),
                };
                format!("+ log {} kcal · {when} · {}", c.kcal, c.description)
            }
            Op::DeleteCalories { label, .. } => format!("- remove calories {label}"),
            Op::LogWeight(w) => format!("+ weight {:.1} kg · {}", w.kg, w.date.format("%a %b %-d")),
            Op::Settings(s) => s.lines.join("\n"),
            Op::SaveFood(f) => format!("+ food {} · {:.0} kcal/100 g · 1 {} = {:.0} g", f.name, f.kcal_per_100g, f.serving_name, f.serving_g),
            Op::Journal(e) => format!("+ journal {} {} · {}", e.date.format("%a %b %-d"), crate::journal::time_label(e), e.text),
        }
    }
}

#[derive(Default)]
pub struct Plan {
    pub ops: Vec<Op>,
    pub errors: Vec<String>,
    /// Optional note from whoever wrote the import (e.g. an assumption Claude made).
    pub message: Option<String>,
}

/// Cuts the JSON out of text that may be wrapped in a markdown fence or chatter.
fn extract_json(text: &str) -> &str {
    let start = text.find(['{', '[']);
    let end = text.rfind(['}', ']']);
    match (start, end) {
        (Some(s), Some(e)) if e > s => &text[s..=e],
        _ => text.trim(),
    }
}

/// Escapes raw newlines/tabs inside JSON strings, a common slip in AI-written JSON.
fn escape_raw_controls(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    let (mut in_str, mut escaped) = (false, false);
    for c in json.chars() {
        if in_str {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_str = false,
                '\n' => {
                    out.push_str("\\n");
                    continue;
                }
                '\r' => continue,
                '\t' => {
                    out.push_str("\\t");
                    continue;
                }
                _ => {}
            }
        } else if c == '"' {
            in_str = true;
        }
        out.push(c);
    }
    out
}

pub fn plan(text: &str, existing: &[Item], calories: &[CalorieEntry], settings: &Settings, foods: &Library, today: NaiveDate) -> Plan {
    let mut plan = Plan::default();
    let json = extract_json(text);
    let parsed = serde_json::from_str(json).or_else(|e| serde_json::from_str(&escape_raw_controls(json)).map_err(|_| e));
    let value: Value = match parsed {
        Ok(v) => v,
        Err(e) => {
            plan.errors.push(format!("Not valid JSON: {e}"));
            return plan;
        }
    };
    let mut cal_entries = Vec::new();
    let mut weight_entries = Vec::new();
    // Day that calories, weight, journal entries and check-offs belong to unless they say
    // otherwise: today, or the import's top-level "date" (e.g. yesterday, after midnight).
    let mut day = today;
    let (items, deletes) = match value {
        Value::Array(a) => (a, Vec::new()),
        Value::Object(mut o) => {
            match o.remove("date") {
                None | Some(Value::Null) => {}
                Some(v) => match v.as_str().and_then(|d| parse_date(d, today)) {
                    Some(d) => day = d,
                    None => plan.errors.push("\"date\": use YYYY-MM-DD (or yesterday-style words)".into()),
                },
            }
            let items = match o.remove("items") {
                Some(Value::Array(a)) => a,
                None => Vec::new(),
                Some(_) => {
                    plan.errors.push("\"items\" must be an array".into());
                    Vec::new()
                }
            };
            let deletes = match o.remove("delete") {
                Some(Value::Array(a)) => a,
                None => Vec::new(),
                Some(_) => {
                    plan.errors.push("\"delete\" must be an array of ids".into());
                    Vec::new()
                }
            };
            match o.remove("weight") {
                Some(Value::Array(a)) => weight_entries = a,
                None => {}
                Some(_) => plan.errors.push("\"weight\" must be an array".into()),
            }
            match o.remove("calories") {
                Some(Value::Array(a)) => cal_entries = a,
                None => {}
                Some(_) => plan.errors.push("\"calories\" must be an array".into()),
            }
            match o.remove("foods") {
                Some(Value::Array(a)) => {
                    for (n, v) in a.into_iter().enumerate() {
                        match food_from(v, foods) {
                            Ok(f) => plan.ops.push(Op::SaveFood(f)),
                            Err(e) => plan.errors.push(format!("foods {}: {e}", n + 1)),
                        }
                    }
                }
                None => {}
                Some(_) => plan.errors.push("\"foods\" must be an array".into()),
            }
            match o.remove("journal") {
                Some(Value::Array(a)) => {
                    for (n, v) in a.into_iter().enumerate() {
                        match journal_from(v, today, day) {
                            Ok(e) => plan.ops.push(Op::Journal(e)),
                            Err(e) => plan.errors.push(format!("journal {}: {e}", n + 1)),
                        }
                    }
                }
                None => {}
                Some(_) => plan.errors.push("\"journal\" must be an array".into()),
            }
            if let Some(s) = o.remove("settings") {
                match settings_from(s, settings, existing) {
                    Ok(change) => plan.ops.push(Op::Settings(Box::new(change))),
                    Err(errs) => plan.errors.extend(errs.into_iter().map(|e| format!("settings: {e}"))),
                }
            }
            o.remove("morrowkeep_import");
            o.remove("lifehub_import"); // the app's previous name
            if let Some(Value::String(m)) = o.remove("message") {
                plan.message = Some(m);
            }
            for k in o.keys() {
                plan.errors.push(format!("unknown top-level field \"{k}\" (expected \"items\", \"calories\", \"weight\", \"foods\", \"journal\", \"settings\", \"delete\", \"message\")"));
            }
            (items, deletes)
        }
        _ => {
            plan.errors.push("Expected a JSON object with \"items\", or an array of items".into());
            return plan;
        }
    };

    for (n, v) in items.into_iter().enumerate() {
        let label = format!("item {}", n + 1);
        let Value::Object(obj) = v else {
            plan.errors.push(format!("{label}: must be an object"));
            continue;
        };
        match item_from(obj, existing, today, day) {
            Ok(op) => plan.ops.push(op),
            Err(errs) => plan.errors.extend(errs.into_iter().map(|e| format!("{label}: {e}"))),
        }
    }
    for (n, v) in cal_entries.into_iter().enumerate() {
        let label = format!("calories {}", n + 1);
        let Value::Object(obj) = v else {
            plan.errors.push(format!("{label}: must be an object"));
            continue;
        };
        match calories_from(obj, today, day, foods) {
            Ok(c) => plan.ops.push(Op::LogCalories(c)),
            Err(errs) => plan.errors.extend(errs.into_iter().map(|e| format!("{label}: {e}"))),
        }
    }
    for (n, v) in weight_entries.into_iter().enumerate() {
        let label = format!("weight {}", n + 1);
        let kg = v.get("kg").and_then(Value::as_f64);
        let date = match v.get("date").and_then(Value::as_str) {
            None => Some(day),
            Some(d) => parse_date(d, today),
        };
        let unknown: Vec<&String> = v.as_object().map(|o| o.keys().filter(|k| *k != "kg" && *k != "date").collect()).unwrap_or_default();
        match (kg, date) {
            _ if !unknown.is_empty() => plan.errors.push(format!("{label}: unknown field \"{}\"", unknown[0])),
            (Some(kg), Some(date)) if (20.0..=400.0).contains(&kg) => {
                plan.ops.push(Op::LogWeight(WeightEntry { date, kg: ((kg * 10.0).round() / 10.0) as f32 }))
            }
            (Some(_), Some(_)) => plan.errors.push(format!("{label}: \"kg\" must be between 20 and 400")),
            (None, _) => plan.errors.push(format!("{label}: \"kg\" (a number) is required")),
            (_, None) => plan.errors.push(format!("{label}: \"date\": use YYYY-MM-DD")),
        }
    }
    for v in deletes {
        let Some(id) = v.as_str() else {
            plan.errors.push(format!("delete: {v} is not an id string"));
            continue;
        };
        if let Some(i) = existing.iter().find(|i| i.id == id) {
            plan.ops.push(Op::Delete { id: id.to_string(), title: i.title.clone() });
        } else if let Some(c) = calories.iter().find(|c| c.id == id) {
            let label = format!("{} kcal · {}", c.kcal, c.description);
            plan.ops.push(Op::DeleteCalories { id: id.to_string(), label });
        } else {
            plan.errors.push(format!("delete: no item or calorie entry with id \"{id}\""));
        }
    }
    if plan.ops.is_empty() && plan.errors.is_empty() && plan.message.is_none() {
        plan.errors.push("Nothing to import".into());
    }
    plan
}

fn calories_from(o: Map<String, Value>, today: NaiveDate, day: NaiveDate, foods: &Library) -> Result<CalorieEntry, Vec<String>> {
    let mut errs = Vec::new();
    let mut kcal = None;
    let mut food: Option<String> = None;
    let mut amount = None;
    let mut date = day;
    let mut time = None;
    let mut description = String::new();
    for (key, val) in o {
        let s = val.as_str().map(str::trim);
        match key.as_str() {
            "kcal" | "calories" => match val.as_f64() {
                Some(n) if (1.0..=20000.0).contains(&n) => kcal = Some(n.ceil() as u32),
                _ => errs.push(format!("\"{key}\" must be a number between 1 and 20000")),
            },
            "date" => match s.and_then(|d| parse_date(d, today)) {
                Some(d) => date = d,
                None => errs.push("\"date\": use YYYY-MM-DD (or today/yesterday-style words)".into()),
            },
            "time" => match s.and_then(parse_time) {
                Some(t) => time = Some(t),
                None if val.is_null() => {}
                None => errs.push("\"time\": use HH:MM".into()),
            },
            "food" => match s {
                Some(f) => food = Some(f.to_string()),
                None => errs.push("\"food\" must be text".into()),
            },
            "servings" => match val.as_f64().filter(|n| *n > 0.0 && *n <= 50.0) {
                Some(n) => amount = Some(Amount::Servings(n as f32)),
                None => errs.push("\"servings\" must be a number above 0".into()),
            },
            "grams" => match val.as_f64().filter(|n| *n > 0.0 && *n <= 10000.0) {
                Some(n) => amount = Some(Amount::Grams(n as f32)),
                None => errs.push("\"grams\" must be a number above 0".into()),
            },
            "description" | "notes" => match s {
                Some(d) => description = d.to_string(),
                None => errs.push("\"description\" must be text".into()),
            },
            other => errs.push(format!("unknown field \"{other}\"")),
        }
    }
    // a food from the library: Morrowkeep computes the calories itself
    if let Some(name) = &food {
        match foods.find(name) {
            Some(p) if errs.is_empty() => {
                let mut e = Data::food_entry(&p, amount.unwrap_or(Amount::Servings(1.0)), date, "claude");
                if time.is_some() {
                    e.time = time;
                }
                return Ok(e);
            }
            Some(_) => return Err(errs),
            None if kcal.is_none() => errs.push(format!("\"{name}\" isn't in the food library; give \"kcal\" (and a \"description\") instead")),
            None => {
                if description.is_empty() {
                    description = name.clone();
                }
            }
        }
    }
    let Some(kcal) = kcal else {
        if errs.is_empty() {
            errs.push("\"kcal\" is required".into());
        }
        return Err(errs);
    };
    if !errs.is_empty() {
        return Err(errs);
    }
    let mut entry = CalorieEntry::new(date, kcal, description, "import");
    if time.is_some() {
        entry.time = time;
    }
    Ok(entry)
}

fn item_from(mut o: Map<String, Value>, existing: &[Item], today: NaiveDate, day: NaiveDate) -> Result<Op, Vec<String>> {
    let mut errs = Vec::new();
    let (mut item, is_update) = match o.remove("id") {
        Some(Value::String(id)) => match existing.iter().find(|i| i.id == id) {
            Some(i) => (i.clone(), true),
            None => return Err(vec![format!("no existing item with id \"{id}\" (leave out \"id\" to create a new item)")]),
        },
        None | Some(Value::Null) => (Item::new(""), false),
        Some(_) => return Err(vec!["\"id\" must be a string".into()]),
    };

    // Each closure-free block: missing key = keep, null = clear, otherwise validate.
    let mut end_time = None;
    // applied after all fields, once we know whether the item repeats
    let mut done_flag: Option<bool> = None;
    let mut done_date: Option<NaiveDate> = None;
    for (key, val) in o {
        let null = val.is_null();
        let s = val.as_str().map(str::trim);
        match key.as_str() {
            "title" => match s {
                Some(t) if !t.is_empty() => item.title = t.to_string(),
                _ => errs.push("\"title\" must be non-empty text".into()),
            },
            "kind" | "type" => match s {
                Some(k) if !k.is_empty() => item.kind = k.to_lowercase(),
                _ if null => item.kind = "task".into(),
                _ => errs.push("\"kind\" must be text".into()),
            },
            "date" | "start_date" => match s {
                _ if null => item.date = None,
                Some(d) => match parse_date(d, today) {
                    Some(d) => item.date = Some(d),
                    None => errs.push(format!("\"date\": can't read \"{d}\" (use YYYY-MM-DD)")),
                },
                None => errs.push("\"date\" must be a string".into()),
            },
            "time" | "start_time" => match s {
                _ if null => item.time = None,
                Some(t) => match parse_time(t) {
                    Some(t) => item.time = Some(t),
                    None => errs.push(format!("\"time\": can't read \"{t}\" (use HH:MM)")),
                },
                None => errs.push("\"time\" must be a string like \"18:30\"".into()),
            },
            "end" | "end_time" => match s.and_then(parse_time) {
                Some(t) => end_time = Some(t),
                None if null => {}
                None => errs.push(format!("\"{key}\": use HH:MM")),
            },
            "duration_min" | "duration" | "minutes" => {
                if null {
                    item.duration_min = None;
                } else if let Some(n) = val.as_u64() {
                    item.duration_min = Some(n as u32);
                } else if let Some(m) = s.and_then(parse_duration) {
                    item.duration_min = Some(m);
                } else {
                    errs.push(format!("\"{key}\" must be a number of minutes"));
                }
            }
            "repeat" | "recurrence" => match s {
                _ if null => item.repeat = None,
                Some("") | Some("none") | Some("never") => item.repeat = None,
                Some(r) => match Repeat::parse(r) {
                    Ok(rule) => item.repeat = Some(rule.describe()),
                    Err(e) => errs.push(format!("\"repeat\": {e}")),
                },
                None => errs.push("\"repeat\" must be text like \"weekly mon,thu\"".into()),
            },
            "until" => match s {
                _ if null => item.until = None,
                Some(d) => match parse_date(d, today) {
                    Some(d) => item.until = Some(d),
                    None => errs.push(format!("\"until\": can't read \"{d}\"")),
                },
                None => errs.push("\"until\" must be a date string".into()),
            },
            "notes" | "description" => match s {
                _ if null => item.notes.clear(),
                Some(n) => item.notes = n.to_string(),
                None => errs.push("\"notes\" must be text".into()),
            },
            "tags" => match &val {
                Value::Null => item.tags.clear(),
                Value::Array(a) if a.iter().all(Value::is_string) => {
                    item.tags = a.iter().filter_map(|t| t.as_str().map(String::from)).collect()
                }
                _ => errs.push("\"tags\" must be an array of strings".into()),
            },
            "done" => match val.as_bool() {
                Some(b) => done_flag = Some(b),
                None => errs.push("\"done\" must be true or false".into()),
            },
            "done_date" => match s.and_then(|d| parse_date(d, today)) {
                Some(d) => done_date = Some(d),
                None => errs.push("\"done_date\": use YYYY-MM-DD".into()),
            },
            "sync_google" => match val.as_bool() {
                Some(b) => item.sync_google = b,
                None => errs.push("\"sync_google\" must be true or false".into()),
            },
            other => errs.push(format!("unknown field \"{other}\"")),
        }
    }
    if let (Some(end), Some(start)) = (end_time, item.time) {
        let mins = (end - start).num_minutes();
        if mins > 0 {
            item.duration_min = Some(mins as u32);
        } else {
            errs.push("\"end\" must be after \"time\"".into());
        }
    }
    if let Some(done) = done_flag {
        if item.repeat.is_some() {
            // repeating items are done per occurrence: today unless done_date says otherwise
            let on = done_date.unwrap_or(day);
            if item.is_done_on(on) != done {
                item.toggle_done(on);
            }
        } else {
            item.set_done(done);
            if done {
                // finished on another day (e.g. "I did it yesterday")
                if let Some(d) = done_date.or((day != today).then_some(day)) {
                    item.done_on = Some(d);
                }
            }
        }
    }
    if item.title.is_empty() {
        errs.push("\"title\" is required".into());
    }
    if item.repeat.is_some() && item.date.is_none() {
        // a repeating item needs a first day
        if let Some(rule) = item.repeat_rule() {
            item.date = Some(rule.first_occurrence(today));
        }
    }
    if !errs.is_empty() {
        return Err(errs);
    }
    item.touch();
    Ok(if is_update { Op::Update(item) } else { Op::Add(item) })
}

/// Applies a validated plan. Returns a summary like "3 added, 1 updated".
pub fn apply(data: &mut Data, ops: Vec<Op>) -> String {
    let (mut a, mut u, mut d) = (0, 0, 0);
    let (mut kcal, mut cal_changed) = (0u32, false);
    let mut weights = 0;
    let mut settings_note = None;
    let mut journals = 0;
    let mut foods_saved = 0;
    for op in ops {
        match op {
            Op::Add(item) => {
                data.items.push(item);
                a += 1;
            }
            Op::Update(item) => {
                if let Some(idx) = data.item_index(&item.id) {
                    data.items[idx] = item;
                    u += 1;
                }
            }
            Op::Delete { id, .. } => {
                data.delete_item(&id);
                d += 1;
            }
            Op::LogCalories(c) => {
                kcal += c.kcal;
                data.calories.push(c);
                cal_changed = true;
            }
            Op::DeleteCalories { id, .. } => {
                data.calories.retain(|c| c.id != id);
                cal_changed = true;
                d += 1;
            }
            Op::LogWeight(w) => {
                data.set_weight(w.date, Some(w.kg));
                weights += 1;
            }
            Op::SaveFood(f) => {
                let key = crate::foods::norm(&f.name);
                match data.foods.foods.iter_mut().find(|x| crate::foods::norm(&x.name) == key) {
                    Some(existing) => {
                        let id = existing.id.clone();
                        *existing = Food { id, ..f };
                    }
                    None => data.foods.foods.push(f),
                }
                data.save_foods();
                foods_saved += 1;
            }
            Op::Journal(e) => {
                data.journal.push(e);
                data.save_journal();
                journals += 1;
            }
            Op::Settings(change) => {
                let SettingsChange { mut new, renames, deletes, .. } = *change;
                data.move_kinds(&renames, &deletes);
                if new.theme != data.settings.theme {
                    if let Err(e) = crate::ui::theme::activate(&data.paths.root, &new.theme) {
                        settings_note = Some(format!("theme not changed: {e}"));
                        new.theme = data.settings.theme.clone();
                    }
                }
                data.settings = new;
                data.save_settings();
                categories::apply(&data.settings.categories, &data.settings.colors);
                settings_note.get_or_insert_with(|| "settings changed".into());
            }
        }
    }
    data.save_items();
    if cal_changed {
        data.save_calories();
    }
    let mut parts = Vec::new();
    for (n, w) in [(a, "added"), (u, "updated"), (d, "deleted")] {
        if n > 0 {
            parts.push(format!("{n} {w}"));
        }
    }
    if kcal > 0 {
        parts.push(format!("{kcal} kcal logged"));
    }
    if weights > 0 {
        parts.push(format!("{weights} weight entr{}", if weights == 1 { "y" } else { "ies" }));
    }
    if foods_saved > 0 {
        parts.push(format!("{foods_saved} food{} saved", if foods_saved == 1 { "" } else { "s" }));
    }
    if journals > 0 {
        parts.push(format!("{journals} journal entr{}", if journals == 1 { "y" } else { "ies" }));
    }
    parts.extend(settings_note);
    parts.join(", ")
}

/// Imports every *.json file in the inbox. Good files move to inbox/imported, bad ones to
/// inbox/failed together with a .errors.txt explaining why. Returns a status message.
pub fn process_inbox(data: &mut Data) -> Option<String> {
    if data.load_error.is_some() {
        return None;
    }
    let entries = fs::read_dir(&data.paths.inbox).ok()?;
    let mut msgs = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        if !path.is_file() || path.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else { continue }; // probably still being written
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let stamp = Local::now().format("%Y%m%d-%H%M%S");
        let today = Local::now().date_naive();
        let p = plan(&text, &data.items, &data.calories, &data.settings, &data.foods, today);
        if p.errors.is_empty() {
            let summary = apply(data, p.ops);
            let _ = fs::rename(&path, data.paths.inbox.join("imported").join(format!("{stamp}-{name}")));
            msgs.push(format!("{name}: {summary}"));
        } else {
            let failed = data.paths.inbox.join("failed").join(format!("{stamp}-{name}"));
            let _ = fs::rename(&path, &failed);
            let _ = fs::write(failed.with_extension("errors.txt"), p.errors.join("\n"));
            msgs.push(format!("{name} rejected ({} errors, see inbox\\failed)", p.errors.len()));
        }
    }
    (!msgs.is_empty()).then(|| format!("Inbox: {}", msgs.join("; ")))
}

/// A food for the library: {"name", "kcal_per_100g", "serving_g"?, "serving_name"?,
/// "protein_per_100g"?, "carbs_per_100g"?, "fat_per_100g"?, "aliases"?}.
fn food_from(v: Value, foods: &Library) -> Result<Food, String> {
    let Value::Object(o) = v else { return Err("must be an object".into()) };
    let name = o.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()).ok_or("\"name\" is required")?;
    // start from the existing one, so a partial update keeps the rest
    let mut f = foods
        .foods
        .iter()
        .find(|x| crate::foods::norm(&x.name) == crate::foods::norm(name))
        .cloned()
        .unwrap_or(Food { id: uuid::Uuid::new_v4().to_string(), name: name.to_string(), serving_g: 100.0, serving_name: "serving".into(), ..Default::default() });
    let num = |k: &str, max: f64| -> Result<Option<f32>, String> {
        match o.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v.as_f64().filter(|n| *n >= 0.0 && *n <= max).map(|n| Some(n as f32)).ok_or(format!("\"{k}\" must be a number from 0 to {max}")),
        }
    };
    for k in o.keys() {
        if !["name", "kcal_per_100g", "serving_g", "serving_name", "protein_per_100g", "carbs_per_100g", "fat_per_100g", "aliases"].contains(&k.as_str()) {
            return Err(format!("unknown field \"{k}\""));
        }
    }
    match num("kcal_per_100g", 900.0)? {
        Some(k) => f.kcal_per_100g = k,
        None if f.kcal_per_100g <= 0.0 => return Err("\"kcal_per_100g\" is required for a new food".into()),
        None => {}
    }
    if let Some(g) = num("serving_g", 5000.0)?.filter(|g| *g >= 1.0) {
        f.serving_g = g;
    }
    if let Some(n) = o.get("serving_name").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
        f.serving_name = n.to_lowercase();
    }
    for (k, slot) in [("protein_per_100g", &mut f.protein_per_100g), ("carbs_per_100g", &mut f.carbs_per_100g), ("fat_per_100g", &mut f.fat_per_100g)] {
        if o.contains_key(k) {
            *slot = num(k, 100.0)?;
        }
    }
    if let Some(Value::Array(a)) = o.get("aliases") {
        f.aliases = a.iter().filter_map(Value::as_str).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    }
    Ok(f)
}

/// A journal entry from an import: {"text", "date"?, "time"?, "mood"? (1-5)}.
fn journal_from(v: Value, today: NaiveDate, day: NaiveDate) -> Result<crate::journal::JournalEntry, String> {
    let Value::Object(o) = v else { return Err("must be an object".into()) };
    let mut e = crate::journal::JournalEntry::new("");
    e.date = day;
    let mut timed = false;
    for (k, val) in o {
        match k.as_str() {
            "text" => e.text = val.as_str().map(|s| s.trim().to_string()).ok_or("\"text\" must be text")?,
            "date" => e.date = val.as_str().and_then(|d| parse_date(d, today)).ok_or("\"date\": use YYYY-MM-DD")?,
            "time" => {
                e.time = Some(val.as_str().and_then(parse_time).ok_or("\"time\": use HH:MM")?);
                timed = true;
            }
            "mood" => e.mood = Some(val.as_u64().filter(|m| (1..=5).contains(m)).ok_or("\"mood\" must be 1 to 5")? as u8),
            other => return Err(format!("unknown field \"{other}\" (expected text, date, time, mood)")),
        }
    }
    if e.text.is_empty() {
        return Err("\"text\" is required".into());
    }
    // written now for an earlier day: the current clock time would be misleading
    if !timed && e.date != today {
        e.time = None;
    }
    Ok(e)
}

/// A validated change to settings from an import (Claude or pasted JSON).
pub struct SettingsChange {
    pub new: Settings,
    /// Category renames (old → new) and deletions, which move tasks along on apply.
    pub renames: BTreeMap<String, String>,
    pub deletes: HashSet<String>,
    /// One line per change, for the preview.
    pub lines: Vec<String>,
}

/// Settings an import may change directly (besides "colors" and "categories").
const SIMPLE_SETTINGS: &[&str] = &[
    "theme",
    "font",
    "default_input_mode",
    "reminders_enabled",
    "reminder_interval_min",
    "remind_anytime",
    "remind_calories",
    "calorie_reminder_after",
    "remind_weight",
    "weight_reminder_after",
    "remind_journal",
    "journal_reminder_after",
    "quiet_start",
    "quiet_end",
    "close_to_tray",
    "always_on_top",
    "auto_sync",
    "sync_interval_min",
];

fn hex_or_theme(c: Option<&str>, theme: eframe::egui::Color32) -> String {
    match c {
        Some(h) => h.to_string(),
        None => format!("{} (theme)", categories::to_hex(theme)),
    }
}

fn settings_from(v: Value, current: &Settings, existing: &[Item]) -> Result<SettingsChange, Vec<String>> {
    let Value::Object(mut o) = v else { return Err(vec!["must be an object".into()]) };
    let mut errs = Vec::new();
    let mut lines = Vec::new();
    let th = crate::ui::theme::th();

    // plain values: merge into the current settings and let serde check the types
    let mut merged = serde_json::to_value(current).unwrap_or(Value::Null);
    let mut simple_changed = Vec::new();
    for key in SIMPLE_SETTINGS {
        let Some(val) = o.remove(*key) else { continue };
        let ok = match *key {
            "default_input_mode" => matches!(val.as_str(), Some("local" | "claude")),
            "font" => val.as_str().is_some_and(|f| crate::ui::theme::FONTS.iter().any(|(v, _)| *v == f)),
            "theme" => val.as_str().is_some_and(|t| !t.trim().is_empty()),
            k if k.ends_with("_after") || k.starts_with("quiet_") => {
                val.as_str().is_some_and(|t| chrono::NaiveTime::parse_from_str(t, "%H:%M").is_ok())
            }
            _ => true,
        };
        if !ok {
            errs.push(format!("{key}: {val} isn't a valid value"));
            continue;
        }
        if merged[key] != val {
            lines.push(format!("~ setting {key}: {} → {}", merged[key], val));
            merged[*key] = val;
            simple_changed.push(*key);
        }
    }
    let mut new: Settings = match serde_json::from_value(merged) {
        Ok(s) => s,
        Err(e) => {
            errs.push(format!("{e}"));
            current.clone()
        }
    };

    // colors: "#RRGGBB" or null (back to the theme's)
    if let Some(c) = o.remove("colors") {
        match c {
            Value::Object(m) => {
                for (k, val) in m {
                    let slot = match k.as_str() {
                        "calories" => (&mut new.colors.calories, th.data.calories),
                        "weight" => (&mut new.colors.weight, th.data.weight),
                        _ => {
                            errs.push(format!("colors: unknown color \"{k}\" (expected \"calories\" or \"weight\")"));
                            continue;
                        }
                    };
                    let value = match &val {
                        Value::Null => None,
                        Value::String(s) => match categories::parse_hex(s) {
                            Some(c) => Some(categories::to_hex(c)),
                            None => {
                                errs.push(format!("colors.{k}: \"{s}\" isn't a #RRGGBB color"));
                                continue;
                            }
                        },
                        _ => {
                            errs.push(format!("colors.{k}: use \"#RRGGBB\" or null"));
                            continue;
                        }
                    };
                    if *slot.0 != value {
                        lines.push(format!("~ {k} color: {} → {}", hex_or_theme(slot.0.as_deref(), slot.1), hex_or_theme(value.as_deref(), slot.1)));
                        *slot.0 = value;
                    }
                }
            }
            _ => errs.push("colors: must be an object".into()),
        }
    }

    // categories: upsert by name, optional rename
    let mut renames = BTreeMap::new();
    let count = |kind: &str| existing.iter().filter(|i| i.kind == kind).count();
    if let Some(c) = o.remove("categories") {
        let Value::Array(arr) = c else { return Err(vec!["categories: must be an array".into()]) };
        for (n, cv) in arr.into_iter().enumerate() {
            let label = format!("categories {}", n + 1);
            let Value::Object(co) = cv else {
                errs.push(format!("{label}: must be an object"));
                continue;
            };
            let Some(name) = co.get("name").and_then(Value::as_str).map(categories::clean_name).filter(|n| !n.is_empty()) else {
                errs.push(format!("{label}: \"name\" is required"));
                continue;
            };
            let idx = new.categories.iter().position(|x| x.name == name);
            let before = idx.map(|i| new.categories[i].clone());
            let mut cat = before.clone().unwrap_or_else(|| Category::new(&name, "⭐", StreakStyle::Constellation));
            for (k, val) in &co {
                match k.as_str() {
                    "name" => {}
                    "icon" => match val.as_str().filter(|s| !s.trim().is_empty()) {
                        Some(s) => cat.icon = s.trim().to_string(),
                        None => errs.push(format!("{label}: \"icon\" must be text")),
                    },
                    "color" => match val {
                        Value::Null => cat.color = None,
                        Value::String(s) => match categories::parse_hex(s) {
                            Some(c) => cat.color = Some(categories::to_hex(c)),
                            None => errs.push(format!("{label}: \"{s}\" isn't a #RRGGBB color")),
                        },
                        _ => errs.push(format!("{label}: \"color\" must be \"#RRGGBB\" or null")),
                    },
                    "style" => match serde_json::from_value::<StreakStyle>(val.clone()) {
                        Ok(s) => cat.style = s,
                        Err(_) => errs.push(format!("{label}: \"style\" must be ribbon, comet, arcs, vine or constellation")),
                    },
                    "rename" => {
                        let to = val.as_str().map(categories::clean_name).unwrap_or_default();
                        if name == categories::DEFAULT_KIND {
                            errs.push(format!("{label}: \"task\" can't be renamed"));
                        } else if to.is_empty() || new.categories.iter().any(|x| x.name == to) {
                            errs.push(format!("{label}: can't rename to \"{to}\""));
                        } else {
                            if cat.color.is_none() {
                                cat.color = Some(categories::to_hex(crate::ui::kind_color(&name)));
                            }
                            renames.insert(name.clone(), to.clone());
                            lines.push(format!("~ rename category {name} → {to} ({} tasks move along)", count(&name)));
                            cat.name = to;
                        }
                    }
                    other => errs.push(format!("{label}: unknown field \"{other}\" (expected name, icon, color, style, rename)")),
                }
            }
            match (idx, before) {
                (Some(i), Some(b)) => {
                    let mut what = Vec::new();
                    if b.icon != cat.icon {
                        what.push(format!("icon {} → {}", b.icon, cat.icon));
                    }
                    if b.color != cat.color && !renames.contains_key(&b.name) {
                        let theme = th.kind(&b.name);
                        what.push(format!("color {} → {}", hex_or_theme(b.color.as_deref(), theme), hex_or_theme(cat.color.as_deref(), theme)));
                    }
                    if b.style != cat.style {
                        what.push(format!("style {} → {}", b.style.label().to_lowercase(), cat.style.label().to_lowercase()));
                    }
                    if !what.is_empty() {
                        lines.push(format!("~ category {}: {}", b.name, what.join(", ")));
                    }
                    new.categories[i] = cat;
                }
                _ => {
                    lines.push(format!(
                        "+ category {} {} · {} · {}",
                        cat.name,
                        cat.icon,
                        cat.style.label().to_lowercase(),
                        hex_or_theme(cat.color.as_deref(), th.kind(&cat.name))
                    ));
                    new.categories.push(cat);
                }
            }
        }
    }
    let mut deletes = HashSet::new();
    if let Some(d) = o.remove("delete_categories") {
        let Value::Array(arr) = d else { return Err(vec!["delete_categories: must be an array of names".into()]) };
        for v in arr {
            let name = v.as_str().map(categories::clean_name).unwrap_or_default();
            if name == categories::DEFAULT_KIND {
                errs.push("delete_categories: \"task\" can't be deleted".into());
            } else if let Some(i) = new.categories.iter().position(|c| c.name == name) {
                new.categories.remove(i);
                lines.push(format!("- delete category {name} ({} tasks become \"task\")", count(&name)));
                deletes.insert(name);
            } else {
                errs.push(format!("delete_categories: no category \"{name}\""));
            }
        }
    }
    for k in o.keys() {
        errs.push(format!("unknown setting \"{k}\""));
    }
    if !errs.is_empty() {
        return Err(errs);
    }
    if lines.is_empty() {
        lines.push("(settings unchanged)".into());
    }
    Ok(SettingsChange { new, renames, deletes, lines })
}

/// Current settings as Claude sees them: effective colors (yours or the theme's) and the
/// values it may change.
fn settings_context(data: &Data) -> Value {
    let th = crate::ui::theme::th();
    let s = &data.settings;
    let mut from_theme = Vec::new();
    if s.colors.calories.is_none() {
        from_theme.push("calories");
    }
    if s.colors.weight.is_none() {
        from_theme.push("weight");
    }
    let cats: Vec<Value> = s
        .categories
        .iter()
        .map(|c| {
            serde_json::json!({
                "name": c.name,
                "icon": c.icon,
                "color": categories::to_hex(crate::ui::kind_color(&c.name)),
                "color_from_theme": c.color.is_none(),
                "style": c.style,
            })
        })
        .collect();
    let mut v = serde_json::json!({
        "colors": {
            "calories": categories::to_hex(crate::ui::calorie_color()),
            "weight": categories::to_hex(crate::ui::weight_color()),
        },
        "colors_from_theme": from_theme,
        "categories": cats,
        "available_themes": crate::ui::theme::list(&data.paths.root).into_iter().map(|(stem, _)| stem).collect::<Vec<_>>(),
        "available_fonts": crate::ui::theme::FONTS.iter().map(|(v, _)| *v).collect::<Vec<_>>(),
        "theme_background": categories::to_hex(th.surface.page),
    });
    let all = serde_json::to_value(s).unwrap_or(Value::Null);
    for k in SIMPLE_SETTINGS {
        v[*k] = all[*k].clone();
    }
    v
}

/// Text to paste into an external AI chat: the format spec plus today's date and the
/// user's upcoming/backlog items (with ids) so it can plan around them and update them.
pub fn ai_context(data: &Data, days_ahead: i64) -> String {
    let now = Local::now();
    let today = now.date_naive();
    let until = today + Duration::days(days_ahead);
    let relevant: Vec<Value> = data
        .items
        .iter()
        .filter(|i| match i.date {
            None => !i.done,
            Some(d) => i.repeat.is_some() && i.until.is_none_or(|u| u >= today) || (d >= today && d <= until),
        })
        .map(|i| {
            let mut v = serde_json::to_value(i).unwrap_or(Value::Null);
            if let Value::Object(o) = &mut v {
                for k in ["created", "updated", "google_id", "google_synced", "done_dates", "sync_google"] {
                    o.remove(k);
                }
            }
            v
        })
        .collect();
    let todays_calories: Vec<&CalorieEntry> = data.calories.iter().filter(|c| c.date == today).collect();
    format!(
        "{IMPORT_FORMAT}\n\n---\n\n## Context\n\nToday is {} ({}), the time is {}. Time zone: {}. {}\n\nExisting items (repeating items, backlog, and the next {days_ahead} days):\n\n```json\n{}\n```\n\nCalories logged today ({} kcal total):\n\n```json\n{}\n```\n\nWhen I ask you to plan or change something, or tell you what I ate, answer with a Morrowkeep import JSON as described above.\n",
        today.format("%Y-%m-%d"),
        today.format("%A"),
        now.format("%H:%M"),
        iana_time_zone::get_timezone().unwrap_or_else(|_| "local".into()),
        {
            let mut t = String::new();
            if now.hour() < 5 {
                let ended = today - Duration::days(1);
                t += &format!(
                    "It's just past midnight: the day that just ended was {} ({}). When I talk about \"today\", \"tonight\" or \"this evening\" now, I most likely mean {}, so set \"date\" to it.\n\n",
                    ended.format("%Y-%m-%d"),
                    ended.format("%A"),
                    ended.format("%Y-%m-%d")
                );
            }
            t += &match data.weights.last() {
                Some(w) => format!("Latest weight: {:.1} kg on {}.", w.kg, w.date),
                None => "No weight logged yet.".to_string(),
            };
            if let Some(g) = &data.body.goal {
                t += &format!(" Goal: {:.1} kg by {}.", g.target_kg, g.target_date);
            }
            match data.daily_limit() {
                0 => {}
                k => t += &format!(" Daily calorie limit: {k} kcal."),
            }
            t += &format!("\n\nCategories (use these as \"kind\"): {}.", crate::categories::names().join(", "));
            if !data.foods.foods.is_empty() || !data.foods.meals.is_empty() {
                let lines: Vec<String> = data
                    .foods
                    .all()
                    .iter()
                    .take(300)
                    .map(|p| {
                        let kind = if p.is_meal { " (meal)" } else { "" };
                        format!("- {}{kind}: {:.0} kcal/100 g, 1 {} = {:.0} g = {} kcal", p.name, p.per100.kcal, p.serving_name, p.serving_g, p.kcal(Amount::Servings(1.0)))
                    })
                    .collect();
                t += &format!(
                    "\n\nMy food library (log these with \"food\" + \"servings\" or \"grams\", and Morrowkeep computes the calories):\n{}",
                    lines.join("\n")
                );
            }
            t += &format!(
                "\n\nCurrent settings (change them with a top-level \"settings\" object, see above):\n\n```json\n{}\n```",
                serde_json::to_string_pretty(&settings_context(data)).unwrap_or_default()
            );
            t
        },
        serde_json::to_string_pretty(&relevant).unwrap_or_default(),
        data.calories_on(today),
        serde_json::to_string_pretty(&todays_calories).unwrap_or_default()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_add_update_delete() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 10).unwrap();
        let mut existing = Item::new("Old");
        existing.id = "abc".into();
        let text = r#"```json
        {"morrowkeep_import":1,"items":[
            {"title":"Leg day","kind":"workout","date":"2026-10-12","time":"18:00","end":"19:15","repeat":"every mon and thu"},
            {"id":"abc","notes":"changed"}
        ],"delete":[]}
        ```"#;
        let p = plan(text, &[existing.clone()], &[], &Settings::default(), &Library::default(), today);
        assert!(p.errors.is_empty(), "{:?}", p.errors);
        assert_eq!(p.ops.len(), 2);
        match &p.ops[0] {
            Op::Add(i) => {
                assert_eq!(i.duration_min, Some(75));
                assert_eq!(i.repeat.as_deref(), Some("weekly mon,thu"));
            }
            _ => panic!(),
        }
        match &p.ops[1] {
            Op::Update(i) => assert_eq!((i.title.as_str(), i.notes.as_str()), ("Old", "changed")),
            _ => panic!(),
        }
        let raw_newline = plan("{\"items\":[{\"title\":\"Bowl\",\"notes\":\"line 1\nline 2\"}],\"message\":\"hi\"}", &[], &[], &Settings::default(), &Library::default(), today);
        assert!(raw_newline.errors.is_empty(), "{:?}", raw_newline.errors);
        assert_eq!(raw_newline.message.as_deref(), Some("hi"));
        let bad = plan(r#"[{"title":"x","date":"someday","colour":"red"}]"#, &[], &[], &Settings::default(), &Library::default(), today);
        assert_eq!(bad.errors.len(), 2, "{:?}", bad.errors);

        let cal = plan(
            r#"{"calories":[{"kcal":652.4,"description":"2 fried eggs ~240, toast + butter ~330, latte ~90","time":"08:30"},{"kcal":0}]}"#,
            &[],
            &[],
            &Settings::default(),
            &Library::default(),
            today,
        );
        assert_eq!(cal.errors.len(), 1, "{:?}", cal.errors); // kcal 0 is rejected
        let ok = plan(r#"{"calories":[{"kcal":652.4,"description":"eggs","date":"2026-10-09"}]}"#, &[], &[], &Settings::default(), &Library::default(), today);
        match &ok.ops[0] {
            Op::LogCalories(c) => assert_eq!((c.kcal, c.date.to_string(), c.time), (653, "2026-10-09".to_string(), None)),
            _ => panic!(),
        }
    }

    #[test]
    fn settings_changes_are_validated_and_described() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 10).unwrap();
        let mut chore = Item::new("Vacuum");
        chore.kind = "chore".into();
        let current = Settings::default();
        let text = r##"{"settings":{"colors":{"calories":"#ff9a50"},"quiet_start":"23:00",
            "categories":[{"name":"meal","icon":"☕","style":"vine"},{"name":"study","icon":"📚","color":"#3366FF"},{"name":"chore","rename":"chores"}],
            "delete_categories":["event"]}}"##;
        let p = plan(text, &[chore], &[], &current, &Library::default(), today);
        assert!(p.errors.is_empty(), "{:?}", p.errors);
        let Op::Settings(ch) = &p.ops[0] else { panic!() };
        assert_eq!(ch.new.colors.calories.as_deref(), Some("#FF9A50"));
        assert_eq!(ch.new.quiet_start, "23:00");
        let meal = ch.new.categories.iter().find(|c| c.name == "meal").unwrap();
        assert_eq!((meal.icon.as_str(), meal.style), ("☕", StreakStyle::Vine));
        assert!(ch.new.categories.iter().any(|c| c.name == "study" && c.color.as_deref() == Some("#3366FF")));
        assert!(ch.new.categories.iter().any(|c| c.name == "chores" && c.color.is_some())); // keeps its color
        assert!(!ch.new.categories.iter().any(|c| c.name == "event" || c.name == "chore"));
        assert_eq!(ch.renames.get("chore").map(String::as_str), Some("chores"));
        assert!(ch.lines.iter().any(|l| l.contains("rename category chore → chores (1 tasks")));
        // bad values are rejected with reasons, nothing applied
        let bad = plan(r##"{"settings":{"colors":{"calories":"orange","sky":"#000000"},"quiet_end":"late","google_client_secret":"x","delete_categories":["task"]}}"##, &[], &[], &current, &Library::default(), today);
        assert_eq!(bad.errors.len(), 5, "{:?}", bad.errors);
    }

    #[test]
    fn calories_from_the_food_library_and_saving_foods() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 11).unwrap();
        let lib = Library {
            version: 1,
            foods: vec![Food { id: "k".into(), name: "Kvarg".into(), kcal_per_100g: 62.0, serving_g: 250.0, serving_name: "jar".into(), protein_per_100g: Some(11.0), ..Default::default() }],
            meals: vec![],
        };
        let p = plan(r#"{"calories":[{"food":"kvarg","servings":2},{"food":"Kvarg","grams":100,"kcal":999}]}"#, &[], &[], &Settings::default(), &lib, today);
        assert!(p.errors.is_empty(), "{:?}", p.errors);
        let kcals: Vec<u32> = p.ops.iter().map(|o| if let Op::LogCalories(c) = o { c.kcal } else { 0 }).collect();
        assert_eq!(kcals, [310, 62]); // computed from the library, Claude's 999 ignored
        if let Op::LogCalories(c) = &p.ops[0] {
            assert_eq!((c.food.as_deref(), c.grams, c.protein), (Some("Kvarg"), Some(500.0), Some(55.0)));
        }
        // unknown food without kcal is an error; with kcal it's a normal entry
        let bad = plan(r#"{"calories":[{"food":"burrito"}]}"#, &[], &[], &Settings::default(), &lib, today);
        assert_eq!(bad.errors.len(), 1);
        let ok = plan(r#"{"calories":[{"food":"burrito","kcal":800}]}"#, &[], &[], &Settings::default(), &lib, today);
        assert!(ok.errors.is_empty());
        // saving foods: new, and a partial update of an existing one
        let f = plan(r#"{"foods":[{"name":"Oats","kcal_per_100g":370,"serving_g":40},{"name":"kvarg","serving_g":500}]}"#, &[], &[], &Settings::default(), &lib, today);
        assert!(f.errors.is_empty(), "{:?}", f.errors);
        if let Op::SaveFood(k) = &f.ops[1] {
            assert_eq!((k.id.as_str(), k.kcal_per_100g, k.serving_g), ("k", 62.0, 500.0));
        }
        let nf = plan(r#"{"foods":[{"name":"Mystery"}]}"#, &[], &[], &Settings::default(), &lib, today);
        assert_eq!(nf.errors.len(), 1); // a new food needs kcal
    }

    #[test]
    fn entries_for_another_day() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 15).unwrap();
        let yesterday = today - Duration::days(1);
        let mut task = Item::new("Vacuum");
        task.id = "v".into();
        let mut run = Item::new("Run");
        run.id = "r".into();
        run.date = Some(today - Duration::days(10));
        run.repeat = Some("daily".into());
        // a top-level date applies to everything that doesn't name its own
        let text = r#"{"date":"yesterday","journal":[{"text":"Quiet day"}],"calories":[{"kcal":500,"description":"pasta"}],
            "weight":[{"kg":82}],"items":[{"id":"v","done":true},{"id":"r","done":true}]}"#;
        let p = plan(text, &[task, run], &[], &Settings::default(), &Library::default(), today);
        assert!(p.errors.is_empty(), "{:?}", p.errors);
        for op in &p.ops {
            match op {
                Op::Journal(e) => assert_eq!((e.date, e.time), (yesterday, None)),
                Op::LogCalories(c) => assert_eq!((c.date, c.time), (yesterday, None)),
                Op::LogWeight(w) => assert_eq!(w.date, yesterday),
                Op::Update(i) if i.id == "v" => assert_eq!(i.done_on, Some(yesterday)),
                Op::Update(i) => assert!(i.is_done_on(yesterday) && !i.is_done_on(today)),
                _ => panic!("unexpected op"),
            }
        }
        // per-entry dates and times still win; no top-level date means today
        let p = plan(r#"{"journal":[{"text":"a","date":"2026-10-10","time":"23:40"},{"text":"b"}]}"#, &[], &[], &Settings::default(), &Library::default(), today);
        let Op::Journal(a) = &p.ops[0] else { panic!() };
        assert_eq!((a.date.to_string(), a.time.map(|t| t.to_string())), ("2026-10-10".into(), Some("23:40:00".into())));
        let Op::Journal(b) = &p.ops[1] else { panic!() };
        assert!(b.date == today && b.time.is_some());
    }
}
