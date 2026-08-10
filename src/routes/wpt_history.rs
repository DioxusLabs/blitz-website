use std::{fmt::Write, ops::Deref, sync::Arc};

use dioxus::prelude::*;

use crate::components::Page;
use crate::routes::{StatusHeader, StatusTabs};
use crate::wpt_history::{HistoryRun, WptHistory};

#[derive(Clone)]
pub struct ArcWptHistory(pub Arc<WptHistory>);
impl PartialEq for ArcWptHistory {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Deref for ArcWptHistory {
    type Target = WptHistory;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Days since 1970-01-01 for a year/month/day (Howard Hinnant's civil-days algorithm)
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Parse a date of the form YYYY-MM-DD or YYYY-MM-DDTHH:MM:SSZ into fractional
/// days since the unix epoch.
fn parse_date(date: &str) -> Option<f64> {
    let year: i64 = date.get(0..4)?.parse().ok()?;
    let month: i64 = date.get(5..7)?.parse().ok()?;
    let day: i64 = date.get(8..10)?.parse().ok()?;
    let mut days = days_from_civil(year, month, day) as f64;

    if let (Some(h), Some(m), Some(s)) = (date.get(11..13), date.get(14..16), date.get(17..19)) {
        let (h, m, s): (f64, f64, f64) = (
            h.parse().unwrap_or(0.0),
            m.parse().unwrap_or(0.0),
            s.parse().unwrap_or(0.0),
        );
        days += (h * 3600.0 + m * 60.0 + s) / 86400.0;
    }

    Some(days)
}

/// Convert fractional days since the unix epoch back into a (year, month) pair
fn civil_from_days(z: i64) -> (i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m)
}

const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub enum ChartRange {
    Month1,
    Months3,
    Months6,
    Year1,
    #[default]
    All,
}

impl ChartRange {
    pub const ALL: [ChartRange; 5] = [
        ChartRange::Month1,
        ChartRange::Months3,
        ChartRange::Months6,
        ChartRange::Year1,
        ChartRange::All,
    ];

    pub fn from_query(value: Option<&str>) -> Self {
        match value {
            Some("1m") => ChartRange::Month1,
            Some("3m") => ChartRange::Months3,
            Some("6m") => ChartRange::Months6,
            Some("1y") => ChartRange::Year1,
            _ => ChartRange::All,
        }
    }

    fn query_value(self) -> &'static str {
        match self {
            ChartRange::Month1 => "1m",
            ChartRange::Months3 => "3m",
            ChartRange::Months6 => "6m",
            ChartRange::Year1 => "1y",
            ChartRange::All => "all",
        }
    }

    fn label(self) -> &'static str {
        match self {
            ChartRange::Month1 => "1 month",
            ChartRange::Months3 => "3 months",
            ChartRange::Months6 => "6 months",
            ChartRange::Year1 => "1 year",
            ChartRange::All => "All time",
        }
    }

    fn days(self) -> Option<f64> {
        match self {
            ChartRange::Month1 => Some(30.0),
            ChartRange::Months3 => Some(91.0),
            ChartRange::Months6 => Some(183.0),
            ChartRange::Year1 => Some(365.0),
            ChartRange::All => None,
        }
    }

    /// The earliest date (in fractional epoch-days) included in the range,
    /// measured back from the date of the latest run
    fn min_x(self, history: &WptHistory) -> f64 {
        let latest = history
            .runs
            .last()
            .and_then(|run| parse_date(&run.date))
            .unwrap_or(0.0);
        match self.days() {
            Some(days) => latest - days,
            None => f64::NEG_INFINITY,
        }
    }
}

/// A single line on the history chart
#[derive(Clone, PartialEq)]
pub struct ChartSeries {
    /// The focus area charted (e.g. "css/css-flexbox")
    pub area: String,
    /// The label shown in the legend and tooltip
    pub label: String,
    pub color: &'static str,
}

struct Series {
    label: String,
    color: &'static str,
    points: Vec<(f64, f64)>,
}

/// The subtest total of the most recent run with data for this area.
/// Percentages are computed against this so that adding tests to WPT does
/// not distort historical pass rates.
fn latest_subtest_total(history: &WptHistory, area_idx: usize) -> Option<u32> {
    history
        .runs
        .iter()
        .rev()
        .filter_map(|run| *run.scores.get(area_idx)?)
        .map(|(_, _, total_subtests, _)| total_subtests)
        .find(|total| *total != 0)
}

