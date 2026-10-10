//! Charts tab: weight history (line) and calories per day (bars) over a chosen period.
//! Two separate charts on a shared date axis; never one chart with two y-scales.

use super::{weight_color, calorie_color};
use crate::store::Data;
use chrono::{Datelike, Duration, Local, NaiveDate};
use eframe::egui::{self, RichText, Ui};
use egui_plot::{Bar, BarChart, Corner, GridInput, GridMark, HLine, Legend, Line, LineStyle, Plot, PlotPoints, Points};


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
    // Both charts run to the goal's target date when it's within one more period, so their
    // date axes stay aligned.
    let end = match &data.body.goal {
        Some(g) if g.target_date > today && (g.target_date - today) <= (today - from) + Duration::days(1) => g.target_date + Duration::days(2),
        _ => today,
    };
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        super::streaks::streaks_section(ui, data, period.days());
        ui.add_space(18.0);
        weight_chart(ui, data, from, today, end, chart_h);
        ui.add_space(14.0);
        calorie_chart(ui, data, from, today, end, chart_h);
    });
}

/// Part of the straight line a→b that lies within [lo, hi] on the x axis.
fn clip(a: [f64; 2], b: [f64; 2], lo: f64, hi: f64) -> Option<Vec<[f64; 2]>> {
    let (x0, x1) = (a[0].max(lo), b[0].min(hi));
    if x1 <= x0 || b[0] <= a[0] {
        return None;
    }
    let at = |x: f64| [x, a[1] + (b[1] - a[1]) * (x - a[0]) / (b[0] - a[0])];
    Some(vec![at(x0), at(x1)])
}

/// Weigh-ins, their 7-day average and where the recent pace leads, plus the goal: target
/// line, a star on the target date and the planned path from where the goal started.
fn weight_chart(ui: &mut Ui, data: &Data, from: NaiveDate, to: NaiveDate, end: NaiveDate, height: f32) {
    let t = super::theme::th();
    let pts: Vec<[f64; 2]> = data
        .weights
        .iter()
        .filter(|w| w.date >= from && w.date <= to)
        .map(|w| [x(w.date), w.kg as f64])
        .collect();
    let goal = data.body.goal.clone();
    let progress = goal.as_ref().and_then(|g| crate::body::progress(g, &data.weights, to));
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
    if let (Some(g), Some(p)) = (&goal, &progress) {
        let mut text = format!("Goal {:.1} kg by {}", g.target_kg, g.target_date.format("%b %-d, %Y"));
        if g.start_kg > g.target_kg {
            text += &format!(" · {:.1} kg to go · {}", p.to_go.max(0.0), p.status());
            if let Some(eta) = p.eta {
                text += &format!(" · at this pace {:.1} kg around {}", g.target_kg, eta.format("%b %-d"));
            }
        }
        ui.label(RichText::new(text).color(t.data.goal));
    }
    if pts.is_empty() {
        super::streaks::empty_state(ui, "No weight logged in this period", "Type your weight in the entry bar (e.g. \"82.4 kg\") or on the Habits page.");
        return;
    }
    let (lo_x, hi_x) = (x(from) - 0.5, x(end) + 0.5);
    let trend: Vec<[f64; 2]> = crate::body::trend_series(&data.weights)
        .into_iter()
        .filter(|(d, _)| *d >= from && *d <= to)
        .map(|(d, k)| [x(d), k as f64])
        .collect();
    let ideal = goal.as_ref().and_then(|g| clip([x(g.set_on), g.start_kg as f64], [x(g.target_date), g.target_kg as f64], lo_x, hi_x));
    // Projection: the recent pace carried forward from the last average point, until the
    // target (or the edge of the chart).
    let projection = trend.last().and_then(|last| {
        let pace = crate::body::recent_pace(&data.weights)? as f64;
        let mut stop = hi_x;
        if let Some(g) = goal.as_ref().filter(|g| pace < 0.0 && (g.target_kg as f64) < last[1]) {
            stop = stop.min(last[0] + (last[1] - g.target_kg as f64) / -pace);
        }
        (stop - last[0] >= 2.0).then(|| vec![*last, [stop, last[1] + pace * (stop - last[0])]])
    });

    let mut ys: Vec<f64> = pts.iter().map(|p| p[1]).collect();
    if let Some(g) = &goal {
        ys.push(g.target_kg as f64);
    }
    ys.extend(ideal.iter().flatten().map(|p| p[1]));
    ys.extend(projection.iter().flatten().map(|p| p[1]));
    let lo = ys.iter().copied().fold(f64::MAX, f64::min);
    let hi = ys.iter().copied().fold(f64::MIN, f64::max);
    let pad = ((hi - lo) * 0.15).max(1.0);
    let n = pts.len();
    let resp = base_plot("weight_chart", from, end, height)
        .include_y(lo - pad)
        .include_y(hi + pad)
        .legend(Legend::default().position(Corner::LeftBottom))
        .y_axis_formatter(|m, _| format!("{:.0} kg", m.value))
        .label_formatter(|_, p| match date(p.x) {
            Some(d) => format!("{}\n{:.1} kg", d.format("%a %b %-d"), p.y),
            None => String::new(),
        })
        .show(ui, |plot| {
            let fill = t.charts.line_fill_alpha;
            let mut line = Line::new(PlotPoints::new(pts.clone())).color(weight_color().gamma_multiply(0.7)).width(1.2).name("Weigh-ins");
            if fill > 0.0 {
                line = line.fill((lo - pad) as f32).fill_alpha(fill);
            }
            plot.line(line);
            // markers only while they stay readable
            if n <= 120 {
                plot.points(Points::new(PlotPoints::new(pts)).color(weight_color()).radius(3.5).name("Weigh-ins"));
            }
            if trend.len() >= 2 {
                plot.line(Line::new(PlotPoints::new(trend)).color(t.data.trend).width(2.5).name("7-day average"));
            }
            if let Some(p) = projection {
                plot.line(Line::new(PlotPoints::new(p)).color(t.data.trend).width(2.0).style(LineStyle::dotted_dense()).name("At this pace"));
            }
            if let Some(g) = &goal {
                plot.hline(
                    HLine::new(g.target_kg as f64)
                        .color(t.data.goal.gamma_multiply(0.8))
                        .width(1.5)
                        .style(LineStyle::dashed_loose())
                        .name(format!("Goal {:.1} kg", g.target_kg)),
                );
                if let Some(path) = ideal {
                    plot.line(Line::new(PlotPoints::new(path)).color(t.data.goal.gamma_multiply(0.45)).width(1.5).style(LineStyle::dashed_dense()).name("Planned path"));
                }
            }
        });
    // the target: a star on the target date
    if let Some(g) = &goal {
        let gx = x(g.target_date);
        if gx >= lo_x && gx <= hi_x {
            let tr = resp.transform;
            let pos = egui::pos2(tr.position_from_point_x(gx), tr.position_from_point_y(g.target_kg as f64));
            let painter = ui.painter_at(*tr.frame());
            painter.circle_filled(pos, 11.0, t.data.goal.gamma_multiply(0.2));
            super::streaks::four_point_star(&painter, pos, 8.0, t.data.goal);
        }
    }
}

