//! Steps: every change to your data as a small, ordered, serializable operation. The desktop app
//! applies them to its files; the phone applies the same steps to its own copy, so what the phone
//! shows while offline is exactly what the PC will do when the steps reach it.
//!
//! New things (tasks, calorie and journal entries, foods, meals, workout sessions) carry their
//! id from wherever the step was made, so later steps can refer to them: add task A, then tick A
//! off, works in one queue.

use crate::body::{Body, Goal};
use crate::foods::{Amount, Food, Library, Meal, Portionable};
use crate::journal::JournalEntry;
use crate::model::{CalorieEntry, Item, WeightEntry};
use crate::parse::{parse_command, parse_quick, Command};
use crate::workouts::{Session, WorkoutLog};
use chrono::{NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Step {
    TaskAdd { item: Item },
    /// The edit sheet's fields, all of them (what you saw is what's saved).
    TaskUpdate { id: String, title: String, date: Option<NaiveDate>, time: Option<NaiveTime>, kind: String, notes: String },
    /// Tick off (or un-tick) on `day`; a one-off task is simply done or not.
    TaskToggle { id: String, day: NaiveDate },
    /// Tick off on `day` if it isn't yet ("done vacuum" never un-ticks).
    TaskDone { id: String, day: NaiveDate },
    TaskDelete { id: String },
    CaloriesAdd { entry: CalorieEntry },
    /// An amount of a library food or meal; the calories are worked out when the step is applied
    /// (so a food changed earlier in the queue, or read from a label, counts with its new numbers).
    /// `id` is the new calorie entry's id.
    FoodLog { id: String, food: String, servings: Option<f32>, grams: Option<f32>, date: NaiveDate, time: Option<NaiveTime> },
    CaloriesDelete { id: String },
    WeightSet { day: NaiveDate, kg: Option<f32> },
    GoalSet { kg: f32, date: NaiveDate },
    JournalAdd { entry: JournalEntry },
    JournalUpdate { id: String, text: String, transcript: Option<String>, mood: Option<u8>, pinned: bool },
    JournalDelete { id: String },
    FoodSave { food: Food },
    MealSave { meal: Meal },
    /// An ingredient (also taken out of meals that use it) or a meal.
    FoodDelete { id: String },
    WorkoutSession { session: Session },
    // Steps only the PC can carry out (Claude, Whisper):
    /// Something for Claude, as typed; `food` = log what I ate.
    Claude { text: String, food: bool },
    /// A nutrition label photo (sent separately) to be read into the food `food_id`.
    Label { food_id: String },
    /// A voice journal entry; the recording is sent separately and transcribed on the PC.
    JournalAudio { entry: JournalEntry },
    /// "Reflect on my week" for the week ending on `day`.
    Reflect { day: NaiveDate },
}

impl Step {
    /// True for steps only the PC can carry out.
    pub fn needs_pc(&self) -> bool {
        matches!(self, Step::Claude { .. } | Step::Label { .. } | Step::JournalAudio { .. } | Step::Reflect { .. })
    }
}

/// Which file a step changed (the PC saves it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Items,
    Calories,
    Weight,
    Body,
    Journal,
    Foods,
    WorkoutLog,
}

/// Where steps are applied: the PC's data, or the phone's copy.
pub trait Store {
    fn items(&mut self) -> &mut Vec<Item>;
    fn calories(&mut self) -> &mut Vec<CalorieEntry>;
    fn weights(&mut self) -> &mut Vec<WeightEntry>;
    fn body(&mut self) -> &mut Body;
    fn journal(&mut self) -> &mut Vec<JournalEntry>;
    fn foods(&mut self) -> &mut Library;
    fn workout_log(&mut self) -> &mut WorkoutLog;
    /// The task a workout ticks off when finished.
    fn workout_task(&self, workout_id: &str) -> Option<String>;
    /// A part was changed (the PC saves that file).
    fn changed(&mut self, part: Part);
    /// A task was removed (the PC remembers its Google Calendar event, to delete it there).
    fn task_removed(&mut self, _item: &Item) {}
    /// A journal entry was removed (the PC moves its recording aside).
    fn journal_removed(&mut self, _entry: &JournalEntry) {}
}

