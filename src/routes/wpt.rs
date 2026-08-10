use std::{collections::BTreeMap, ops::Deref, sync::Arc};

use dioxus::prelude::*;
use wptreport::{wpt_report::WptReport, AreaScores};

use crate::{
    components::{CommitInfoDisplay, Page},
    github::CommitInfo,
    routes::{
        ArcWptHistory, ChartRange, ChartRangeSelector, ChartSeries, StatusHeader, StatusTabs,
        WptHistoryChart, SERIES_COLORS,
    },
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
    history: Option<ArcWptHistory>,
    range: ChartRange,
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
            FolderHistoryChart { folder: "css", scores: scores.clone(), history, range, base_path: "/status/wpt" }
            CommitInfoDisplay { commit_info, label: "Data from commit:" }
            WptResults { scores }
        }
    }
}

#[component]
pub fn WptFolderPage(
    folder: String,
    scores: ArcWptScores,
    commit_info: Option<CommitInfo>,
    history: Option<ArcWptHistory>,
    range: ChartRange,
) -> Element {
    // Breadcrumb segments: (path-so-far, segment name)
    let crumbs: Vec<(String, String)> = folder
        .split('/')
        .scan(String::new(), |path, segment| {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(segment);
            Some((path.clone(), segment.to_string()))
        })
        .collect();

    rsx! {
        Page { title: "Status: WPT: {folder}".into(),
            StatusHeader {}
            StatusTabs { current_tab: "wpt" }
            p {
                font_size: "18px",
                a { href: "/status/wpt", "wpt" }
                for (path, segment) in crumbs {
                    " / "
                    a { href: "/status/wpt/{path}", {segment} }
                }
            }
            hr {}
            FolderHistoryChart {
                folder: folder.clone(),
                scores: scores.clone(),
                history,
                range,
                base_path: "/status/wpt/{folder}",
                compact: true,
            }
            CommitInfoDisplay { commit_info, label: "Data from commit:" }
            WptFolderResults { folder, scores }
        }
    }
}

/// The direct child areas of a folder, largest (by subtest count) first
fn child_areas(scores: &WptScores, folder: &str) -> Vec<(String, AreaScores)> {
    let prefix = format!("{folder}/");
    let mut children: Vec<(String, AreaScores)> = scores
        .iter()
        .filter(|(area, _)| {
            area.strip_prefix(&prefix)
                .is_some_and(|rest| !rest.contains('/'))
        })
        .map(|(area, scores)| (area.clone(), *scores))
        .collect();
    children.sort_by_key(|(_, scores)| std::cmp::Reverse(scores.subtests.total));
    children
}

/// A history chart for a folder. Compact charts show a single short line for
/// the folder's total; full-size charts also show a line for each of the
/// folder's largest direct children.
#[component]
fn FolderHistoryChart(
    folder: String,
    scores: ArcWptScores,
    history: Option<ArcWptHistory>,
    range: ChartRange,
    base_path: String,
    #[props(default = false)] compact: bool,
) -> Element {
    let Some(history) = history else {
        return rsx!( p { "No history data available" } );
    };

    let mut series_spec = vec![ChartSeries {
        area: folder.clone(),
        label: if folder == "css" {
            "all css".to_string()
        } else {
            folder.clone()
        },
        color: SERIES_COLORS[0],
    }];
    if !compact {
        let prefix = format!("{folder}/");
        for (child, _) in child_areas(&scores, &folder)
            .into_iter()
            .take(SERIES_COLORS.len() - 1)
        {
            series_spec.push(ChartSeries {
                label: child.strip_prefix(&prefix).unwrap_or(&child).to_string(),
                area: child,
                color: SERIES_COLORS[series_spec.len()],
            });
        }
    }

    let height = if compact { 240.0 } else { 440.0 };

    rsx! {
        ChartRangeSelector { current_range: range, base_path }
        WptHistoryChart { history, series_spec, range, height }
    }
}

#[component]
pub fn WptFolderResults(folder: String, scores: ArcWptScores) -> Element {
    let mut areas: Vec<(String, AreaScores)> = child_areas(&scores, &folder);
    if let Some(own) = scores.get(&folder) {
        areas.insert(0, (folder, *own));
    }
    areas.sort_by(|(a, _), (b, _)| a.cmp(b));
    wpt_score_table(areas)
}

fn is_focus_area(area: &str) -> bool {
    let slash_count = area.chars().filter(|c| *c == '/').count();
    slash_count < 2 || (slash_count == 2 && area.starts_with("css/CSS2"))
}

#[component]
pub fn WptResults(scores: ArcWptScores) -> Element {
    let areas: Vec<(String, AreaScores)> = scores
        .iter()
        .filter(|(area, _)| is_focus_area(area))
        .map(|(area, scores)| (area.clone(), *scores))
        .collect();
    wpt_score_table(areas)
}

fn wpt_score_table(areas: Vec<(String, AreaScores)>) -> Element {
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
                areas.iter().map(|(area, scores)| {

                    let tests = scores.tests;
                    let subtests = scores.subtests;

                    let color = COLORS.get(subtests.pass_fraction() as f32);

                    rsx!(
                        tr {
                            background_color: format!("rgb({},{},{})", color[0], color[1], color[2]),
                            td {
                                background_color: "white",
                                a {
                                    color: "inherit",
                                    href: "/status/wpt/{area}",
                                    {area.clone()}
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
                })
            }
        }
    )
}
