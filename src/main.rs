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
    NLNetInstructionsPage, TestPageTab, WptHistoryPage, WptHistoryPageProps, WptResultsPage,
    WptResultsPageProps, WptTestPage, WptTestPageProps,
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
use wpt_history::{load_wpt_history, WPT_HISTORY_CACHE};

mod cache;
mod components;
mod downloads;
mod github;
mod routes;
mod wpt;
mod wpt_history;
mod wpt_source;

#[derive(Deserialize)]
struct WptPageQuery {
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
            "/downloads",
            get(async || {
                let etag = DOWNLOAD_CACHE
                    .get_cloned()
                    .and_then(|entry| entry.etag.clone());
                // Serve directly for 30s; any older entry is served stale
                // while revalidating in the background (builds are heavy to
                // fetch, so a request never awaits a refresh once primed)
                let entry = DOWNLOAD_CACHE
                    .get_or_refresh(Duration::from_secs(30), Duration::MAX, || {
                        load_downloads(etag)
                    })
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
    tokio::spawn(async move { load_wpt_results(None).await });
    tokio::spawn(async move { load_wpt_history(vec!["css".to_string()]).await });

    if std::env::var("PRECACHE_DOWNLOADS").is_ok() {
        tokio::spawn(async move { load_downloads(None).await });
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
                || load_wpt_history(areas)
            },
        )
        .await?;
    Some(entry.merged(&areas))
}

/// Get the cached WPT report, revalidating it if it is stale.
/// Revalidation is awaited if the cache is more than 30 minutes old,
/// and performed in the background if it is between 30 seconds and 30 minutes old.
async fn fresh_wpt_cache_entry() -> std::sync::Arc<cache::Cached<wpt::WptReportCacheEntry>> {
    let etag = WPT_REPORT_CACHE
        .get_cloned()
        .and_then(|entry| entry.etag.clone());
    WPT_REPORT_CACHE
        .get_or_refresh(Duration::from_secs(30), Duration::from_mins(30), || {
            load_wpt_results(etag)
        })
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