fn subtest_pass_percent(run: &HistoryRun, area_idx: usize, latest_total: u32) -> Option<f64> {
    let (_, _, total_subtests, total_subtests_passed) = (*run.scores.get(area_idx)?)?;
    if total_subtests == 0 {
        return None;
    }
    Some(total_subtests_passed as f64 / latest_total as f64 * 100.0)
}

fn area_series(history: &WptHistory, min_x: f64, spec: &ChartSeries) -> Option<Series> {
    let area_idx = history.focus_areas.iter().position(|a| *a == spec.area)?;
    let latest_total = latest_subtest_total(history, area_idx)?;
    let points: Vec<(f64, f64)> = history
        .runs
        .iter()
        .filter_map(|run| {
            Some((
                parse_date(&run.date)?,
                subtest_pass_percent(run, area_idx, latest_total)?,
            ))
        })
        .filter(|(x, _)| *x >= min_x)
        .collect();
    if points.is_empty() {
        return None;
    }
    Some(Series {
        label: spec.label.clone(),
        color: spec.color,
        points,
    })
}

fn polyline_points(
    points: &[(f64, f64)],
    x_min: f64,
    x_max: f64,
    plot: (f64, f64, f64, f64), // (x, y, width, height) of plot area
) -> String {
    if points.is_empty() {
        return String::new();
    }

    let (px, py, pw, ph) = plot;
    let x_range = (x_max - x_min).max(f64::EPSILON);

    // Downsample long series to at most ~400 points to keep the SVG small
    let stride = points.len().div_ceil(400).max(1);
    let mut out = String::new();
    let mut plot_point = |&(x, y): &(f64, f64)| {
        let sx = px + (x - x_min) / x_range * pw;
        let sy = py + (1.0 - y / 100.0) * ph;
        write!(out, "{sx:.1},{sy:.1} ").unwrap();
    };
    for point in points.iter().step_by(stride) {
        plot_point(point);
    }
    // Always include the final point so the latest result is shown
    let last_plotted = (points.len() - 1) / stride * stride;
    if last_plotted != points.len() - 1 {
        plot_point(&points[points.len() - 1]);
    }
    out
}

/// Month boundaries (as fractional epoch-days) within the given range, for x-axis ticks
fn month_ticks(x_min: f64, x_max: f64) -> Vec<(f64, String)> {
    let (mut year, mut month) = civil_from_days(x_min.floor() as i64);
    // Advance to the first month boundary >= x_min
    month += 1;
    if month > 12 {
        month = 1;
        year += 1;
    }

    let mut ticks = Vec::new();
    loop {
        let days = days_from_civil(year, month, 1) as f64;
        if days > x_max {
            break;
        }
        let label = format!("{} {}", MONTH_NAMES[(month - 1) as usize], year);
        ticks.push((days, label));
        month += 1;
        if month > 12 {
            month = 1;
            year += 1;
        }
    }

    // Thin out ticks if there are too many to label legibly
    let stride = ticks.len().div_ceil(12).max(1);
    ticks.into_iter().step_by(stride).collect()
}

#[component]
pub fn WptHistoryPage(history: ArcWptHistory, range: ChartRange) -> Element {
    rsx! {
        Page { title: "Status: WPT History".into(),
            StatusHeader {}
            StatusTabs { current_tab: "history" }
            p {
                dangerous_inner_html: r#"
                This page charts Blitz's scores on the "css" subsuite of the <a href="https://github.com/web-platform-tests/wpt" target="_blank">Web Platform Tests</a> over time.
                Scores are the percentage of subtests passing (of those that Blitz can run). Data is recorded for every commit to the main branch."#
            }
            hr {}
            ChartRangeSelector { current_range: range, base_path: "/status/wpt/history" }
            WptHistoryChart {
                history: history.clone(),
                series_spec: highlight_series(),
                range,
            }
            h2 { "Per-area history" }
            WptHistorySparklines { history, range }
        }
    }
}

#[component]
pub fn ChartRangeSelector(current_range: ChartRange, base_path: String) -> Element {
    rsx! {
        div {
            display: "flex",
            gap: "8px",
            justify_content: "flex-end",
            font_size: "14px",
            margin_bottom: "8px",

            for range in ChartRange::ALL {
                a {
                    href: "{base_path}?range={range.query_value()}",
                    padding: "2px 10px",
                    border_radius: "12px",
                    text_decoration: "none",
                    color: if range == current_range { "white" } else { "inherit" },
                    background_color: if range == current_range { "#2b6e6c" } else { "#eee" },
                    {range.label()}
                }
            }
        }
    }
}

