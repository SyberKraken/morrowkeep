//! Offline parsing of quick-add text (typed or dictated) and of loose date/time strings.
//!
//! Example: "Leg day tomorrow at 6pm for 1 hour every mon and thu #workout"

use crate::model::{parse_weekday, small_number, Repeat, KINDS};
use chrono::{Datelike, Duration, NaiveDate, NaiveTime};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Parsed {
    pub title: String,
    pub kind: Option<String>,
    pub date: Option<NaiveDate>,
    pub time: Option<NaiveTime>,
    pub duration_min: Option<u32>,
    pub repeat: Option<String>,
    pub until: Option<NaiveDate>,
    /// "someday" / "backlog" was said: keep the item undated.
    pub someday: bool,
}

const MONTHS: [&str; 12] = [
    "january", "february", "march", "april", "may", "june", "july", "august", "september",
    "october", "november", "december",
];

fn month_num(s: &str) -> Option<u32> {
    let s = s.trim_end_matches('.');
    if s.len() < 3 {
        return None;
    }
    MONTHS.iter().position(|m| m.starts_with(s)).map(|i| i as u32 + 1)
}

fn ordinal(s: &str) -> Option<u32> {
    let s = s.trim_end_matches("st").trim_end_matches("nd").trim_end_matches("rd").trim_end_matches("th");
    s.parse().ok().filter(|d| (1..=31).contains(d))
}

/// Next date (today included) that falls on `wd`.
fn upcoming(today: NaiveDate, wd: chrono::Weekday) -> NaiveDate {
    let diff = (wd.num_days_from_monday() as i64 - today.weekday().num_days_from_monday() as i64).rem_euclid(7);
    today + Duration::days(diff)
}

/// Month/day without year: this year, or next year if that's already well past.
fn month_day(today: NaiveDate, m: u32, d: u32) -> Option<NaiveDate> {
    let this = NaiveDate::from_ymd_opt(today.year(), m, d)?;
    if this < today - Duration::days(30) {
        NaiveDate::from_ymd_opt(today.year() + 1, m, d)
    } else {
        Some(this)
    }
}

/// Parses a standalone date string: "2026-10-12", "today", "tomorrow", "monday",
/// "next friday", "+3d", "in 2 weeks", "oct 12", "12 october".
pub fn parse_date(s: &str, today: NaiveDate) -> Option<NaiveDate> {
    let s = s.trim().to_lowercase();
    if let Ok(d) = NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
        return Some(d);
    }
    let toks: Vec<String> = clean_tokens(&s);
    let refs: Vec<&str> = toks.iter().map(|t| t.as_str()).collect();
    match date_at(&refs, 0, today) {
        Some((d, n)) if n == refs.len() => Some(d),
        _ => None,
    }
}

/// Parses "18:00", "6pm", "6:30 pm", "noon".
pub fn parse_time(s: &str) -> Option<NaiveTime> {
    let toks = clean_tokens(&s.to_lowercase());
    let refs: Vec<&str> = toks.iter().map(|t| t.as_str()).collect();
    match time_at(&refs, 0, false) {
        Some((t, n)) if n == refs.len() => Some(t),
        _ => None,
    }
}

/// Parses "45m", "1h", "1h30m", "90", "1.5h", "2 hours".
pub fn parse_duration(s: &str) -> Option<u32> {
    let toks = clean_tokens(&s.to_lowercase());
    let refs: Vec<&str> = toks.iter().map(|t| t.as_str()).collect();
    if let [n] = refs.as_slice() {
        if let Ok(m) = n.parse::<u32>() {
            return Some(m);
        }
    }
    match duration_at(&refs, 0) {
        Some((m, n)) if n == refs.len() => Some(m),
        _ => None,
    }
}

/// Lowercases-already input → tokens with surrounding punctuation stripped and
/// "p.m." style spellings normalized.
fn clean_tokens(s: &str) -> Vec<String> {
    s.split_whitespace()
        .map(|t| {
            let t = t.trim_matches(|c: char| matches!(c, ',' | '!' | '?' | ';' | '"' | '(' | ')'));
            let t = t.strip_suffix('.').unwrap_or(t);
            t.replace("a.m", "am").replace("p.m", "pm")
        })
        .filter(|t| !t.is_empty())
        .collect()
}

