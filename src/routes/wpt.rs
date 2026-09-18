use std::sync::LazyLock;

use dioxus::prelude::*;
use syntect::{highlighting::Theme, parsing::SyntaxSet};

use crate::{
    components::{BarePage, CommitInfoDisplay, Page},
    github::CommitInfo,
    routes::{
        ArcWptHistory, ChartRange, ChartRangeSelector, ChartSeries, StatusHeader, StatusTabs,
        WptHistoryChart, SERIES_COLORS,
    },
    wpt_db::{status_str, AreaScore, RunTestDetail, TestRow},
    wpt_source::{RefLink, SourceResult},
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
            p {
                font_size: "smaller",
                "Note: As it does not have a JavaScript engine, Blitz can only run about 20% of the total subtests. Tests that Blitz can't run count as failures
                in the numbers below, as they do on the "
                a { href: "/wpt/{area}", "comparison pages" }
                "."
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
                    href: format!("/status/wpt/{}", encode_test_path(&name)),
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

/// Encode a WPT test name (an absolute path, possibly containing a query-string
/// variant) so that it can be used as the path portion of a URL.
pub fn encode_test_path(name: &str) -> String {
    name.replace('%', "%25")
        .replace('?', "%3F")
        .replace('#', "%23")
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum TestPageTab {
    Summary,
    Test,
    TestSource,
    Ref,
    RefSource,
}

impl TestPageTab {
    pub fn from_query(tab: Option<&str>) -> Self {
        match tab {
            Some("summary") => Self::Summary,
            Some("test-source") => Self::TestSource,
            Some("ref") => Self::Ref,
            Some("ref-source") => Self::RefSource,
            _ => Self::Test,
        }
    }

    fn query_value(self) -> &'static str {
        match self {
            Self::Summary => "summary",
            Self::Test => "test",
            Self::TestSource => "test-source",
            Self::Ref => "ref",
            Self::RefSource => "ref-source",
        }
    }
}

#[component]
pub fn WptTestPage(
    test: RunTestDetail,
    commit_info: Option<CommitInfo>,
    tab: TestPageTab,
    source: SourceResult,
    refs: Vec<RefLink>,
    ref_source: Option<SourceResult>,
) -> Element {
    let name = test.name.trim_start_matches('/').to_string();

    let file_name = name
        .rsplit_once('/')
        .map(|(_, file)| file)
        .unwrap_or(&name)
        .to_string();

    // The path used to fetch the source (test name without any query-string variant)
    let source_path = format!("/{}", name.split('?').next().unwrap_or(&name));
    let first_ref = refs.first().cloned();
    let counts = (test.subtest_pass, test.denom.max(1));
    let show_subtests = test.subtests.len() > 1;

    rsx! {
        BarePage { title: format!("WPT: {file_name}").into(),
            div {
                class: "wpt-test-page",
                div {
                    class: "wpt-test-page__breadcrumbs",
                    WptBreadcrumb { area: name.trim_start_matches('/').to_string() }
                }
                div {
                    class: "wpt-test-page__header",
                    p {
                        b { "Status: " }
                        {status_str(test.status)}
                        if let Some(duration) = test.duration_ms {
                            " | "
                            b { "Duration: " }
                            {format!("{duration}ms")}
                        }
                        " | "
                        b { "Subtests: " }
                        {format!("{}/{}", counts.0, counts.1)}
                        " | "
                        a {
                            href: format!("https://wpt.live/{name}"),
                            target: "_blank",
                            "Open test on wpt.live"
                        }
                        if let Some(ref_link) = &first_ref {
                            " | "
                            a {
                                href: format!("https://wpt.live{}", ref_link.href),
                                target: "_blank",
                                "Open ref on wpt.live"
                            }
                        }
                        " | "
                        a {
                            href: format!("https://wpt.fyi/results/{name}"),
                            target: "_blank",
                            "wpt.fyi"
                        }
                    }
                    TestPageTabs {
                        name: name.clone(),
                        current_tab: tab,
                        ref_link: first_ref.clone(),
                        counts,
                        show_subtests,
                    }
                }
                div {
                    class: "wpt-test-page__content",
                    if show_subtests {
                        TabPanel { tab: TestPageTab::Summary, current_tab: tab,
                            CommitInfoDisplay { commit_info, label: "Data from commit:" }
                            TestSummary { test: test.clone() }
                        }
                    }
                    TabPanel { tab: TestPageTab::Test, current_tab: tab,
                        if first_ref.is_none() {
                            p {
                                class: "wpt-js-toggle",
                                label {
                                    input {
                                        r#type: "checkbox",
                                        id: "enable-js-toggle",
                                    }
                                    " Enable JavaScript in test iframe"
                                }
                            }
                        }
                        TestIframe {
                            path: format!("/{name}"),
                            fixed_size: first_ref.is_some(),
                            sandboxed: first_ref.is_none(),
                        }
                    }
                    TabPanel { tab: TestPageTab::TestSource, current_tab: tab,
                        SourceView { path: source_path.clone(), source: source.clone() }
                    }
                    if let Some(ref_link) = &first_ref {
                        TabPanel { tab: TestPageTab::Ref, current_tab: tab,
                            TestIframe { path: ref_link.href.clone(), fixed_size: true, sandboxed: false }
                        }
                        TabPanel { tab: TestPageTab::RefSource, current_tab: tab,
                            if let Some(ref_source) = &ref_source {
                                SourceView { path: ref_link.href.clone(), source: ref_source.clone() }
                            }
                        }
                    }
                }
                script { dangerous_inner_html: TAB_SCRIPT }
            }
        }
    }
}

/// Client-side tab switching. Panels for all tabs are rendered up front
/// (so iframes keep their state when switching); this toggles which panel is
/// visible and updates the URL. The `?tab=` links still work without JS.
/// Also wires up the "enable JavaScript" toggle: the test iframe is sandboxed
/// without `allow-scripts` by default, and the toggle lifts the sandbox and
/// reloads the iframe.
const TAB_SCRIPT: &str = r#"
document.querySelectorAll('.tab-container a[data-tab]').forEach(function (link) {
    link.addEventListener('click', function (event) {
        event.preventDefault();
        var tab = link.getAttribute('data-tab');
        document.querySelectorAll('.tab-container a[data-tab]').forEach(function (l) {
            l.classList.toggle('tab--selected', l === link);
        });
        document.querySelectorAll('.wpt-tab-panel').forEach(function (panel) {
            panel.classList.toggle('wpt-tab-panel--active', panel.getAttribute('data-tab') === tab);
        });
        history.replaceState(null, '', link.getAttribute('href'));
    });
});

var jsToggle = document.getElementById('enable-js-toggle');
if (jsToggle) {
    jsToggle.addEventListener('change', function () {
        var iframe = document.querySelector('.wpt-tab-panel[data-tab=\"test\"] iframe');
        if (!iframe) return;
        // Replace the iframe node rather than resetting src, which would
        // add a browser history entry.
        var replacement = iframe.cloneNode();
        if (jsToggle.checked) {
            replacement.removeAttribute('sandbox');
        } else {
            replacement.setAttribute('sandbox', 'allow-same-origin');
        }
        iframe.replaceWith(replacement);
    });
}
"#;

#[component]
fn TabPanel(tab: TestPageTab, current_tab: TestPageTab, children: Element) -> Element {
    rsx! {
        div {
            class: if tab == current_tab { "wpt-tab-panel wpt-tab-panel--active" } else { "wpt-tab-panel" },
            "data-tab": tab.query_value(),
            {children}
        }
    }
}

#[component]
fn TestPageTabs(
    name: String,
    current_tab: TestPageTab,
    ref_link: Option<RefLink>,
    counts: (u32, u32),
    show_subtests: bool,
) -> Element {
    let base = format!("/status/wpt/{}", encode_test_path(&name));

    let mut tabs: Vec<(TestPageTab, String)> = vec![(TestPageTab::Test, "Test".to_string())];
    if let Some(ref_link) = &ref_link {
        let label = if ref_link.rel == "mismatch" {
            "Ref (mismatch)"
        } else {
            "Ref"
        };
        tabs.push((TestPageTab::Ref, label.to_string()));
    }
    tabs.push((TestPageTab::TestSource, "Test Source".to_string()));
    if ref_link.is_some() {
        tabs.push((TestPageTab::RefSource, "Ref Source".to_string()));
    }
    if show_subtests {
        tabs.push((
            TestPageTab::Summary,
            format!("Subtests ({}/{})", counts.0, counts.1),
        ));
    }

    rsx! {
        div {
            class: "tab-container",
            for (tab, label) in tabs {
                a {
                    class: if tab == current_tab { "tab tab--selected" } else { "tab" },
                    href: format!("{base}?tab={}", tab.query_value()),
                    "data-tab": tab.query_value(),
                    {label}
                }
            }
        }
    }
}

#[component]
fn TestSummary(test: RunTestDetail) -> Element {
    rsx! {
        if let Some(message) = &test.message {
            p { b { "Message: " } {message.clone()} }
        }
        if !test.subtests.is_empty() {
            table {
                width: "100%",
                margin_top: "24px",
                tr {
                    th { "Subtest" }
                    th { "Status" }
                    th { "Message" }
                }
                for subtest in &test.subtests {
                    tr {
                        td { {subtest.name.clone()} }
                        td {
                            background_color: subtest_status_color(subtest.status),
                            {status_str(subtest.status)}
                        }
                        td { {subtest.message.clone().unwrap_or_default()} }
                    }
                }
            }
        }
    }
}

fn subtest_status_color(status: i64) -> &'static str {
    match status_str(status) {
        "PASS" => "rgb(129,199,132)",
        "FAIL" | "ERROR" => "rgb(229,115,115)",
        _ => "rgb(255,213,79)",
    }
}

#[component]
fn TestIframe(path: String, fixed_size: bool, sandboxed: bool) -> Element {
    rsx! {
        iframe {
            class: if fixed_size { "wpt-test-iframe wpt-test-iframe--fixed" } else { "wpt-test-iframe" },
            src: format!("https://wpt.live{path}"),
            "sandbox": if sandboxed { "allow-same-origin" },
        }
    }
}

#[component]
fn SourceView(path: String, source: SourceResult) -> Element {
    rsx! {
        p {
            a {
                href: format!("https://wpt.live{path}"),
                target: "_blank",
                {path.clone()}
            }
        }
        match &source {
            Ok(source) => rsx! {
                div {
                    class: "wpt-source",
                    dangerous_inner_html: highlight_source(source, &path),
                }
            },
            Err(err) => rsx! {
                p { color: "#8c3037", "Failed to load source: {err}" }
            },
        }
    }
}

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME: LazyLock<Theme> = LazyLock::new(|| {
    syntect::highlighting::ThemeSet::load_defaults()
        .themes
        .remove("InspiredGitHub")
        .unwrap()
});

fn highlight_source(source: &str, path: &str) -> String {
    let extension = match path.rsplit('.').next().unwrap_or("html") {
        "xht" => "xhtml",
        ext => ext,
    };
    let syntax = SYNTAX_SET
        .find_syntax_by_extension(extension)
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());

    syntect::html::highlighted_html_for_string(source, &SYNTAX_SET, syntax, &THEME)
        .unwrap_or_else(|_| format!("<pre>{}</pre>", html_escape(source)))
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
