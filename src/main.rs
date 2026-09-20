use axum::{
    body::{Body, Bytes},
    extract::{Path, Query, RawQuery},
    http::{header, StatusCode},
    response::{AppendHeaders, Html, IntoResponse, Redirect, Response},
    routing::{get, get_service},
    Router,
};
use tokio_util::io::ReaderStream;
// use axum::{
//     body::,
//     http::{, StatusCode},
//     response::{Headers, IntoResponse},
//     routing::get,
//     Router,
// };
use dashmap::DashMap;
use dioxus::{core::ComponentFunction, prelude::*};
use dioxus_html_macro::html;
use downloads::{load_downloads, DOWNLOAD_CACHE};
use github::CommitInfo;
use routes::{
    encode_test_path, product_color, product_label, AboutPage, ArcDownloadLinks, ArcWptHistory,
    BlitzAreaResults, ChartLine, ChartRange, ChartSeries, CssSupportPage, DownloadsPage,
    DownloadsPageProps, DownloadsUnavailablePage, ElementSupportPage, EventSupportPage,
    GettingStartedPage, HomePage, NLNetInstructionsPage, TestPageTab, WptComparePage,
    WptComparePageProps, WptCompareTestPage, WptCompareTestPageProps, WptFocusAreasPage,
    WptFocusAreasPageProps, WptHistoryPage, WptHistoryPageProps, WptResultsPage,
    WptResultsPageProps, WptUnavailablePage, WptUnavailablePageProps,
};
use serde::Deserialize;
use std::{
    net::{IpAddr, SocketAddr},
    sync::LazyLock,
    time::{Duration, Instant},
};
use tokio::net::TcpListener;
use tokio::sync::{Semaphore, SemaphorePermit};
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::{DefaultOnResponse, TraceLayer},
};
use tracing::Level;
use wpt_compare::{load_wpt_compare, WPT_COMPARE_CACHE};
use wpt_db::{RunRow, WPT_COMPARE_DB};
use wpt_summaries::{load_wpt_summaries, WPT_SUMMARY_CACHE};

mod cache;
mod components;
mod downloads;
mod git_mirror;
mod github;
mod routes;
mod wpt_compare;
mod wpt_db;
mod wpt_fyi;
mod wpt_history;
mod wpt_source;
mod wpt_spec_meta;
mod wpt_summaries;

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[derive(Deserialize)]
struct WptPageQuery {
    range: Option<String>,
}

#[derive(Deserialize)]
struct WptCompareQuery {
    sort: Option<String>,
    range: Option<String>,
    tab: Option<String>,
}

#[derive(Deserialize)]
struct DownloadLinkKey {
    platform: String,
    arch: String,
    bundle_format: String,
}