fn date_at(t: &[&str], i: usize, today: NaiveDate) -> Option<(NaiveDate, usize)> {
    let w = *t.get(i)?;
    let next = t.get(i + 1).copied();
    match w {
        "today" | "tonight" => return Some((today, 1)),
        "tomorrow" | "tmrw" => return Some((today + Duration::days(1), 1)),
        "yesterday" => return Some((today - Duration::days(1), 1)),
        "day" if next == Some("after") && t.get(i + 2) == Some(&"tomorrow") => {
            return Some((today + Duration::days(2), 3))
        }
        "on" => {
            let (d, n) = date_at(t, i + 1, today)?;
            return Some((d, n + 1));
        }
        "next" | "this" => {
            if let Some(wd) = next.and_then(parse_weekday) {
                let mut d = upcoming(today, wd);
                if w == "next" && d <= today {
                    d += Duration::days(7);
                }
                return Some((d, 2));
            }
            if w == "next" && next == Some("week") {
                return Some((upcoming(today + Duration::days(1), chrono::Weekday::Mon), 2));
            }
            return None;
        }
        "in" => {
            let n = small_number(next?)? as i64;
            let unit = *t.get(i + 2)?;
            let days = match unit.trim_end_matches('s') {
                "day" => n,
                "week" => n * 7,
                _ => return None,
            };
            return Some((today + Duration::days(days), 3));
        }
        _ => {}
    }
    if let Ok(d) = NaiveDate::parse_from_str(w, "%Y-%m-%d") {
        return Some((d, 1));
    }
    if let Some(rest) = w.strip_prefix('+') {
        if let Some(n) = rest.strip_suffix('d').and_then(|n| n.parse::<i64>().ok()) {
            return Some((today + Duration::days(n), 1));
        }
        if let Some(n) = rest.strip_suffix('w').and_then(|n| n.parse::<i64>().ok()) {
            return Some((today + Duration::days(n * 7), 1));
        }
    }
    if let Some(wd) = parse_weekday(w) {
        // "mondays" means a repeat, not a date
        if !w.ends_with('s') || w == "tues" || w == "thurs" || w == "weds" {
            return Some((upcoming(today, wd), 1));
        }
    }
    // "oct 12", "october 12th"
    if let Some(m) = month_num(w) {
        if let Some(d) = next.and_then(ordinal) {
            return Some((month_day(today, m, d)?, 2));
        }
    }
    // "12 oct", "12th of october"
    if let Some(d) = ordinal(w) {
        if let Some(m) = next.and_then(month_num) {
            return Some((month_day(today, m, d)?, 2));
        }
        if next == Some("of") {
            if let Some(m) = t.get(i + 2).and_then(|s| month_num(s)) {
                return Some((month_day(today, m, d)?, 3));
            }
        }
    }
    None
}

/// `after_at`: the word "at" preceded this token, so a bare number is a time.
fn time_at(t: &[&str], i: usize, after_at: bool) -> Option<(NaiveTime, usize)> {
    let w = *t.get(i)?;
    match w {
        "at" | "@" => {
            let (tm, n) = time_at(t, i + 1, true)?;
            return Some((tm, n + 1));
        }
        "noon" | "midday" => return Some((NaiveTime::from_hms_opt(12, 0, 0)?, 1)),
        "midnight" => return Some((NaiveTime::from_hms_opt(0, 0, 0)?, 1)),
        _ => {}
    }
    // split "6pm" / "6:30pm" into number + suffix, or take suffix from the next token ("6 pm")
    let (num, mut suffix, mut used) = if let Some(n) = w.strip_suffix("am") {
        (n, Some("am"), 1)
    } else if let Some(n) = w.strip_suffix("pm") {
        (n, Some("pm"), 1)
    } else {
        (w, None, 1)
    };
    if suffix.is_none() {
        match t.get(i + 1) {
            Some(&"am") => (suffix, used) = (Some("am"), 2),
            Some(&"pm") => (suffix, used) = (Some("pm"), 2),
            _ => {}
        }
    }
    let (h, m) = match num.split_once([':', '.']) {
        Some((h, m)) if m.len() == 2 => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        Some(_) => return None,
        None => {
            // A bare number only counts as a time with am/pm or after "at".
            if suffix.is_none() && !after_at {
                return None;
            }
            (small_number(num)?, 0)
        }
    };
    let h = match suffix {
        Some("am") if h == 12 => 0,
        Some("am") => h,
        Some(_) if h < 12 => h + 12,
        Some(_) => h,
        // "at 6" in plain speech almost always means the evening
        None if after_at && !num.contains(':') && (1..=6).contains(&h) => h + 12,
        None => h,
    };
    Some((NaiveTime::from_hms_opt(h, m, 0)?, used))
}

