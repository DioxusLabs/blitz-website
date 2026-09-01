use std::sync::LazyLock;

use dioxus::prelude::*;

use crate::{
    components::Page,
    routes::wpt_compare::{compare_area_row, product_label, RunInfoDisplay},
    wpt_db::{AreaScore, RunRow},
};

/// A named set of WPT focus areas with its own summary page at
/// `/wpt/focus-areas/{slug}`. Sets are defined by `data/focus_areas/*.json`
/// files; the slug is the file name.
#[derive(serde::Deserialize)]
pub struct FocusAreaSet {
    #[serde(skip)]
    pub slug: String,
    pub label: String,
    /// Introduction paragraph for the summary page (raw HTML)
    pub intro: String,
    pub areas: Vec<String>,
}

pub static FOCUS_AREA_SETS: LazyLock<Vec<FocusAreaSet>> = LazyLock::new(|| {
    // Relative to the crate root when run via cargo, or the working
    // directory for a deployed binary (like the `static` and `data` assets)
    let root = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let dir = std::path::Path::new(&root).join("data/focus_areas");
    let mut sets: Vec<FocusAreaSet> = std::fs::read_dir(&dir)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", dir.display()))
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .map(|path| {
            let json = std::fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));
            let mut set: FocusAreaSet = serde_json::from_str(&json)
                .unwrap_or_else(|err| panic!("invalid {}: {err}", path.display()));
            set.slug = path.file_stem().unwrap().to_string_lossy().into_owned();
            set
        })
        .collect();
    sets.sort_by(|a, b| a.slug.cmp(&b.slug));
    sets
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
