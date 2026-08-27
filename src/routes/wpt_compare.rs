use dioxus::prelude::*;

use crate::{
    components::Page,
    routes::{encode_test_path, score_color, StatusHeader, StatusTabs},
    wpt_db::{status_str, AreaScore, RunRow, SubtestRow, TestDetail, TestRow, TestRunResult},
};

/// Display name for a product identifier (e.g. "chrome" -> "Chrome")
fn product_label(product: &str) -> String {
    let mut chars = product.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
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
    total: Vec<Option<AreaScore>>,
    child_areas: Vec<(String, Vec<Option<AreaScore>>)>,
    tests: Vec<TestRow>,
) -> Element {
    let child_prefix = if area.is_empty() {
        String::new()
    } else {
        format!("{area}/")
    };

    rsx! {
        Page { title: "Status: WPT Comparison".into(),
            StatusHeader {}
            StatusTabs { current_tab: "wpt-compare" }
            p {
                dangerous_inner_html: r#"
                This page compares scores on the <a href="https://github.com/web-platform-tests/wpt" target="_blank">Web Platform Tests</a>
                across web engines, using the latest master runs from <a href="https://wpt.fyi" target="_blank">wpt.fyi</a> and Blitz's own test runner."#
            }
            p {
                font_size: "smaller",
                "All engines are scored against the same denominator: the union of tests reported by any engine,
                with each test's subtest total being the maximum reported across engines. Tests an engine did not
                run count as failing. Blitz's numbers therefore differ from the "
                a { href: "/status/wpt", "Blitz-normalized WPT dashboard" }
                ", which ignores tests Blitz cannot run."
            }
            hr {}
            WptCompareBreadcrumb { area: area.clone() }
            RunInfoDisplay { runs: runs.clone() }
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
                        Some(format!("/status/wpt-compare/{child}")),
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

#[component]
fn RunInfoDisplay(runs: Vec<RunRow>) -> Element {
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
                if !run.wpt_revision.is_empty() {
                    {format!(" (wpt@{})", &run.wpt_revision[..run.wpt_revision.len().min(9)])}
                }
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
            (segment.to_string(), format!("/status/wpt-compare/{prefix}"))
        })
        .collect();

    rsx!(
        p {
            a { href: "/status/wpt-compare", "wpt" }
            for (segment, href) in segments {
                " / "
                a { href, {segment} }
            }
        }
    )
}

fn compare_area_row(label: String, href: Option<String>, scores: &[Option<AreaScore>]) -> Element {
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
                    href: format!("/status/wpt-compare/{}", encode_test_path(test.name.trim_start_matches('/'))),
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

#[component]
pub fn WptCompareTestPage(runs: Vec<RunRow>, detail: TestDetail) -> Element {
    let name = detail.name.clone();
    let file_name = name
        .rsplit_once('/')
        .map(|(_, file)| file)
        .unwrap_or(&name)
        .to_string();
    let denom = detail
        .results
        .iter()
        .flatten()
        .map(|result| result.subtest_total)
        .max()
        .unwrap_or(1)
        .max(1);

    rsx! {
        Page { title: format!("WPT Comparison: {file_name}").into(),
            StatusHeader {}
            StatusTabs { current_tab: "wpt-compare" }
            WptCompareBreadcrumb { area: name.trim_start_matches('/').to_string() }
            RunInfoDisplay { runs: runs.clone() }
            p {
                a {
                    href: format!("/status/wpt/{}", crate::routes::encode_test_path(name.trim_start_matches('/'))),
                    "View on the Blitz WPT dashboard"
                }
                " | "
                a {
                    href: format!("https://wpt.live/{name}"),
                    target: "_blank",
                    "Open test on wpt.live"
                }
                " | "
                a {
                    href: format!("https://wpt.fyi/results/{name}"),
                    target: "_blank",
                    "wpt.fyi"
                }
            }
            table {
                width: "100%",
                tr {
                    th { width: "min-content", "" }
                    for run in &runs {
                        th { text_align: "center", {product_label(&run.product)} }
                    }
                }
                tr {
                    td { background_color: "white", b { "Total" } }
                    for result in &detail.results {
                        if let Some(result) = result {
                            td {
                                text_align: "right",
                                background_color: score_color(result.subtest_pass as f32 / denom as f32),
                                {format!("{}/{}", result.subtest_pass, denom)}
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
            }
            if !detail.subtests.is_empty() {
                table {
                    width: "100%",
                    margin_top: "24px",
                    tr {
                        th { "Subtest" }
                        for run in &runs {
                            th { text_align: "center", {product_label(&run.product)} }
                        }
                    }
                    for subtest in &detail.subtests {
                        CompareSubtestRow { subtest: subtest.clone() }
                    }
                }
            }
        }
    }
}

#[component]
fn CompareSubtestRow(subtest: SubtestRow) -> Element {
    rsx!(
        tr {
            td { background_color: "white", {subtest.name.clone()} }
            for status in &subtest.statuses {
                if let Some(status) = status {
                    td {
                        text_align: "right",
                        background_color: score_color((*status == 0) as u32 as f32),
                        {status_str(*status)}
                    }
                } else {
                    td {
                        text_align: "right",
                        background_color: "#eee",
                        "—"
                    }
                }
            }
        }
    )
}