/// Applies one step; returns what happened, or why it couldn't be done. Applying a step whose
/// new thing already exists (same id) does nothing, so applying twice is harmless.
pub fn apply(s: &mut impl Store, step: &Step, today: NaiveDate) -> Result<String, String> {
    match step {
        Step::TaskAdd { item } => {
            if s.items().iter().any(|i| i.id == item.id) {
                return Ok(format!("\"{}\" was already added", item.title));
            }
            s.items().push(item.clone());
            s.changed(Part::Items);
            Ok(format!("Added \"{}\"{}", item.title, item.date.map(|d| d.format(" · %a %b %-d").to_string()).unwrap_or_else(|| " to the backlog".into())))
        }
        Step::TaskUpdate { id, title, date, time, kind, notes } => {
            let title = title.trim();
            if title.is_empty() {
                return Err("The title can't be empty".into());
            }
            let it = task(s, id)?;
            it.title = title.to_string();
            it.date = *date;
            it.time = *time;
            if !kind.trim().is_empty() {
                it.kind = kind.trim().to_lowercase();
            }
            it.notes = notes.trim().to_string();
            it.touch();
            s.changed(Part::Items);
            Ok(format!("Saved \"{title}\""))
        }
        Step::TaskToggle { id, day } => {
            let it = task(s, id)?;
            it.toggle_done(*day);
            let msg = format!("{} {}", if it.is_done_on(*day) { "✔" } else { "Un-ticked" }, it.title);
            s.changed(Part::Items);
            Ok(msg)
        }
        Step::TaskDone { id, day } => {
            let it = task(s, id)?;
            let msg = format!("✔ {}", it.title);
            if !it.is_done_on(*day) {
                it.toggle_done(*day);
                s.changed(Part::Items);
            }
            Ok(msg)
        }
        Step::TaskDelete { id } => {
            let pos = s.items().iter().position(|i| i.id == *id).ok_or("That task isn't there any more")?;
            let item = s.items().remove(pos);
            s.task_removed(&item);
            s.changed(Part::Items);
            Ok(format!("Deleted \"{}\"", item.title))
        }
        Step::CaloriesAdd { entry } => {
            if !s.calories().iter().any(|c| c.id == entry.id) {
                s.calories().push(entry.clone());
                s.changed(Part::Calories);
            }
            let total: u32 = s.calories().iter().filter(|c| c.date == entry.date).map(|c| c.kcal).sum();
            let what = if entry.description.is_empty() { String::new() } else { format!(" · {}", entry.description) };
            Ok(format!("+{} kcal{what} · {total} kcal {}", entry.kcal, day_words(entry.date, today)))
        }
        Step::FoodLog { id, food, servings, grams, date, time } => {
            if let Some(e) = s.calories().iter().find(|c| c.id == *id) {
                return Ok(format!("+{} kcal · {}", e.kcal, e.description));
            }
            let p = s.foods().by_id(food).ok_or("That food isn't in your library any more")?;
            let amount = match (grams, servings) {
                (Some(g), _) if *g > 0.0 => Amount::Grams(*g),
                (_, Some(n)) if *n > 0.0 => Amount::Servings(*n),
                _ => Amount::Servings(1.0),
            };
            let mut e = food_entry(&p, amount, *date, "library");
            e.id = id.clone();
            e.time = *time;
            let msg = format!("+{} kcal · {}", e.kcal, e.description);
            s.calories().push(e);
            s.changed(Part::Calories);
            let total: u32 = s.calories().iter().filter(|c| c.date == *date).map(|c| c.kcal).sum();
            Ok(format!("{msg} · {total} kcal {}", day_words(*date, today)))
        }
        Step::CaloriesDelete { id } => {
            let pos = s.calories().iter().position(|c| c.id == *id).ok_or("That calorie entry isn't there any more")?;
            let e = s.calories().remove(pos);
            s.changed(Part::Calories);
            Ok(format!("Removed {} kcal", e.kcal))
        }
        Step::WeightSet { day, kg } => {
            if let Some(kg) = kg {
                if !(20.0..=400.0).contains(kg) {
                    return Err(format!("{kg} kg doesn't look like a body weight"));
                }
            }
            let w = s.weights();
            w.retain(|x| x.date != *day);
            if let Some(kg) = kg {
                w.push(WeightEntry { date: *day, kg: (kg * 10.0).round() / 10.0 });
                w.sort_by_key(|x| x.date);
            }
            s.changed(Part::Weight);
            let mut msg = match kg {
                Some(kg) => format!("Weight {kg:.1} kg saved"),
                None => "Weight removed".into(),
            };
            // the first weigh-in at or under the goal marks it reached
            if let (Some(kg), Some(g)) = (kg, s.body().goal.as_mut()) {
                if *day == today && g.reached_on.is_none() && g.start_kg > g.target_kg && *kg <= g.target_kg {
                    g.reached_on = Some(today);
                    msg = format!("🎉 Goal reached: {:.1} kg! The plan switches to maintenance.", g.target_kg);
                    s.changed(Part::Body);
                }
            }
            Ok(msg)
        }
        Step::GoalSet { kg, date } => {
            let now = crate::body::current_weight(s.weights()).ok_or("Log your weight first, so the goal has a starting point")?;
            set_goal(s.body(), now, *kg, *date, today)?;
            s.changed(Part::Body);
            Ok(format!("Goal: {kg:.1} kg by {}", date.format("%b %-d, %Y")))
        }
        Step::JournalAdd { entry } | Step::JournalAudio { entry } => {
            if !s.journal().iter().any(|e| e.id == entry.id) {
                s.journal().push(entry.clone());
                s.changed(Part::Journal);
            }
            Ok("Journal entry saved".into())
        }
        Step::JournalUpdate { id, text, transcript, mood, pinned } => {
            let e = s.journal().iter_mut().find(|e| e.id == *id).ok_or("That journal entry isn't there any more")?;
            if e.text != text.trim() || (e.audio.is_some() && transcript.as_deref().map(str::trim) != e.transcript.as_deref()) {
                e.edited = Some(crate::clock::now());
            }
            e.text = text.trim().to_string();
            if e.audio.is_some() {
                if let Some(t) = transcript {
                    e.transcript = Some(t.trim().to_string());
                }
            }
            e.mood = mood.filter(|m| (1..=5).contains(m));
            e.pinned = *pinned;
            s.changed(Part::Journal);
            Ok("Journal entry saved".into())
        }
        Step::JournalDelete { id } => {
            let pos = s.journal().iter().position(|e| e.id == *id).ok_or("That journal entry isn't there any more")?;
            let e = s.journal().remove(pos);
            s.journal_removed(&e);
            s.changed(Part::Journal);
            Ok("Journal entry deleted".into())
        }
        Step::FoodSave { food } => {
            if food.name.trim().is_empty() {
                return Err("Give it a name".into());
            }
            let lib = s.foods();
            match lib.foods.iter_mut().find(|f| f.id == food.id) {
                Some(f) => *f = food.clone(),
                None => lib.foods.push(food.clone()),
            }
            s.changed(Part::Foods);
            Ok(format!("{} saved to your foods", food.name))
        }
        Step::MealSave { meal } => {
            if meal.name.trim().is_empty() {
                return Err("Give it a name".into());
            }
            let lib = s.foods();
            if let Some(g) = meal.ingredients.iter().find(|g| lib.food(&g.food).is_none()) {
                return Err(format!("Ingredient {} isn't in your library", g.food));
            }
            match lib.meals.iter_mut().find(|m| m.id == meal.id) {
                Some(m) => *m = meal.clone(),
                None => lib.meals.push(meal.clone()),
            }
            s.changed(Part::Foods);
            Ok(format!("{} saved to your foods", meal.name))
        }
        Step::FoodDelete { id } => {
            let lib = s.foods();
            let name = if let Some(i) = lib.foods.iter().position(|f| f.id == *id) {
                let f = lib.foods.remove(i);
                for m in &mut lib.meals {
                    m.ingredients.retain(|g| g.food != f.id);
                }
                f.name
            } else if let Some(i) = lib.meals.iter().position(|m| m.id == *id) {
                lib.meals.remove(i).name
            } else {
                return Err("That food isn't in your library any more".into());
            };
            s.changed(Part::Foods);
            Ok(format!("Deleted {name}"))
        }
        Step::WorkoutSession { session } => {
            if s.workout_log().sessions.iter().any(|x| x.id == session.id) {
                return Ok(format!("Workout saved: {}", session.workout));
            }
            let (day, completed) = (session.date, session.completed);
            s.workout_log().sessions.push(session.clone());
            s.changed(Part::WorkoutLog);
            let mut msg = format!("Workout saved: {}", session.workout);
            // a finished workout ticks off its task for that day
            if completed {
                if let Some(id) = s.workout_task(&session.workout_id) {
                    if let Some(it) = s.items().iter_mut().find(|i| i.id == id) {
                        if !it.is_done_on(day) {
                            it.toggle_done(day);
                            msg = format!("Workout saved · ✔ {}", it.title);
                            s.changed(Part::Items);
                        }
                    }
                }
            }
            Ok(msg)
        }
        Step::Claude { .. } | Step::Label { .. } | Step::Reflect { .. } => Err("This needs Claude on your PC".into()),
    }
}

