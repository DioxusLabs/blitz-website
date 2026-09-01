use axum::{
    body::{Body, Bytes},
    extract::{Path, Query},
    http::{header, StatusCode},
    response::{AppendHeaders, Html, IntoResponse, Redirect},
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
use routes::child_areas;
use routes::{
    AboutPage, ArcDownloadLinks, ArcWptHistory, ChartRange, CssSupportPage, DownloadsPage,
    DownloadsPageProps, ElementSupportPage, EventSupportPage, GettingStartedPage, HomePage,
    NLNetInstructionsPage, TestPageTab, WptComparePage, WptComparePageProps, WptCompareTestPage,
    WptCompareTestPageProps, WptFocusAreasPage, WptFocusAreasPageProps, WptHistoryPage,
    WptHistoryPageProps, WptResultsPage, WptResultsPageProps, WptTestPage, WptTestPageProps,
};
use serde::Deserialize;
use std::{
    net::{IpAddr, SocketAddr},
    sync::LazyLock,
    time::{Duration, Instant},
};
use tokio::net::TcpListener;
use tower_http::{services::ServeDir, trace::TraceLayer};
use wpt::{load_wpt_results, WPT_REPORT_CACHE};
use wpt_compare::{load_wpt_compare, WPT_COMPARE_CACHE};
use wpt_db::WPT_COMPARE_DB;
use wpt_history::{load_wpt_history, WPT_HISTORY_CACHE};

mod cache;
mod components;
mod downloads;
mod github;
mod routes;
mod wpt;
mod wpt_compare;
mod wpt_db;
mod wpt_fyi;
mod wpt_history;
mod wpt_source;
mod wpt_spec_meta;

#[derive(Deserialize)]
struct WptPageQuery {
    range: Option<String>,
    tab: Option<String>,
}

#[derive(Deserialize)]
struct WptCompareQuery {
    sort: Option<String>,
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
                let entry = fresh_wpt_cache_entry().await;
                let mut areas = vec!["css".to_string()];
                areas.extend(
                    child_areas(&entry.scores.0, "css")
                        .into_iter()
                        .map(|(area, _)| area),
                );
                areas[1..].sort();
                let Some(history) = fresh_wpt_history(areas).await else {
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
                async |Path(area): Path<String>, Query(query): Query<WptPageQuery>| {
                    let area = area.trim_matches('/').to_string();
                    let entry = fresh_wpt_cache_entry().await;

                    if entry.scores.contains_key(&area) {
                        let range = ChartRange::from_query(query.range.as_deref());
                        // Folder pages chart a single line for the folder
                        // itself
                        let history = fresh_wpt_history(vec![area.clone()]).await;
                        let props = WptResultsPageProps {
                            report: entry.report.clone(),
                            scores: entry.scores.clone(),
                            commit_info: entry.commit_info.clone(),
                            area,
                            history,
                            range,
                        };

                        return Ok(dx_route_with_props(WptResultsPage, props).await);
                    }

                    if let Some(&test_index) = entry.test_index.get(&area) {
                        let tab = TestPageTab::from_query(query.tab.as_deref());
                        let revision = entry.report.run_info.revision.clone();

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

                        let props = WptTestPageProps {
                            report: entry.report.clone(),
                            commit_info: entry.commit_info.clone(),
                            test_index,
                            tab,
                            source,
                            refs,
                            ref_source,
                        };

                        return Ok(dx_route_with_props(WptTestPage, props).await);
                    }

                    Err((StatusCode::NOT_FOUND, format!("Unknown WPT area: {area}")))
                },
            ),
        )
        .route(
            "/wpt",
            get(async |Query(query): Query<WptCompareQuery>| {
                wpt_compare_route(String::new(), query.sort).await
            }),
        )
        .route("/wpt/focus-areas", get(wpt_focus_areas_route))
        .route(
            "/wpt/{*area}",
            get(
                async |Path(area): Path<String>, Query(query): Query<WptCompareQuery>| {
                    wpt_compare_route(area.trim_matches('/').to_string(), query.sort).await
                },
            ),
        )
        .route(
            "/downloads",
            get(async || {
                // Serve directly for 30s; any older entry is served stale
                // while revalidating in the background (builds are heavy to
                // fetch, so a request never awaits a refresh once primed)
                let entry = DOWNLOAD_CACHE
                    .get_or_refresh(Duration::from_secs(30), Duration::MAX, load_downloads)
                    .await
                    .unwrap();
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
        .layer(TraceLayer::new_for_http());

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

    // Prime WPT result and download caches
    tokio::spawn(WPT_REPORT_CACHE.refresh(load_wpt_results));
    tokio::spawn(
        WPT_HISTORY_CACHE.refresh(|existing| load_wpt_history(vec!["css".to_string()], existing)),
    );
    tokio::spawn(async move { load_wpt_compare().await });

    if std::env::var("PRECACHE_DOWNLOADS").is_ok() {
        tokio::spawn(DOWNLOAD_CACHE.refresh(load_downloads));
    }

    let msg = format!("Serving blitz-website at http://{addr}").replace("[::]", "localhost");
    println!("{msg}");

    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}

/// Get the latest WPT history data for a set of areas, refreshing it if it
/// is stale (serve directly for 30s; serve stale-while-revalidate for 30min).
/// A refresh only revalidates runs.json (all summary files change together),
/// so cached areas are reused and only missing area files are fetched.
async fn fresh_wpt_history(areas: Vec<String>) -> Option<ArcWptHistory> {
    let entry = WPT_HISTORY_CACHE
        .get_usable_or_refresh(
            Duration::from_secs(30),
            Duration::from_mins(30),
            |entry| entry.contains_areas(&areas),
            {
                let areas = areas.clone();
                |existing| load_wpt_history(areas, existing)
            },
        )
        .await?;
    Some(entry.merged(&areas))
}

async fn wpt_compare_route(
    area: String,
    sort: Option<String>,
) -> Result<(StatusCode, Html<String>), (StatusCode, String)> {
    // Default: top-level areas by subtest count, deeper levels alphabetical
    let sort = match sort.as_deref() {
        Some("alpha") => wpt_db::AreaSort::Alpha,
        Some("subtests") => wpt_db::AreaSort::Subtests,
        _ if area.is_empty() => wpt_db::AreaSort::Subtests,
        _ => wpt_db::AreaSort::Alpha,
    };
    let Some(entry) = fresh_wpt_compare_entry().await else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "WPT comparison data not available".to_string(),
        ));
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
            WPT_COMPARE_DB.with(|conn| {
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
            let props = WptComparePageProps {
                runs: runs.0.as_ref().clone(),
                area,
                sort,
                total,
                child_areas: children,
                tests,
            };
            Ok(dx_route_with_props(WptComparePage, props).await)
        }
        PageData::Test(detail) => {
            let props = WptCompareTestPageProps {
                runs: runs.0.as_ref().clone(),
                detail,
            };
            Ok(dx_route_with_props(WptCompareTestPage, props).await)
        }
        PageData::NotFound => Err((StatusCode::NOT_FOUND, format!("Unknown WPT area: {area}"))),
    }
}

async fn wpt_focus_areas_route() -> Result<(StatusCode, Html<String>), (StatusCode, String)> {
    let Some(entry) = fresh_wpt_compare_entry().await else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "WPT comparison data not available".to_string(),
        ));
    };
    let runs = entry.runs.clone();
    let run_ids: Vec<i64> = runs.iter().map(|run| run.id).collect();

    let scores = tokio::task::spawn_blocking(move || {
        WPT_COMPARE_DB.with(|conn| {
            routes::SERVO_FOCUS_AREAS
                .iter()
                .map(|area| (area.to_string(), wpt_db::area_score(conn, &run_ids, area)))
                .collect::<Vec<_>>()
        })
    })
    .await
    .unwrap();

    let props = WptFocusAreasPageProps {
        runs: runs.0.as_ref().clone(),
        scores,
    };
    Ok(dx_route_with_props(WptFocusAreasPage, props).await)
}