#[tokio::main]
async fn main() {
    // initialize tracing
    tracing_subscriber::fmt::init();

    // build our application with a route
    let app = Router::new()
        .route("/", get(|| dx_route_cached(|| html!(<HomePage />))))
        .route("/about", get(|| dx_route_cached(|| html!(<AboutPage />))))
        .route(
            "/nlnet-testing-instructions",
            get(|| dx_route_cached(|| html!(<NLNetInstructionsPage />))),
        )
        .route(
            "/downloads/file",
            get(async |query: Query<DownloadLinkKey>| {
                let query: DownloadLinkKey = query.0;
                let Some(cache_entry) = DOWNLOAD_CACHE.get_cloned() else {
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Downloads not available".to_string(),
                    ));
                };

                let Some(link) =
                    cache_entry
                        .artifacts
                        .iter()
                        .find(|artifact: &&downloads::DownloadLink| {
                            artifact.arch == query.arch
                                && artifact.platform == query.platform
                                && artifact.bundle_format == query.bundle_format
                        })
                else {
                    return Err((
                        StatusCode::NOT_FOUND,
                        "Matching artifact not found".to_string(),
                    ));
                };

                // `File` implements `AsyncRead`
                let file = match tokio::fs::File::open(&link.file_path).await {
                    Ok(file) => file,
                    Err(err) => {
                        return Err((StatusCode::NOT_FOUND, format!("File not found: {}", err)))
                    }
                };
                // convert the `AsyncRead` into a `Stream`
                let stream = ReaderStream::new(file);
                // convert the `Stream` into an `axum::body::HttpBody`
                let body = Body::from_stream(stream);

                let headers = AppendHeaders([
                    (header::CONTENT_TYPE, "text/toml; charset=utf-8".to_string()),
                    (
                        header::CONTENT_DISPOSITION,
                        format!("attachment; filename=\"{}\"", link.filename),
                    ),
                ]);

                Ok((headers, body))
            }),
        )
        .route(
            "/status/wpt",
            get(|| async { Redirect::to("/status/wpt/css") }),
        )
        .route(
            "/status/wpt/history",
            get(async |query: Query<WptPageQuery>| {
                let range = ChartRange::from_query(query.range.as_deref());
                // The history page charts "css" plus every one of its
                // direct children (sparklines show them all)
                let (run, _slot) = match blitz_status_run().await {
                    Ok(run) => run,
                    Err(response) => return *response,
                };
                let (areas, union_totals): (Vec<String>, Vec<Option<u32>>) =
                    tokio::task::spawn_blocking(move || {
                        WPT_COMPARE_DB.with_reader(|conn| {
                            let run_ids = [run.id];
                            let css = wpt_db::area_score(conn, &run_ids, "css");
                            std::iter::once(("css".to_string(), css))
                                .chain(wpt_db::child_area_scores(
                                    conn,
                                    &run_ids,
                                    "css",
                                    wpt_db::AreaSort::Alpha,
                                ))
                                .map(|(area, scores)| (area, union_subtest_total(&scores)))
                                .unzip()
                        })
                    })
                    .await
                    .unwrap();
                let Some(history) = fresh_wpt_history(areas, union_totals).await else {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Html("History data not available".to_string()),
                    )
                        .into_response();
                };
                let props = WptHistoryPageProps { history, range };
                dx_route_with_props(WptHistoryPage, props)
                    .await
                    .into_response()
            }),
        )
        .route(
            "/status/wpt/{*area}",
            get(
                async |Path(area): Path<String>,
                       Query(query): Query<WptPageQuery>,
                       RawQuery(raw_query): RawQuery| {
                    let area = area.trim_matches('/').to_string();
                    let (run, _slot) = match blitz_status_run().await {
                        Ok(run) => run,
                        Err(response) => return *response,
                    };
                    let run_id = run.id;

                    let is_area = {
                        let area = area.clone();
                        tokio::task::spawn_blocking(move || {
                            WPT_COMPARE_DB.with_reader(|conn| wpt_db::area_exists(conn, &area))
                        })
                        .await
                        .unwrap()
                    };
                    // Test pages live in the comparison section now; the
                    // `?tab=` query carries over
                    if !is_area {
                        let mut target = format!("/wpt/{}", encode_test_path(&area));
                        if let Some(raw_query) = raw_query {
                            target.push('?');
                            target.push_str(&raw_query);
                        }
                        return Redirect::permanent(&target).into_response();
                    }

                    let results = {
                        let area = area.clone();
                        tokio::task::spawn_blocking(move || {
                            WPT_COMPARE_DB.with_reader(|conn| {
                                if !wpt_db::area_exists(conn, &area) {
                                    return None;
                                }
                                let run_ids = [run_id];
                                let score = wpt_db::area_score(conn, &run_ids, &area)[0];
                                let child_areas = wpt_db::child_area_scores(
                                    conn,
                                    &run_ids,
                                    &area,
                                    wpt_db::AreaSort::Subtests,
                                )
                                .into_iter()
                                .map(|(name, scores)| (name, scores[0]))
                                .collect();
                                let tests = wpt_db::tests_in_area(conn, &run_ids, &area);
                                Some(BlitzAreaResults {
                                    area,
                                    score,
                                    child_areas,
                                    tests,
                                })
                            })
                        })
                        .await
                        .unwrap()
                    };

                    // Folder pages chart a single line for the folder itself;
                    // the history lookup also loads Blitz's run list, which
                    // holds the commit message and date for the header
                    let subtest_total = results
                        .as_ref()
                        .and_then(|results| union_subtest_total(&[results.score]));
                    let history = fresh_wpt_history(vec![area.clone()], vec![subtest_total]).await;
                    let commit_info = blitz_commit_info(&run).await;

                    if let Some(results) = results {
                        let range = ChartRange::from_query(query.range.as_deref());
                        let props = WptResultsPageProps {
                            results,
                            commit_info,
                            history,
                            range,
                        };

                        return dx_route_with_props(WptResultsPage, props)
                            .await
                            .into_response();
                    }

                    (StatusCode::NOT_FOUND, format!("Unknown WPT area: {area}")).into_response()
                },
            ),
        )
        .route(
            "/wpt",
            get(async |Query(query): Query<WptCompareQuery>| {
                wpt_compare_route(String::new(), query).await
            }),
        )
        .route(
            "/wpt/focus-areas/{set}",
            get(async |Path(set): Path<String>| wpt_focus_areas_route(set).await),
        )
        .route(
            "/wpt/{*area}",
            get(
                async |Path(area): Path<String>, Query(query): Query<WptCompareQuery>| {
                    wpt_compare_route(area.trim_matches('/').to_string(), query).await
                },
            ),
        )
        .route(
            "/downloads",
            get(async || {
                // Serve directly for 30s; any older entry is served stale
                // while revalidating in the background (builds are heavy to
                // fetch, so a request never awaits a refresh once primed)
                let Some(entry) = DOWNLOAD_CACHE
                    .get_or_refresh(Duration::from_secs(30), Duration::MAX, load_downloads)
                    .await
                else {
                    let (_, html) = dx_route_with_props(DownloadsUnavailablePage, ()).await;
                    return (StatusCode::SERVICE_UNAVAILABLE, html);
                };
                let props = DownloadsPageProps {
                    links: ArcDownloadLinks(entry.artifacts.clone()),
                    commit_info: entry.commit_info.clone(),
                };

                dx_route_with_props(DownloadsPage, props).await
            }),
        )
        .route("/status", get(|| async { Redirect::to("/status/css") }))
        .route(
            "/status/css",
            get(|| dx_route_cached(|| html!(<CssSupportPage />))),
        )
        .route(
            "/status/elements",
            get(|| dx_route_cached(|| html!(<ElementSupportPage />))),
        )
        .route(
            "/status/events",
            get(|| dx_route_cached(|| html!(<EventSupportPage />))),
        )
        .route(
            "/getting-started",
            get(|| dx_route_cached(|| html!(<GettingStartedPage />))),
        )
        .nest_service("/static", get_service(ServeDir::new("static")))
        .route_service("/robots.txt", ServeFile::new("static/robots.txt"))
        // One line per response at info level, including the user agent so
        // crawler traffic can be identified from the logs
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &axum::http::Request<Body>| {
                    let user_agent = request
                        .headers()
                        .get(header::USER_AGENT)
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or("-");
                    tracing::info_span!(
                        "request",
                        method = %request.method(),
                        uri = %request.uri(),
                        user_agent,
                    )
                })
                .on_request(())
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        );

    // run our app with hyper
    // `axum::Server` is a re-export of `hyper::Server`
    let host: IpAddr = std::env::var("HOST")
        .ok()
        .and_then(|h| h.parse().ok())
        .unwrap_or("::".parse().unwrap());
    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3333);
    let addr = SocketAddr::from((host, port));
    let listener = TcpListener::bind(addr).await.unwrap();

    // Clone (or fetch) the score history repository so the first history
    // page doesn't wait on it
    tokio::spawn(WPT_SUMMARY_CACHE.refresh(|existing| {
        load_wpt_summaries(vec![("blitz".to_string(), "css".to_string())], existing)
    }));
    // Serve the runs from the previous process right away, then refresh WPT
    // comparison data on startup and every 15 minutes (the first tick fires
    // immediately), so new runs are ingested off the request path
    tokio::task::spawn_blocking(wpt_compare::seed_from_db)
        .await
        .unwrap();
    tokio::spawn(async {
        let mut interval = tokio::time::interval(Duration::from_mins(15));
        loop {
            interval.tick().await;
            WPT_COMPARE_CACHE.refresh(load_wpt_compare).await;
        }
    });

    if std::env::var("PRECACHE_DOWNLOADS").is_ok() {
        tokio::spawn(DOWNLOAD_CACHE.refresh(load_downloads));
    }

    let msg = format!("Serving blitz-website at http://{addr}").replace("[::]", "localhost");
    println!("{msg}");

    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}

