//! Fetching and refreshing of multi-engine WPT runs for the comparison view.
//!
//! The latest master run for each engine is discovered via the wpt.fyi runs
//! API; new runs have their raw wptreport.json downloaded (gzip on the wire)
//! and stream-ingested into the SQLite database. Blitz's own report is
//! fetched from its published location and ingested alongside them.

use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use reqwest::Client;
use serde::Deserialize;

use crate::wpt_db::{self, RunMeta, RunRow, WPT_COMPARE_DB};

/// Engines compared against Blitz, in display order
const PRODUCTS: &[&str] = &["chrome", "firefox", "safari", "servo", "ladybird"];

const BLITZ_REPORT_URL: &str = "https://dioxuslabs.github.io/blitz/wptreport.json.zst";

pub static WPT_COMPARE_CACHE: WptCompareCache = WptCompareCache::new();

pub struct WptCompareCache(Mutex<Option<Arc<WptCompareCacheEntry>>>);

impl WptCompareCache {
    const fn new() -> Self {
        Self(Mutex::new(None))
    }

    pub fn get_cloned(&self) -> Option<Arc<WptCompareCacheEntry>> {
        (*self.0.lock().unwrap()).clone()
    }

    fn update(&self, runs: Vec<RunRow>) {
        *self.0.lock().unwrap() = Some(Arc::new(WptCompareCacheEntry {
            cached_at: Instant::now(),
            runs: ArcRunRows(Arc::new(runs)),
        }));
    }
}

pub struct WptCompareCacheEntry {
    pub cached_at: Instant,
    /// The latest run for each product (column order for comparison pages)
    pub runs: ArcRunRows,
}

#[derive(Clone)]
pub struct ArcRunRows(pub Arc<Vec<RunRow>>);
impl PartialEq for ArcRunRows {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl std::ops::Deref for ArcRunRows {
    type Target = Vec<RunRow>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Deserialize)]
struct FyiRun {
    id: i64,
    browser_name: String,
    browser_version: String,
    os_name: Option<String>,
    full_revision_hash: String,
    time_end: Option<String>,
    raw_results_url: String,
}

/// Check for new runs on wpt.fyi (and a new Blitz report), ingest any that
/// are missing, and refresh the cached run list.
pub async fn load_wpt_compare() {
    println!("Checking for new WPT comparison runs...");
    let client = Client::new();

    let mut ingested_any = false;

    match fetch_fyi_runs(&client).await {
        Ok(runs) => {
            for run in runs {
                let meta = RunMeta {
                    product: run.browser_name.clone(),
                    browser_version: run.browser_version.clone(),
                    os: run.os_name.clone(),
                    wpt_revision: run.full_revision_hash.clone(),
                    run_time: run.time_end.clone(),
                    source_run_id: Some(run.id),
                };
                match ingest_fyi_run(&client, meta, &run.raw_results_url).await {
                    Ok(true) => ingested_any = true,
                    Ok(false) => {}
                    Err(err) => {
                        println!("Failed to ingest {} run: {err}", run.browser_name);
                    }
                }
            }
        }
        Err(err) => println!("Failed to fetch wpt.fyi runs: {err}"),
    }

    match ingest_blitz_run(&client).await {
        Ok(true) => ingested_any = true,
        Ok(false) => {}
        Err(err) => println!("Failed to ingest Blitz run: {err}"),
    }

    let runs = tokio::task::spawn_blocking(move || {
        WPT_COMPARE_DB.with(|conn| {
            if ingested_any {
                let t0 = Instant::now();
                wpt_db::recompute_area_scores(conn);
                println!(
                    "Recomputed WPT comparison area scores in {:.0}ms",
                    t0.elapsed().as_secs_f64() * 1000.0
                );
            }
            wpt_db::latest_runs(conn)
        })
    })
    .await
    .unwrap();

    // Order columns: wpt.fyi products first (in PRODUCTS order), then Blitz
    let mut ordered: Vec<RunRow> = Vec::with_capacity(runs.len());
    for product in PRODUCTS.iter().copied().chain(["blitz"]) {
        ordered.extend(runs.iter().filter(|run| run.product == product).cloned());
    }

    WPT_COMPARE_CACHE.update(ordered);
    println!("WPT comparison runs refreshed.");
}

