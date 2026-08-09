use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};

use reqwest::Client;
use serde::Deserialize;
use wptreport::score_summary::{RunScores, RunSummary, ScoreSummaryReport};

use crate::routes::{ArcCommitMessages, ArcWptSummary};

const SUMMARY_URL: &str =
    "https://raw.githubusercontent.com/DioxusLabs/blitz-wpt-results/main/summary.json";
const COMMIT_MESSAGES_URL: &str =
    "https://raw.githubusercontent.com/DioxusLabs/blitz-wpt-results/main/commit-messages.json";

/// The compact on-disk format of summary.json: per-area scores are stored as
/// `[total_tests, total_score, total_subtests, total_subtests_passed]` arrays
/// (parallel to `focus_areas`)
#[derive(Deserialize)]
struct CompactSummary {
    focus_areas: Vec<String>,
    runs: Vec<CompactRun>,
}

#[derive(Deserialize)]
struct CompactRun {
    date: String,
    wpt_revision: String,
    product_revision: String,
    scores: Vec<(u32, f64, u32, u32)>,
}

impl From<CompactSummary> for ScoreSummaryReport {
    fn from(compact: CompactSummary) -> Self {
        ScoreSummaryReport {
            focus_areas: compact.focus_areas,
            runs: compact
                .runs
                .into_iter()
                .map(|run| RunSummary {
                    date: run.date,
                    wpt_revision: run.wpt_revision,
                    product_revision: run.product_revision,
                    scores: run
                        .scores
                        .into_iter()
                        .map(
                            |(total_tests, total_score, total_subtests, total_subtests_passed)| {
                                RunScores {
                                    total_tests,
                                    total_score,
                                    total_subtests,
                                    total_subtests_passed,
                                }
                            },
                        )
                        .collect(),
                })
                .collect(),
        }
    }
}

pub static WPT_HISTORY_CACHE: WptHistoryCache = WptHistoryCache::new();

pub struct WptHistoryCache(Mutex<Option<Arc<WptHistoryCacheEntry>>>);
impl WptHistoryCache {
    const fn new() -> Self {
        Self(Mutex::new(None))
    }

    pub fn get_cloned(&self) -> Option<Arc<WptHistoryCacheEntry>> {
        (*self.0.lock().unwrap()).clone()
    }

    pub fn update(
        &self,
        etag: Option<Arc<str>>,
        summary: ArcWptSummary,
        commit_messages: ArcCommitMessages,
    ) {
        let cached_at = Instant::now();
        *self.0.lock().unwrap() = Some(Arc::new(WptHistoryCacheEntry {
            etag,
            cached_at,
            summary,
            commit_messages,
        }));
    }

    pub fn mark_as_fresh(&self) {
        let cached_at = Instant::now();
        let mut inner = self.0.lock().unwrap();
        if let Some(entry) = inner.take() {
            *inner = Some(Arc::new(WptHistoryCacheEntry {
                cached_at,
                etag: entry.etag.clone(),
                summary: entry.summary.clone(),
                commit_messages: entry.commit_messages.clone(),
            }));
        }
    }
}

pub struct WptHistoryCacheEntry {
    pub etag: Option<Arc<str>>,
    pub cached_at: Instant,
    pub summary: ArcWptSummary,
    pub commit_messages: ArcCommitMessages,
}

pub async fn load_wpt_history(etag: Option<Arc<str>>) {
    println!("Checking for new WPT history summary...");

    let client = Client::new();
    let mut builder = client.get(SUMMARY_URL);

    if let Some(etag) = etag.as_ref() {
        builder = builder.header("If-None-Match", &**etag);
    }
    let result = builder.send().await.unwrap();

    if result.status() == 304 {
        println!("WPT history summary unchanged");
        WPT_HISTORY_CACHE.mark_as_fresh();
        return;
    }

    let etag = result
        .headers()
        .get("etag")
        .and_then(|header| header.to_str().ok())
        .map(Arc::from);

    println!("New WPT history summary found. etag: {etag:?}");

    let body = result.bytes().await.unwrap();
    let compact: CompactSummary = serde_json::from_slice(&body).unwrap();
    let summary = ScoreSummaryReport::from(compact);

    // Commit messages are optional: tooltips degrade gracefully without them
    let commit_messages: HashMap<String, String> = match client
        .get(COMMIT_MESSAGES_URL)
        .send()
        .await
        .and_then(|res| res.error_for_status())
    {
        Ok(res) => serde_json::from_slice(&res.bytes().await.unwrap()).unwrap_or_default(),
        Err(err) => {
            println!("Failed to fetch commit messages: {err}");
            HashMap::new()
        }
    };

    WPT_HISTORY_CACHE.update(
        etag,
        ArcWptSummary(Arc::new(summary)),
        ArcCommitMessages(Arc::new(commit_messages)),
    );

    println!("New WPT history summary processed and cached.");
}
