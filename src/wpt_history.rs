use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};

use reqwest::Client;
use serde::Deserialize;

use crate::routes::ArcWptHistory;

const SUMMARY_BASE_URL: &str =
    "https://raw.githubusercontent.com/DioxusLabs/blitz-wpt-results/main/summary";

/// Per-area scores for a single run:
/// `[total_tests, total_score, total_subtests, total_subtests_passed]`
pub type ScoreTuple = (u32, f64, u32, u32);

/// One entry of runs.json: metadata shared by all per-area score files
#[derive(Deserialize)]
struct RunMeta {
    date: String,
    #[allow(dead_code)]
    wpt_revision: String,
    product_revision: String,
    commit_message: Option<String>,
}

#[derive(Deserialize)]
struct RunsFile {
    runs: Vec<RunMeta>,
}

/// One summary/areas/<group>.json file: `scores` has one row per run
/// (index-aligned with runs.json), each row parallel to `focus_areas`
#[derive(Deserialize)]
struct AreaFile {
    focus_areas: Vec<String>,
    scores: Vec<Vec<Option<ScoreTuple>>>,
}

/// The merged history for one group of areas (one top-level WPT folder)
pub struct WptHistory {
    pub focus_areas: Vec<String>,
    pub runs: Vec<HistoryRun>,
}

pub struct HistoryRun {
    pub date: String,
    pub product_revision: String,
    pub commit_message: Option<String>,
    pub scores: Vec<Option<ScoreTuple>>,
}

/// The group (data file, nested under the root suite's directory) that holds
/// history for a given WPT folder path: the root suite itself and its direct
/// children are in the root group (e.g. "css/css"); deeper folders are in
/// the group named for their top-level folder (e.g. "css/css-flexbox").
pub fn history_group(area: &str) -> String {
    let mut components = area.split('/');
    let root = components.next().unwrap_or("css");
    match components.next() {
        Some(second) => format!("{root}/{second}"),
        None => format!("{root}/{root}"),
    }
}

pub static WPT_HISTORY_CACHE: WptHistoryCache = WptHistoryCache::new();

pub struct WptHistoryCacheEntry {
    pub runs_etag: Option<Arc<str>>,
    pub area_etag: Option<Arc<str>>,
    pub cached_at: Instant,
    pub history: ArcWptHistory,
}

/// A cache of merged history data, keyed by group name
pub struct WptHistoryCache(Mutex<Option<HashMap<String, Arc<WptHistoryCacheEntry>>>>);
impl WptHistoryCache {
    const fn new() -> Self {
        Self(Mutex::new(None))
    }

    pub fn get_cloned(&self, group: &str) -> Option<Arc<WptHistoryCacheEntry>> {
        self.0.lock().unwrap().as_ref()?.get(group).cloned()
    }

    fn update(&self, group: &str, entry: WptHistoryCacheEntry) {
        self.0
            .lock()
            .unwrap()
            .get_or_insert_with(HashMap::new)
            .insert(group.to_string(), Arc::new(entry));
    }

    fn mark_as_fresh(&self, group: &str) {
        let mut inner = self.0.lock().unwrap();
        let Some(entry) = inner.as_mut().and_then(|map| map.get_mut(group)) else {
            return;
        };
        *entry = Arc::new(WptHistoryCacheEntry {
            cached_at: Instant::now(),
            runs_etag: entry.runs_etag.clone(),
            area_etag: entry.area_etag.clone(),
            history: entry.history.clone(),
        });
    }
}

async fn fetch(
    client: &Client,
    url: &str,
    etag: Option<&Arc<str>>,
) -> Option<(Option<Arc<str>>, Option<Vec<u8>>)> {
    let mut builder = client.get(url);
    if let Some(etag) = etag {
        builder = builder.header("If-None-Match", &**etag);
    }
    let result = builder.send().await.ok()?;
    if result.status() == 304 {
        return Some((etag.cloned(), None));
    }
    if !result.status().is_success() {
        println!("Failed to fetch {url}: HTTP {}", result.status());
        return None;
    }
    let etag = result
        .headers()
        .get("etag")
        .and_then(|header| header.to_str().ok())
        .map(Arc::from);
    Some((etag, Some(result.bytes().await.ok()?.to_vec())))
}

/// Fetch (or revalidate) the history data for one group
pub async fn load_wpt_history(group: String) {
    println!("Checking for new WPT history data for group {group:?}...");

    let existing = WPT_HISTORY_CACHE.get_cloned(&group);
    let (runs_etag, area_etag) = existing
        .as_ref()
        .map(|entry| (entry.runs_etag.clone(), entry.area_etag.clone()))
        .unwrap_or((None, None));

    let client = Client::new();
    let runs_url = format!("{SUMMARY_BASE_URL}/runs.json");
    let area_url = format!("{SUMMARY_BASE_URL}/areas/{group}.json");
    let results = tokio::join!(
        fetch(&client, &runs_url, runs_etag.as_ref()),
        fetch(&client, &area_url, area_etag.as_ref()),
    );
    let (Some(runs_result), Some(area_result)) = results else {
        return;
    };

    if runs_result.1.is_none() && area_result.1.is_none() {
        println!("WPT history data for group {group:?} unchanged");
        WPT_HISTORY_CACHE.mark_as_fresh(&group);
        return;
    }

    // If only one of the two files changed, refetch the other unconditionally
    // so the pair stays consistent (they must be index-aligned)
    let (runs_etag, runs_body) = match runs_result {
        (etag, Some(body)) => (etag, body),
        (_, None) => {
            let Some((etag, body)) = fetch(&client, &runs_url, None).await else {
                return;
            };
            (etag, body.unwrap())
        }
    };
    let (area_etag, area_body) = match area_result {
        (etag, Some(body)) => (etag, body),
        (_, None) => {
            let Some((etag, body)) = fetch(&client, &area_url, None).await else {
                return;
            };
            (etag, body.unwrap())
        }
    };

    let runs_file: RunsFile = serde_json::from_slice(&runs_body).unwrap();
    let area_file: AreaFile = serde_json::from_slice(&area_body).unwrap();
    assert_eq!(
        runs_file.runs.len(),
        area_file.scores.len(),
        "area file {group} is misaligned with runs.json"
    );

    let history = WptHistory {
        focus_areas: area_file.focus_areas,
        runs: runs_file
            .runs
            .into_iter()
            .zip(area_file.scores)
            .map(|(meta, scores)| HistoryRun {
                date: meta.date,
                product_revision: meta.product_revision,
                commit_message: meta.commit_message,
                scores,
            })
            .collect(),
    };

    println!("New WPT history data for group {group:?} processed and cached.");
    WPT_HISTORY_CACHE.update(
        &group,
        WptHistoryCacheEntry {
            runs_etag,
            area_etag,
            cached_at: Instant::now(),
            history: ArcWptHistory(Arc::new(history)),
        },
    );
}
