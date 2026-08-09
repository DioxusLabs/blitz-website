use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

use reqwest::Client;
use wptreport::score_summary::ScoreSummaryReport;

use crate::routes::ArcWptSummary;

const SUMMARY_URL: &str =
    "https://raw.githubusercontent.com/DioxusLabs/blitz-wpt-results/main/summary.json";

pub static WPT_HISTORY_CACHE: WptHistoryCache = WptHistoryCache::new();

pub struct WptHistoryCache(Mutex<Option<Arc<WptHistoryCacheEntry>>>);
impl WptHistoryCache {
    const fn new() -> Self {
        Self(Mutex::new(None))
    }

    pub fn get_cloned(&self) -> Option<Arc<WptHistoryCacheEntry>> {
        (*self.0.lock().unwrap()).clone()
    }

    pub fn update(&self, etag: Option<Arc<str>>, summary: ArcWptSummary) {
        let cached_at = Instant::now();
        *self.0.lock().unwrap() = Some(Arc::new(WptHistoryCacheEntry {
            etag,
            cached_at,
            summary,
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
            }));
        }
    }
}

pub struct WptHistoryCacheEntry {
    pub etag: Option<Arc<str>>,
    pub cached_at: Instant,
    pub summary: ArcWptSummary,
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
    let summary: ScoreSummaryReport = serde_json::from_slice(&body).unwrap();

    WPT_HISTORY_CACHE.update(etag, ArcWptSummary(Arc::new(summary)));

    println!("New WPT history summary processed and cached.");
}
