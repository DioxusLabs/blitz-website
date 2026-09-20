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
    /// One entry per focus area: the cross-engine union subtest total of
    /// the area from the WPT comparison database, when known
    pub denominators: Vec<Option<u32>>,
    pub runs: Vec<HistoryRun>,
}

impl WptHistory {
    /// The subtest total percentages for an area are computed against: the
    /// union subtest total across all engines' latest runs, so every
    /// engine's line is on the same scale, falling back to the product's
    /// own most recent subtest total. Either way a fixed denominator keeps
    /// tests being added to WPT from distorting historical pass rates.
    pub fn denominator(&self, area_idx: usize) -> Option<u32> {
        self.denominators
            .get(area_idx)
            .copied()
            .flatten()
            .filter(|total| *total != 0)
            .or_else(|| self.latest_subtest_total(area_idx))
    }

    /// The subtest total of the most recent run with data for this area
    fn latest_subtest_total(&self, area_idx: usize) -> Option<u32> {
        self.runs
            .iter()
            .rev()
            .filter_map(|run| *run.scores.get(area_idx)?)
            .map(|(_, _, total_subtests, _)| total_subtests)
            .find(|total| *total != 0)
    }
}

pub struct HistoryRun {
    pub date: String,
    pub product_revision: String,
    pub commit_message: Option<String>,
    /// One entry per focus area
    pub scores: Vec<Option<ScoreTuple>>,
}
