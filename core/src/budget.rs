//! The daily calorie limit and the month's budget: from the Body plan when the profile is
//! complete (else the fallback limit), and the recommended calories for a day (what's left of the
//! month's budget spread over the days left). Shared so the phone works them out offline too.

use crate::body::{Body, Numbers, Plan};
use crate::model::{CalorieEntry, WeightEntry};
use chrono::{Datelike, NaiveDate};

/// Profile numbers and the plan for `day`, if the profile is complete and a weight is logged.
pub fn body_plan(body: &Body, weights: &[WeightEntry], day: NaiveDate) -> Option<(Numbers, Plan)> {
    let n = body.numbers(crate::body::current_weight(weights)?, day)?;
    let p = body.plan(&n, day);
    Some((n, p))
}

/// The daily calorie limit (0 = none): the plan's when the Body profile is complete, otherwise
/// the fallback limit; and whether it came from the plan.
pub fn daily_limit(body: &Body, weights: &[WeightEntry], today: NaiveDate) -> (u32, bool) {
    match body_plan(body, weights, today) {
        Some((_, p)) => (p.daily, true),
        None => (body.fallback_daily_limit, false),
    }
}

pub fn days_in_month(day: NaiveDate) -> u32 {
    let first = day.with_day(1).unwrap();
    let next = if day.month() == 12 { NaiveDate::from_ymd_opt(day.year() + 1, 1, 1) } else { NaiveDate::from_ymd_opt(day.year(), day.month() + 1, 1) }.unwrap();
    (next - first).num_days() as u32
}

/// Logged kcal per day of the month containing `day` (index 0 = the 1st); `None` for days with
/// no entries at all.
pub fn month_days(calories: &[CalorieEntry], day: NaiveDate) -> Vec<Option<u32>> {
    let mut days = vec![None; days_in_month(day) as usize];
    for c in calories {
        if c.date.year() == day.year() && c.date.month() == day.month() {
            let slot = &mut days[c.date.day0() as usize];
            *slot = Some(slot.unwrap_or(0) + c.kcal);
        }
    }
    days
}

#[derive(Default, Debug, PartialEq)]
pub struct MonthTally {
    /// kcal actually logged this month.
    pub logged: u32,
    /// Past days with no entries, counted at the daily limit.
    pub unlogged_past_days: u32,
    /// logged + unlogged_past_days × limit: what counts against the budget.
    pub counted: u32,
}

/// How the month containing `day` counts against its budget, as of `today`. Past days with
/// nothing logged count as the daily limit: "no food entered", not "ate nothing".
pub fn month_tally(calories: &[CalorieEntry], limit: u32, day: NaiveDate, today: NaiveDate) -> MonthTally {
    let mut t = MonthTally::default();
    for (i, kcal) in month_days(calories, day).into_iter().enumerate() {
        let date = day.with_day(i as u32 + 1).unwrap();
        match kcal {
            Some(k) => {
                t.logged += k;
                t.counted += k;
            }
            None if date < today && limit > 0 => {
                t.unlogged_past_days += 1;
                t.counted += limit;
            }
            None => {}
        }
    }
    t
}

/// Recommended calories for `day`: what's left of the month's budget after the earlier days of
/// the month, spread evenly over the days left (including `day`). Earlier days with nothing
/// logged count as exactly the daily limit. None without a limit.
pub fn recommended_for(calories: &[CalorieEntry], limit: u32, day: NaiveDate) -> Option<u32> {
    if limit == 0 {
        return None;
    }
    let budget = (limit * days_in_month(day)) as i64;
    let before = &month_days(calories, day)[..day.day0() as usize];
    let days_left = (days_in_month(day) - day.day0()) as i64;
    Some(spread_budget(budget, counted(before, limit), days_left))
}

/// Sum of days' kcal, counting days with nothing logged as the daily limit.
pub fn counted(days: &[Option<u32>], limit: u32) -> i64 {
    days.iter().map(|d| d.unwrap_or(limit) as i64).sum()
}

/// What's left of a budget, split evenly over the remaining days (never below 0).
pub fn spread_budget(budget: i64, eaten_before: i64, days_left: i64) -> u32 {
    ((budget - eaten_before) / days_left.max(1)).max(0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overeating_lowers_the_following_days() {
        // 31-day month, 2000/day limit, nothing logged until the 29th (3000 kcal).
        // Unlogged days count as 2000, so on the 30th (2 days left): (62000 - 59000) / 2 = 1500.
        let budget = 2000 * 31;
        let mut before = vec![None; 28];
        before.push(Some(3000));
        assert_eq!(counted(&before, 2000), 59000);
        assert_eq!(spread_budget(budget, counted(&before, 2000), 2), 1500);
        // A logged day of 0 kcal is still 0, not the limit.
        assert_eq!(counted(&[Some(0), None], 2000), 2000);
        // Way over budget never goes negative.
        assert_eq!(spread_budget(budget, 70000, 2), 0);
        let d = NaiveDate::from_ymd_opt(2026, 10, 10).unwrap();
        assert_eq!(days_in_month(d), 31);
        assert_eq!(days_in_month(NaiveDate::from_ymd_opt(2028, 2, 3).unwrap()), 29);
        assert_eq!(days_in_month(NaiveDate::from_ymd_opt(2026, 12, 31).unwrap()), 31);
        // the same through recommended_for: 3000 kcal on Oct 29, asking for Oct 30
        let e = CalorieEntry::new(NaiveDate::from_ymd_opt(2026, 10, 29).unwrap(), 3000, "", "test");
        assert_eq!(recommended_for(&[e], 2000, NaiveDate::from_ymd_opt(2026, 10, 30).unwrap()), Some(1500));
        assert_eq!(recommended_for(&[], 0, d), None);
    }
}
