//! Core data types: items (tasks, chores, meals, workouts...), repeat rules and
//! read-only events pulled from Google Calendar.

use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime, Timelike, Weekday};
use serde::{Deserialize, Serialize};


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub title: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    /// First (or only) day. `None` = backlog item with no date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<NaiveDate>,
    /// Start time. `None` = any time that day.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "hm_opt")]
    pub time: Option<NaiveTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_min: Option<u32>,
    /// Repeat rule in plain text, e.g. "daily", "weekly mon,thu", "every 2 weeks".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat: Option<String>,
    /// Last day a repeating item can occur (inclusive).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Done flag for non-repeating items.
    #[serde(default, skip_serializing_if = "is_false")]
    pub done: bool,
    /// When a non-repeating item was completed (for history views).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_on: Option<NaiveDate>,
    /// Completed occurrences of repeating items.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub done_dates: Vec<NaiveDate>,
    #[serde(default = "yes")]
    pub sync_google: bool,
    pub created: DateTime<Local>,
    pub updated: DateTime<Local>,
    /// Google Calendar event id of the mirrored event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub google_id: Option<String>,
    /// Value of `updated` when the item was last pushed to Google.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub google_synced: Option<DateTime<Local>>,
}

fn default_kind() -> String {
    "task".into()
}
fn yes() -> bool {
    true
}
fn is_false(b: &bool) -> bool {
    !*b
}

impl Item {
    pub fn new(title: impl Into<String>) -> Self {
        let now = Local::now();
        Item {
            id: uuid::Uuid::new_v4().to_string(),
            title: title.into(),
            kind: default_kind(),
            date: None,
            time: None,
            duration_min: None,
            repeat: None,
            until: None,
            notes: String::new(),
            tags: Vec::new(),
            done: false,
            done_on: None,
            done_dates: Vec::new(),
            sync_google: true,
            created: now,
            updated: now,
            google_id: None,
            google_synced: None,
        }
    }

    pub fn touch(&mut self) {
        self.updated = Local::now();
    }

    pub fn repeat_rule(&self) -> Option<Repeat> {
        self.repeat.as_deref().and_then(|r| Repeat::parse(r).ok())
    }

    pub fn is_done_on(&self, day: NaiveDate) -> bool {
        if self.repeat.is_some() {
            self.done_dates.contains(&day)
        } else {
            self.done
        }
    }

    pub fn toggle_done(&mut self, day: NaiveDate) {
        if self.repeat.is_some() {
            if let Some(pos) = self.done_dates.iter().position(|d| *d == day) {
                self.done_dates.remove(pos);
            } else {
                self.done_dates.push(day);
                self.done_dates.sort();
            }
            // Not touched: occurrence check-offs aren't mirrored to Google, so no re-push.
        } else {
            self.set_done(!self.done);
        }
    }

    /// Marks a non-repeating item done or not, recording when it was completed.
    pub fn set_done(&mut self, done: bool) {
        if done && !self.done {
            self.done_on = Some(Local::now().date_naive());
        } else if !done {
            self.done_on = None;
        }
        self.done = done;
        self.touch();
    }

    /// Day a non-repeating item was completed. Items finished before completion dates were
    /// recorded fall back to their scheduled day.
    pub fn completed_on(&self) -> Option<NaiveDate> {
        (self.repeat.is_none() && self.done).then(|| self.done_on.or(self.date)).flatten()
    }

    pub fn end_time(&self) -> Option<NaiveTime> {
        self.time
            .map(|t| t + chrono::Duration::minutes(self.duration_min.unwrap_or(30) as i64))
    }
}

/// Days in `[from, to]` on which each item occurs, as `(day, item index)` sorted by day.
pub fn occurrences(items: &[Item], from: NaiveDate, to: NaiveDate) -> Vec<(NaiveDate, usize)> {
    let mut out = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        let Some(start) = item.date else { continue };
        match item.repeat_rule() {
            None => {
                if start >= from && start <= to {
                    out.push((start, idx));
                }
            }
            Some(rule) => {
                let mut d = from.max(start);
                let last = item.until.map_or(to, |u| u.min(to));
                while d <= last {
                    if rule.occurs_on(start, d) {
                        out.push((d, idx));
                    }
                    d = d.succ_opt().unwrap();
                }
            }
        }
    }
    out.sort_by_key(|(d, idx)| (*d, items[*idx].time, *idx));
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repeat {
    pub freq: Freq,
    pub interval: u32,
    /// Only for weekly rules; empty = the start date's weekday.
    pub days: Vec<Weekday>,
}