fn task<'a>(s: &'a mut impl Store, id: &str) -> Result<&'a mut Item, String> {
    s.items().iter_mut().find(|i| i.id == id).ok_or_else(|| "That task isn't there any more".to_string())
}

fn day_words(day: NaiveDate, today: NaiveDate) -> String {
    if day == today {
        "today".into()
    } else {
        day.format("on %a %b %-d").to_string()
    }
}

/// Sets (or updates) the goal. An active goal keeps its start so the ideal path stays put; a
/// reached goal is archived and a new one starts from `now_kg` (the current 7-day average).
pub fn set_goal(body: &mut Body, now_kg: f32, target_kg: f32, target_date: NaiveDate, today: NaiveDate) -> Result<(), String> {
    if !(30.0..=300.0).contains(&target_kg) {
        return Err(format!("{target_kg} kg doesn't look like a goal weight"));
    }
    match &mut body.goal {
        Some(g) if g.reached_on.is_none() => {
            g.target_kg = target_kg;
            g.target_date = target_date;
        }
        slot => {
            if let Some(old) = slot.take() {
                body.past_goals.push(old);
            }
            *slot = Some(Goal { target_kg, target_date, set_on: today, start_kg: now_kg, reached_on: None });
        }
    }
    Ok(())
}

/// The current time to the minute (what entries made "now" are stamped with).
pub fn now_hm() -> NaiveTime {
    use chrono::Timelike;
    let t = crate::clock::now().time();
    NaiveTime::from_hms_opt(t.hour(), t.minute(), 0).unwrap()
}

