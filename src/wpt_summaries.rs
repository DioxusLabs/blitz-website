//! Per-area WPT score history for every engine (Blitz, Chrome, Firefox,
//! Safari, Ladybird, Servo, ...) from DioxusLabs/browser-wpt-results, which
//! stores one dataset per product: `summary/<product>/{runs.json,areas/<area>.json}`
//! (see [`crate::wpt_history`] for the format).
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
const REPO_URL_ENV: &str = "WPT_SUMMARIES_REPO";

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

pub static WPT_SUMMARY_CACHE: Cache<SummaryCacheEntry> = Cache::new();

/// Cached summary data, keyed on the commit it was read from: every update
/// to the repository is one commit touching all of a product's files, so
/// while the commit is unchanged every cached file is still valid.
#[derive(Clone)]
pub struct SummaryCacheEntry {
    commit: gix::ObjectId,
    synced_at: Instant,
    runs: HashMap<String, Arc<Vec<RunMeta>>>,
    areas: HashMap<(String, String), Arc<Vec<Option<ScoreTuple>>>>,
    /// `(product, area)` pairs looked up and found to have no data file, so
    /// they aren't looked up again until the repository changes
    missing: HashSet<(String, String)>,
}

impl SummaryCacheEntry {
    /// Whether every request has been looked up (found or not)
    pub fn contains(&self, requests: &[(String, String)]) -> bool {
        requests
            .iter()
            .all(|request| self.areas.contains_key(request) || self.missing.contains(request))
    }

    /// The metadata of a product's run by its `product_revision` (the commit
    /// sha for Blitz), if the product's run list has been loaded
    pub fn run_meta(&self, product: &str, product_revision: &str) -> Option<&RunMeta> {
        self.runs
            .get(product)?
            .iter()
            .rev()
            .find(|meta| meta.product_revision == product_revision)
    }

    /// The merged history of a product for a set of areas (silently dropping
    /// areas the product has no data file for), or `None` if the product is
    /// unknown or has none of the areas. `denominators` is index-aligned
    /// with `areas`: the cross-engine union subtest total of each area, if
    /// known (see [`WptHistory::denominator`]).
    pub fn history(
        &self,
        product: &str,
        areas: &[String],
        denominators: &[Option<u32>],
    ) -> Option<ArcWptHistory> {
        let runs = self.runs.get(product)?;
        let present: Vec<PresentArea> = areas
            .iter()
            .enumerate()
            .filter_map(|(i, area)| {
                let scores = self.areas.get(&(product.to_string(), area.clone()))?;
                Some((area, denominators.get(i).copied().flatten(), scores))
            })
            .collect();
        if present.is_empty() {
            return None;
        }
        let runs = runs
            .iter()
            .enumerate()
            .map(|(i, meta)| HistoryRun {
                date: meta.date.clone(),
                product_revision: meta.product_revision.clone(),
                commit_message: meta.commit_message.clone(),
                scores: present.iter().map(|(_, _, scores)| scores[i]).collect(),
            })
            .collect();
        Some(ArcWptHistory(Arc::new(WptHistory {
            focus_areas: present.iter().map(|(area, _, _)| (*area).clone()).collect(),
            denominators: present.iter().map(|(_, denom, _)| *denom).collect(),
            runs,
        })))
    }
}

/// An area a product has data for: `(area, union denominator, per-run scores)`
type PresentArea<'a> = (&'a String, Option<u32>, &'a Arc<Vec<Option<ScoreTuple>>>);

/// Only one fetch at a time; a refresh that finds another in flight leaves
/// the cache as it is
static SYNC_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Load (or revalidate) the history for a set of `(product, area)` pairs
pub async fn load_wpt_summaries(
    requests: Vec<(String, String)>,
    existing: Option<Arc<Cached<SummaryCacheEntry>>>,
) -> RefreshOutcome<SummaryCacheEntry> {
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
    existing: Option<Arc<Cached<SummaryCacheEntry>>>,
    fetch: bool,
) -> RefreshOutcome<SummaryCacheEntry> {
    let mut synced_at = existing
        .as_ref()
        .map(|entry| entry.synced_at)
        .unwrap_or_else(Instant::now);
    if fetch {
        println!("Syncing WPT summaries repository...");
        match MIRROR.sync() {
            Ok(()) => synced_at = Instant::now(),
            // A stale clone is still usable; only a missing one is fatal
            Err(err) => println!("Failed to sync WPT summaries repository: {err}"),
        }
    }
    let snapshot = match MIRROR.head() {
        Ok(snapshot) => snapshot,
        Err(err) => {
            println!("WPT summaries repository unavailable: {err}");
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
        // The empty area is the whole-run total, stored beside runs.json
        let path = if area.is_empty() {
            format!("summary/{product}/total.json")
        } else {
            format!("summary/{product}/areas/{area}.json")
        };
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
            "WPT summaries: read {read} area files at {}",
            snapshot.commit
        );
    }

    RefreshOutcome::Updated(SummaryCacheEntry {
        commit: snapshot.commit,
        synced_at,
        runs,
        areas,
        missing,
    })
}
