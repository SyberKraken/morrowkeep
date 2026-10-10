//! Importing items from JSON (pasted, or dropped into the inbox folder). See IMPORT_FORMAT.md.

use crate::model::{CalorieEntry, Item, Repeat, WeightEntry};
use crate::parse::{parse_date, parse_duration, parse_time};
use crate::store::{Data, IMPORT_FORMAT};
use chrono::{Duration, Local, NaiveDate};
use serde_json::{Map, Value};
use std::fs;

pub enum Op {
    Add(Item),
    Update(Item),
    Delete { id: String, title: String },
    LogCalories(CalorieEntry),
    LogWeight(WeightEntry),
    DeleteCalories { id: String, label: String },
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

pub fn plan(text: &str, existing: &[Item], calories: &[CalorieEntry], today: NaiveDate) -> Plan {
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
    let (items, deletes) = match value {
        Value::Array(a) => (a, Vec::new()),
        Value::Object(mut o) => {
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
            o.remove("morrowkeep_import");
            o.remove("lifehub_import"); // the app's previous name
            if let Some(Value::String(m)) = o.remove("message") {
                plan.message = Some(m);
            }
            for k in o.keys() {
                plan.errors.push(format!("unknown top-level field \"{k}\" (expected \"items\", \"calories\", \"weight\", \"delete\", \"message\")"));
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
        match item_from(obj, existing, today) {
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
        match calories_from(obj, today) {
            Ok(c) => plan.ops.push(Op::LogCalories(c)),
            Err(errs) => plan.errors.extend(errs.into_iter().map(|e| format!("{label}: {e}"))),
        }
    }
    for (n, v) in weight_entries.into_iter().enumerate() {
        let label = format!("weight {}", n + 1);
        let kg = v.get("kg").and_then(Value::as_f64);
        let date = match v.get("date").and_then(Value::as_str) {
            None => Some(today),
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

fn calories_from(o: Map<String, Value>, today: NaiveDate) -> Result<CalorieEntry, Vec<String>> {
    let mut errs = Vec::new();
    let mut kcal = None;
    let mut date = today;
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
            "description" | "food" | "notes" => match s {
                Some(d) => description = d.to_string(),
                None => errs.push("\"description\" must be text".into()),
            },
            other => errs.push(format!("unknown field \"{other}\"")),
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

fn item_from(mut o: Map<String, Value>, existing: &[Item], today: NaiveDate) -> Result<Op, Vec<String>> {
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
                Some(b) => item.done = b,
                None => errs.push("\"done\" must be true or false".into()),
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
        let p = plan(&text, &data.items, &data.calories, today);
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
        match data.weights.last() {
            Some(w) => format!("Latest weight: {:.1} kg on {}.", w.kg, w.date),
            None => "No weight logged yet.".to_string(),
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
        let p = plan(text, &[existing.clone()], &[], today);
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
        let raw_newline = plan("{\"items\":[{\"title\":\"Bowl\",\"notes\":\"line 1\nline 2\"}],\"message\":\"hi\"}", &[], &[], today);
        assert!(raw_newline.errors.is_empty(), "{:?}", raw_newline.errors);
        assert_eq!(raw_newline.message.as_deref(), Some("hi"));
        let bad = plan(r#"[{"title":"x","date":"someday","colour":"red"}]"#, &[], &[], today);
        assert_eq!(bad.errors.len(), 2, "{:?}", bad.errors);

        let cal = plan(
            r#"{"calories":[{"kcal":652.4,"description":"2 fried eggs ~240, toast + butter ~330, latte ~90","time":"08:30"},{"kcal":0}]}"#,
            &[],
            &[],
            today,
        );
        assert_eq!(cal.errors.len(), 1, "{:?}", cal.errors); // kcal 0 is rejected
        let ok = plan(r#"{"calories":[{"kcal":652.4,"description":"eggs","date":"2026-10-09"}]}"#, &[], &[], today);
        match &ok.ops[0] {
            Op::LogCalories(c) => assert_eq!((c.kcal, c.date.to_string(), c.time), (653, "2026-10-09".to_string(), None)),
            _ => panic!(),
        }
    }
}
