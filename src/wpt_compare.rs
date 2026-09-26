//! Fetching and refreshing of multi-engine WPT runs for the comparison view.
//!
//! The latest master run for each engine is discovered via the wpt.fyi runs
//! API; new runs have their raw wptreport.json downloaded (gzip on the wire)
//! and stream-ingested into the SQLite database. Blitz's own report is
//! fetched from its published location and ingested alongside them.

use std::sync::Arc;
use std::time::Instant;

use reqwest::Client;
use serde::Deserialize;

use crate::cache::{Cache, Cached, RefreshOutcome};
use crate::wpt_db::{self, RunMeta, RunRow, WPT_COMPARE_DB};
use crate::wpt_fyi;

/// Engines compared against Blitz, in display order. The experimental
/// channels match wpt.fyi's default dashboard (the stable Safari runs in
/// particular score far lower, e.g. collapsing on the wasm suite).
const PRODUCTS: &[&str] = &[
    "chrome[experimental]",
    "firefox[experimental]",
    "safari[experimental]",
    "ladybird",
    "servo",
    "flow",
];

/// Products that are ingested (so their data stays current) but left out of
/// the comparison columns and history charts
const HIDDEN_PRODUCTS: &[&str] = &["flow"];

const BLITZ_REPORT_URL: &str = "https://dioxuslabs.github.io/blitz/wptreport.json.zst";

pub static WPT_COMPARE_CACHE: Cache<WptCompareCacheEntry> = Cache::new();

