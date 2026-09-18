use dioxus::prelude::*;

use crate::{
    components::Page,
    routes::{encode_test_path, score_color, ChartLine, ChartRange, ChartRangeSelector, HistoryLineChart},
    wpt_db::{status_str, AreaScore, AreaSort, RunRow, TestRow, TestRunResult},
};

use super::wpt_focus_areas::FOCUS_AREA_SETS;

/// Display name for a product identifier (e.g. "chrome" -> "Chrome")
pub fn product_label(product: &str) -> String {
    let mut chars = product.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// The colour of a product's line on history charts
pub fn product_color(product: &str) -> &'static str {
    match product {
        "blitz" => "#000000",
        "chrome" => "#e57373",
        "firefox" => "#ffb74d",
        "safari" => "#64b5f6",
        "ladybird" => "#ba68c8",
        "servo" => "#4db6ac",
        "flow" => "#8d6e63",
        _ => "#a1887f",
    }
}

fn short_version(version: &str) -> String {
    // Nightly versions can be long (e.g. full commit hashes for Blitz/Servo)
    if version.len() > 12 {
        version[..9].to_string()
    } else {
        version.to_string()
    }
}

#[component]
pub fn WptComparePage(
    runs: Vec<RunRow>,
    area: String,
    sort: AreaSort,
    total: Vec<Option<AreaScore>>,
    child_areas: Vec<(String, Vec<Option<AreaScore>>)>,
    tests: Vec<TestRow>,
    history: Vec<ChartLine>,
    range: ChartRange,
    /// Whether the history chart starts expanded (it does when a range was
    /// chosen explicitly, so the range buttons don't collapse it)
    history_open: bool,
) -> Element {
    let child_prefix = if area.is_empty() {
        String::new()
    } else {
        format!("{area}/")
    };

    rsx! {
        Page { title: "WPT".into(),
            h1 { "WPT" }
            p {
                class: "introduction",
                dangerous_inner_html: r#"
                This page compares scores on the <a href="https://github.com/web-platform-tests/wpt" target="_blank">Web Platform Tests</a>
                across web engines, using the latest master runs from <a href="https://wpt.fyi" target="_blank">wpt.fyi</a> and Blitz's own test runner."#
            }
            hr {}
            if area.is_empty() {
                p {
                    font_size: "smaller",
                    b { "Focus areas: " }
                    for (i, set) in FOCUS_AREA_SETS.iter().enumerate() {
                        if i > 0 {
                            " | "
                        }
                        a { href: format!("/wpt/focus-areas/{}", set.slug), "{set.label}" }
                    }
                }
            }
            WptCompareBreadcrumb { area: area.clone() }
            SpecInfoDisplay { area: area.clone() }
            RunInfoDisplay { runs: runs.clone() }
            CompareHistoryChart { area: area.clone(), history, range, open: history_open }
            SortToggle { area: area.clone(), sort }
            table {
                width: "100%",
                tr {
                    th { width: "min-content", "Area" }
                    for run in &runs {
                        th { text_align: "center", {product_label(&run.product)} }
                    }
                }
                {compare_area_row("Total".to_string(), None, &total)}
                for (child, scores) in &child_areas {
                    {compare_area_row(
                        child[child_prefix.len().min(child.len())..].to_string(),
                        Some(format!("/wpt/{child}")),
                        scores,
                    )}
                }
            }
            if !tests.is_empty() {
                table {
                    width: "100%",
                    margin_top: "24px",
                    tr {
                        th { width: "min-content", "Test" }
                        for run in &runs {
                            th { text_align: "center", {product_label(&run.product)} }
                        }
                    }
                    for test in &tests {
                        CompareTestRow { test: test.clone() }
                    }
                }
            }
        }
    }
}

/// Score history of an area, one line per engine
#[component]
fn CompareHistoryChart(
    area: String,
    history: Vec<ChartLine>,
    range: ChartRange,
    open: bool,
) -> Element {
    if history.is_empty() {
        return rsx! {};
    }
    rsx! {
        details {
            open,
            summary { "Score history" }
            p {
                font_size: "smaller",
                "Percentage of subtests passing over time, one master run per day. Each engine's
                percentage is relative to the subtest count of its own latest run, so lines are
                not distorted by tests being added to WPT (Blitz only runs the subtests it can)."
            }
            ChartRangeSelector {
                current_range: range,
                base_path: if area.is_empty() { "/wpt".to_string() } else { format!("/wpt/{area}") },
            }
            HistoryLineChart { lines: history, range, height: 320.0 }
        }
    }
}

/// Shown when a comparison page can't be served right now (no run list
/// yet, or too many requests in flight), with `message` explaining why
#[component]
pub fn WptUnavailablePage(message: String) -> Element {
    rsx! {
        Page { title: "WPT".into(),
            h1 { "WPT" }
            p { {message} }
            p {
                a { href: "javascript:location.reload()", "Try again" }
                " | "
                a { href: "/status/wpt/css", "View the Blitz WPT dashboard" }
            }
        }
    }
}