/// A calorie entry for an amount of a library food (calories rounded up, grams and macros kept).
pub fn food_entry(p: &Portionable, amount: Amount, date: NaiveDate, source: &str) -> CalorieEntry {
    let n = p.nutrition(amount);
    let mut e = CalorieEntry::new(date, p.kcal(amount), format!("{} · {}", p.name, p.describe(amount)), source);
    e.food = Some(p.name.clone());
    e.grams = Some((p.grams(amount) * 10.0).round() / 10.0);
    let r = |v: Option<f32>| v.map(|g| (g * 10.0).round() / 10.0);
    (e.protein, e.carbs, e.fat) = (r(n.protein), r(n.carbs), r(n.fat));
    e
}

// ---------------------------------------------------------------- the entry bar

/// What the data looks like, for turning entry-bar text into steps.
pub struct View<'a> {
    pub items: &'a [Item],
    pub foods: &'a Library,
    pub weights: &'a [WeightEntry],
}

/// What a line typed into the entry bar becomes.
#[derive(Debug, PartialEq)]
pub enum Planned {
    Steps(Vec<Step>),
    /// Food that isn't in the library ("ate a burrito"): Claude should estimate it.
    Claude(String),
    /// Nothing to do; the message says why.
    Error(String),
}

/// The entry bar's Local mode: a food from the library ("1 serving of kvarg"), a command ("450
/// kcal pasta", "82.4 kg", "done vacuum", "journal: …", "goal 76 kg by feb 1") or a new task.
/// `day` is the day entries go to (and new tasks without a date of their own); `today` is the
/// real date. New things get fresh ids here.
pub fn plan_entry(text: &str, day: NaiveDate, today: NaiveDate, v: &View) -> Planned {
    let text = text.trim();
    if text.is_empty() {
        return Planned::Error("Type something first".into());
    }
    if let Some((p, amount)) = crate::foods::parse_log(text, v.foods) {
        let (servings, grams) = match amount {
            Amount::Servings(n) => (Some(n), None),
            Amount::Grams(g) => (None, Some(g)),
        };
        let time = (day == today).then(|| now_hm());
        return Planned::Steps(vec![Step::FoodLog { id: crate::clock::new_id(), food: p.id, servings, grams, date: day, time }]);
    }
    let step = match parse_command(text) {
        Some(Command::Calories { kcal, what }) => Step::CaloriesAdd { entry: CalorieEntry::new(day, kcal, what, "manual") },
        Some(Command::Weight(kg)) if (20.0..=400.0).contains(&kg) => Step::WeightSet { day, kg: Some(kg) },
        Some(Command::Weight(kg)) => return Planned::Error(format!("{kg} kg doesn't look like a body weight")),
        Some(Command::Goal { kg, by }) => {
            let date = match &by {
                Some(t) => match crate::parse::parse_date(t, today).filter(|x| *x > today) {
                    Some(x) => x,
                    None => return Planned::Error(format!("Couldn't read \"{t}\" as a future date")),
                },
                None => crate::body::default_target_date(crate::body::current_weight(v.weights).unwrap_or(kg), kg, today),
            };
            Step::GoalSet { kg, date }
        }
        Some(Command::Journal(t)) => {
            let mut e = JournalEntry::new(t.trim().to_string());
            if day != today {
                e.date = day;
                e.time = None;
            }
            Step::JournalAdd { entry: e }
        }
        Some(Command::Done(q)) => match find_task(v.items, &q, day) {
            Some(i) => Step::TaskDone { id: v.items[i].id.clone(), day },
            None => return Planned::Error(format!("No task matching \"{q}\" {}", day_words(day, today))),
        },
        Some(Command::Food(what)) => return Planned::Claude(what),
        None => {
            let p = parse_quick(text, today);
            if p.title.is_empty() {
                return Planned::Error("Couldn't find a title in that".into());
            }
            let mut item = Item::new(p.title);
            if let Some(k) = p.kind {
                item.kind = k;
            }
            item.date = if p.someday { None } else { p.date.or(Some(day)) };
            item.time = p.time;
            item.duration_min = p.duration_min;
            item.repeat = p.repeat;
            item.until = p.until;
            Step::TaskAdd { item }
        }
    };
    Planned::Steps(vec![step])
}

