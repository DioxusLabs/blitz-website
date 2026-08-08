use std::{
    collections::BTreeMap,
    ops::Deref,
    sync::{Arc, LazyLock},
};

use dioxus::prelude::*;
use syntect::{highlighting::Theme, parsing::SyntaxSet};
use wptreport::{
    wpt_report::{SubtestStatus, TestResult, TestStatus, WptReport},
    AreaScores, SubtestCounts, TestResultIter,
};

use crate::{
    components::{BarePage, CommitInfoDisplay, Page},
    github::CommitInfo,
    routes::{StatusHeader, StatusTabs},
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

const COLORS: Colors = Colors(&[
    [229, 115, 115],
    [255, 183, 77],
    [255, 213, 79],
    [255, 241, 118],
    [220, 231, 117],
    [174, 213, 129],
    [129, 199, 132],
]);

#[derive(Clone)]
pub struct ArcWptReport(pub Arc<WptReport>);
impl PartialEq for ArcWptReport {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Deref for ArcWptReport {
    type Target = WptReport;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

type WptScores = BTreeMap<String, AreaScores>;

#[derive(Clone)]
pub struct ArcWptScores(pub Arc<WptScores>);
impl PartialEq for ArcWptScores {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Deref for ArcWptScores {
    type Target = WptScores;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[component]
pub fn WptResultsPage(
    report: ArcWptReport,
    scores: ArcWptScores,
    commit_info: Option<CommitInfo>,
    area: Option<String>,
) -> Element {
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
                "Note: As it does not have a JavaScript engine, Blitz can only run about 20% of the total subtests. In the numbers below, tests that Blitz can't run are ignored
                and percentages are relative to the number of tests run.
                "
            }
            hr {}
            CommitInfoDisplay { commit_info, label: "Data from commit:" }
            if let Some(area) = area {
                WptBreadcrumb { area: area.clone() }
                WptAreaResults { report, scores, area }
            } else {
                WptResults { scores }
            }
        }
    }
}

fn is_focus_area(area: &str) -> bool {
    let slash_count = area.chars().filter(|c| *c == '/').count();
    slash_count < 2
}

#[component]
pub fn WptResults(scores: ArcWptScores) -> Element {
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
            {
                scores.iter().filter(|(area, _)| is_focus_area(area)).map(|(area, scores)| {
                    area_score_row(area.clone(), Some(format!("/status/wpt/{area}")), *scores)
                })
            }
        }
    )
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
pub fn WptAreaResults(report: ArcWptReport, scores: ArcWptScores, area: String) -> Element {
    let child_prefix = format!("{area}/");
    let child_areas: Vec<(&String, &AreaScores)> = scores
        .iter()
        .filter(|(key, _)| {
            key.starts_with(&child_prefix) && !key[child_prefix.len()..].contains('/')
        })
        .collect();

    let tests: Vec<&TestResult> = report
        .results
        .iter()
        .filter(|test| {
            test.test
                .rsplit_once('/')
                .map(|(dir, _)| dir == area)
                .unwrap_or(false)
        })
        .collect();

    let area_scores = scores.get(&area).copied().unwrap_or_default();

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
            {area_score_row("Total".to_string(), None, area_scores)}
            for (key, scores) in child_areas {
                {area_score_row(
                    key[child_prefix.len()..].to_string(),
                    Some(format!("/status/wpt/{key}")),
                    *scores,
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
                    TestScoreRow { name: test.test.clone(), status: test.status, counts: test.subtest_counts() }
                }
            }
        }
    )
}

fn area_score_row(label: String, href: Option<String>, scores: AreaScores) -> Element {
    let tests = scores.tests;
    let subtests = scores.subtests;

    let color = COLORS.get(subtests.pass_fraction() as f32);

    rsx!(
        tr {
            background_color: format!("rgb({},{},{})", color[0], color[1], color[2]),
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
                {format!("{:.2}%", (scores.interop_score() as f32 / 1000.0) * 100.0)}
            }
            td {
                text_align: "right",
                {format!("({}/{})", tests.pass, tests.total)}
            }
            td {
                text_align: "right",
                {format!("{:.2}%", tests.pass_fraction() * 100.0)}
            }
            td {
                text_align: "right",
                {format!("({}/{})", subtests.pass, subtests.total)}
            }
            td {
                text_align: "right",
                {format!("{:.2}%", subtests.pass_fraction() * 100.0)}
            }
        }
    )
}

