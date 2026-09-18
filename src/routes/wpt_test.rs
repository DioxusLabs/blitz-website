//! The per-test page of the WPT comparison section: renders the test and
//! its reference (via wpt.live), shows their sources, and lists every
//! engine's result for each subtest.

use std::sync::LazyLock;

use dioxus::prelude::*;
use syntect::{highlighting::Theme, parsing::SyntaxSet};

use crate::{
    components::BarePage,
    routes::{
        encode_test_path, product_label, score_color, RunInfoDisplay, WptCompareBreadcrumb,
    },
    wpt_db::{status_str, RunRow, SubtestRow, TestDetail},
    wpt_source::{RefLink, SourceResult},
};

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum TestPageTab {
    Results,
    Test,
    TestSource,
    Ref,
    RefSource,
}

impl TestPageTab {
    pub fn from_query(tab: Option<&str>) -> Self {
        match tab {
            // "summary" was the name of the results tab on the old Blitz-only
            // test page
            Some("results") | Some("summary") => Self::Results,
            Some("test-source") => Self::TestSource,
            Some("ref") => Self::Ref,
            Some("ref-source") => Self::RefSource,
            _ => Self::Test,
        }
    }

    fn query_value(self) -> &'static str {
        match self {
            Self::Results => "results",
            Self::Test => "test",
            Self::TestSource => "test-source",
            Self::Ref => "ref",
            Self::RefSource => "ref-source",
        }
    }
}

