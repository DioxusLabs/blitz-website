use std::{fmt::Write, ops::Deref, sync::Arc};

use dioxus::prelude::*;
use wptreport::score_summary::{RunSummary, ScoreSummaryReport};

use crate::routes::{StatusHeader, StatusTabs};
use crate::components::Page;

#[derive(Clone)]
pub struct ArcCommitMessages(pub Arc<std::collections::HashMap<String, String>>);
impl PartialEq for ArcCommitMessages {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Deref for ArcCommitMessages {
    type Target = std::collections::HashMap<String, String>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Clone)]
pub struct ArcWptSummary(pub Arc<ScoreSummaryReport>);
impl PartialEq for ArcWptSummary {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Deref for ArcWptSummary {
    type Target = ScoreSummaryReport;
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
    fn min_x(self, summary: &ScoreSummaryReport) -> f64 {
        let latest = summary
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

struct Series {
    name: &'static str,
    color: &'static str,
    points: Vec<(f64, f64)>,
}

fn subtest_pass_percent(run: &RunSummary, area_idx: usize) -> Option<f64> {
    let scores = run.scores.get(area_idx)?;
    if scores.total_subtests == 0 {
        return None;
    }
    Some(scores.total_subtests_passed as f64 / scores.total_subtests as f64 * 100.0)
}

fn area_series(
    summary: &ScoreSummaryReport,
    min_x: f64,
    area: &'static str,
    color: &'static str,
) -> Option<Series> {
    let area_idx = summary.focus_areas.iter().position(|a| a == area)?;
    let points: Vec<(f64, f64)> = summary
        .runs
        .iter()
        .filter_map(|run| Some((parse_date(&run.date)?, subtest_pass_percent(run, area_idx)?)))
        .filter(|(x, _)| *x >= min_x)
        .collect();
    if points.is_empty() {
        return None;
    }
    Some(Series {
        name: area,
        color,
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
pub fn WptHistoryPage(
    summary: ArcWptSummary,
    range: ChartRange,
    commit_messages: ArcCommitMessages,
) -> Element {
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
            ChartRangeSelector { current_range: range }
            WptHistoryChart { summary: summary.clone(), range, commit_messages }
            h2 { "Per-area history" }
            WptHistorySparklines { summary, range }
        }
    }
}

#[component]
pub fn ChartRangeSelector(current_range: ChartRange) -> Element {
    rsx! {
        div {
            display: "flex",
            gap: "8px",
            justify_content: "flex-end",
            font_size: "14px",
            margin_bottom: "8px",

            for range in ChartRange::ALL {
                a {
                    href: "/status/wpt/history?range={range.query_value()}",
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

const HIGHLIGHT_AREAS: &[(&str, &str)] = &[
    ("css", "#000000"),
    ("css/CSS2", "#e57373"),
    ("css/css-flexbox", "#7986cb"),
    ("css/css-grid", "#4db6ac"),
    ("css/css-text", "#ffb74d"),
    ("css/css-position", "#ba68c8"),
];

/// Inline script implementing the hover tooltip as a progressive enhancement.
/// Reads run data from the `#wpt-history-data` JSON blob and shows the nearest
/// run's commit id, commit message, and per-area pass percentages.
const TOOLTIP_JS: &str = r##"
(function () {
    var container = document.getElementById("wpt-history-chart");
    var dataEl = document.getElementById("wpt-history-data");
    if (!container || !dataEl) return;
    var svg = container.querySelector("svg");
    var data = JSON.parse(dataEl.textContent);
    if (!svg || !data.runs.length) return;

    var tip = document.createElement("div");
    tip.style.cssText =
        "position:absolute;pointer-events:none;display:none;background:rgba(255,255,255,0.96);" +
        "border:1px solid #999;border-radius:4px;padding:6px 8px;font:12px sans-serif;" +
        "box-shadow:0 1px 4px rgba(0,0,0,0.25);z-index:10;width:260px;box-sizing:border-box";
    container.appendChild(tip);

    var guide = document.createElementNS("http://www.w3.org/2000/svg", "line");
    guide.setAttribute("stroke", "#888");
    guide.setAttribute("stroke-dasharray", "3,3");
    guide.setAttribute("y1", data.plot[1]);
    guide.setAttribute("y2", data.plot[1] + data.plot[3]);
    guide.style.display = "none";
    svg.appendChild(guide);

    var dot = document.createElementNS("http://www.w3.org/2000/svg", "circle");
    dot.setAttribute("r", 4);
    dot.setAttribute("stroke", "white");
    dot.setAttribute("stroke-width", 1.5);
    dot.style.display = "none";
    svg.appendChild(dot);

    function esc(s) {
        return s.replace(/&/g, "&amp;").replace(/</g, "&lt;");
    }

    // Nearest hoverable run (runs before data.first only serve as deltas)
    function nearest(x) {
        var runs = data.runs, lo = data.first, hi = runs.length - 1;
        while (lo < hi) {
            var mid = (lo + hi) >> 1;
            if (runs[mid].x < x) lo = mid + 1; else hi = mid;
        }
        if (lo > data.first && Math.abs(runs[lo - 1].x - x) < Math.abs(runs[lo].x - x)) lo--;
        return lo;
    }

    function hide() {
        tip.style.display = "none";
        guide.style.display = "none";
        dot.style.display = "none";
    }

    // Only show the series whose line is within this vertical distance
    // (in viewBox units) of the cursor
    var Y_THRESHOLD = 12;

    svg.addEventListener("mousemove", function (ev) {
        var rect = svg.getBoundingClientRect();
        var scale = data.width / rect.width;
        var vx = (ev.clientX - rect.left) * scale;
        var vy = (ev.clientY - rect.top) * scale;
        var px = data.plot[0], py = data.plot[1], pw = data.plot[2], ph = data.plot[3];
        if (vx < px || vx > px + pw) { hide(); return; }

        var xRange = data.xMax - data.xMin;
        var runIdx = nearest(data.xMin + ((vx - px) / pw) * xRange);
        var run = data.runs[runIdx];
        var prev = runIdx > 0 ? data.runs[runIdx - 1] : null;

        // Pick the single series whose line is vertically nearest the cursor
        var best = -1, bestDist = Y_THRESHOLD;
        for (var i = 0; i < data.series.length; i++) {
            if (run.v[i] == null) continue;
            var sy = py + (1 - (run.v[i][0] / run.v[i][1])) * ph;
            var dist = Math.abs(sy - vy);
            if (dist < bestDist) { best = i; bestDist = dist; }
        }
        if (best < 0) { hide(); return; }

        var runVx = px + ((run.x - data.xMin) / xRange) * pw;
        guide.setAttribute("x1", runVx);
        guide.setAttribute("x2", runVx);
        guide.style.display = "";

        dot.setAttribute("cx", runVx);
        dot.setAttribute("cy", py + (1 - (run.v[best][0] / run.v[best][1])) * ph);
        dot.setAttribute("fill", data.series[best].color);
        dot.style.display = "";

        var html = "<div style='font-weight:bold'>" + esc(run.sha.slice(0, 9)) + " (" + esc(run.d) + ")</div>";
        if (run.msg) {
            html += "<div style='margin-bottom:4px;white-space:nowrap;overflow:hidden;" +
                "text-overflow:ellipsis'>" + esc(run.msg) + "</div>";
        }
        var pass = run.v[best][0], total = run.v[best][1];
        html += "<div><span style='color:" + data.series[best].color + "'>\u25CF</span> " +
            esc(data.series[best].name) + ": " + (100 * pass / total).toFixed(1) + "% (" +
            pass.toLocaleString() + "/" + total.toLocaleString() + ")</div>";

        // Change relative to the previous run
        if (prev && prev.v[best] != null) {
            var dPass = pass - prev.v[best][0];
            var dPct = 100 * (pass / total - prev.v[best][0] / prev.v[best][1]);
            var sign = dPass > 0 ? "+" : "";
            var color = dPass > 0 ? "#2e7d32" : (dPass < 0 ? "#c62828" : "#666");
            html += "<div style='color:" + color + "'>Change: " + sign +
                dPass.toLocaleString() + " (" + sign + dPct.toFixed(2) + "%)</div>";
        }
        tip.innerHTML = html;
        tip.style.display = "block";

        var crect = container.getBoundingClientRect();
        var cx = ev.clientX - crect.left, cy = ev.clientY - crect.top;
        var left = cx + 14;
        if (left + tip.offsetWidth > container.clientWidth) left = cx - tip.offsetWidth - 14;
        tip.style.left = left + "px";
        tip.style.top = (cy + 14) + "px";
    });
    svg.addEventListener("mouseleave", hide);
})();
"##;

#[component]
pub fn WptHistoryChart(
    summary: ArcWptSummary,
    range: ChartRange,
    commit_messages: ArcCommitMessages,
) -> Element {
    const WIDTH: f64 = 900.0;
    const HEIGHT: f64 = 440.0;
    const PLOT: (f64, f64, f64, f64) = (50.0, 15.0, WIDTH - 65.0, HEIGHT - 55.0);
    let (px, py, pw, ph) = PLOT;

    let min_x = range.min_x(&summary);
    let series: Vec<Series> = HIGHLIGHT_AREAS
        .iter()
        .filter_map(|(area, color)| area_series(&summary, min_x, area, color))
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
    let area_indices: Vec<Option<usize>> = HIGHLIGHT_AREAS
        .iter()
        .map(|(area, _)| summary.focus_areas.iter().position(|a| a == area))
        .collect();
    // Include the run immediately before the visible range (if any) so the
    // first visible run's tooltip can show a delta against it
    let first_visible = summary
        .runs
        .iter()
        .position(|run| parse_date(&run.date).is_some_and(|x| x >= min_x))
        .unwrap_or(0);
    let start = first_visible.saturating_sub(1);
    let runs_json: Vec<serde_json::Value> = summary.runs[start..]
        .iter()
        .filter_map(|run| {
            let x = parse_date(&run.date)?;
            let values: Vec<serde_json::Value> = area_indices
                .iter()
                .map(|idx| {
                    idx.and_then(|idx| run.scores.get(idx))
                        .filter(|scores| scores.total_subtests != 0)
                        .map(|scores| {
                            serde_json::json!([
                                scores.total_subtests_passed,
                                scores.total_subtests
                            ])
                        })
                        .unwrap_or(serde_json::Value::Null)
                })
                .collect();
            Some(serde_json::json!({
                "x": x,
                "d": run.date.split('T').next().unwrap_or(&run.date),
                "sha": run.product_revision,
                "msg": commit_messages.get(&run.product_revision),
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
        "series": series
            .iter()
            .map(|s| serde_json::json!({
                "name": s.name.strip_prefix("css/").unwrap_or("all css"),
                "color": s.color,
            }))
            .collect::<Vec<_>>(),
        "runs": runs_json,
    })
    .to_string()
    // Prevent "</script>" in commit messages from terminating the data block
    .replace('<', "\\u003c");

    rsx! {
        div {
            id: "wpt-history-chart",
            position: "relative",

        svg {
            view_box: "0 0 {WIDTH} {HEIGHT}",
            width: "100%",
            font_family: "sans-serif",

            // Horizontal gridlines + y-axis labels (every 10%)
            for i in 0..=10u32 {
                line {
                    x1: "{px}",
                    x2: "{px + pw}",
                    y1: "{py + ph * (1.0 - (i as f64) / 10.0)}",
                    y2: "{py + ph * (1.0 - (i as f64) / 10.0)}",
                    stroke: if i % 5 == 0 { "#bbb" } else { "#e5e5e5" },
                    stroke_width: "1",
                }
                text {
                    x: "{px - 6.0}",
                    y: "{py + ph * (1.0 - (i as f64) / 10.0) + 4.0}",
                    text_anchor: "end",
                    font_size: "12",
                    fill: "#666",
                    "{i * 10}%"
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
            for s in series.iter() {
                polyline {
                    points: polyline_points(&s.points, x_min, x_max, PLOT),
                    fill: "none",
                    stroke: s.color,
                    stroke_width: if s.name == "css" { "2.5" } else { "1.5" },
                }
            }

            // Legend
            for (i, s) in series.iter().enumerate() {
                line {
                    x1: "{px + 10.0 + (i as f64) * 140.0}",
                    x2: "{px + 34.0 + (i as f64) * 140.0}",
                    y1: "{HEIGHT - 10.0}",
                    y2: "{HEIGHT - 10.0}",
                    stroke: s.color,
                    stroke_width: "3",
                }
                text {
                    x: "{px + 40.0 + (i as f64) * 140.0}",
                    y: "{HEIGHT - 6.0}",
                    font_size: "12",
                    fill: "#333",
                    {s.name.strip_prefix("css/").unwrap_or("all css")}
                }
            }
        }

        script {
            id: "wpt-history-data",
            r#type: "application/json",
            dangerous_inner_html: tooltip_data,
        }
        script { dangerous_inner_html: TOOLTIP_JS }
        }
    }
}

#[component]
pub fn WptHistorySparklines(summary: ArcWptSummary, range: ChartRange) -> Element {
    const WIDTH: f64 = 260.0;
    const HEIGHT: f64 = 60.0;
    const PLOT: (f64, f64, f64, f64) = (0.0, 2.0, WIDTH, HEIGHT - 4.0);

    let range_min_x = range.min_x(&summary);
    let x_min = summary
        .runs
        .first()
        .and_then(|run| parse_date(&run.date))
        .unwrap_or(0.0)
        .max(range_min_x);
    let x_max = summary
        .runs
        .last()
        .and_then(|run| parse_date(&run.date))
        .unwrap_or(1.0);

    rsx! {
        div {
            display: "grid",
            grid_template_columns: "repeat(auto-fill, minmax(280px, 1fr))",
            gap: "16px",

            for (area_idx, area) in summary.focus_areas.iter().enumerate() {
                {
                    let points: Vec<(f64, f64)> = summary
                        .runs
                        .iter()
                        .filter_map(|run| {
                            Some((parse_date(&run.date)?, subtest_pass_percent(run, area_idx)?))
                        })
                        .filter(|(x, _)| *x >= range_min_x)
                        .collect();

                    let latest = points.last().map(|p| p.1).unwrap_or(0.0);

                    rsx! {
                        div {
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