/// Get the score history for a set of `(product, area)` pairs from the local
/// clone of browser-wpt-results (fetching every 5 minutes; a clone up to a
/// day old is served while re-fetching in the background). Pairs not yet
/// cached are read from the clone before returning.
async fn fresh_wpt_summaries(
    requests: Vec<(String, String)>,
) -> Option<std::sync::Arc<cache::Cached<wpt_summaries::SummaryCacheEntry>>> {
    WPT_SUMMARY_CACHE
        .get_usable_or_refresh(
            wpt_summaries::FETCH_INTERVAL,
            Duration::from_hours(24),
            |entry| entry.contains(&requests),
            {
                let requests = requests.clone();
                |existing| load_wpt_summaries(requests, existing)
            },
        )
        .await
}

/// Blitz's score history for a set of areas; `union_totals` is
/// index-aligned with `areas` (see [`wpt_history::WptHistory::subtest_total`])
async fn fresh_wpt_history(
    areas: Vec<String>,
    union_totals: Vec<Option<u32>>,
) -> Option<ArcWptHistory> {
    let requests = areas
        .iter()
        .map(|area| ("blitz".to_string(), area.clone()))
        .collect();
    fresh_wpt_summaries(requests)
        .await?
        .history("blitz", &areas, &union_totals)
}