#[component]
fn TestScoreRow(name: String, status: TestStatus, counts: SubtestCounts) -> Element {
    let color = COLORS.get(counts.pass_fraction() as f32);
    let file_name = name.rsplit_once('/').map(|(_, file)| file).unwrap_or(&name);

    rsx!(
        tr {
            background_color: format!("rgb({},{},{})", color[0], color[1], color[2]),
            td {
                background_color: "white",
                a {
                    href: format!("/status/wpt/{}", encode_test_path(&name)),
                    {file_name.to_string()}
                }
            }
            td {
                text_align: "right",
                {format!("({}/{})", counts.pass, counts.total)}
            }
            td {
                text_align: "right",
                {format!("{:.2}%", counts.pass_fraction() * 100.0)}
            }
            td {
                text_align: "right",
                {format!("{status:?}").to_uppercase()}
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
    report: ArcWptReport,
    commit_info: Option<CommitInfo>,
    test_index: usize,
    tab: TestPageTab,
    source: SourceResult,
    refs: Vec<RefLink>,
    ref_source: Option<SourceResult>,
) -> Element {
    let test = &report.results[test_index];
    let name = test.test.clone();

    let file_name = name
        .rsplit_once('/')
        .map(|(_, file)| file)
        .unwrap_or(&name)
        .to_string();

    // The path used to fetch the source (test name without any query-string variant)
    let source_path = format!("/{}", name.split('?').next().unwrap_or(&name));
    let first_ref = refs.first().cloned();
    let counts = test.subtest_counts();

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
                    p {
                        b { "Status: " }
                        {format!("{:?}", test.status).to_uppercase()}
                        " | "
                        b { "Duration: " }
                        {format!("{}ms", test.duration)}
                        " | "
                        b { "Subtests: " }
                        {format!("{}/{}", counts.pass, counts.total)}
                    }
                    TestPageTabs {
                        name: name.clone(),
                        current_tab: tab,
                        ref_link: first_ref.clone(),
                        counts,
                    }
                }
                div {
                    class: "wpt-test-page__content",
                    TabPanel { tab: TestPageTab::Summary, current_tab: tab,
                        CommitInfoDisplay { commit_info, label: "Data from commit:" }
                        TestSummary { report: report.clone(), test_index }
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
        if (jsToggle.checked) {
            iframe.removeAttribute('sandbox');
        } else {
            iframe.setAttribute('sandbox', 'allow-same-origin');
        }
        iframe.src = iframe.src;
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
    counts: SubtestCounts,
) -> Element {
    let base = format!("/status/wpt/{}", encode_test_path(&name));

    let mut tabs: Vec<(TestPageTab, String)> = vec![
        (TestPageTab::Test, "Test".to_string()),
        (TestPageTab::TestSource, "Test Source".to_string()),
    ];
    if let Some(ref_link) = &ref_link {
        let label = if ref_link.rel == "mismatch" {
            "Ref (mismatch)"
        } else {
            "Ref"
        };
        tabs.push((TestPageTab::Ref, label.to_string()));
        tabs.push((TestPageTab::RefSource, "Ref Source".to_string()));
    }
    tabs.push((
        TestPageTab::Summary,
        format!("Results ({}/{})", counts.pass, counts.total),
    ));

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
fn TestSummary(report: ArcWptReport, test_index: usize) -> Element {
    let test = &report.results[test_index];
    let counts = test.subtest_counts();

    rsx! {
        table {
            tr {
                th { "Status" }
                th { "Duration" }
                th { "Subtests Passed" }
            }
            tr {
                td { {format!("{:?}", test.status).to_uppercase()} }
                td { {format!("{}ms", test.duration)} }
                td { {format!("{}/{}", counts.pass, counts.total)} }
            }
        }
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
                            {format!("{:?}", subtest.status).to_uppercase()}
                        }
                        td { {subtest.message.clone().unwrap_or_default()} }
                    }
                }
            }
        }
    }
}

fn subtest_status_color(status: SubtestStatus) -> &'static str {
    match status {
        SubtestStatus::Pass => "rgb(129,199,132)",
        SubtestStatus::Fail | SubtestStatus::Error => "rgb(229,115,115)",
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
    let extension = path.rsplit('.').next().unwrap_or("html");
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
