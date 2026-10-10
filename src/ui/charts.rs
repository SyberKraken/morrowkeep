//! Charts tab: weight history (line) and calories per day (bars) over a chosen period.
//! Two separate charts on a shared date axis; never one chart with two y-scales.

use super::{weight_color, calorie_color, not_logged_color};
use crate::store::Data;
use chrono::{Datelike, Duration, Local, NaiveDate};
use eframe::egui::{self, RichText, Ui};
use egui_plot::{Bar, BarChart, GridInput, GridMark, HLine, Line, LineStyle, Plot, PlotPoints, Points};


#[derive(Clone, Copy, PartialEq)]
pub enum Period {
    Week,
    Month,
    ThreeMonths,
    SixMonths,
    Year,
}

impl Period {
    const ALL: [Period; 5] = [Period::Week, Period::Month, Period::ThreeMonths, Period::SixMonths, Period::Year];

    fn label(self) -> &'static str {
        match self {
            Period::Week => "Week",
            Period::Month => "Month",
            Period::ThreeMonths => "3 months",
            Period::SixMonths => "6 months",
            Period::Year => "Year",
        }
    }

    fn days(self) -> i64 {
        match self {
            Period::Week => 7,
            Period::Month => 30,
            Period::ThreeMonths => 91,
            Period::SixMonths => 182,
            Period::Year => 365,
        }
    }
}

/// Plot x coordinate for a date (days since 0001-01-01).
fn x(d: NaiveDate) -> f64 {
    d.num_days_from_ce() as f64
}

fn date(x: f64) -> Option<NaiveDate> {
    NaiveDate::from_num_days_from_ce_opt(x.round() as i32)
}

/// Date ticks: weekly for short ranges, the 1st of each month (or every 2nd/3rd month) otherwise.
fn date_grid(input: GridInput) -> Vec<GridMark> {
    let (lo, hi) = input.bounds;
    let span = hi - lo;
    let (Some(start), Some(end)) = (date(lo.floor()), date(hi.ceil())) else { return Vec::new() };
    let mut marks = Vec::new();
    if span <= 12.0 {
        // every day
        let mut d = start;
        while d <= end {
            marks.push(GridMark { value: x(d), step_size: 1.0 });
            d += Duration::days(1);
        }
    } else if span <= 45.0 {
        // Mondays
        let mut d = start + Duration::days((7 - start.weekday().num_days_from_monday() as i64) % 7);
        while d <= end {
            marks.push(GridMark { value: x(d), step_size: 7.0 });
            d += Duration::days(7);
        }
    } else {
        let every = if span <= 200.0 { 1 } else if span <= 400.0 { 2 } else { 3 };
        let mut d = start.with_day(1).unwrap();
        while d <= end {
            if d >= start && (d.month0() % every) == 0 {
                marks.push(GridMark { value: x(d), step_size: 30.0 * every as f64 });
            }
            d = if d.month() == 12 {
                NaiveDate::from_ymd_opt(d.year() + 1, 1, 1).unwrap()
            } else {
                NaiveDate::from_ymd_opt(d.year(), d.month() + 1, 1).unwrap()
            };
        }
    }
    // keep labels clear of the plot edges so they aren't clipped
    marks.retain(|m| m.value >= lo + 1.0 && m.value <= hi - 2.0);
    marks
}

fn axis_label(mark: GridMark, span: f64) -> String {
    match date(mark.value) {
        Some(d) if span <= 12.0 => d.format("%a %-d").to_string(),
        Some(d) if span <= 45.0 => d.format("%b %-d").to_string(),
        Some(d) if d.month() == 1 => d.format("%b %Y").to_string(),
        Some(d) => d.format("%b").to_string(),
        None => String::new(),
    }
}

/// A plot with fixed date bounds, no zoom/drag, and the shared date axis.
fn base_plot<'a>(id: &'a str, from: NaiveDate, to: NaiveDate, height: f32) -> Plot<'a> {
    Plot::new(id)
        .height(height)
        .include_x(x(from) - 0.5)
        .include_x(x(to) + 0.5)
        .allow_zoom(false)
        .allow_drag(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .allow_double_click_reset(false)
        .x_grid_spacer(date_grid)
        .x_axis_formatter(|mark, range| axis_label(mark, range.end() - range.start()))
        .y_axis_min_width(44.0)
        .link_cursor("charts_cursor", [true, false])
}

pub fn charts(ui: &mut Ui, data: &Data, period: &mut Period) {
    let today = Local::now().date_naive();
    ui.add_space(8.0);
    // Time range filter: one row above the charts.
    ui.horizontal(|ui| {
        ui.heading("Charts");
        ui.add_space(16.0);
        for p in Period::ALL {
            ui.selectable_value(period, p, p.label());
        }
    });
    let from = today - Duration::days(period.days() - 1);
    let chart_h = ((ui.available_height() - 120.0) / 2.0).clamp(180.0, 320.0);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        super::streaks::streaks_section(ui, data, period.days());
        ui.add_space(18.0);
        weight_chart(ui, data, from, today, chart_h);
        ui.add_space(14.0);
        calorie_chart(ui, data, from, today, chart_h);
    });
}