/// The cross-engine union subtest total of an area from its per-run
/// comparison scores. `area_scores` counts every test known to any engine
/// against every engine with the max subtest total, so the total is the
/// same for every run that has a score; take the largest in case a run's
/// scores predate the others.
fn union_subtest_total(scores: &[Option<wpt_db::AreaScore>]) -> Option<u32> {
    scores
        .iter()
        .flatten()
        .map(|score| score.subtests_total)
        .filter(|total| *total != 0)
        .max()
}

/// One history chart line per engine that has results for `area` in the
/// comparison (`total` is index-aligned with `runs`). The empty area is the
/// whole-run total.
async fn compare_history(
    runs: &[wpt_db::RunRow],
    area: &str,
    total: &[Option<wpt_db::AreaScore>],
) -> Vec<ChartLine> {
    let products: Vec<&str> = runs
        .iter()
        .zip(total)
        .filter(|(_, total)| total.is_some())
        .map(|(run, _)| run.product.as_str())
        .collect();

    let requests: Vec<(String, String)> = products
        .iter()
        .map(|product| (product.to_string(), area.to_string()))
        .collect();
    let Some(summaries) = fresh_wpt_summaries(requests).await else {
        return Vec::new();
    };

    let areas = [area.to_string()];
    let union_totals = [union_subtest_total(total)];
    let mut lines = Vec::new();
    for product in products {
        if let Some(history) = summaries.history(product, &areas, &union_totals) {
            lines.push(ChartLine {
                history,
                series: ChartSeries {
                    area: area.to_string(),
                    label: product_label(product),
                    color: product_color(product),
                },
            });
        }
    }
    lines
}

