use std::sync::LazyLock;

use dioxus::prelude::*;

use crate::{
    components::Page,
    routes::wpt_compare::{compare_area_row, product_label, RunInfoDisplay},
    wpt_db::{AreaScore, RunRow},
};

/// A named set of WPT focus areas with its own summary page at
/// `/wpt/focus-areas/{slug}`.
pub struct FocusAreaSet {
    pub slug: &'static str,
    pub label: &'static str,
    /// Introduction paragraph for the summary page (raw HTML)
    pub intro: &'static str,
    pub areas: Vec<String>,
}

pub static FOCUS_AREA_SETS: LazyLock<Vec<FocusAreaSet>> = LazyLock::new(|| {
    vec![
        // Servo's WPT focus areas, as tracked by
        // <https://github.com/servo/internal-wpt-dashboard>. The empty string
        // is the full test suite; the dashboard's combined "/css/CSS2/tables/
        // & /css/css-tables/" entry is split into its two constituent rows.
        FocusAreaSet {
            slug: "servo",
            label: "Servo",
            intro: r#"
                This page compares scores on Servo's <a href="https://github.com/servo/internal-wpt-dashboard" target="_blank">WPT focus areas</a>
                across web engines, using the latest master runs from <a href="https://wpt.fyi" target="_blank">wpt.fyi</a> and Blitz's own test runner."#,
            areas: serde_json::from_str(include_str!("../../data/focus_areas/servo.json"))
                .expect("invalid data/focus_areas/servo.json"),
        },
        // Text-related WPT areas, as exercised by Blitz's text stack (Parley);
        // see <https://github.com/DioxusLabs/blitz/blob/main/docs/parley.md>.
        FocusAreaSet {
            slug: "text",
            label: "Text",
            intro: r#"
                This page compares scores on text layout WPT areas (those exercising <a href="https://github.com/linebender/parley" target="_blank">Parley</a>,
                Blitz's text layout engine — see <a href="https://github.com/DioxusLabs/blitz/blob/main/docs/parley.md" target="_blank">docs/parley.md</a>)
                across web engines, using the latest master runs from <a href="https://wpt.fyi" target="_blank">wpt.fyi</a> and Blitz's own test runner."#,
            areas: serde_json::from_str(include_str!("../../data/focus_areas/text.json"))
                .expect("invalid data/focus_areas/text.json"),
        },
    ]
});

pub fn focus_area_set(slug: &str) -> Option<&'static FocusAreaSet> {
    FOCUS_AREA_SETS.iter().find(|set| set.slug == slug)
}

#[component]
pub fn WptFocusAreasPage(
    label: String,
    intro: String,
    runs: Vec<RunRow>,
    scores: Vec<(String, Vec<Option<AreaScore>>)>,
) -> Element {
    rsx! {
        Page { title: format!("WPT: {label} focus areas").into(),
            h1 { "WPT" }
            p {
                class: "introduction",
                dangerous_inner_html: intro,
            }
            hr {}
            p {
                a { href: "/wpt", "wpt" }
                " / focus areas / {label}"
            }
            RunInfoDisplay { runs: runs.clone() }
            table {
                width: "100%",
                tr {
                    th { width: "min-content", "Focus area" }
                    for run in &runs {
                        th { text_align: "center", {product_label(&run.product)} }
                    }
                }
                for (area, area_scores) in &scores {
                    if area.is_empty() {
                        {compare_area_row("All WPT tests".to_string(), Some("/wpt".to_string()), area_scores)}
                    } else {
                        {compare_area_row(format!("/{area}/"), Some(format!("/wpt/{area}")), area_scores)}
                    }
                }
            }
        }
    }
}
