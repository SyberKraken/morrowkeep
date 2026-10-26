//! The entry bar's Local mode, shared by the PC and the phone: one line of text becomes a food
//! from the library ("1 serving of kvarg"), a command ("450 kcal pasta", "82.4 kg", "done
//! vacuum", "journal: …", "goal 76 kg by feb 1") or, failing those, a new task.

use crate::model::Item;
use crate::parse::{parse_command, parse_quick, Command};
use crate::store::Data;
use chrono::NaiveDate;

pub enum Outcome {
    /// Saved; the message says what happened.
    Done(String),
    /// Food that isn't in the library ("ate a burrito"): Claude should estimate it.
    Claude(String),
    /// Nothing saved; the message says why.
    Error(String),
}

/// Runs one line of entry-bar text. `day` is the day entries go to (and new tasks without a
/// date of their own); `today` is the real date.
pub fn run(d: &mut Data, text: &str, day: NaiveDate, today: NaiveDate) -> Outcome {
    let text = text.trim();
    if text.is_empty() {
        return Outcome::Error("Type something first".into());
    }
    if let Some((p, amount)) = crate::foods::parse_log(text, &d.foods) {
        let e = Data::food_entry(&p, amount, day, "manual");
        let msg = format!("+{} kcal · {}", e.kcal, e.description);
        d.calories.push(e);
        d.save_calories();
        return Outcome::Done(msg);
    }
    match parse_command(text) {
        Some(Command::Calories { kcal, what }) => {
            d.calories.push(crate::model::CalorieEntry::new(day, kcal, what, "manual"));
            d.save_calories();
            Outcome::Done(format!("+{kcal} kcal · {} kcal {}", d.calories_on(day), if day == today { "today".to_string() } else { day.format("on %a %b %-d").to_string() }))
        }
        Some(Command::Weight(kg)) if (20.0..=400.0).contains(&kg) => Outcome::Done(set_weight(d, day, Some(kg), today)),
        Some(Command::Weight(kg)) => Outcome::Error(format!("{kg} kg doesn't look like a body weight")),
        Some(Command::Goal { kg, by }) => {
            let date = match &by {
                Some(t) => match crate::parse::parse_date(t, today).filter(|x| *x > today) {
                    Some(x) => x,
                    None => return Outcome::Error(format!("Couldn't read \"{t}\" as a future date")),
                },
                None => crate::body::default_target_date(d.current_weight().unwrap_or(kg), kg, today),
            };
            match crate::ui::body::set_goal(d, kg, date) {
                Ok(()) => Outcome::Done(format!("Goal: {kg:.1} kg by {}", date.format("%b %-d, %Y"))),
                Err(e) => Outcome::Error(e),
            }
        }
        Some(Command::Journal(t)) => {
            let mut e = crate::journal::JournalEntry::new(t.trim().to_string());
            if day != today {
                e.date = day;
                e.time = None;
            }
            d.journal.push(e);
            d.save_journal();
            Outcome::Done("Journal entry saved".into())
        }
        Some(Command::Done(q)) => match find_task(d, &q, day) {
            Some(i) => {
                if !d.items[i].is_done_on(day) {
                    d.items[i].toggle_done(day);
                    d.save_items();
                }
                Outcome::Done(format!("✔ {}", d.items[i].title))
            }
            None => Outcome::Error(format!("No task matching \"{q}\" {}", if day == today { "today".to_string() } else { day.format("on %a %b %-d").to_string() })),
        },
        Some(Command::Food(what)) => Outcome::Claude(what),
        None => {
            let p = parse_quick(text, today);
            if p.title.is_empty() {
                return Outcome::Error("Couldn't find a title in that".into());
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
            let msg = format!("Added \"{}\"{}", item.title, item.date.map(|x| x.format(" · %a %b %-d").to_string()).unwrap_or_else(|| " to the backlog".into()));
            d.items.push(item);
            d.save_items();
            Outcome::Done(msg)
        }
    }
}

/// Sets (or clears) a day's weight; the first weigh-in at or under the goal marks it reached.
pub fn set_weight(d: &mut Data, day: NaiveDate, kg: Option<f32>, today: NaiveDate) -> String {
    d.set_weight(day, kg);
    let mut msg = match kg {
        Some(kg) => format!("Weight {kg:.1} kg saved"),
        None => "Weight removed".into(),
    };
    if let (Some(kg), Some(g)) = (kg, d.body.goal.as_mut()) {
        if day == today && g.reached_on.is_none() && g.start_kg > g.target_kg && kg <= g.target_kg {
            g.reached_on = Some(today);
            msg = format!("🎉 Goal reached: {:.1} kg! The plan switches to maintenance.", g.target_kg);
            d.save_body();
        }
    }
    msg
}

/// The task best matching `query` for "done …": exact title, then title containing the words,
/// among the day's occurrences first, then any undone one-off.
pub fn find_task(d: &Data, query: &str, day: NaiveDate) -> Option<usize> {
    let q = query.trim().to_lowercase();
    let days: Vec<usize> = crate::model::occurrences(&d.items, day, day).into_iter().map(|(_, i)| i).collect();
    let others: Vec<usize> = (0..d.items.len()).filter(|i| d.items[*i].repeat.is_none() && !d.items[*i].done).collect();
    let matches = |i: &usize, exact: bool| {
        let t = d.items[*i].title.to_lowercase();
        if exact {
            t == q
        } else {
            q.split_whitespace().all(|w| t.contains(w))
        }
    };
    for exact in [true, false] {
        if let Some(i) = days.iter().find(|i| matches(i, exact) && !d.items[**i].is_done_on(day)) {
            return Some(*i);
        }
        if let Some(i) = others.iter().find(|i| matches(i, exact)) {
            return Some(*i);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_bar_lines() {
        // a fresh, empty data folder (the only test that loads Data)
        let dir = std::env::temp_dir().join(format!("mk-entry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("MORROWKEEP_DIR", &dir);
        let mut d = Data::load();
        let today = NaiveDate::from_ymd_opt(2026, 10, 26).unwrap();
        let yesterday = NaiveDate::from_ymd_opt(2026, 10, 25).unwrap();

        // a task, dated by its own words; without a date it goes to `day`
        assert!(matches!(run(&mut d, "dentist friday 10am", today, today), Outcome::Done(_)));
        assert_eq!(d.items[0].date, NaiveDate::from_ymd_opt(2026, 10, 30));
        assert!(matches!(run(&mut d, "water the plants", yesterday, today), Outcome::Done(_)));
        assert_eq!(d.items[1].date, Some(yesterday));

        // calories and weight for an earlier day
        assert!(matches!(run(&mut d, "450 kcal pasta", yesterday, today), Outcome::Done(_)));
        assert_eq!((d.calories_on(yesterday), d.calories_on(today)), (450, 0));
        assert!(matches!(run(&mut d, "82,4 kg", yesterday, today), Outcome::Done(_)));
        assert_eq!(d.weight_on(yesterday), Some(82.4));
        assert!(matches!(run(&mut d, "weight 500", today, today), Outcome::Error(_)));

        // "done …" ticks the matching task off on that day
        match run(&mut d, "done water plants", yesterday, today) {
            Outcome::Done(m) => assert!(m.contains("Water the plants"), "{m}"),
            _ => panic!("not ticked"),
        }
        assert!(d.items[1].is_done_on(yesterday));
        assert!(matches!(run(&mut d, "done skydiving", today, today), Outcome::Error(_)));

        // journal for an earlier day has no time; unknown food goes to Claude
        assert!(matches!(run(&mut d, "journal: quiet sunday", yesterday, today), Outcome::Done(_)));
        assert_eq!((d.journal[0].date, d.journal[0].time), (yesterday, None));
        assert!(matches!(run(&mut d, "ate a chicken burrito", today, today), Outcome::Claude(f) if f == "a chicken burrito"));
        assert!(matches!(run(&mut d, "   ", today, today), Outcome::Error(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