#[derive(Clone)]
pub struct WptCompareCacheEntry {
    /// The latest run for each product (column order for comparison pages)
    pub runs: ArcRunRows,
    /// ETag of the last Blitz report fetched, so unchanged reports aren't
    /// re-downloaded and decompressed on every refresh
    blitz_report_etag: Option<Arc<str>>,
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

/// Seed the cache with the runs already in the database (from a previous
/// process), so comparison pages can be served immediately after startup
/// while the first refresh checks wpt.fyi for new runs. A no-op when the
/// database is empty. Blocking: call from `spawn_blocking`.
pub fn seed_from_db() {
    let runs = WPT_COMPARE_DB.with_reader(wpt_db::latest_runs);
    if runs.is_empty() {
        return;
    }
    println!(
        "Seeded WPT comparison run list from database ({} runs)",
        runs.len()
    );
    WPT_COMPARE_CACHE.update(WptCompareCacheEntry {
        runs: ArcRunRows(Arc::new(order_runs(runs))),
        blitz_report_etag: None,
    });
}

/// Order columns: wpt.fyi products first (in `PRODUCTS` order), then Blitz.
/// `HIDDEN_PRODUCTS` are omitted.
fn order_runs(runs: Vec<RunRow>) -> Vec<RunRow> {
    let mut ordered: Vec<RunRow> = Vec::with_capacity(runs.len());
    for spec in PRODUCTS.iter().copied().chain(["blitz"]) {
        let product = spec.split('[').next().unwrap();
        if HIDDEN_PRODUCTS.contains(&product) {
            continue;
        }
        ordered.extend(runs.iter().filter(|run| run.product == product).cloned());
    }
    ordered
}

/// Check for new runs on wpt.fyi (and a new Blitz report), ingest any that
/// are missing, and refresh the cached run list. Only one refresh runs at a
/// time; calls that arrive while one is in flight return immediately and
/// leave its result in place (nothing awaits a refresh: requests that find
/// no cached data render an "unavailable" page instead).
pub async fn load_wpt_compare(
    existing: Option<Arc<Cached<WptCompareCacheEntry>>>,
) -> RefreshOutcome<WptCompareCacheEntry> {
    let Ok(_guard) = REFRESH_LOCK.try_lock() else {
        return RefreshOutcome::Unchanged;
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
                match ingest_wpt_fyi_run(meta, &run.raw_results_url).await {
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

    let mut blitz_report_etag = existing
        .as_ref()
        .and_then(|entry| entry.blitz_report_etag.clone());
    match ingest_blitz_run(&client, blitz_report_etag.as_deref()).await {
        Ok((ingested, etag)) => {
            ingested_any |= ingested;
            blitz_report_etag = etag;
        }
        Err(err) => println!("Failed to ingest Blitz run: {err}"),
    }

    let runs = tokio::task::spawn_blocking(move || {
        WPT_COMPARE_DB.with_writer(|conn| {
            // Also recompute if a previous attempt failed and left the
            // scores without one of the latest runs, so the failure is
            // retried on the next refresh rather than only on the next
            // ingest
            let stale = match wpt_db::area_scores_stale(conn) {
                Ok(stale) => stale,
                Err(err) => {
                    println!("Failed to check WPT comparison area scores: {err}");
                    false
                }
            };
            if ingested_any || stale {
                let t0 = Instant::now();
                match wpt_db::recompute_area_scores(conn) {
                    Ok(()) => println!(
                        "Recomputed WPT comparison area scores in {:.0}ms",
                        t0.elapsed().as_secs_f64() * 1000.0
                    ),
                    Err(err) => println!("Failed to recompute WPT comparison area scores: {err}"),
                }
            }
            // A no-op when there are no superseded runs, so run it on
            // every refresh in case a post-ingest prune failed
            prune_and_checkpoint(conn);
            wpt_db::latest_runs(conn)
        })
    })
    .await
    .unwrap();

    println!("WPT comparison runs refreshed.");

    RefreshOutcome::Updated(WptCompareCacheEntry {
        runs: ArcRunRows(Arc::new(order_runs(runs))),
        blitz_report_etag,
    })
}

/// Drop runs superseded by a newer one of the same product and fold the WAL
/// back into the database file. Called right after every ingest (not just at
/// the end of a refresh) so that a backlog of new runs, e.g. after a spell of
/// failed downloads, only ever costs the disk one superseded run at a time
/// rather than one per product; failures are logged, not propagated.
fn prune_and_checkpoint(conn: &mut rusqlite::Connection) {
    if let Err(err) = wpt_db::prune_old_runs(conn) {
        println!("Failed to prune old WPT comparison runs: {err}");
    }
    if let Err(err) = wpt_db::checkpoint(conn) {
        println!("Failed to checkpoint WPT comparison database: {err}");
    }
}

/// Download and ingest a wpt.fyi raw report if it hasn't been ingested yet.
/// Returns whether a new run was ingested.
async fn ingest_wpt_fyi_run(
    meta: RunMeta,
    raw_results_url: &str,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let product = meta.product.clone();
    let exists = {
        let meta = meta.clone();
        tokio::task::spawn_blocking(move || {
            WPT_COMPARE_DB.with_reader(|conn| wpt_db::run_exists(conn, &meta))
        })
        .await?
    };
    if exists {
        return Ok(false);
    }

    println!("Downloading {product} WPT report...");
    let t0 = Instant::now();
    let compressed = wpt_fyi::fetch_raw_report(raw_results_url).await?;
    println!(
        "Downloaded {product} WPT report ({} bytes) in {:.1}s",
        compressed.len(),
        t0.elapsed().as_secs_f64()
    );

    tokio::task::spawn_blocking(move || {
        let t0 = Instant::now();
        let reader = wpt_fyi::report_reader(&compressed);
        WPT_COMPARE_DB
            .with_writer(|conn| {
                wpt_db::ingest_report(conn, &meta, reader)?;
                prune_and_checkpoint(conn);
                Ok(())
            })
            .map_err(|err: Box<dyn std::error::Error + Send + Sync>| err)?;
        println!(
            "Ingested {product} WPT report in {:.1}s",
            t0.elapsed().as_secs_f64()
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await??;

    Ok(true)
}

/// Format an epoch timestamp (seconds, or milliseconds as the wptreport
/// format specifies) as an ISO 8601 UTC datetime string, matching the format
/// of wpt.fyi's run timestamps.
fn iso_datetime_from_epoch(timestamp: u64) -> String {
    const MS_THRESHOLD: u64 = 100_000_000_000;
    let secs = if timestamp >= MS_THRESHOLD {
        timestamp / 1000
    } else {
        timestamp
    } as i64;
    let ts = jiff::Timestamp::from_second(secs).unwrap_or_default();
    ts.strftime("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Fetch Blitz's published report (skipping the download if its ETag still
/// matches `etag`) and ingest it if it is a new run. Returns whether a new
/// run was ingested and the report's current ETag.
///
/// Skipping unchanged reports matters beyond bandwidth: decompressing the
/// report allocates its ~32MB zstd window through the C allocator, and
/// glibc keeps that memory pinned in the arena of whichever blocking thread
/// ran the decode, so re-decoding on every refresh slowly grows the process.
async fn ingest_blitz_run(
    client: &Client,
    etag: Option<&str>,
) -> Result<(bool, Option<Arc<str>>), Box<dyn std::error::Error + Send + Sync>> {
    #[derive(Deserialize)]
    struct BlitzRunInfo {
        browser_version: Option<String>,
        revision: Option<String>,
        os: Option<String>,
    }
    #[derive(Deserialize)]
    struct BlitzReportHead {
        run_info: BlitzRunInfo,
        time_end: Option<u64>,
    }

    let mut request = client.get(BLITZ_REPORT_URL);
    if let Some(etag) = etag {
        request = request.header("If-None-Match", etag);
    }
    let response = request.send().await?;
    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok((false, etag.map(Arc::from)));
    }
    let response = response.error_for_status()?;
    let new_etag = response
        .headers()
        .get("etag")
        .and_then(|value| value.to_str().ok())
        .map(Arc::from);
    let compressed = response.bytes().await?;

    let ingested = tokio::task::spawn_blocking(move || {
        let decompressed = zstd::decode_all(std::io::Cursor::new(&compressed))?;
        let head: BlitzReportHead = serde_json::from_slice(&decompressed)?;
        let meta = RunMeta {
            product: "blitz".to_string(),
            browser_version: head.run_info.browser_version.unwrap_or_default(),
            os: head.run_info.os,
            wpt_revision: head.run_info.revision.unwrap_or_default(),
            run_time: head.time_end.map(iso_datetime_from_epoch),
            source_run_id: None,
        };
        WPT_COMPARE_DB.with_writer(|conn| {
            if wpt_db::run_exists(conn, &meta) {
                wpt_db::backfill_run_time(conn, &meta)?;
                return Ok(false);
            }
            let t0 = Instant::now();
            wpt_db::ingest_report(conn, &meta, &decompressed[..])?;
            prune_and_checkpoint(conn);
            println!(
                "Ingested blitz WPT report in {:.1}s",
                t0.elapsed().as_secs_f64()
            );
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(true)
        })
    })
    .await??;
    Ok((ingested, new_etag))
}