async fn wpt_compare_route(area: String, query: WptCompareQuery) -> Response {
    // Browser history is far shorter than Blitz's, so default to a year
    let range = match query.range.as_deref() {
        Some(range) => ChartRange::from_query(Some(range)),
        None => ChartRange::Year1,
    };
    let history_open = query.range.is_some();
    // Default: top-level areas by subtest count, deeper levels alphabetical
    let sort = match query.sort.as_deref() {
        Some("alpha") => wpt_db::AreaSort::Alpha,
        Some("subtests") => wpt_db::AreaSort::Subtests,
        _ if area.is_empty() => wpt_db::AreaSort::Subtests,
        _ => wpt_db::AreaSort::Alpha,
    };
    let Some(entry) = get_wpt_comparison_run_list().await else {
        return wpt_unavailable_response(WPT_LOADING_MESSAGE).await;
    };
    let Some(_slot) = acquire_wpt_db_slot().await else {
        return wpt_unavailable_response(WPT_BUSY_MESSAGE).await;
    };
    let runs = entry.runs.clone();
    let run_ids: Vec<i64> = runs.iter().map(|run| run.id).collect();

    enum PageData {
        Area {
            total: Vec<Option<wpt_db::AreaScore>>,
            children: Vec<(String, Vec<Option<wpt_db::AreaScore>>)>,
            tests: Vec<wpt_db::TestRow>,
        },
        Test(wpt_db::TestDetail),
        NotFound,
    }

    let data = {
        let area = area.clone();
        tokio::task::spawn_blocking(move || {
            WPT_COMPARE_DB.with_reader(|conn| {
                if wpt_db::area_exists(conn, &area) {
                    PageData::Area {
                        total: wpt_db::area_score(conn, &run_ids, &area),
                        children: wpt_db::child_area_scores(conn, &run_ids, &area, sort),
                        tests: wpt_db::tests_in_area(conn, &run_ids, &area),
                    }
                } else if let Some(detail) =
                    wpt_db::test_detail(conn, &run_ids, &format!("/{area}"))
                {
                    PageData::Test(detail)
                } else {
                    PageData::NotFound
                }
            })
        })
        .await
        .unwrap()
    };

    match data {
        PageData::Area {
            total,
            children,
            tests,
        } => {
            let history = compare_history(&runs, &area, &total).await;
            let props = WptComparePageProps {
                runs: runs.0.as_ref().clone(),
                area,
                sort,
                total,
                child_areas: children,
                tests,
                history,
                range,
                history_open,
            };
            dx_route_with_props(WptComparePage, props)
                .await
                .into_response()
        }
        PageData::Test(detail) => {
            let tab = TestPageTab::from_query(query.tab.as_deref());
            // Sources are read at the WPT revision Blitz was run against
            // (the run whose results are most likely being investigated),
            // falling back to any run
            let revision = runs
                .iter()
                .find(|run| run.product == "blitz")
                .or(runs.first())
                .map(|run| run.wpt_revision.clone())
                .unwrap_or_default();

            // Fetch the test source (needed to detect ref tests even when
            // the source itself is not being displayed)
            let source_path = format!("/{}", area.split('?').next().unwrap());
            let source = wpt_source::fetch_test_source(&revision, &source_path).await;
            let refs = source
                .as_deref()
                .map(|source| wpt_source::parse_ref_links(source, &source_path))
                .unwrap_or_default();
            let ref_source = if let Some(ref_link) = refs.first() {
                let ref_path = ref_link.href.split('?').next().unwrap();
                Some(wpt_source::fetch_test_source(&revision, ref_path).await)
            } else {
                None
            };

            let props = WptCompareTestPageProps {
                runs: runs.0.as_ref().clone(),
                detail,
                tab,
                source,
                refs,
                ref_source,
            };
            dx_route_with_props(WptCompareTestPage, props)
                .await
                .into_response()
        }
        PageData::NotFound => {
            (StatusCode::NOT_FOUND, format!("Unknown WPT area: {area}")).into_response()
        }
    }
}