fn weight_chart(ui: &mut Ui, data: &Data, from: NaiveDate, to: NaiveDate, height: f32) {
    let pts: Vec<[f64; 2]> = data
        .weights
        .iter()
        .filter(|w| w.date >= from && w.date <= to)
        .map(|w| [x(w.date), w.kg as f64])
        .collect();
    ui.horizontal(|ui| {
        ui.label(RichText::new("Weight").size(17.0).strong());
        if let (Some(first), Some(last)) = (pts.first(), pts.last()) {
            let change = last[1] - first[1];
            let lo = pts.iter().map(|p| p[1]).fold(f64::MAX, f64::min);
            let hi = pts.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
            ui.label(RichText::new(format!("{:.1} kg now", last[1])).strong());
            ui.label(RichText::new(format!("{:+.1} kg over this period · range {lo:.1}–{hi:.1} kg", change)).weak());
        }
    });
    if pts.is_empty() {
        ui.label(RichText::new("No weight logged in this period. Enter it on the Today page.").color(not_logged_color()));
        return;
    }
    let lo = pts.iter().map(|p| p[1]).fold(f64::MAX, f64::min);
    let hi = pts.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
    let pad = ((hi - lo) * 0.15).max(1.0);
    let n = pts.len();
    base_plot("weight_chart", from, to, height)
        .include_y(lo - pad)
        .include_y(hi + pad)
        .y_axis_formatter(|m, _| format!("{:.0} kg", m.value))
        .label_formatter(|_, p| match date(p.x) {
            Some(d) => format!("{}\n{:.1} kg", d.format("%a %b %-d"), p.y),
            None => String::new(),
        })
        .show(ui, |plot| {
            plot.line(Line::new(PlotPoints::new(pts.clone())).color(weight_color()).width(2.0).name("Weight"));
            // markers only while they stay readable
            if n <= 120 {
                plot.points(Points::new(PlotPoints::new(pts)).color(weight_color()).radius(3.5).name("Weight"));
            }
        });
}

fn calorie_chart(ui: &mut Ui, data: &Data, from: NaiveDate, to: NaiveDate, height: f32) {
    let mut daily: Vec<(NaiveDate, u32)> = Vec::new();
    let mut d = from;
    while d <= to {
        let k = data.calories_on(d);
        if k > 0 {
            daily.push((d, k));
        }
        d += Duration::days(1);
    }
    let limit = data.settings.daily_calorie_limit;
    // Past days without food logged, counted from when calorie tracking started.
    let tracked_from = data.calories.iter().map(|c| c.date).min().unwrap_or(to).max(from);
    let unlogged = ((to - tracked_from).num_days().max(0) as usize)
        .saturating_sub(daily.iter().filter(|(d, _)| *d >= tracked_from && *d < to).count());
    ui.horizontal(|ui| {
        ui.label(RichText::new("Calories per day").size(17.0).strong());
        if !daily.is_empty() {
            let avg = daily.iter().map(|(_, k)| *k as u64).sum::<u64>() / daily.len() as u64;
            ui.label(RichText::new(format!("{avg} kcal average")).strong())
                .on_hover_text("Average over the days with food logged");
            let mut info = format!("{} days logged", daily.len());
            if limit > 0 {
                let over = daily.iter().filter(|(_, k)| *k > limit).count();
                info += &format!(" · {over} over the {limit} kcal limit");
            }
            if unlogged > 0 {
                info += &format!(" · {unlogged} past days not logged");
            }
            ui.label(RichText::new(info).weak());
        }
    });
    if daily.is_empty() {
        ui.label(RichText::new("No calories logged in this period.").color(not_logged_color()));
        return;
    }
    let max = daily.iter().map(|(_, k)| *k).max().unwrap_or(0).max(limit);
    let bar_w = if (to - from).num_days() > 120 { 0.9 } else { 0.75 };
    let bars: Vec<Bar> = daily
        .iter()
        .map(|(d, k)| {
            Bar::new(x(*d), *k as f64)
                .width(bar_w)
                .fill(calorie_color())
                .stroke(egui::Stroke::NONE)
                .name(d.format("%a %b %-d").to_string())
        })
        .collect();
    base_plot("calorie_chart", from, to, height)
        .include_y(0.0)
        .include_y(max as f64 * 1.1)
        .y_axis_formatter(|m, _| format!("{:.0}", m.value))
        // bars have their own tooltips; elsewhere just show the date under the cursor
        .label_formatter(|name, p| if name.is_empty() { date(p.x).map(|d| d.format("%a %b %-d").to_string()).unwrap_or_default() } else { String::new() })
        .show(ui, |plot| {
            plot.bar_chart(
                BarChart::new(bars)
                    .color(calorie_color())
                    .element_formatter(Box::new(|bar, _| format!("{}\n{:.0} kcal", bar.name, bar.value))),
            );
            if limit > 0 {
                plot.hline(
                    HLine::new(limit as f64)
                        .color(super::theme::th().data.chart_limit_line)
                        .width(1.5)
                        .style(LineStyle::Dashed { length: 8.0 })
                        .name(format!("Daily limit ({limit})")),
                );
            }
        });
    if limit > 0 {
        ui.label(RichText::new(format!("Dashed line: your daily limit ({limit} kcal)")).weak().size(12.0));
    }
}
