//! The entry bar's Local mode, shared by the PC and the phone: one line of text becomes a food
//! from the library ("1 serving of kvarg"), a command ("450 kcal pasta", "82.4 kg", "done
//! vacuum", "journal: …", "goal 76 kg by feb 1") or, failing those, a new task.

use crate::store::Data;
use chrono::NaiveDate;
use morrowkeep_core::steps::{self, Planned};

pub enum Outcome {
    /// Saved; the message says what happened.
    Done(String),
    /// Food that isn't in the library ("ate a burrito"): Claude should estimate it.
    Claude(String),
    /// Nothing saved; the message says why.
    Error(String),
}

/// Runs one line of entry-bar text. `day` is the day entries go to (and new tasks without a
/// date of their own); `today` is the real date. The line becomes steps (shared with the phone,
/// see `morrowkeep_core::steps`) which are applied here.
pub fn run(d: &mut Data, text: &str, day: NaiveDate, today: NaiveDate) -> Outcome {
    match steps::plan_entry(text, day, today, &d.view()) {
        Planned::Claude(food) => Outcome::Claude(food),
        Planned::Error(e) => Outcome::Error(e),
        Planned::Steps(list) => {
            let mut msg = String::new();
            for st in &list {
                match steps::apply(d, st, today) {
                    Ok(m) => msg = m,
                    Err(e) => return Outcome::Error(e),
                }
            }
            Outcome::Done(msg)
        }
    }
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