fn calorie_chart(ui: &mut Ui, data: &Data, from: NaiveDate, to: NaiveDate, end: NaiveDate, height: f32) {
    let mut daily: Vec<(NaiveDate, u32)> = Vec::new();
    let mut d = from;
    while d <= to {
        let k = data.calories_on(d);
        if k > 0 {
            daily.push((d, k));
        }
        d += Duration::days(1);
    }
    let limit = data.daily_limit();
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
        super::streaks::empty_state(ui, "No calories logged in this period", "Type what you ate in the entry bar (e.g. \"450 kcal pasta\" or \"ate a burrito\").");
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
    // egui_plot can't round bars, so the chart's own bars are invisible (they still give the
    // hover tooltips) and rounded bars are painted on top through the plot's transform.
    let resp = base_plot("calorie_chart", from, end, height)
        .include_y(0.0)
        .include_y(max as f64 * 1.1)
        .y_axis_formatter(|m, _| format!("{:.0}", m.value))
        // bars have their own tooltips; elsewhere just show the date under the cursor
        .label_formatter(|name, p| if name.is_empty() { date(p.x).map(|d| d.format("%a %b %-d").to_string()).unwrap_or_default() } else { String::new() })
        .show(ui, |plot| {
            plot.bar_chart(
                BarChart::new(bars)
                    .color(egui::Color32::TRANSPARENT)
                    .element_formatter(Box::new(|bar, _| format!("{}\n{:.0} kcal", bar.name, bar.value))),
            );
        });
    let tr = resp.transform;
    let painter = ui.painter_at(*tr.frame());
    let t = super::theme::th();
    let hover_x = resp.response.hover_pos().map(|p| p.x);
    for (d, k) in &daily {
        let x0 = tr.position_from_point_x(x(*d) - bar_w / 2.0);
        let x1 = tr.position_from_point_x(x(*d) + bar_w / 2.0);
        let rect = egui::Rect::from_min_max(egui::pos2(x0, tr.position_from_point_y(*k as f64)), egui::pos2(x1, tr.position_from_point_y(0.0)));
        let r = t.charts.bar_radius.min(((x1 - x0) / 2.0) as u8);
        let hovered = hover_x.is_some_and(|hx| hx >= x0 && hx <= x1);
        let c = if hovered { calorie_color().gamma_multiply(1.25) } else { calorie_color() };
        painter.rect_filled(rect, egui::CornerRadius { nw: r, ne: r, sw: 0, se: 0 }, c);
    }
    if limit > 0 {
        let y = tr.position_from_point_y(limit as f64);
        let f = tr.frame();
        painter.extend(egui::Shape::dashed_line(&[egui::pos2(f.left(), y), egui::pos2(f.right(), y)], egui::Stroke::new(1.5, t.data.chart_limit_line), 8.0, 6.0));
    }
    if limit > 0 {
        let source = if data.limit_from_plan() { "from your Body plan" } else { "the fallback limit" };
        ui.label(RichText::new(format!("Dashed line: today's daily limit ({limit} kcal, {source})")).weak().size(12.0));
    }
}