async fn wpt_focus_areas_route(set: String) -> Response {
    let Some(set) = routes::focus_area_set(&set) else {
        return (
            StatusCode::NOT_FOUND,
            format!("Unknown focus area set: {set}"),
        )
            .into_response();
    };
    let Some(entry) = get_wpt_comparison_run_list().await else {
        return wpt_unavailable_response(WPT_LOADING_MESSAGE).await;
    };
    let Some(_slot) = acquire_wpt_db_slot().await else {
        return wpt_unavailable_response(WPT_BUSY_MESSAGE).await;
    };
    let runs = entry.runs.clone();
    let run_ids: Vec<i64> = runs.iter().map(|run| run.id).collect();

    let scores = tokio::task::spawn_blocking(move || {
        WPT_COMPARE_DB.with_reader(|conn| {
            set.areas
                .iter()
                .map(|area| (area.clone(), wpt_db::area_score(conn, &run_ids, area)))
                .collect::<Vec<_>>()
        })
    })
    .await
    .unwrap();

    let props = WptFocusAreasPageProps {
        label: set.label.clone(),
        intro: set.intro.clone(),
        runs: runs.0.as_ref().clone(),
        scores,
    };
    dx_route_with_props(WptFocusAreasPage, props)
        .await
        .into_response()
}

/// Get the cached WPT comparison run list, revalidating (checking wpt.fyi
/// for new runs and ingesting them) in the background if it is stale (new
/// runs only appear roughly daily, so stale data is always usable). If the
/// cache is still empty (the startup refresh hasn't finished, or it is
/// ingesting new runs, which can take minutes) a refresh is triggered in the
/// background but not awaited: `None` is returned so the request can fail
/// fast instead of hanging.
async fn get_wpt_comparison_run_list(
) -> Option<std::sync::Arc<cache::Cached<wpt_compare::WptCompareCacheEntry>>> {
    if WPT_COMPARE_CACHE.get_cloned().is_none() {
        tokio::spawn(WPT_COMPARE_CACHE.refresh(load_wpt_compare));
        return None;
    }
    WPT_COMPARE_CACHE
        .get_or_refresh(Duration::from_mins(30), Duration::MAX, load_wpt_compare)
        .await
}

/// Maximum number of WPT comparison requests that may be querying (or
/// waiting on a reader connection for) the database at once. Each one
/// occupies a blocking thread, so without a bound a burst of requests parks
/// hundreds of threads; beyond this they wait briefly as cheap futures and
/// are then turned away with a 503.
///
/// The slot is held through rendering, and a large page (hundreds of test
/// rows) has a transient working set of ~30MB, so this also caps the
/// memory a burst of requests can take: 8 keeps it around 250MB on the
/// 1GB machine, where 32 let crawler bursts push the process past 600MB.
const WPT_DB_SLOTS: usize = 8;
static WPT_DB_SLOT_SEMAPHORE: Semaphore = Semaphore::const_new(WPT_DB_SLOTS);

/// Acquire a slot for a database-backed request, waiting up to 2s for one
/// to free up. `None` means the server is saturated and the request should
/// be rejected.
async fn acquire_wpt_db_slot() -> Option<SemaphorePermit<'static>> {
    tokio::time::timeout(Duration::from_secs(2), WPT_DB_SLOT_SEMAPHORE.acquire())
        .await
        .ok()?
        .ok()
}

const WPT_LOADING_MESSAGE: &str = "The WPT comparison data is still loading. This usually takes a few seconds after the site starts, but can take a few minutes if new test runs are being imported.";
const WPT_BUSY_MESSAGE: &str =
    "The WPT comparison pages are receiving too many requests right now. Please try again in a moment.";