async fn fetch_fyi_runs(
    client: &Client,
) -> Result<Vec<FyiRun>, Box<dyn std::error::Error + Send + Sync>> {
    let url = format!(
        "https://wpt.fyi/api/runs?label=master&products={}&max-count=1",
        PRODUCTS.join(",")
    );
    Ok(client.get(url).send().await?.json::<Vec<FyiRun>>().await?)
}

/// Download and ingest a wpt.fyi raw report if it hasn't been ingested yet.
/// Returns whether a new run was ingested.
async fn ingest_fyi_run(
    client: &Client,
    meta: RunMeta,
    raw_results_url: &str,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let product = meta.product.clone();
    let exists = {
        let meta = meta.clone();
        tokio::task::spawn_blocking(move || {
            WPT_COMPARE_DB.with(|conn| wpt_db::run_exists(conn, &meta))
        })
        .await?
    };
    if exists {
        return Ok(false);
    }

    println!("Downloading {product} WPT report...");
    let t0 = Instant::now();
    // The raw reports are stored gzip-encoded (~17-20 MB); download the
    // compressed bytes and decompress while stream-ingesting.
    let compressed = client
        .get(raw_results_url)
        .header("Accept-Encoding", "gzip")
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    println!(
        "Downloaded {product} WPT report ({} bytes) in {:.1}s",
        compressed.len(),
        t0.elapsed().as_secs_f64()
    );

    tokio::task::spawn_blocking(move || {
        let t0 = Instant::now();
        let reader: Box<dyn Read> = if compressed.starts_with(&[0x1f, 0x8b]) {
            Box::new(flate2::read::GzDecoder::new(&compressed[..]))
        } else {
            // Already decompressed by the HTTP client
            Box::new(&compressed[..])
        };
        WPT_COMPARE_DB
            .with(|conn| wpt_db::ingest_report(conn, &meta, reader))
            .map(|_| ())
            .map_err(|err| -> Box<dyn std::error::Error + Send + Sync> { err })?;
        println!(
            "Ingested {product} WPT report in {:.1}s",
            t0.elapsed().as_secs_f64()
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await??;

    Ok(true)
}

/// Fetch Blitz's published report and ingest it if it is a new run.
/// Returns whether a new run was ingested.
async fn ingest_blitz_run(
    client: &Client,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    #[derive(Deserialize)]
    struct BlitzRunInfo {
        browser_version: Option<String>,
        revision: Option<String>,
        os: Option<String>,
    }
    #[derive(Deserialize)]
    struct BlitzReportHead {
        run_info: BlitzRunInfo,
    }

    let compressed = client
        .get(BLITZ_REPORT_URL)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    tokio::task::spawn_blocking(move || {
        let decompressed = zstd::decode_all(std::io::Cursor::new(&compressed))?;
        let head: BlitzReportHead = serde_json::from_slice(&decompressed)?;
        let meta = RunMeta {
            product: "blitz".to_string(),
            browser_version: head.run_info.browser_version.unwrap_or_default(),
            os: head.run_info.os,
            wpt_revision: head.run_info.revision.unwrap_or_default(),
            run_time: None,
            source_run_id: None,
        };
        WPT_COMPARE_DB.with(|conn| {
            if wpt_db::run_exists(conn, &meta) {
                return Ok(false);
            }
            let t0 = Instant::now();
            wpt_db::ingest_report(conn, &meta, &decompressed[..])?;
            println!(
                "Ingested blitz WPT report in {:.1}s",
                t0.elapsed().as_secs_f64()
            );
            Ok(true)
        })
    })
    .await?
}