/// "45m", "1h", "1h30m", "1h30", "90min", "1.5h", "2 hours", "an hour", "half an hour", "30 minutes"
fn duration_at(t: &[&str], i: usize) -> Option<(u32, usize)> {
    let w = *t.get(i)?;
    if w == "for" {
        let (m, n) = duration_at(t, i + 1)?;
        return Some((m, n + 1));
    }
    if w == "half" && t.get(i + 1) == Some(&"an") && t.get(i + 2).is_some_and(|u| u.starts_with("hour")) {
        return Some((30, 3));
    }
    // compact forms
    let compact = |s: &str| -> Option<u32> {
        if let Some((h, rest)) = s.split_once('h') {
            let h: f32 = h.parse().ok()?;
            let rest = rest.trim_start_matches("rs").trim_start_matches("r").trim_end_matches("min").trim_end_matches('m');
            let m: u32 = if rest.is_empty() { 0 } else { rest.parse().ok()? };
            return Some((h * 60.0).round() as u32 + m);
        }
        for suf in ["mins", "min", "m"] {
            if let Some(n) = s.strip_suffix(suf) {
                return n.parse().ok();
            }
        }
        None
    };
    if let Some(m) = compact(w).filter(|m| *m > 0) {
        return Some((m, 1));
    }
    // "<number> <unit>"
    let unit = *t.get(i + 1)?;
    let n: f32 = match w.parse::<f32>() {
        Ok(n) => n,
        Err(_) => small_number(w)? as f32,
    };
    let mins = if unit.starts_with("hour") || unit == "hr" || unit == "hrs" || unit == "h" {
        n * 60.0
    } else if unit.starts_with("min") {
        n
    } else {
        return None;
    };
    Some((mins.round() as u32, 2))
}

fn is_repeat_word(w: &str) -> bool {
    matches!(
        w,
        "and" | "on" | "other" | "day" | "days" | "week" | "weeks" | "month" | "months" | "year"
            | "years" | "weekday" | "weekdays" | "weekend" | "weekends"
    ) || parse_weekday(w).is_some()
        || w.parse::<u32>().is_ok()
        || small_number(w).is_some_and(|_| !matches!(w, "a" | "an"))
}