/// Colors assigned to chart lines, in order. The first is reserved for the
/// "whole folder" line.
pub const SERIES_COLORS: &[&str] = &[
    "#000000", "#e57373", "#7986cb", "#4db6ac", "#ffb74d", "#ba68c8", "#64b5f6", "#a1887f",
];

const HIGHLIGHT_AREAS: &[&str] = &[
    "css",
    "css/CSS2",
    "css/css-flexbox",
    "css/css-grid",
    "css/css-text",
    "css/css-position",
];

fn highlight_series() -> Vec<ChartSeries> {
    HIGHLIGHT_AREAS
        .iter()
        .zip(SERIES_COLORS)
        .map(|(area, color)| ChartSeries {
            area: area.to_string(),
            label: area
                .strip_prefix("css/")
                .unwrap_or("all css")
                .to_string(),
            color,
        })
        .collect()
}

#[component]
pub fn WptHistoryChart(
    history: ArcWptHistory,
    series_spec: Vec<ChartSeries>,
    range: ChartRange,
    #[props(default = 440.0)] height: f64,
) -> Element {
    const WIDTH: f64 = 900.0;
    let plot: (f64, f64, f64, f64) = (50.0, 15.0, WIDTH - 65.0, height - 55.0);
    let (px, py, pw, ph) = plot;

    let min_x = range.min_x(&history);
    let series: Vec<Series> = series_spec
        .iter()
        .filter_map(|spec| area_series(&history, min_x, spec))
        .collect();

    let x_min = series
        .iter()
        .filter_map(|s| s.points.first().map(|p| p.0))
        .fold(f64::INFINITY, f64::min);
    let x_max = series
        .iter()
        .filter_map(|s| s.points.last().map(|p| p.0))
        .fold(f64::NEG_INFINITY, f64::max);

    if !x_min.is_finite() || !x_max.is_finite() {
        return rsx!( p { "No history data available" } );
    }

    let ticks = month_ticks(x_min, x_max);
    let x_range = (x_max - x_min).max(f64::EPSILON);

    // Per-run data for the hover tooltip (a JS progressive enhancement)
    let area_indices: Vec<Option<usize>> = series_spec
        .iter()
        .map(|spec| history.focus_areas.iter().position(|a| *a == spec.area))
        .collect();
    // Latest subtest total per series: the denominator for plotted
    // percentages (must match `area_series`)
    let latest_totals: Vec<Option<u32>> = area_indices
        .iter()
        .map(|idx| latest_subtest_total(&history, (*idx)?))
        .collect();
    // Include the run immediately before the visible range (if any) so the
    // first visible run's tooltip can show a delta against it
    let first_visible = history
        .runs
        .iter()
        .position(|run| parse_date(&run.date).is_some_and(|x| x >= min_x))
        .unwrap_or(0);
    let start = first_visible.saturating_sub(1);
    let runs_json: Vec<serde_json::Value> = history.runs[start..]
        .iter()
        .filter_map(|run| {
            let x = parse_date(&run.date)?;
            let values: Vec<serde_json::Value> = area_indices
                .iter()
                .map(|idx| {
                    idx.and_then(|idx| *run.scores.get(idx)?)
                        .filter(|(_, _, total_subtests, _)| *total_subtests != 0)
                        .map(|(_, _, total_subtests, total_subtests_passed)| {
                            serde_json::json!([total_subtests_passed, total_subtests])
                        })
                        .unwrap_or(serde_json::Value::Null)
                })
                .collect();
            Some(serde_json::json!({
                "x": x,
                "d": run.date.split('T').next().unwrap_or(&run.date),
                "sha": run.product_revision,
                "msg": run.commit_message,
                "v": values,
            }))
        })
        .collect();
    let tooltip_data = serde_json::json!({
        "first": first_visible - start,
        "width": WIDTH,
        "plot": [px, py, pw, ph],
        "xMin": x_min,
        "xMax": x_max,
        "series": series_spec
            .iter()
            .zip(&latest_totals)
            .map(|(s, latest)| serde_json::json!({ "name": s.label, "color": s.color, "latest": latest }))
            .collect::<Vec<_>>(),
        "runs": runs_json,
    })
    .to_string()
    // Prevent "</script>" in commit messages from terminating the data block
    .replace('<', "\\u003c");

    rsx! {
        div {
            position: "relative",

        svg {
            view_box: "0 0 {WIDTH} {height}",
            width: "100%",
            font_family: "sans-serif",

            // Horizontal gridlines + y-axis labels (every 10%; label every
            // 20% when the chart is short)
            for i in 0..=10u32 {
                line {
                    x1: "{px}",
                    x2: "{px + pw}",
                    y1: "{py + ph * (1.0 - (i as f64) / 10.0)}",
                    y2: "{py + ph * (1.0 - (i as f64) / 10.0)}",
                    stroke: if i % 5 == 0 { "#bbb" } else { "#e5e5e5" },
                    stroke_width: "1",
                }
                if ph >= 300.0 || i % 2 == 0 {
                    text {
                        x: "{px - 6.0}",
                        y: "{py + ph * (1.0 - (i as f64) / 10.0) + 4.0}",
                        text_anchor: "end",
                        font_size: "12",
                        fill: "#666",
                        "{i * 10}%"
                    }
                }
            }

            // X-axis ticks and labels (month boundaries)
            for (days, label) in ticks {
                line {
                    x1: "{px + (days - x_min) / x_range * pw}",
                    x2: "{px + (days - x_min) / x_range * pw}",
                    y1: "{py}",
                    y2: "{py + ph + 4.0}",
                    stroke: "#e5e5e5",
                    stroke_width: "1",
                }
                text {
                    x: "{px + (days - x_min) / x_range * pw}",
                    y: "{py + ph + 18.0}",
                    text_anchor: "middle",
                    font_size: "12",
                    fill: "#666",
                    {label}
                }
            }

            // Data series
            for (i, s) in series.iter().enumerate() {
                polyline {
                    points: polyline_points(&s.points, x_min, x_max, plot),
                    fill: "none",
                    stroke: s.color,
                    stroke_width: if i == 0 { "2.5" } else { "1.5" },
                }
            }

            // Legend
            for (i, s) in series.iter().enumerate() {
                line {
                    x1: "{px + 10.0 + (i as f64) * 140.0}",
                    x2: "{px + 34.0 + (i as f64) * 140.0}",
                    y1: "{height - 10.0}",
                    y2: "{height - 10.0}",
                    stroke: s.color,
                    stroke_width: "3",
                }
                text {
                    x: "{px + 40.0 + (i as f64) * 140.0}",
                    y: "{height - 6.0}",
                    font_size: "12",
                    fill: "#333",
                    {s.label.clone()}
                }
            }
        }

        script {
            "data-wpt-history-data": "true",
            r#type: "application/json",
            dangerous_inner_html: tooltip_data,
        }
        script { src: "/static/wpt-history-tooltip.js" }
        }
    }
}