/// The task best matching `query` for "done …": exact title, then title containing the words,
/// among the day's occurrences first, then any undone one-off.
pub fn find_task(items: &[Item], query: &str, day: NaiveDate) -> Option<usize> {
    let q = query.trim().to_lowercase();
    let days: Vec<usize> = crate::model::occurrences(items, day, day).into_iter().map(|(_, i)| i).collect();
    let others: Vec<usize> = (0..items.len()).filter(|i| items[*i].repeat.is_none() && !items[*i].done).collect();
    let matches = |i: &usize, exact: bool| {
        let t = items[*i].title.to_lowercase();
        if exact {
            t == q
        } else {
            q.split_whitespace().all(|w| t.contains(w))
        }
    };
    for exact in [true, false] {
        if let Some(i) = days.iter().find(|i| matches(i, exact) && !items[**i].is_done_on(day)) {
            return Some(*i);
        }
        if let Some(i) = others.iter().find(|i| matches(i, exact)) {
            return Some(*i);
        }
    }
    None
}

// ---------------------------------------------------------------- a plain store

/// Your data as plain values: the phone's copy, and tests.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    pub items: Vec<Item>,
    pub calories: Vec<CalorieEntry>,
    pub weights: Vec<WeightEntry>,
    pub body: Body,
    pub journal: Vec<JournalEntry>,
    pub foods: Library,
    pub workout_log: WorkoutLog,
    /// workout id → the task it ticks off
    pub workout_tasks: Vec<(String, String)>,
}