/// The 503 response for WPT comparison pages when they can't be served
/// right now (no run list cached yet, or too many requests in flight)
async fn wpt_unavailable_response(message: &'static str) -> Response {
    let props = WptUnavailablePageProps {
        message: message.to_string(),
    };
    let (_, html) = dx_route_with_props(WptUnavailablePage, props).await;
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [(header::RETRY_AFTER, "30")],
        html,
    )
        .into_response()
}

/// The latest Blitz run in the comparison database, with a database slot
/// held for querying it, or the 503 response to send when it isn't
/// available
async fn blitz_status_run() -> Result<(RunRow, SemaphorePermit<'static>), Box<Response>> {
    let Some(entry) = get_wpt_comparison_run_list().await else {
        return Err(Box::new(
            wpt_unavailable_response(WPT_LOADING_MESSAGE).await,
        ));
    };
    let Some(run) = entry
        .runs
        .iter()
        .find(|run| run.product == "blitz")
        .cloned()
    else {
        return Err(Box::new(
            wpt_unavailable_response(WPT_LOADING_MESSAGE).await,
        ));
    };
    let Some(slot) = acquire_wpt_db_slot().await else {
        return Err(Box::new(wpt_unavailable_response(WPT_BUSY_MESSAGE).await));
    };
    Ok((run, slot))
}

/// The commit a Blitz run was built from: its sha is the run's
/// `browser_version`; the message and date come from Blitz's run list in
/// the summaries clone when it has the run
async fn blitz_commit_info(run: &RunRow) -> Option<CommitInfo> {
    if run.browser_version.is_empty() {
        return None;
    }
    let entry = fresh_wpt_summaries(vec![("blitz".to_string(), "css".to_string())]).await;
    let meta = entry
        .as_ref()
        .and_then(|entry| entry.run_meta("blitz", &run.browser_version));
    Some(CommitInfo {
        sha: run.browser_version.clone(),
        message: meta.and_then(|meta| meta.commit_message.clone()),
        timestamp: meta.map(|meta| meta.date.clone()),
    })
}

async fn dx_route_cached(render_fn: fn() -> Element) -> impl IntoResponse {
    static CACHE: LazyLock<DashMap<usize, Bytes>> = LazyLock::new(DashMap::new);

    let fn_key = render_fn as *const () as usize;

    let html = CACHE.entry(fn_key).or_insert_with(|| {
        let (html, duration) = render_component(render_fn, ());

        let duration_millis = duration.as_micros() as f64 / 1000.0;
        println!("Rendered in {duration_millis:.2}ms",);

        Bytes::from(html)
    });

    (StatusCode::OK, Html(html.clone()))
}

#[allow(unused)]
async fn dx_route(render_fn: fn() -> Element) -> impl IntoResponse {
    let (html, duration) = render_component(render_fn, ());

    let duration_millis = duration.as_micros() as f64 / 1000.0;
    println!("Rendered dx in {duration_millis:.2}ms",);

    (StatusCode::OK, Html(html))
}

#[allow(unused)]
async fn dx_route_with_props<P: Clone + 'static, M: 'static>(
    render_fn: impl ComponentFunction<P, M>,
    props: P,
) -> (StatusCode, Html<String>) {
    let (html, duration) = render_component(render_fn, props);

    let duration_millis = duration.as_micros() as f64 / 1000.0;
    println!("Rendered dx in {duration_millis:.2}ms",);

    (StatusCode::OK, Html(html))
}

fn render_component<P: Clone + 'static, M: 'static>(
    render_fn: impl ComponentFunction<P, M>,
    props: P,
) -> (String, Duration) {
    let start = Instant::now();

    let mut dom = VirtualDom::new_with_props(render_fn, props);
    dom.rebuild_in_place();
    let rendered = dioxus_ssr::render(&dom);
    let html = format!(
        "<!DOCTYPE html><html{}</html>",
        &rendered[4..(rendered.len() - 6)]
    );

    (html, start.elapsed())
}