#[component]
fn SortToggle(area: String, sort: AreaSort) -> Element {
    let base = if area.is_empty() {
        "/wpt".to_string()
    } else {
        format!("/wpt/{area}")
    };
    rsx! {
        p {
            font_size: "smaller",
            b { "Sort areas: " }
            if sort == AreaSort::Alpha {
                "alphabetical"
            } else {
                a { href: format!("{base}?sort=alpha"), "alphabetical" }
            }
            " | "
            if sort == AreaSort::Subtests {
                "by subtest count"
            } else {
                a { href: format!("{base}?sort=subtests"), "by subtest count" }
            }
        }
    }
}

#[component]
fn SpecInfoDisplay(area: String) -> Element {
    let Some(meta) = crate::wpt_spec_meta::lookup(&area) else {
        return rsx! {};
    };
    rsx! {
        p {
            font_size: "smaller",
            b { "Spec: " }
            a {
                href: meta.spec.clone(),
                target: "_blank",
                {meta.title.clone().unwrap_or_else(|| meta.spec.clone())}
                " \u{2197}"
            }
        }
    }
}

/// Parenthesised WPT revision and run date, e.g. " (wpt@1234abcde, 2026-09-01)"
fn run_details(run: &RunRow) -> String {
    let mut details: Vec<String> = Vec::new();
    if !run.wpt_revision.is_empty() {
        let short_rev = &run.wpt_revision[..run.wpt_revision.len().min(9)];
        details.push(format!("wpt@{short_rev}"));
    }
    if let Some(date) = run.run_time.as_deref().and_then(|t| t.get(..10)) {
        details.push(date.to_string());
    }
    if details.is_empty() {
        String::new()
    } else {
        format!(" ({})", details.join(", "))
    }
}

#[component]
pub(super) fn RunInfoDisplay(runs: Vec<RunRow>) -> Element {
    rsx! {
        p {
            font_size: "smaller",
            b { "Runs: " }
            for (idx, run) in runs.iter().enumerate() {
                if idx > 0 {
                    " | "
                }
                {product_label(&run.product)}
                " "
                {short_version(&run.browser_version)}
                {run_details(run)}
            }
        }
    }
}

#[component]
pub fn WptCompareBreadcrumb(area: String) -> Element {
    let mut prefix = String::new();
    let segments: Vec<(String, String)> = area
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(segment);
            (segment.to_string(), format!("/wpt/{prefix}"))
        })
        .collect();

    rsx!(
        p {
            a { href: "/wpt", "wpt" }
            for (segment, href) in segments {
                " / "
                a { href, {segment} }
            }
        }
    )
}

pub(super) fn compare_area_row(
    label: String,
    href: Option<String>,
    scores: &[Option<AreaScore>],
) -> Element {
    rsx!(
        tr {
            td {
                background_color: "white",
                if let Some(href) = href {
                    a { href, {label} }
                } else {
                    {label}
                }
            }
            for score in scores {
                if let Some(score) = score {
                    td {
                        text_align: "right",
                        background_color: score_color(score.subtest_fraction()),
                        title: format!(
                            "Tests fully passing: {}/{} | Interop score: {:.1}%",
                            score.tests_pass,
                            score.tests_total,
                            score.interop_fraction() * 100.0,
                        ),
                        {format!("{:.1}%", score.subtest_fraction() * 100.0)}
                        span {
                            display: "block",
                            font_size: "12px",
                            color: "rgba(0, 0, 0, 0.6)",
                            {format!("{}/{}", score.subtests_pass, score.subtests_total)}
                        }
                    }
                } else {
                    td {
                        text_align: "right",
                        background_color: score_color(0.0),
                        "NO DATA"
                    }
                }
            }
        }
    )
}

#[component]
fn CompareTestRow(test: TestRow) -> Element {
    let file_name = test
        .name
        .rsplit_once('/')
        .map(|(_, file)| file)
        .unwrap_or(&test.name)
        .to_string();
    let denom = test.denom.max(1);

    rsx!(
        tr {
            td {
                background_color: "white",
                a {
                    href: format!("/wpt/{}", encode_test_path(test.name.trim_start_matches('/'))),
                    {file_name}
                }
            }
            for result in &test.results {
                if let Some(result) = result {
                    td {
                        text_align: "right",
                        background_color: score_color(result.subtest_pass as f32 / denom as f32),
                        if denom > 1 {
                            {format!("{}/{}", result.subtest_pass, denom)}
                        } else if result.subtest_pass >= denom {
                            "PASS"
                        } else {
                            {status_label(*result)}
                        }
                    }
                } else {
                    td {
                        text_align: "right",
                        background_color: score_color(0.0),
                        "NOT RUN"
                    }
                }
            }
        }
    )
}

fn status_label(result: TestRunResult) -> String {
    match result.status {
        // A harness status of OK/PASS with failing subtests reads better
        // as FAIL in a single-cell summary
        0 => "FAIL".to_string(),
        status => status_str(status).to_string(),
    }
}
