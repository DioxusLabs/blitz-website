use std::{
    io::Cursor,
    sync::{Arc, Mutex, MutexGuard},
    time::Instant,
};

use reqwest::Client;
use wptreport::{
    score_wpt_report,
    wpt_report::{TestStatus, WptReport},
};

use crate::routes::{ArcWptReport, ArcWptScores};

pub static WPT_REPORT_CACHE: WptReportCache = WptReportCache::new();

pub struct WptReportCache(Mutex<Option<Arc<WptReportCacheEntry>>>);
impl WptReportCache {
    const fn new() -> Self {
        Self(Mutex::new(None))
    }

    pub fn get_cloned(&self) -> Option<Arc<WptReportCacheEntry>> {
        (*self.0.lock().unwrap()).clone()
    }

    #[allow(dead_code)]
    pub fn get_mut(&self) -> MutexGuard<'_, Option<Arc<WptReportCacheEntry>>> {
        self.0.lock().unwrap()
    }

    pub fn update(&self, etag: Option<Arc<str>>, report: ArcWptReport, scores: ArcWptScores) {
        let cached_at = Instant::now();
        *self.0.lock().unwrap() = Some(Arc::new(WptReportCacheEntry {
            etag,
            cached_at,
            report,
            scores,
        }));
    }

    pub fn mark_as_fresh(&self) {
        let cached_at = Instant::now();
        let mut inner = self.0.lock().unwrap();
        if let Some(entry) = inner.take() {
            *inner = Some(Arc::new(WptReportCacheEntry {
                cached_at,
                etag: entry.etag.clone(),
                report: entry.report.clone(),
                scores: entry.scores.clone(),
            }));
        }
    }
}

pub struct WptReportCacheEntry {
    pub etag: Option<Arc<str>>,
    pub cached_at: Instant,
    pub report: ArcWptReport,
    pub scores: ArcWptScores,
}

pub async fn load_wpt_results(etag: Option<Arc<str>>) {
    println!("Checking for new WPT results...");

    // Request latest WPT report (with etag)
    let client = Client::new();
    let mut builder = client.get("https://dioxuslabs.github.io/blitz/wptreport.json.zst");

    if let Some(etag) = etag.as_ref() {
        builder = builder.header("If-None-Match", &**etag);
    }
    let result = builder.send().await.unwrap();

    if result.status() == 304 {
        println!("WPT results unchanged");
        WPT_REPORT_CACHE.mark_as_fresh();
        return;
    }

    let etag = result
        .headers()
        .get("etag")
        .and_then(|header| header.to_str().ok())
        .map(|s| Arc::from(s));

    println!("New WPT results found. etag: {etag:?}");

    let compressed_report = result.bytes().await.unwrap();

    let uncompressed_report = zstd::decode_all(Cursor::new(&compressed_report)).unwrap();
    let mut report: WptReport = serde_json::from_slice(&uncompressed_report).unwrap();

    // Strip skipped tests
    report
        .results
        .retain(|test| test.status != TestStatus::Skip);

    let scores = score_wpt_report::<WptReport>(&report);

    let report = ArcWptReport(Arc::new(report));
    let scores = ArcWptScores(Arc::new(scores));

    WPT_REPORT_CACHE.update(etag, report.clone(), scores.clone());

    println!("New WPT results processed and cached.");
}
