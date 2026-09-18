//! Types for the per-area WPT score history format shared by
//! blitz-wpt-results and browser-wpt-results (`summary/<product>/runs.json` +
//! `summary/<product>/areas/<area>.json`). Loaded by [`crate::wpt_summaries`].

use serde::Deserialize;

/// Per-area scores for a single run:
/// `[total_tests, total_score, total_subtests, total_subtests_passed]`
pub type ScoreTuple = (u32, f64, u32, u32);

/// One entry of runs.json: metadata shared by all per-area score files
#[derive(Deserialize)]
pub struct RunMeta {
    pub date: String,
    #[allow(dead_code)]
    pub wpt_revision: String,
    /// Browser version, or the Blitz commit sha
    pub product_revision: String,
    /// Blitz only
    pub commit_message: Option<String>,
}

#[derive(Deserialize)]
pub struct RunsFile {
    pub runs: Vec<RunMeta>,
}

/// One summary/areas/<area>.json file: `scores` has one entry per run,
/// index-aligned with runs.json
#[derive(Deserialize)]
pub struct AreaFile {
    pub scores: Vec<Option<ScoreTuple>>,
}

/// The merged history of one product for a set of areas
pub struct WptHistory {
    pub focus_areas: Vec<String>,
    pub runs: Vec<HistoryRun>,
}

pub struct HistoryRun {
    pub date: String,
    pub product_revision: String,
    pub commit_message: Option<String>,
    /// One entry per focus area
    pub scores: Vec<Option<ScoreTuple>>,
}