#[component]
pub fn WptCompareTestPage(
    runs: Vec<RunRow>,
    detail: TestDetail,
    tab: TestPageTab,
    source: SourceResult,
    refs: Vec<RefLink>,
    ref_source: Option<SourceResult>,
) -> Element {
    let name = detail.name.trim_start_matches('/').to_string();
    let file_name = name
        .rsplit_once('/')
        .map(|(_, file)| file)
        .unwrap_or(&name)
        .to_string();

    // The path used to fetch the source (test name without any query-string variant)
    let source_path = format!("/{}", name.split('?').next().unwrap_or(&name));
    let first_ref = refs.first().cloned();

    rsx! {
        BarePage { title: format!("WPT: {file_name}").into(),
            div {
                class: "wpt-test-page",
                div {
                    class: "wpt-test-page__breadcrumbs",
                    WptCompareBreadcrumb { area: name.clone() }
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
                    TestPageTabs {
                        name: name.clone(),
                        current_tab: tab,
                        ref_link: first_ref.clone(),
                    }
                }
                div {
                    class: "wpt-test-page__content",
                    TabPanel { tab: TestPageTab::Results, current_tab: tab,
                        RunInfoDisplay { runs: runs.clone() }
                        TestResults { runs: runs.clone(), detail: detail.clone() }
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
fn TestPageTabs(name: String, current_tab: TestPageTab, ref_link: Option<RefLink>) -> Element {
    let base = format!("/wpt/{}", encode_test_path(&name));

    let mut tabs: Vec<(TestPageTab, &str)> =
        vec![(TestPageTab::Results, "Results"), (TestPageTab::Test, "Test")];
    if let Some(ref_link) = &ref_link {
        let label = if ref_link.rel == "mismatch" {
            "Ref (mismatch)"
        } else {
            "Ref"
        };
        tabs.push((TestPageTab::Ref, label));
    }
    tabs.push((TestPageTab::TestSource, "Test Source"));
    if ref_link.is_some() {
        tabs.push((TestPageTab::RefSource, "Ref Source"));
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

/// Every engine's result for the test and each of its subtests. Messages
/// (stored for Blitz's runs only) are shown as a tooltip on the status cell
/// and, for subtests, in a trailing column.
#[component]
fn TestResults(runs: Vec<RunRow>, detail: TestDetail) -> Element {
    let denom = detail
        .results
        .iter()
        .flatten()
        .map(|result| result.subtest_total)
        .max()
        .unwrap_or(1)
        .max(1);
    // The engines with any subtest message stored (messages are only kept
    // for Blitz's runs, so this is normally just "Blitz"); the message
    // column is named after them
    let message_products: Vec<String> = runs
        .iter()
        .enumerate()
        .filter(|(idx, _)| {
            detail
                .subtests
                .iter()
                .any(|subtest| subtest.messages[*idx].is_some())
        })
        .map(|(_, run)| product_label(&run.product))
        .collect();
    let has_messages = !message_products.is_empty();
    let message_header = format!("{} error message", message_products.join(" / "));

    rsx! {
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
                for (result, message) in detail.results.iter().zip(&detail.messages) {
                    if let Some(result) = result {
                        td {
                            text_align: "right",
                            background_color: score_color(result.subtest_pass as f32 / denom as f32),
                            title: message.clone(),
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
                    if has_messages {
                        th { {message_header.clone()} }
                    }
                }
                for subtest in &detail.subtests {
                    SubtestResultsRow { runs: runs.clone(), subtest: subtest.clone(), has_messages }
                }
            }
        }
    }
}

#[component]
fn SubtestResultsRow(runs: Vec<RunRow>, subtest: SubtestRow, has_messages: bool) -> Element {
    // One line per engine with a message; the product is only named when
    // more than one engine has one
    let messages: Vec<(String, String)> = runs
        .iter()
        .zip(&subtest.messages)
        .filter_map(|(run, message)| Some((product_label(&run.product), message.clone()?)))
        .collect();

    rsx!(
        tr {
            td { background_color: "white", {subtest.name.clone()} }
            for (status, message) in subtest.statuses.iter().zip(&subtest.messages) {
                if let Some(status) = status {
                    td {
                        text_align: "right",
                        background_color: score_color((*status == 0) as u32 as f32),
                        title: message.clone(),
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
            if has_messages {
                td {
                    font_size: "smaller",
                    for (product, message) in &messages {
                        div {
                            if messages.len() > 1 {
                                b { "{product}: " }
                            }
                            FormattedMessage { message: message.clone() }
                        }
                    }
                }
            }
        }
    )
}

/// One piece of a parsed testharness message
#[derive(Debug, PartialEq)]
enum MessagePart {
    /// The `assert_*` (or `promise_test`, ...) name the line starts with
    Assertion(String),
    Text(String),
    Expected(String),
    Actual(String),
    /// A dumped HTML element (check-layout tests print the failing
    /// element's outerHTML)
    Html(String),
}

/// Split a testharness.js message into lines of structured parts.
///
/// Handles the standard assertion phrasings:
/// - `assert_equals: <desc> expected <E> but got <A>`
/// - `assert_approx_equals: expected <E> +/- <eps> but got <A>`
/// - `<desc> expected <E> got <A>` (check-layout `data-expected-*`)
/// - `assert_true: <desc> expected true got false` (the tautological
///   expected/got is dropped)
/// - `Colors do not match.\nActual: <A>\nExpected: <E>.\nError: ...`
///
/// Anything else is passed through as text.
fn parse_message(message: &str) -> Vec<Vec<MessagePart>> {
    message
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(parse_message_line)
        .collect()
}

fn parse_message_line(line: &str) -> Vec<MessagePart> {
    let line = line.trim();
    if line.starts_with('<') {
        return vec![MessagePart::Html(line.to_string())];
    }
    if let Some(actual) = line.strip_prefix("Actual:") {
        return vec![MessagePart::Actual(actual.trim().to_string())];
    }
    if let Some(expected) = line.strip_prefix("Expected:") {
        let expected = expected.trim().trim_end_matches('.');
        return vec![MessagePart::Expected(expected.to_string())];
    }
    let line = line.strip_prefix("Error: ").unwrap_or(line);

    let is_assertion_name = |name: &str| {
        !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
    };
    let mut parts = Vec::new();
    let mut body = line;
    // `assert_equals: ...`, `promise_test: ...` (or just `assert_equals:`
    // when the rest of the message is on the following lines)
    if let Some(name) = line.strip_suffix(':').filter(|name| is_assertion_name(name)) {
        return vec![MessagePart::Assertion(name.to_string())];
    }
    if let Some((name, rest)) = line.split_once(": ") {
        if is_assertion_name(name) {
            parts.push(MessagePart::Assertion(name.to_string()));
            body = rest;
        }
    }

    for tautology in ["expected true got false", "expected false got true"] {
        if let Some(desc) = body.strip_suffix(tautology) {
            let desc = desc.trim();
            if !desc.is_empty() {
                parts.push(MessagePart::Text(desc.to_string()));
            }
            return parts;
        }
    }

    // `<desc> expected <E> but got <A>` or `<desc> expected <E> got <A>`
    let split = body
        .rfind(" but got ")
        .map(|idx| (idx, " but got ".len()))
        .or_else(|| body.rfind(" got ").map(|idx| (idx, " got ".len())));
    if let Some((got_idx, got_len)) = split {
        let (before, actual) = (&body[..got_idx], &body[got_idx + got_len..]);
        let expected_idx = if let Some(rest) = before.strip_prefix("expected ") {
            Some((0, rest))
        } else {
            before
                .rfind(" expected ")
                .map(|idx| (idx, &before[idx + " expected ".len()..]))
        };
        if let Some((desc_end, expected)) = expected_idx {
            let desc = before[..desc_end].trim();
            if !desc.is_empty() {
                parts.push(MessagePart::Text(desc.to_string()));
            }
            parts.push(MessagePart::Expected(expected.trim().to_string()));
            parts.push(MessagePart::Actual(actual.trim().to_string()));
            return parts;
        }
    }

    parts.push(MessagePart::Text(body.to_string()));
    parts
}

/// A testharness message with its expected/actual values and dumped
/// markup set apart from the description
#[component]
fn FormattedMessage(message: String) -> Element {
    let lines = parse_message(&message);
    rsx! {
        for parts in lines {
            div {
                class: "wpt-message",
                for part in parts {
                    match part {
                        MessagePart::Assertion(name) => rsx! {
                            span { class: "wpt-message__assertion", {name} }
                            " "
                        },
                        MessagePart::Text(text) => rsx! {
                            span { {text} }
                            " "
                        },
                        MessagePart::Expected(value) => rsx! {
                            span { class: "wpt-message__label", "expected " }
                            code { class: "wpt-message__expected", {value} }
                            " "
                        },
                        MessagePart::Actual(value) => rsx! {
                            span { class: "wpt-message__label", "got " }
                            code { class: "wpt-message__actual", {value} }
                            " "
                        },
                        MessagePart::Html(html) => rsx! {
                            details {
                                class: "wpt-message__html",
                                summary { "element markup" }
                                pre { {html} }
                            }
                        },
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_message, MessagePart::*};

    #[test]
    fn assert_equals_with_description() {
        assert_eq!(
            parse_message("assert_equals: data-offset-x expected 120 got 0"),
            vec![vec![
                Assertion("assert_equals".into()),
                Text("data-offset-x".into()),
                Expected("120".into()),
                Actual("0".into()),
            ]]
        );
    }

    #[test]
    fn assert_equals_but_got() {
        assert_eq!(
            parse_message("assert_equals: expected \"a b\" but got \"c got d\""),
            vec![vec![
                Assertion("assert_equals".into()),
                Expected("\"a b\"".into()),
                Actual("\"c got d\"".into()),
            ]]
        );
    }

    #[test]
    fn assert_true_drops_tautology() {
        assert_eq!(
            parse_message("assert_true: 'auto' value should be supported expected true got false"),
            vec![vec![
                Assertion("assert_true".into()),
                Text("'auto' value should be supported".into()),
            ]]
        );
    }

    #[test]
    fn approx_equals() {
        assert_eq!(
            parse_message("assert_approx_equals: expected 0.88 +/- 0.01 but got 0.47"),
            vec![vec![
                Assertion("assert_approx_equals".into()),
                Expected("0.88 +/- 0.01".into()),
                Actual("0.47".into()),
            ]]
        );
    }

    #[test]
    fn colors_do_not_match() {
        assert_eq!(
            parse_message(
                "Colors do not match.\nActual:   color(srgb 0 0 0)\nExpected: hsl(none none none).\nError: assert_array_approx_equals: lengths differ, expected 0 got 3"
            ),
            vec![
                vec![Text("Colors do not match.".into())],
                vec![Actual("color(srgb 0 0 0)".into())],
                vec![Expected("hsl(none none none)".into())],
                vec![
                    Assertion("assert_array_approx_equals".into()),
                    Text("lengths differ,".into()),
                    Expected("0".into()),
                    Actual("3".into()),
                ],
            ]
        );
    }

    #[test]
    fn check_layout_html_dump() {
        assert_eq!(
            parse_message("assert_equals: \n<div class=\"grid\">X</div>\nwidth expected 25 but got 50"),
            vec![
                vec![Assertion("assert_equals".into())],
                vec![Html("<div class=\"grid\">X</div>".into())],
                vec![Text("width".into()), Expected("25".into()), Actual("50".into())],
            ]
        );
    }

    #[test]
    fn plain_text_passes_through() {
        assert_eq!(
            parse_message("not a callable function"),
            vec![vec![Text("not a callable function".into())]]
        );
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
