use dioxus::prelude::*;

use crate::{
    components::{CommitInfoDisplay, Page},
    github::CommitInfo,
    routes::{
        ArcWptHistory, ChartRange, ChartRangeSelector, ChartSeries, StatusHeader, StatusTabs,
        WptHistoryChart, SERIES_COLORS,
    },
    wpt_db::{status_str, AreaScore, TestRow},
};

struct Colors(&'static [[u8; 3]]);

impl Colors {
    fn get(&self, pass_fraction: f32) -> [u8; 3] {
        if pass_fraction == 0.0 {
            return self.0[0];
        }

        if pass_fraction == 1.0 {
            return self.0[self.0.len() - 1];
        }

        self.0[((self.0.len() - 2) as f32 * pass_fraction).floor() as usize + 1]
    }
}

/// The background color for a score cell, from the shared red-to-green ramp
pub fn score_color(pass_fraction: f32) -> String {
    let color = COLORS.get(pass_fraction);
    format!("rgb({},{},{})", color[0], color[1], color[2])
}

const COLORS: Colors = Colors(&[
    [229, 115, 115],
    [255, 183, 77],
    [255, 213, 79],
    [255, 241, 118],
    [220, 231, 117],
    [174, 213, 129],
    [129, 199, 132],
]);

/// Blitz's latest results for one WPT folder, from the comparison database
#[derive(Clone, PartialEq)]
pub struct BlitzAreaResults {
    pub area: String,
    /// The folder's own score (None if Blitz has no data for it)
    pub score: Option<AreaScore>,
    /// Direct child folders, largest first
    pub child_areas: Vec<(String, Option<AreaScore>)>,
    /// Tests directly in the folder, with Blitz's result as the only run
    pub tests: Vec<TestRow>,
}

#[component]
pub fn WptResultsPage(
    results: BlitzAreaResults,
    commit_info: Option<CommitInfo>,
    history: Option<ArcWptHistory>,
    range: ChartRange,
) -> Element {
    let area = results.area.clone();
    rsx! {
        Page { title: "Status: WPT".into(),
            StatusHeader {}
            StatusTabs { current_tab: "wpt" }
            p {
                dangerous_inner_html: r#"
                This page documents Blitz's scores on the "css" subsuite of the <a href="https://github.com/web-platform-tests/wpt" target="_blank">Web Platform Tests</a>."#
            }
            hr {}
            WptBreadcrumb { area: area.clone() }
            FolderHistoryChart {
                folder: area.clone(),
                history,
                range,
                base_path: "/status/wpt/{area}",
            }
            CommitInfoDisplay { commit_info, label: "Data from commit:" }
            WptAreaResults { results }
        }
    }
}

/// A short history chart for a folder, showing a single line for the
/// folder's total
#[component]
fn FolderHistoryChart(
    folder: String,
    history: Option<ArcWptHistory>,
    range: ChartRange,
    base_path: String,
) -> Element {
    let Some(history) = history else {
        return rsx!( p { "No history data available" } );
    };

    let series_spec = vec![ChartSeries {
        area: folder.clone(),
        label: if folder == "css" {
            "all css".to_string()
        } else {
            folder.clone()
        },
        color: SERIES_COLORS[0],
    }];

    rsx! {
        details {
            summary { "Score history" }
            ChartRangeSelector { current_range: range, base_path }
            WptHistoryChart { history, series_spec, range, height: 240.0 }
        }
    }
}

#[component]
pub fn WptBreadcrumb(area: String) -> Element {
    let mut prefix = String::new();
    let segments: Vec<(String, String)> = area
        .split('/')
        .map(|segment| {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(segment);
            (segment.to_string(), format!("/status/wpt/{prefix}"))
        })
        .collect();

    rsx!(
        p {
            a { href: "/status/wpt", "wpt" }
            for (segment, href) in segments {
                " / "
                a { href, {segment} }
            }
        }
    )
}

#[component]
pub fn WptAreaResults(results: BlitzAreaResults) -> Element {
    let BlitzAreaResults {
        area,
        score,
        child_areas,
        tests,
    } = results;
    let child_prefix = format!("{area}/");

    rsx!(
        table {
            width: "100%",
            tr {
                th { width: "min-content", "Area",  }
                th { "Interop Score", }
                th { "Tests", }
                th { "Test %", }
                th { "Subtests" }
                th { "Subtest %" }
            }
            {area_score_row("Total".to_string(), None, score.unwrap_or_default())}
            for (key, scores) in child_areas {
                {area_score_row(
                    key[child_prefix.len()..].to_string(),
                    Some(format!("/status/wpt/{key}")),
                    scores.unwrap_or_default(),
                )}
            }
        }
        if !tests.is_empty() {
            table {
                width: "100%",
                margin_top: "24px",
                tr {
                    th { width: "min-content", "Test",  }
                    th { "Subtests" }
                    th { "Subtest %" }
                    th { "Status", }
                }
                for test in tests {
                    TestScoreRow { test }
                }
            }
        }
    )
}

fn area_score_row(label: String, href: Option<String>, scores: AreaScore) -> Element {
    let test_fraction = if scores.tests_total == 0 {
        0.0
    } else {
        scores.tests_pass as f32 / scores.tests_total as f32
    };

    rsx!(
        tr {
            background_color: score_color(scores.subtest_fraction()),
            td {
                background_color: "white",
                if let Some(href) = href {
                    a { href, {label} }
                } else {
                    {label}
                }
            }
            td {
                text_align: "right",
                {format!("{:.2}%", scores.interop_fraction() * 100.0)}
            }
            td {
                text_align: "right",
                {format!("({}/{})", scores.tests_pass, scores.tests_total)}
            }
            td {
                text_align: "right",
                {format!("{:.2}%", test_fraction * 100.0)}
            }
            td {
                text_align: "right",
                {format!("({}/{})", scores.subtests_pass, scores.subtests_total)}
            }
            td {
                text_align: "right",
                {format!("{:.2}%", scores.subtest_fraction() * 100.0)}
            }
        }
    )
}

/// A test row on the folder page: `test.results` holds Blitz's run only
#[component]
fn TestScoreRow(test: TestRow) -> Element {
    let name = test.name.trim_start_matches('/').to_string();
    let file_name = name.rsplit_once('/').map(|(_, file)| file).unwrap_or(&name);
    let result = test.results.first().copied().flatten();
    let pass = result.map(|result| result.subtest_pass).unwrap_or(0);
    let denom = test.denom.max(1);
    let fraction = pass as f32 / denom as f32;
    let status = result.map(|result| status_str(result.status)).unwrap_or("MISSING");

    rsx!(
        tr {
            background_color: score_color(fraction),
            td {
                background_color: "white",
                a {
                    href: test_page_href(&test),
                    {file_name.to_string()}
                }
            }
            td {
                text_align: "right",
                {format!("({pass}/{denom})")}
            }
            td {
                text_align: "right",
                {format!("{:.2}%", fraction * 100.0)}
            }
            td {
                text_align: "right",
                {status}
            }
        }
    )
}

/// The link to a test's page: tests with several subtests open on the
/// per-subtest results, single-result tests on the rendered test
pub fn test_page_href(test: &TestRow) -> String {
    let path = encode_test_path(test.name.trim_start_matches('/'));
    if test.denom > 1 {
        format!("/wpt/{path}?tab=results")
    } else {
        format!("/wpt/{path}")
    }
}

/// Encode a WPT test name (an absolute path, possibly containing a query-string
/// variant) so that it can be used as the path portion of a URL.
pub fn encode_test_path(name: &str) -> String {
    name.replace('%', "%25")
        .replace('?', "%3F")
        .replace('#', "%23")
}