/// Parses free text into item fields. Whatever isn't recognized becomes the title.
pub fn parse_quick(input: &str, today: NaiveDate) -> Parsed {
    let mut p = Parsed::default();
    let raw: Vec<&str> = input.split_whitespace().collect();
    let lower: Vec<String> = clean_tokens(&input.to_lowercase());
    // clean_tokens may drop pure-punctuation tokens; keep the two lists aligned
    if lower.len() != raw.len() {
        let joined: String = raw
            .iter()
            .filter(|r| !clean_tokens(&r.to_lowercase()).is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
        if joined != input {
            return parse_quick(&joined, today);
        }
    }
    let t: Vec<&str> = lower.iter().map(|s| s.as_str()).collect();
    let mut title: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < t.len() {
        let w = t[i];
        // "#workout", "hashtag workout", or a leading "workout:"
        if let Some(k) = w.strip_prefix('#') {
            p.kind = Some(k.to_string());
            i += 1;
            continue;
        }
        if w == "hashtag" && i + 1 < t.len() {
            p.kind = Some(t[i + 1].to_string());
            i += 2;
            continue;
        }
        if i == 0 {
            if let Some(k) = raw[0].strip_suffix(':').map(|k| k.to_lowercase()) {
                if KINDS.contains(&k.as_str()) {
                    p.kind = Some(k);
                    i += 1;
                    continue;
                }
            }
        }
        // repeats: "every ...", "daily", "weekly on mon"
        if matches!(w, "every" | "daily" | "weekly" | "monthly" | "yearly" | "weekdays" | "biweekly") {
            let mut j = i + 1;
            while j < t.len() && is_repeat_word(t[j]) {
                j += 1;
            }
            // don't swallow a trailing "on" / "and" that belongs to something else
            while j > i + 1 && matches!(t[j - 1], "on" | "and") {
                j -= 1;
            }
            if let Ok(rule) = Repeat::parse(&t[i..j].join(" ")) {
                p.repeat = Some(rule.describe());
                i = j;
                continue;
            }
        }
        if matches!(w, "someday" | "sometime" | "backlog") {
            p.someday = true;
            i += 1;
            continue;
        }
        if w == "until" {
            if let Some((d, n)) = date_at(&t, i + 1, today) {
                p.until = Some(d);
                i += 1 + n;
                continue;
            }
        }
        // "from 6pm to 7:30pm"
        if w == "from" {
            if let Some((start, n1)) = time_at(&t, i + 1, true) {
                if matches!(t.get(i + 1 + n1), Some(&"to") | Some(&"until") | Some(&"-")) {
                    if let Some((end, n2)) = time_at(&t, i + 2 + n1, true) {
                        p.time = Some(start);
                        let mins = (end - start).num_minutes();
                        if mins > 0 {
                            p.duration_min = Some(mins as u32);
                        }
                        i += 2 + n1 + n2;
                        continue;
                    }
                }
            }
        }
        if p.date.is_none() {
            if let Some((d, n)) = date_at(&t, i, today) {
                p.date = Some(d);
                i += n;
                continue;
            }
        }
        if p.time.is_none() {
            if let Some((tm, n)) = time_at(&t, i, false) {
                p.time = Some(tm);
                i += n;
                continue;
            }
        }
        if p.duration_min.is_none() {
            if let Some((m, n)) = duration_at(&t, i) {
                p.duration_min = Some(m);
                i += n;
                continue;
            }
        }
        title.push(raw[i]);
        i += 1;
    }
    // drop dangling connector words left behind by removed phrases
    while title.last().is_some_and(|w| matches!(w.to_lowercase().as_str(), "at" | "on" | "for" | "from" | "to" | "and")) {
        title.pop();
    }
    let mut s = title.join(" ");
    s = s.trim_end_matches(['.', ',', '!']).to_string();
    if let Some(first) = s.chars().next() {
        s = first.to_uppercase().collect::<String>() + &s[first.len_utf8()..];
    }
    p.title = s;
    // A repeating item needs a start date: default to its first occurrence from today.
    if p.repeat.is_some() && p.date.is_none() {
        let rule = Repeat::parse(p.repeat.as_ref().unwrap()).unwrap();
        p.date = Some(rule.first_occurrence(today));
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 10).unwrap() // Saturday
    }
    fn t(h: u32, m: u32) -> Option<NaiveTime> {
        NaiveTime::from_hms_opt(h, m, 0)
    }

    #[test]
    fn full_sentence() {
        let p = parse_quick("Leg day tomorrow at 6pm for 1 hour #workout", today());
        assert_eq!(p.title, "Leg day");
        assert_eq!(p.kind.as_deref(), Some("workout"));
        assert_eq!(p.date, NaiveDate::from_ymd_opt(2026, 10, 11));
        assert_eq!(p.time, t(18, 0));
        assert_eq!(p.duration_min, Some(60));
    }

    #[test]
    fn dictated() {
        let p = parse_quick("Workout: run 5k on Monday at 7 a.m. for 45 minutes.", today());
        assert_eq!(p.kind.as_deref(), Some("workout"));
        assert_eq!(p.title, "Run 5k");
        assert_eq!(p.date, NaiveDate::from_ymd_opt(2026, 10, 12));
        assert_eq!(p.time, t(7, 0));
        assert_eq!(p.duration_min, Some(45));
    }

    #[test]
    fn repeating_chore() {
        let p = parse_quick("vacuum every saturday #chore", today());
        assert_eq!(p.title, "Vacuum");
        assert_eq!(p.repeat.as_deref(), Some("weekly sat"));
        assert_eq!(p.date, Some(today()));
        let p = parse_quick("gym every mon and thu at 18:00 1h30m", today());
        assert_eq!(p.repeat.as_deref(), Some("weekly mon,thu"));
        assert_eq!(p.date, NaiveDate::from_ymd_opt(2026, 10, 12));
        assert_eq!(p.time, t(18, 0));
        assert_eq!(p.duration_min, Some(90));
    }

    #[test]
    fn plain_title_untouched() {
        let p = parse_quick("Buy 2 cartons of milk", today());
        assert_eq!(p.title, "Buy 2 cartons of milk");
        assert_eq!(p.date, None);
        assert_eq!(p.time, None);
    }

    #[test]
    fn ranges_and_dates() {
        let p = parse_quick("dentist oct 14 from 2pm to 3:30pm", today());
        assert_eq!(p.title, "Dentist");
        assert_eq!(p.date, NaiveDate::from_ymd_opt(2026, 10, 14));
        assert_eq!(p.time, t(14, 0));
        assert_eq!(p.duration_min, Some(90));
        assert_eq!(parse_date("next friday", today()), NaiveDate::from_ymd_opt(2026, 10, 16));
        assert_eq!(parse_date("+3d", today()), NaiveDate::from_ymd_opt(2026, 10, 13));
        assert_eq!(parse_time("6:30 pm"), t(18, 30));
        assert_eq!(parse_duration("1h30m"), Some(90));
    }
}