pub fn parse_weekday(s: &str) -> Option<Weekday> {
    let s = s.trim_end_matches('s');
    Some(match s {
        "mon" | "monday" => Weekday::Mon,
        "tue" | "tues" | "tuesday" => Weekday::Tue,
        "wed" | "weds" | "wednesday" => Weekday::Wed,
        "thu" | "thur" | "thurs" | "thursday" => Weekday::Thu,
        "fri" | "friday" => Weekday::Fri,
        "sat" | "saturday" => Weekday::Sat,
        "sun" | "sunday" => Weekday::Sun,
        _ => return None,
    })
}

pub fn small_number(s: &str) -> Option<u32> {
    if let Ok(n) = s.parse() {
        return Some(n);
    }
    const WORDS: [&str; 13] = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
        "eleven", "twelve",
    ];
    match s {
        "a" | "an" => Some(1),
        "other" => Some(2),
        _ => WORDS.iter().position(|w| *w == s).map(|n| n as u32),
    }
}

impl Repeat {
    /// Parses rules like "daily", "weekdays", "weekly mon,thu", "every 2 weeks on fri",
    /// "every monday and thursday", "every 3 days", "monthly", "yearly".
    pub fn parse(input: &str) -> Result<Repeat, String> {
        let lower = input.to_lowercase().replace([',', '/', '.'], " ");
        let mut interval = None;
        let mut freq = None;
        let mut days: Vec<Weekday> = Vec::new();
        for tok in lower.split_whitespace() {
            match tok {
                "every" | "on" | "and" | "the" | "each" | "repeat" | "repeats" => {}
                "daily" | "day" | "days" => freq = Some(Freq::Daily),
                "weekly" | "week" | "weeks" => freq = Some(Freq::Weekly),
                "monthly" | "month" | "months" => freq = Some(Freq::Monthly),
                "yearly" | "annually" | "year" | "years" => freq = Some(Freq::Yearly),
                "weekday" | "weekdays" => days.extend([
                    Weekday::Mon,
                    Weekday::Tue,
                    Weekday::Wed,
                    Weekday::Thu,
                    Weekday::Fri,
                ]),
                "weekend" | "weekends" => days.extend([Weekday::Sat, Weekday::Sun]),
                "biweekly" | "fortnightly" => {
                    freq = Some(Freq::Weekly);
                    interval = Some(2);
                }
                t => {
                    if let Some(wd) = parse_weekday(t) {
                        days.push(wd);
                    } else if let Some(n) = small_number(t).filter(|n| *n > 0) {
                        interval = Some(n);
                    } else {
                        return Err(format!("don't understand '{t}' in repeat rule '{input}'"));
                    }
                }
            }
        }
        if !days.is_empty() {
            match freq {
                None | Some(Freq::Weekly) => freq = Some(Freq::Weekly),
                // "every day" + weekdays is just weekly on those days
                Some(Freq::Daily) => freq = Some(Freq::Weekly),
                Some(_) => return Err(format!("weekdays only work with weekly rules: '{input}'")),
            }
        }
        let freq = freq.ok_or_else(|| format!("repeat rule '{input}' needs daily/weekly/monthly/yearly or weekdays"))?;
        days.sort_by_key(|d| d.num_days_from_monday());
        days.dedup();
        Ok(Repeat { freq, interval: interval.unwrap_or(1), days })
    }

    pub fn occurs_on(&self, start: NaiveDate, d: NaiveDate) -> bool {
        if d < start {
            return false;
        }
        let n = self.interval.max(1) as i64;
        match self.freq {
            Freq::Daily => (d - start).num_days() % n == 0,
            Freq::Weekly => {
                let on_day = if self.days.is_empty() {
                    d.weekday() == start.weekday()
                } else {
                    self.days.contains(&d.weekday())
                };
                let week = |x: NaiveDate| x - chrono::Duration::days(x.weekday().num_days_from_monday() as i64);
                on_day && ((week(d) - week(start)).num_days() / 7) % n == 0
            }
            Freq::Monthly => {
                let months = (d.year() - start.year()) as i64 * 12 + d.month() as i64 - start.month() as i64;
                d.day() == start.day() && months % n == 0
            }
            Freq::Yearly => {
                d.month() == start.month()
                    && d.day() == start.day()
                    && ((d.year() - start.year()) as i64) % n == 0
            }
        }
    }

    /// First date on or after `start` that matches the rule.
    pub fn first_occurrence(&self, start: NaiveDate) -> NaiveDate {
        let mut d = start;
        for _ in 0..800 {
            if self.occurs_on(start, d) {
                return d;
            }
            d = d.succ_opt().unwrap();
        }
        start
    }