/// Get the cached WPT comparison run list, revalidating (checking wpt.fyi
/// for new runs and ingesting them) if it is stale. Revalidation is awaited
/// if the cache is missing, and performed in the background if it is between
/// 30 minutes and 24 hours old (new runs only appear roughly daily).
async fn fresh_wpt_compare_entry() -> Option<std::sync::Arc<wpt_compare::WptCompareCacheEntry>> {
    let now = Instant::now();

    let mut await_revalidation = true;
    if let Some(entry) = WPT_COMPARE_CACHE.get_cloned() {
        let cache_age = now.duration_since(entry.cached_at);
        if cache_age <= Duration::from_mins(30) {
            return Some(entry);
        }
        await_revalidation = false;
    }

    let handle = tokio::spawn(async move { load_wpt_compare().await });
    if await_revalidation {
        let _ = handle.await;
    }

    WPT_COMPARE_CACHE.get_cloned()
}

/// Get the cached WPT report, revalidating it if it is stale.
/// Revalidation is awaited if the cache is more than 30 minutes old,
/// and performed in the background if it is between 30 seconds and 30 minutes old.
async fn fresh_wpt_cache_entry() -> std::sync::Arc<cache::Cached<wpt::WptReportCacheEntry>> {
    WPT_REPORT_CACHE
        .get_or_refresh(
            Duration::from_secs(30),
            Duration::from_mins(30),
            load_wpt_results,
        )
        .await
        .unwrap()
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