impl Store for Snapshot {
    fn items(&mut self) -> &mut Vec<Item> {
        &mut self.items
    }
    fn calories(&mut self) -> &mut Vec<CalorieEntry> {
        &mut self.calories
    }
    fn weights(&mut self) -> &mut Vec<WeightEntry> {
        &mut self.weights
    }
    fn body(&mut self) -> &mut Body {
        &mut self.body
    }
    fn journal(&mut self) -> &mut Vec<JournalEntry> {
        &mut self.journal
    }
    fn foods(&mut self) -> &mut Library {
        &mut self.foods
    }
    fn workout_log(&mut self) -> &mut WorkoutLog {
        &mut self.workout_log
    }
    fn workout_task(&self, workout_id: &str) -> Option<String> {
        self.workout_tasks.iter().find(|(w, _)| w == workout_id).map(|(_, t)| t.clone())
    }
    fn changed(&mut self, _part: Part) {}
}

impl Snapshot {
    pub fn view(&self) -> View<'_> {
        View { items: &self.items, foods: &self.foods, weights: &self.weights }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, m, day).unwrap()
    }

    fn steps(p: Planned) -> Vec<Step> {
        match p {
            Planned::Steps(s) => s,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn add_then_tick_off_then_delete_in_one_queue() {
        let today = d(11, 10);
        let mut s = Snapshot::default();
        let add = steps(plan_entry("call mum tomorrow 7pm", today, today, &s.view()));
        let Step::TaskAdd { item } = &add[0] else { panic!() };
        let id = item.id.clone();
        assert_eq!((item.date, item.time), (Some(d(11, 11)), NaiveTime::from_hms_opt(19, 0, 0)));
        // the queue: add, tick off (made before the add reached the PC), edit, delete
        let queue = vec![
            add[0].clone(),
            Step::TaskToggle { id: id.clone(), day: d(11, 11) },
            Step::TaskUpdate { id: id.clone(), title: "Call mum about Sunday".into(), date: Some(d(11, 11)), time: None, kind: "task".into(), notes: String::new() },
        ];
        for st in &queue {
            apply(&mut s, st, today).unwrap();
        }
        assert!(s.items[0].is_done_on(d(11, 11)));
        assert_eq!((s.items[0].title.as_str(), s.items[0].time), ("Call mum about Sunday", None));
        // applying the add again does nothing
        apply(&mut s, &queue[0], today).unwrap();
        assert_eq!(s.items.len(), 1);
        apply(&mut s, &Step::TaskDelete { id: id.clone() }, today).unwrap();
        assert!(s.items.is_empty());
        // a later step on the deleted task fails with a reason
        assert_eq!(apply(&mut s, &Step::TaskToggle { id, day: today }, today).unwrap_err(), "That task isn't there any more");
    }

    #[test]
    fn entry_lines_become_steps() {
        let today = d(11, 10);
        let yesterday = d(11, 9);
        let mut s = Snapshot::default();
        for st in steps(plan_entry("200 kcal", yesterday, today, &s.view())) {
            apply(&mut s, &st, today).unwrap();
        }
        assert_eq!((s.calories.len(), s.calories[0].date, s.calories[0].kcal), (1, yesterday, 200));
        for st in steps(plan_entry("82,4 kg", today, today, &s.view())) {
            apply(&mut s, &st, today).unwrap();
        }
        assert_eq!(s.weights, vec![WeightEntry { date: today, kg: 82.4 }]);
        assert!(matches!(plan_entry("weight 500", today, today, &s.view()), Planned::Error(_)));
        assert_eq!(plan_entry("ate a chicken burrito", today, today, &s.view()), Planned::Claude("a chicken burrito".into()));
        // "done …" finds the task and only ticks it off
        for st in steps(plan_entry("water the plants", yesterday, today, &s.view())) {
            apply(&mut s, &st, today).unwrap();
        }
        let done = steps(plan_entry("done water plants", yesterday, today, &s.view()));
        apply(&mut s, &done[0], today).unwrap();
        apply(&mut s, &done[0], today).unwrap();
        assert!(s.items[0].is_done_on(yesterday));
        assert!(matches!(plan_entry("done skydiving", today, today, &s.view()), Planned::Error(_)));
        // journal for an earlier day has no time
        for st in steps(plan_entry("journal: quiet sunday", yesterday, today, &s.view())) {
            apply(&mut s, &st, today).unwrap();
        }
        assert_eq!((s.journal[0].date, s.journal[0].time), (yesterday, None));
        // a goal needs a weight first, which we have
        for st in steps(plan_entry("goal 76 kg by feb 1", today, today, &s.view())) {
            apply(&mut s, &st, today).unwrap();
        }
        assert_eq!(s.body.goal.as_ref().map(|g| (g.target_kg, g.target_date)), Some((76.0, NaiveDate::from_ymd_opt(2027, 2, 1).unwrap())));
    }

    #[test]
    fn foods_meals_and_workouts() {
        let today = d(11, 10);
        let mut s = Snapshot::default();
        let oats = Food { id: "oats".into(), name: "Oats".into(), kcal_per_100g: 370.0, serving_g: 40.0, serving_name: "bowl".into(), ..Default::default() };
        apply(&mut s, &Step::FoodSave { food: oats }, today).unwrap();
        let meal = Meal { id: "m".into(), name: "Porridge".into(), ingredients: vec![crate::foods::Ingredient { food: "oats".into(), grams: 80.0 }], servings: 2.0, ..Default::default() };
        apply(&mut s, &Step::MealSave { meal }, today).unwrap();
        // "1 bowl oats" uses the library's numbers
        let st = steps(plan_entry("1 bowl oats", today, today, &s.view()));
        apply(&mut s, &st[0], today).unwrap();
        assert_eq!((s.calories[0].kcal, s.calories[0].grams), (148, Some(40.0)));
        // a food changed before the log is counted with its new numbers
        let mut oats2 = s.foods.foods[0].clone();
        oats2.kcal_per_100g = 400.0;
        apply(&mut s, &Step::FoodSave { food: oats2 }, today).unwrap();
        let log = Step::FoodLog { id: "e2".into(), food: "oats".into(), servings: Some(1.0), grams: None, date: today, time: None };
        apply(&mut s, &log, today).unwrap();
        apply(&mut s, &log, today).unwrap();
        assert_eq!((s.calories.len(), s.calories[1].kcal), (2, 160));
        // deleting the ingredient takes it out of the meal
        apply(&mut s, &Step::FoodDelete { id: "oats".into() }, today).unwrap();
        assert!(s.foods.meals[0].ingredients.is_empty());
        // a finished workout ticks off its task
        let mut gym = Item::new("Gym");
        gym.date = Some(today);
        s.items.push(gym.clone());
        s.workout_tasks.push(("w1".into(), gym.id.clone()));
        let session = Session { id: "s1".into(), date: today, start: "18:00".into(), duration_s: 1800, workout_id: "w1".into(), workout: "Legs".into(), sets: vec![], completed: true };
        assert!(apply(&mut s, &Step::WorkoutSession { session }, today).unwrap().contains("✔ Gym"));
        assert!(s.items[0].is_done_on(today));
    }

    #[test]
    fn steps_round_trip_as_json() {
        let st = Step::TaskToggle { id: "a".into(), day: d(11, 10) };
        let json = serde_json::to_string(&st).unwrap();
        assert_eq!(json, r#"{"type":"task_toggle","id":"a","day":"2026-11-10"}"#);
        assert_eq!(serde_json::from_str::<Step>(&json).unwrap(), st);
        assert!(Step::Claude { text: "x".into(), food: true }.needs_pc());
    }
}