    /// RFC 5545 rule body for Google Calendar (without UNTIL).
    pub fn rrule(&self) -> String {
        let freq = match self.freq {
            Freq::Daily => "DAILY",
            Freq::Weekly => "WEEKLY",
            Freq::Monthly => "MONTHLY",
            Freq::Yearly => "YEARLY",
        };
        let mut s = format!("RRULE:FREQ={freq};INTERVAL={}", self.interval.max(1));
        if self.freq == Freq::Weekly && !self.days.is_empty() {
            let days: Vec<&str> = self
                .days
                .iter()
                .map(|d| match d {
                    Weekday::Mon => "MO",
                    Weekday::Tue => "TU",
                    Weekday::Wed => "WE",
                    Weekday::Thu => "TH",
                    Weekday::Fri => "FR",
                    Weekday::Sat => "SA",
                    Weekday::Sun => "SU",
                })
                .collect();
            s += &format!(";BYDAY={}", days.join(","));
        }
        s
    }

    /// Canonical text form, which is also what gets stored in items.json.
    pub fn describe(&self) -> String {
        let unit = match self.freq {
            Freq::Daily => "day",
            Freq::Weekly => "week",
            Freq::Monthly => "month",
            Freq::Yearly => "year",
        };
        let mut s = if self.interval <= 1 {
            match self.freq {
                Freq::Daily => "daily".to_string(),
                Freq::Weekly => "weekly".to_string(),
                Freq::Monthly => "monthly".to_string(),
                Freq::Yearly => "yearly".to_string(),
            }
        } else {
            format!("every {} {unit}s", self.interval)
        };
        if !self.days.is_empty() {
            let days: Vec<String> = self.days.iter().map(|d| d.to_string().to_lowercase()).collect();
            s += &format!(" {}", days.join(","));
        }
        s
    }
}

/// One logged food/drink. Daily totals are the sum of a day's entries.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CalorieEntry {
    pub id: String,
    pub date: NaiveDate,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "hm_opt")]
    pub time: Option<NaiveTime>,
    pub kcal: u32,
    /// What was eaten, ideally with the per-component estimate.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// "manual", "claude" or "import".
    #[serde(default = "manual")]
    pub source: String,
}

fn manual() -> String {
    "manual".into()
}

impl CalorieEntry {
    pub fn new(date: NaiveDate, kcal: u32, description: impl Into<String>, source: &str) -> Self {
        let now = Local::now();
        CalorieEntry {
            id: uuid::Uuid::new_v4().to_string(),
            date,
            // log "now" for today's entries; other days have no meaningful time
            time: (date == now.date_naive()).then(|| {
                NaiveTime::from_hms_opt(now.time().hour(), now.time().minute(), 0).unwrap()
            }),
            kcal,
            description: description.into(),
            source: source.into(),
        }
    }
}

/// Body weight for one day (one entry per day).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WeightEntry {
    pub date: NaiveDate,
    pub kg: f32,
}

/// An event pulled from Google Calendar (read-only, cached locally).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExtEvent {
    pub title: String,
    pub calendar: String,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub all_day: bool,
}

impl ExtEvent {
    pub fn on_day(&self, d: NaiveDate) -> bool {
        let s = self.start.date();
        let e = self.end.date();
        if self.all_day {
            // all-day end dates are exclusive
            d >= s && d < e.max(s.succ_opt().unwrap())
        } else {
            d >= s && d <= e
        }
    }
}

/// Serialize optional times as "HH:MM".
mod hm_opt {
    use chrono::NaiveTime;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(t: &Option<NaiveTime>, s: S) -> Result<S::Ok, S::Error> {
        match t {
            Some(t) => s.serialize_str(&t.format("%H:%M").to_string()),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<NaiveTime>, D::Error> {
        let s: Option<String> = Option::deserialize(d)?;
        match s {
            None => Ok(None),
            Some(s) => NaiveTime::parse_from_str(&s, "%H:%M")
                .or_else(|_| NaiveTime::parse_from_str(&s, "%H:%M:%S"))
                .map(Some)
                .map_err(serde::de::Error::custom),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn parses_rules() {
        let r = Repeat::parse("every monday and thursday").unwrap();
        assert_eq!(r.freq, Freq::Weekly);
        assert_eq!(r.days, vec![Weekday::Mon, Weekday::Thu]);
        assert_eq!(Repeat::parse("every 3 days").unwrap().interval, 3);
        assert_eq!(Repeat::parse("weekly mon,thu").unwrap().describe(), "weekly mon,thu");
        assert_eq!(Repeat::parse("every other week").unwrap().interval, 2);
        assert!(Repeat::parse("sometimes").is_err());
    }

    #[test]
    fn weekly_interval() {
        let r = Repeat::parse("every 2 weeks mon").unwrap();
        let start = d("2026-10-05"); // Monday
        assert!(r.occurs_on(start, d("2026-10-05")));
        assert!(!r.occurs_on(start, d("2026-10-12")));
        assert!(r.occurs_on(start, d("2026-10-19")));
        assert_eq!(r.rrule(), "RRULE:FREQ=WEEKLY;INTERVAL=2;BYDAY=MO");
    }
}
