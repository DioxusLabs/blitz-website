use std::{collections::BTreeMap, io::Cursor, sync::Arc};

use reqwest::Client;
use wptreport::{
    score_wpt_report,
    wpt_report::{TestStatus, WptReport},
};

use crate::cache::{Cache, Cached};
use crate::github::{CommitInfo, GithubClient};
use crate::routes::{ArcWptReport, ArcWptScores};

pub static WPT_REPORT_CACHE: Cache<WptReportCacheEntry> = Cache::new();

#[derive(Clone)]
pub struct WptReportCacheEntry {
    pub etag: Option<Arc<str>>,
    pub report: ArcWptReport,
    pub scores: ArcWptScores,
    /// Maps test name to index within `report.results`
    pub test_index: Arc<BTreeMap<String, usize>>,
    pub commit_info: Option<CommitInfo>,
}

pub async fn load_wpt_results(existing: Option<Arc<Cached<WptReportCacheEntry>>>) {
    println!("Checking for new WPT results...");

    let etag = existing.and_then(|entry| entry.etag.clone());

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
        .map(Arc::from);

    println!("New WPT results found. etag: {etag:?}");

    let compressed_report = result.bytes().await.unwrap();

    let uncompressed_report = zstd::decode_all(Cursor::new(&compressed_report)).unwrap();
    let mut report: WptReport = serde_json::from_slice(&uncompressed_report).unwrap();
    let commit_info = report
        .run_info
        .browser_version
        .clone()
        .map(|sha| CommitInfo {
            sha: sha.clone(),
            message: None,
            timestamp: None,
        });

    let commit_info = if let Some(mut commit_info) = commit_info {
        let github_client = GithubClient::new(std::env::var("GITHUB_TOKEN").ok().as_deref());
        if let Some(github_commit) = github_client.commit_info(&commit_info.sha).await {
            commit_info.message = github_commit.message;
            commit_info.timestamp = github_commit.timestamp;
        }
        Some(commit_info)
    } else {
        None
    };

    // Strip skipped tests
    report
        .results
        .retain(|test| test.status != TestStatus::Skip);

    let scores = score_wpt_report::<WptReport>(&report);

    let test_index: BTreeMap<String, usize> = report
        .results
        .iter()
        .enumerate()
        .map(|(idx, test)| (test.test.clone(), idx))
        .collect();

    let report = ArcWptReport(Arc::new(report));
    let scores = ArcWptScores(Arc::new(scores));
    let test_index = Arc::new(test_index);

    WPT_REPORT_CACHE.update(WptReportCacheEntry {
        etag,
        report: report.clone(),
        scores: scores.clone(),
        test_index,
        commit_info,
    });

    println!("New WPT results processed and cached.");
}
