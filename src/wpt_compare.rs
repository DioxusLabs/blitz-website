//! Fetching and refreshing of multi-engine WPT runs for the comparison view.
//!
//! The latest master run for each engine is discovered via the wpt.fyi runs
//! API; new runs have their raw wptreport.json downloaded (gzip on the wire)
//! and stream-ingested into the SQLite database. Blitz's own report is
//! fetched from its published location and ingested alongside them.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use reqwest::Client;
use serde::Deserialize;

use crate::wpt_db::{self, RunMeta, RunRow, WPT_COMPARE_DB};
use crate::wpt_fyi;

/// Engines compared against Blitz, in display order. The experimental
/// channels match wpt.fyi's default dashboard (the stable Safari runs in
/// particular score far lower, e.g. collapsing on the wasm suite).
const PRODUCTS: &[&str] = &[
    "chrome[experimental]",
    "firefox[experimental]",
    "safari[experimental]",
    "servo",
    "ladybird",
];

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

/// Process-global lock ensuring only one refresh runs at a time (repeated
/// page visits during a slow ingest would otherwise start concurrent ones)
static REFRESH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Check for new runs on wpt.fyi (and a new Blitz report), ingest any that
/// are missing, and refresh the cached run list. Only one refresh runs at a
/// time; calls that arrive while one is in flight wait for it and return
/// without doing their own.
pub async fn load_wpt_compare() {
    let Ok(_guard) = REFRESH_LOCK.try_lock() else {
        // A refresh is already running: wait for it to finish instead
        let _ = REFRESH_LOCK.lock().await;
        return;
    };

    println!("Checking for new WPT comparison runs...");
    let client = Client::new();

    let mut ingested_any = false;

    match wpt_fyi::fetch_latest_runs(&client, PRODUCTS).await {
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
            // Only the latest run per product is kept; a no-op when there
            // are no superseded runs, so run it on every refresh
            wpt_db::prune_old_runs(conn);
            wpt_db::latest_runs(conn)
        })
    })
    .await
    .unwrap();

    // Order columns: wpt.fyi products first (in PRODUCTS order), then Blitz
    let mut ordered: Vec<RunRow> = Vec::with_capacity(runs.len());
    for spec in PRODUCTS.iter().copied().chain(["blitz"]) {
        let product = spec.split('[').next().unwrap();
        ordered.extend(runs.iter().filter(|run| run.product == product).cloned());
    }

    WPT_COMPARE_CACHE.update(ordered);
    println!("WPT comparison runs refreshed.");
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
    let compressed = wpt_fyi::fetch_raw_report(client, raw_results_url).await?;
    println!(
        "Downloaded {product} WPT report ({} bytes) in {:.1}s",
        compressed.len(),
        t0.elapsed().as_secs_f64()
    );

    tokio::task::spawn_blocking(move || {
        let t0 = Instant::now();
        let reader = wpt_fyi::report_reader(&compressed);
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