#[component]
pub fn WptHistorySparklines(history: ArcWptHistory, range: ChartRange) -> Element {
    const WIDTH: f64 = 260.0;
    const HEIGHT: f64 = 60.0;
    const PLOT: (f64, f64, f64, f64) = (0.0, 2.0, WIDTH, HEIGHT - 4.0);

    let range_min_x = range.min_x(&history);
    let x_min = history
        .runs
        .first()
        .and_then(|run| parse_date(&run.date))
        .unwrap_or(0.0)
        .max(range_min_x);
    let x_max = history
        .runs
        .last()
        .and_then(|run| parse_date(&run.date))
        .unwrap_or(1.0);

    rsx! {
        div {
            display: "grid",
            grid_template_columns: "repeat(auto-fill, minmax(280px, 1fr))",
            gap: "16px",

            for (area_idx, area) in history.focus_areas.iter().enumerate() {
                {
                    let latest_total = latest_subtest_total(&history, area_idx);
                    let points: Vec<(f64, f64)> = history
                        .runs
                        .iter()
                        .filter_map(|run| {
                            Some((
                                parse_date(&run.date)?,
                                subtest_pass_percent(run, area_idx, latest_total?)?,
                            ))
                        })
                        .filter(|(x, _)| *x >= range_min_x)
                        .collect();

                    let latest = points.last().map(|p| p.1).unwrap_or(0.0);

                    rsx! {
                        a {
                            href: "/status/wpt/{area}",
                            color: "inherit",
                            text_decoration: "none",
                            div {
                                display: "flex",
                                justify_content: "space-between",
                                font_size: "13px",
                                span { {area.clone()} }
                                span { {format!("{latest:.1}%")} }
                            }
                            svg {
                                view_box: "0 0 {WIDTH} {HEIGHT}",
                                width: "100%",
                                style: "border: 1px solid #ddd",
                                polyline {
                                    points: polyline_points(&points, x_min, x_max, PLOT),
                                    fill: "none",
                                    stroke: "#7986cb",
                                    stroke_width: "1.5",
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
