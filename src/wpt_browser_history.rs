//! Per-area score history for browsers (Chrome, Firefox, Safari, Ladybird,
//! Servo, ...) from DioxusLabs/browser-wpt-results, which stores one
//! blitz-wpt-results-style `summary/` dataset per product:
//! `summary/<product>/{runs.json,areas/<area>.json}`.
//!
//! The repository is kept as a bare clone in the data directory and read with
//! `gix`, so a refresh is one `git fetch` and reading an area is a local
//! object lookup rather than an HTTP request per file.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use crate::cache::{Cache, Cached, RefreshOutcome};
use crate::git_mirror::GitMirror;
use crate::routes::ArcWptHistory;
use crate::wpt_history::{AreaFile, HistoryRun, RunMeta, RunsFile, ScoreTuple, WptHistory};

const REPO_URL: &str = "https://github.com/DioxusLabs/browser-wpt-results.git";
/// Overrides the repository cloned (e.g. a local path for development)
const REPO_URL_ENV: &str = "BROWSER_WPT_RESULTS_REPO";

/// How long a synced clone is trusted before fetching again. Requests for
/// areas not cached yet within this window are served from the local clone
/// without a fetch.
pub const FETCH_INTERVAL: Duration = Duration::from_mins(5);

static MIRROR: LazyLock<GitMirror> = LazyLock::new(|| {
    GitMirror::new(
        std::env::var(REPO_URL_ENV).unwrap_or_else(|_| REPO_URL.to_string()),
        crate::wpt_db::data_dir().join("browser-wpt-results.git"),
    )
});

pub static BROWSER_HISTORY_CACHE: Cache<BrowserHistoryCacheEntry> = Cache::new();

/// Cached summary data, keyed on the commit it was read from: every update
/// to the repository is one commit touching all of a product's files, so
/// while the commit is unchanged every cached file is still valid.
#[derive(Clone)]
pub struct BrowserHistoryCacheEntry {
    commit: gix::ObjectId,
    synced_at: Instant,
    runs: HashMap<String, Arc<Vec<RunMeta>>>,
    areas: HashMap<(String, String), Arc<Vec<Option<ScoreTuple>>>>,
    /// `(product, area)` pairs looked up and found to have no data file, so
    /// they aren't looked up again until the repository changes
    missing: HashSet<(String, String)>,
}

impl BrowserHistoryCacheEntry {
    /// Whether every request has been looked up (found or not)
    pub fn contains(&self, requests: &[(String, String)]) -> bool {
        requests
            .iter()
            .all(|request| self.areas.contains_key(request) || self.missing.contains(request))
    }

    /// The history of a single area for a product, or `None` if the product
    /// has no data for it
    pub fn history(&self, product: &str, area: &str) -> Option<ArcWptHistory> {
        let runs = self.runs.get(product)?;
        let scores = self.areas.get(&(product.to_string(), area.to_string()))?;
        let runs = runs
            .iter()
            .zip(scores.iter())
            .map(|(meta, score)| HistoryRun {
                date: meta.date.clone(),
                product_revision: meta.product_revision.clone(),
                commit_message: meta.commit_message.clone(),
                scores: vec![*score],
            })
            .collect();
        Some(ArcWptHistory(Arc::new(WptHistory {
            focus_areas: vec![area.to_string()],
            runs,
        })))
    }
}

/// Only one fetch at a time; a refresh that finds another in flight leaves
/// the cache as it is
static SYNC_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Load (or revalidate) the history for a set of `(product, area)` pairs
pub async fn load_browser_history(
    requests: Vec<(String, String)>,
    existing: Option<Arc<Cached<BrowserHistoryCacheEntry>>>,
) -> RefreshOutcome<BrowserHistoryCacheEntry> {
    let Ok(_guard) = SYNC_LOCK.try_lock() else {
        return RefreshOutcome::Unchanged;
    };
    // A recently synced clone that merely lacks some areas is read without
    // fetching again
    let fetch = existing
        .as_ref()
        .is_none_or(|entry| entry.synced_at.elapsed() > FETCH_INTERVAL);

    tokio::task::spawn_blocking(move || load_blocking(requests, existing, fetch))
        .await
        .unwrap_or(RefreshOutcome::Failed)
}

fn load_blocking(
    requests: Vec<(String, String)>,
    existing: Option<Arc<Cached<BrowserHistoryCacheEntry>>>,
    fetch: bool,
) -> RefreshOutcome<BrowserHistoryCacheEntry> {
    let mut synced_at = existing
        .as_ref()
        .map(|entry| entry.synced_at)
        .unwrap_or_else(Instant::now);
    if fetch {
        println!("Syncing browser WPT history repository...");
        match MIRROR.sync() {
            Ok(()) => synced_at = Instant::now(),
            // A stale clone is still usable; only a missing one is fatal
            Err(err) => println!("Failed to sync browser WPT history repository: {err}"),
        }
    }
    let snapshot = match MIRROR.head() {
        Ok(snapshot) => snapshot,
        Err(err) => {
            println!("Browser WPT history repository unavailable: {err}");
            return RefreshOutcome::Failed;
        }
    };

    let (mut runs, mut areas, mut missing) = match existing {
        Some(entry) if entry.commit == snapshot.commit => (
            entry.runs.clone(),
            entry.areas.clone(),
            entry.missing.clone(),
        ),
        _ => Default::default(),
    };

    let mut read = 0;
    for (product, area) in requests {
        let key = (product.clone(), area.clone());
        if areas.contains_key(&key) || missing.contains(&key) {
            continue;
        }
        if !runs.contains_key(&product) {
            let path = format!("summary/{product}/runs.json");
            match snapshot.read_file(&path) {
                Ok(Some(body)) => match serde_json::from_slice::<RunsFile>(&body) {
                    Ok(file) => {
                        runs.insert(product.clone(), Arc::new(file.runs));
                    }
                    Err(err) => println!("{path} is not valid JSON: {err}"),
                },
                Ok(None) => {}
                Err(err) => println!("Failed to read {path}: {err}"),
            }
        }
        let Some(product_runs) = runs.get(&product) else {
            missing.insert(key);
            continue;
        };
        let path = format!("summary/{product}/areas/{area}.json");
        match snapshot.read_file(&path) {
            Ok(Some(body)) => match serde_json::from_slice::<AreaFile>(&body) {
                Ok(file) if file.scores.len() == product_runs.len() => {
                    areas.insert(key, Arc::new(file.scores));
                    read += 1;
                    continue;
                }
                Ok(_) => println!("{path} is misaligned with runs.json; skipping"),
                Err(err) => println!("{path} is not valid JSON: {err}"),
            },
            Ok(None) => {}
            Err(err) => println!("Failed to read {path}: {err}"),
        }
        missing.insert(key);
    }
    if read > 0 {
        println!(
            "Browser WPT history: read {read} area files at {}",
            snapshot.commit
        );
    }

    RefreshOutcome::Updated(BrowserHistoryCacheEntry {
        commit: snapshot.commit,
        synced_at,
        runs,
        areas,
        missing,
    })
}
