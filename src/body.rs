//! Body composition: your profile, goal weight and the calorie math behind the daily limit.
//! Stored in body.json. With a complete profile the daily calorie limit comes from here
//! (maintenance minus the deficit the goal needs); otherwise the fallback limit is used.

use crate::model::WeightEntry;
use chrono::{Datelike, Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// kcal in one kg of body fat (the usual rule of thumb).
pub const KCAL_PER_KG: f32 = 7700.0;
/// Fastest loss counted as safe: this share of body weight per week.
pub const SAFE_WEEKLY_SHARE: f32 = 0.01;
/// Pace used when a goal has no date: kg per week.
pub const DEFAULT_KG_PER_WEEK: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sex {
    Male,
    Female,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Activity {
    Sedentary,
    #[default]
    Light,
    Moderate,
    Very,
    Extra,
}

impl Activity {
    pub const ALL: [Activity; 5] = [Activity::Sedentary, Activity::Light, Activity::Moderate, Activity::Very, Activity::Extra];

    /// Multiplier from resting burn to daily burn.
    pub fn factor(self) -> f32 {
        match self {
            Activity::Sedentary => 1.2,
            Activity::Light => 1.375,
            Activity::Moderate => 1.55,
            Activity::Very => 1.725,
            Activity::Extra => 1.9,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Activity::Sedentary => "Sedentary",
            Activity::Light => "Lightly active",
            Activity::Moderate => "Moderately active",
            Activity::Very => "Very active",
            Activity::Extra => "Extra active",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Activity::Sedentary => "desk job, little exercise",
            Activity::Light => "1–3 workouts a week",
            Activity::Moderate => "3–5 workouts a week",
            Activity::Very => "6–7 workouts a week",
            Activity::Extra => "physical job plus training",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Goal {
    pub target_kg: f32,
    pub target_date: NaiveDate,
    /// When the goal was set, and the (7-day average) weight then: the start of the ideal path.
    pub set_on: NaiveDate,
    pub start_kg: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reached_on: Option<NaiveDate>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Body {
    pub version: u32,
    pub sex: Option<Sex>,
    pub birth_date: Option<NaiveDate>,
    pub height_cm: Option<f32>,
    pub activity: Activity,
    /// Optional; when set, resting burn uses Katch-McArdle (lean mass) instead of Mifflin-St Jeor.
    pub body_fat_pct: Option<f32>,
    /// Daily calorie limit used while the profile is incomplete (0 = none).
    pub fallback_daily_limit: u32,
    pub goal: Option<Goal>,
    /// Earlier goals, oldest first.
    pub past_goals: Vec<Goal>,
}

impl Default for Body {
    fn default() -> Self {
        Body {
            version: 1,
            sex: None,
            birth_date: None,
            height_cm: None,
            activity: Activity::Light,
            body_fat_pct: None,
            fallback_daily_limit: 0,
            goal: None,
            past_goals: Vec::new(),
        }
    }
}

/// The derived numbers for a complete profile.
#[derive(Clone, Debug, PartialEq)]
pub struct Numbers {
    pub age: u32,
    /// 7-day average weight the numbers use.
    pub weight_kg: f32,
    pub bmr: f32,
    pub formula: &'static str,
    pub tdee: f32,
    pub bmi: f32,
    /// Weights for BMI 18.5–25 at your height.
    pub healthy_kg: (f32, f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    /// Above the target: eat maintenance minus a deficit.
    Lose,
    /// At or below the target (or no goal): eat maintenance for the target weight.
    Maintain,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub mode: Mode,
    /// The daily calorie limit, rounded to 10 kcal.
    pub daily: u32,
    /// Maintenance calories the plan is based on (at the target weight when maintaining).
    pub maintenance: f32,
    pub deficit: f32,
    pub kg_left: f32,
    pub kg_per_week: f32,
    pub days_left: i64,
    /// Loss faster than SAFE_WEEKLY_SHARE of body weight per week.
    pub too_fast: bool,
    /// Earliest date reachable at the safe pace.
    pub safe_date: Option<NaiveDate>,
    /// The deficit would put you under the calorie floor, so the limit was raised to it.
    pub floored: bool,
    pub floor: u32,
    pub date_passed: bool,
    /// Target weight is under BMI 18.5.
    pub target_underweight: bool,
}

impl Body {
    /// Mifflin-St Jeor, or Katch-McArdle when body fat is known.
    pub fn bmr(&self, weight_kg: f32, age: u32) -> Option<(f32, &'static str)> {
        if let Some(bf) = self.body_fat_pct.filter(|b| (3.0..=70.0).contains(b)) {
            let lean = weight_kg * (1.0 - bf / 100.0);
            return Some((370.0 + 21.6 * lean, "Katch-McArdle (lean mass)"));
        }
        let h = self.height_cm?;
        let base = 10.0 * weight_kg + 6.25 * h - 5.0 * age as f32;
        Some(match self.sex? {
            Sex::Male => (base + 5.0, "Mifflin-St Jeor"),
            Sex::Female => (base - 161.0, "Mifflin-St Jeor"),
        })
    }

    pub fn age_on(&self, day: NaiveDate) -> Option<u32> {
        let b = self.birth_date?;
        let mut age = day.year() - b.year();
        if (day.month(), day.day()) < (b.month(), b.day()) {
            age -= 1;
        }
        (age >= 0).then_some(age as u32)
    }

    /// What's still missing for the calculations, as words ("sex, height").
    pub fn missing(&self, weight: Option<f32>) -> Vec<&'static str> {
        let mut m = Vec::new();
        if self.sex.is_none() {
            m.push("sex");
        }
        if self.birth_date.is_none() {
            m.push("birth date");
        }
        if self.height_cm.is_none() {
            m.push("height");
        }
        if weight.is_none() {
            m.push("a logged weight");
        }
        m
    }

    /// The numbers for `weight_kg` on `day`, if the profile is complete.
    pub fn numbers(&self, weight_kg: f32, day: NaiveDate) -> Option<Numbers> {
        if !self.missing(Some(weight_kg)).is_empty() {
            return None;
        }
        let age = self.age_on(day)?;
        let h = self.height_cm? / 100.0;
        let (bmr, formula) = self.bmr(weight_kg, age)?;
        Some(Numbers {
            age,
            weight_kg,
            bmr,
            formula,
            tdee: bmr * self.activity.factor(),
            bmi: weight_kg / (h * h),
            healthy_kg: (18.5 * h * h, 25.0 * h * h),
        })
    }

    fn calorie_floor(&self) -> u32 {
        match self.sex {
            Some(Sex::Female) => 1200,
            _ => 1500,
        }
    }

    /// The plan for `day` from the numbers at the current weight.
    pub fn plan(&self, n: &Numbers, day: NaiveDate) -> Plan {
        let floor = self.calorie_floor();
        let round10 = |k: f32| ((k / 10.0).round() * 10.0).max(0.0) as u32;
        let maintain = |kg: f32| {
            let maintenance = self.bmr(kg, n.age).map_or(n.tdee, |(b, _)| b * self.activity.factor());
            Plan {
                mode: Mode::Maintain,
                daily: round10(maintenance),
                maintenance,
                deficit: 0.0,
                kg_left: 0.0,
                kg_per_week: 0.0,
                days_left: 0,
                too_fast: false,
                safe_date: None,
                floored: false,
                floor,
                date_passed: false,
                target_underweight: kg < n.healthy_kg.0,
            }
        };
        let Some(goal) = &self.goal else { return maintain(n.weight_kg) };
        if n.weight_kg <= goal.target_kg {
            return maintain(goal.target_kg);
        }
        let kg_left = n.weight_kg - goal.target_kg;
        let safe_per_week = n.weight_kg * SAFE_WEEKLY_SHARE;
        let safe_date = day + Duration::days((kg_left / safe_per_week * 7.0 - 1e-3).ceil() as i64);
        let days = (goal.target_date - day).num_days();
        let date_passed = days <= 0;
        // A date in the past: keep going at the safe pace.
        let days_left = if date_passed { (safe_date - day).num_days().max(1) } else { days };
        let deficit = kg_left * KCAL_PER_KG / days_left as f32;
        let kg_per_week = kg_left / days_left as f32 * 7.0;
        let raw = n.tdee - deficit;
        Plan {
            mode: Mode::Lose,
            daily: round10(raw.max(floor as f32)),
            maintenance: n.tdee,
            deficit,
            kg_left,
            kg_per_week,
            days_left,
            too_fast: kg_per_week > safe_per_week + 0.005,
            safe_date: Some(safe_date),
            floored: raw < floor as f32,
            floor,
            date_passed,
            target_underweight: goal.target_kg < n.healthy_kg.0,
        }
    }
}

/// Default target date for a goal typed without one: DEFAULT_KG_PER_WEEK from `from_kg`.
pub fn default_target_date(from_kg: f32, target_kg: f32, today: NaiveDate) -> NaiveDate {
    let weeks = ((from_kg - target_kg).max(0.0) / DEFAULT_KG_PER_WEEK).ceil().max(1.0);
    today + Duration::days(weeks as i64 * 7)
}

/// Average of the weigh-ins in the 7 days up to and including `day` (smooths out water swings).
pub fn trend_on(weights: &[WeightEntry], day: NaiveDate) -> Option<f32> {
    let from = day - Duration::days(6);
    let w: Vec<f32> = weights.iter().filter(|w| w.date >= from && w.date <= day).map(|w| w.kg).collect();
    (!w.is_empty()).then(|| w.iter().sum::<f32>() / w.len() as f32)
}

/// The 7-day trend at every weigh-in (weights sorted by date).
pub fn trend_series(weights: &[WeightEntry]) -> Vec<(NaiveDate, f32)> {
    weights.iter().filter_map(|w| trend_on(weights, w.date).map(|t| (w.date, t))).collect()
}

/// The weight the calculations use: the trend at the latest weigh-in.
pub fn current_weight(weights: &[WeightEntry]) -> Option<f32> {
    trend_on(weights, weights.last()?.date)
}

/// Least-squares slope (kg per day) of `points` (day number, kg).
fn slope(points: &[(f64, f64)]) -> Option<f64> {
    let n = points.len() as f64;
    if points.len() < 2 {
        return None;
    }
    let mx = points.iter().map(|p| p.0).sum::<f64>() / n;
    let my = points.iter().map(|p| p.1).sum::<f64>() / n;
    let sxx: f64 = points.iter().map(|p| (p.0 - mx).powi(2)).sum();
    (sxx > 0.0).then(|| points.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum::<f64>() / sxx)
}

/// Recent pace in kg per day from the last 3 weeks of weigh-ins (at least 3 spanning a week).
pub fn recent_pace(weights: &[WeightEntry]) -> Option<f32> {
    let last = weights.last()?.date;
    let recent: Vec<&WeightEntry> = weights.iter().filter(|w| w.date > last - Duration::days(21)).collect();
    if recent.len() < 3 || (last - recent[0].date).num_days() < 7 {
        return None;
    }
    let pts: Vec<(f64, f64)> = recent.iter().map(|w| ((w.date - last).num_days() as f64, w.kg as f64)).collect();
    slope(&pts).map(|s| s as f32)
}

/// How the trend compares with the straight path from the goal's start to its target.
#[derive(Clone, Debug, PartialEq)]
pub struct Progress {
    pub lost: f32,
    pub to_go: f32,
    /// Fraction of the way from start to target (0..1).
    pub fraction: f32,
    /// Trend minus where the ideal path is today (negative = ahead).
    pub vs_path: f32,
    /// When the target is reached at the recent pace.
    pub eta: Option<NaiveDate>,
}

impl Progress {
    pub fn status(&self) -> &'static str {
        if self.to_go <= 0.0 {
            "goal reached"
        } else if self.vs_path > 0.3 {
            "behind"
        } else if self.vs_path < -0.3 {
            "ahead"
        } else {
            "on track"
        }
    }
}

/// Where the ideal path from `goal.set_on` to the target is on `day`.
pub fn ideal_on(goal: &Goal, day: NaiveDate) -> f32 {
    let total = (goal.target_date - goal.set_on).num_days().max(1) as f32;
    let t = ((day - goal.set_on).num_days() as f32 / total).clamp(0.0, 1.0);
    goal.start_kg + (goal.target_kg - goal.start_kg) * t
}

pub fn progress(goal: &Goal, weights: &[WeightEntry], today: NaiveDate) -> Option<Progress> {
    let now = current_weight(weights)?;
    let lost = goal.start_kg - now;
    let to_go = now - goal.target_kg;
    let span = goal.start_kg - goal.target_kg;
    let fraction = if span > 0.0 { (lost / span).clamp(0.0, 1.0) } else { 1.0 };
    let last = weights.last()?.date;
    let eta = recent_pace(weights).filter(|p| *p < -0.005 && to_go > 0.0).map(|p| last + Duration::days((to_go / -p).ceil() as i64));
    Some(Progress { lost, to_go, fraction, vs_path: now - ideal_on(goal, today), eta })
}

/// Daily burn measured from your own logs over the 28 days before `today`: average intake on
/// logged days minus the energy in the weight change. Needs 14+ logged days and weigh-ins
/// spanning 14+ days. Returns (kcal/day, logged days).
pub fn measured_burn(daily_kcal: &BTreeMap<NaiveDate, u32>, weights: &[WeightEntry], today: NaiveDate) -> Option<(f32, usize)> {
    let from = today - Duration::days(28);
    let logged: Vec<u32> = daily_kcal.range(from..today).map(|(_, k)| *k).filter(|k| *k > 0).collect();
    if logged.len() < 14 {
        return None;
    }
    let ws: Vec<&WeightEntry> = weights.iter().filter(|w| w.date >= from && w.date < today).collect();
    if ws.len() < 4 || (ws.last()?.date - ws[0].date).num_days() < 14 {
        return None;
    }
    let pts: Vec<(f64, f64)> = ws.iter().map(|w| ((w.date - from).num_days() as f64, w.kg as f64)).collect();
    let kg_per_day = slope(&pts)? as f32;
    let intake = logged.iter().map(|k| *k as f32).sum::<f32>() / logged.len() as f32;
    Some((intake - kg_per_day * KCAL_PER_KG, logged.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn me() -> Body {
        Body { sex: Some(Sex::Male), birth_date: Some(d(1996, 10, 11)), height_cm: Some(180.0), activity: Activity::Sedentary, ..Body::default() }
    }

    #[test]
    fn resting_burn_formulas() {
        let today = d(2026, 10, 10);
        let b = me();
        assert_eq!(b.age_on(today), Some(29)); // birthday is tomorrow
        // Mifflin-St Jeor, male 80 kg 180 cm 30 y: 800 + 1125 - 150 + 5 = 1780
        assert_eq!(b.bmr(80.0, 30).unwrap().0, 1780.0);
        let f = Body { sex: Some(Sex::Female), ..me() };
        // female 60 kg 165 cm 25 y: 600 + 1031.25 - 125 - 161 = 1345.25
        let f = Body { height_cm: Some(165.0), ..f };
        assert_eq!(f.bmr(60.0, 25).unwrap().0, 1345.25);
        // Katch-McArdle, 80 kg at 20 % fat: 370 + 21.6 × 64 = 1752.4
        let k = Body { body_fat_pct: Some(20.0), ..me() };
        assert!((k.bmr(80.0, 30).unwrap().0 - 1752.4).abs() < 0.01);
        // incomplete profile: no numbers
        assert!(Body::default().numbers(80.0, today).is_none());
        let n = b.numbers(80.0, today).unwrap();
        assert!((n.tdee - (1785.0 * 1.2)).abs() < 0.01); // age 29
        assert!((n.bmi - 24.69).abs() < 0.01);
    }

    #[test]
    fn plans_lose_and_maintain() {
        let today = d(2026, 10, 10);
        let mut b = me();
        let n = b.numbers(80.0, today).unwrap(); // TDEE 2142
        // no goal: maintenance
        let p = b.plan(&n, today);
        assert_eq!((p.mode, p.daily), (Mode::Maintain, 2140));
        // 4 kg in 70 days: 4 × 7700 / 70 = 440 kcal/day deficit, 0.4 kg/week (safe)
        b.goal = Some(Goal { target_kg: 76.0, target_date: today + Duration::days(70), set_on: today, start_kg: 80.0, reached_on: None });
        let p = b.plan(&n, today);
        assert_eq!(p.mode, Mode::Lose);
        assert!((p.deficit - 440.0).abs() < 0.01);
        assert_eq!(p.daily, 1700);
        assert!(!p.too_fast && !p.floored);
        // 4 kg in 14 days: too fast, and floored at 1500
        b.goal.as_mut().unwrap().target_date = today + Duration::days(14);
        let p = b.plan(&n, today);
        assert!(p.too_fast && p.floored);
        assert_eq!(p.daily, 1500);
        assert_eq!(p.safe_date, Some(today + Duration::days(35))); // 4 kg at 0.8 kg/week
        // date in the past: continue at the safe pace
        b.goal.as_mut().unwrap().target_date = today - Duration::days(1);
        let p = b.plan(&n, today);
        assert!(p.date_passed && !p.too_fast);
        // target above the current weight: maintenance for the target weight (82 kg)
        b.goal.as_mut().unwrap().target_kg = 82.0;
        let p = b.plan(&n, today);
        assert_eq!(p.mode, Mode::Maintain);
        assert_eq!(p.daily, ((1805.0f32 * 1.2) / 10.0).round() as u32 * 10);
    }

    #[test]
    fn trend_pace_and_measured_burn() {
        let today = d(2026, 10, 10);
        // losing 0.1 kg a day for 28 days while eating 2000 on logged days
        let weights: Vec<WeightEntry> = (0..28).map(|k| WeightEntry { date: today - Duration::days(28 - k), kg: 90.0 - 0.1 * k as f32 }).collect();
        let pace = recent_pace(&weights).unwrap();
        assert!((pace + 0.1).abs() < 0.001);
        // trend at the last weigh-in: average of the last 7 (87.6 .. 87.3 .. 87.0 → 87.6-0.3)
        assert!((current_weight(&weights).unwrap() - 87.6).abs() < 0.01);
        let kcal: BTreeMap<NaiveDate, u32> = (1..=20).map(|k| (today - Duration::days(k), 2000)).collect();
        let (burn, days) = measured_burn(&kcal, &weights, today).unwrap();
        assert_eq!(days, 20);
        assert!((burn - 2770.0).abs() < 1.0); // 2000 + 0.1 × 7700
        let goal = Goal { target_kg: 85.0, target_date: today + Duration::days(30), set_on: today - Duration::days(28), start_kg: 90.0, reached_on: None };
        let p = progress(&goal, &weights, today).unwrap();
        assert!((p.to_go - 2.6).abs() < 0.01);
        assert_eq!(p.status(), "on track"); // 87.6 vs the path's 87.59
        assert_eq!(p.eta, Some(weights.last().unwrap().date + Duration::days(26)));
    }
}
