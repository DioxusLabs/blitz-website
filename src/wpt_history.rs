use std::{collections::HashMap, sync::Arc};

use reqwest::Client;
use serde::Deserialize;

use crate::cache::Cache;
use crate::routes::ArcWptHistory;

const SUMMARY_BASE_URL: &str =
    "https://raw.githubusercontent.com/DioxusLabs/blitz-wpt-results/main/summary";

/// Per-area scores for a single run:
/// `[total_tests, total_score, total_subtests, total_subtests_passed]`
pub type ScoreTuple = (u32, f64, u32, u32);

/// One entry of runs.json: metadata shared by all per-area score files
#[derive(Deserialize)]
pub struct RunMeta {
    pub date: String,
    #[allow(dead_code)]
    pub wpt_revision: String,
    pub product_revision: String,
    pub commit_message: Option<String>,
}

#[derive(Deserialize)]
struct RunsFile {
    runs: Vec<RunMeta>,
}

/// One summary/areas/<area>.json file: `scores` has one entry per run,
/// index-aligned with runs.json
#[derive(Deserialize)]
struct AreaFile {
    scores: Vec<Option<ScoreTuple>>,
}

/// The merged history for a set of areas
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

pub static WPT_HISTORY_CACHE: Cache<WptHistoryCacheEntry> = Cache::new();

/// Cached summary data. All summary files change together (a new run appends
/// one entry to runs.json and to every area file), so the whole cache is
/// keyed on runs.json's ETag: while it revalidates as unchanged, every cached
/// area file is still valid, and area files are only fetched on first use or
/// when runs.json changes.
pub struct WptHistoryCacheEntry {
    pub runs_etag: Option<Arc<str>>,
    runs: Arc<Vec<RunMeta>>,
    areas: HashMap<String, Arc<Vec<Option<ScoreTuple>>>>,
}

impl WptHistoryCacheEntry {
    pub fn contains_areas(&self, areas: &[String]) -> bool {
        areas.iter().all(|area| self.areas.contains_key(area))
    }

    /// Build the merged history for a set of areas (silently dropping any
    /// that have no data file)
    pub fn merged(&self, areas: &[String]) -> ArcWptHistory {
        let present: Vec<(&String, &Arc<Vec<Option<ScoreTuple>>>)> = areas
            .iter()
            .filter_map(|area| self.areas.get(area).map(|scores| (area, scores)))
            .collect();
        let runs = self
            .runs
            .iter()
            .enumerate()
            .map(|(i, meta)| HistoryRun {
                date: meta.date.clone(),
                product_revision: meta.product_revision.clone(),
                commit_message: meta.commit_message.clone(),
                scores: present.iter().map(|(_, scores)| scores[i]).collect(),
            })
            .collect();
        ArcWptHistory(Arc::new(WptHistory {
            focus_areas: present.iter().map(|(area, _)| (*area).clone()).collect(),
            runs,
        }))
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

/// Fetch the score files for a set of areas in parallel, dropping any that
/// fail to fetch or don't align with the run count
async fn fetch_areas(
    client: &Client,
    areas: &[String],
    run_count: usize,
) -> HashMap<String, Arc<Vec<Option<ScoreTuple>>>> {
    let handles: Vec<_> = areas
        .iter()
        .map(|area| {
            let client = client.clone();
            let url = format!("{SUMMARY_BASE_URL}/areas/{area}.json");
            tokio::spawn(async move { fetch(&client, &url, None).await })
        })
        .collect();

    let mut map = HashMap::new();
    for (area, handle) in areas.iter().zip(handles) {
        let Ok(Some((_, Some(body)))) = handle.await else {
            continue;
        };
        let Ok(file) = serde_json::from_slice::<AreaFile>(&body) else {
            println!("Area file {area} is not valid JSON");
            continue;
        };
        if file.scores.len() != run_count {
            println!("Area file {area} is misaligned with runs.json; skipping");
            continue;
        }
        map.insert(area.clone(), Arc::new(file.scores));
    }
    map
}

/// Fetch (or revalidate) the history data for a set of areas
pub async fn load_wpt_history(areas: Vec<String>) {
    println!(
        "Checking for new WPT history data ({} areas)...",
        areas.len()
    );

    let existing = WPT_HISTORY_CACHE.get_cloned();
    let runs_etag = existing.as_ref().and_then(|entry| entry.runs_etag.clone());

    let client = Client::new();
    let runs_url = format!("{SUMMARY_BASE_URL}/runs.json");
    let Some((runs_etag, runs_body)) = fetch(&client, &runs_url, runs_etag.as_ref()).await else {
        return;
    };

    match (runs_body, existing) {
        // runs.json unchanged: all cached area files are still valid; fetch
        // only the requested areas we don't have yet
        (None, Some(existing)) => {
            let missing: Vec<String> = areas
                .iter()
                .filter(|area| !existing.areas.contains_key(*area))
                .cloned()
                .collect();
            let mut cached_areas = existing.areas.clone();
            if !missing.is_empty() {
                println!(
                    "WPT history data unchanged; fetching {} new areas",
                    missing.len()
                );
                cached_areas.extend(fetch_areas(&client, &missing, existing.runs.len()).await);
            } else {
                println!("WPT history data unchanged");
            }
            WPT_HISTORY_CACHE.update(WptHistoryCacheEntry {
                runs_etag,
                runs: existing.runs.clone(),
                areas: cached_areas,
            });
        }
        // runs.json changed (or first load): all cached area files are
        // stale; refetch the requested areas against the new run list
        (Some(body), _) => {
            let runs_file: RunsFile = match serde_json::from_slice(&body) {
                Ok(file) => file,
                Err(err) => {
                    println!("runs.json is not valid JSON: {err}");
                    return;
                }
            };
            let cached_areas = fetch_areas(&client, &areas, runs_file.runs.len()).await;
            println!(
                "New WPT history data cached ({} runs, {} areas)",
                runs_file.runs.len(),
                cached_areas.len()
            );
            WPT_HISTORY_CACHE.update(WptHistoryCacheEntry {
                runs_etag,
                runs: Arc::new(runs_file.runs),
                areas: cached_areas,
            });
        }
        // 304 without a cache entry shouldn't happen (no etag was sent)
        (None, None) => {}
    }
}
