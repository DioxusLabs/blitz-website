use axum::{
    body::Bytes,
    http::StatusCode,
    response::{Html, IntoResponse, Redirect},
    routing::{get, get_service},
    Router,
};
use dashmap::DashMap;
use dioxus::{core::ComponentFunction, prelude::*};
use dioxus_html_macro::html;
use routes::{
    AboutPage, CssSupportPage, ElementSupportPage, EventSupportPage, GettingStartedPage, HomePage,
    NLNetInstructionsPage, WptResultsPage, WptResultsPageProps,
};
use std::{
    net::{IpAddr, SocketAddr},
    sync::LazyLock,
    time::{Duration, Instant},
};
use tokio::net::TcpListener;
use tower_http::{services::ServeDir, trace::TraceLayer};
use wpt::{load_wpt_results, WPT_REPORT_CACHE};

mod components;
mod routes;
mod wpt;

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
            "/status/wpt",
            get(async || {
                let now = Instant::now();
                let cache_entry = WPT_REPORT_CACHE.get_cloned();
                let etag = cache_entry.as_ref().and_then(|entry| entry.etag.clone());

                // Cache with 30s validity
                let mut await_revalidation = true;
                if let Some(entry) = &cache_entry {
                    let cache_age = now.duration_since(entry.cached_at);
                    if cache_age <= Duration::from_secs(30) {
                        let props = WptResultsPageProps {
                            report: entry.report.clone(),
                            scores: entry.scores.clone(),
                        };
                        return dx_route_with_props(WptResultsPage, props).await;
                    } else if cache_age <= Duration::from_mins(30) {
                        await_revalidation = false
                    }
                }

                let handle = tokio::spawn(async move { load_wpt_results(etag).await });

                if await_revalidation {
                    handle.await.unwrap();
                }

                let entry = WPT_REPORT_CACHE.get_cloned().unwrap();
                let props = WptResultsPageProps {
                    report: entry.report.clone(),
                    scores: entry.scores.clone(),
                };

                dx_route_with_props(WptResultsPage, props).await
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

    // Prime WPT result cache
    tokio::spawn(async move { load_wpt_results(None).await });

    let msg = format!("Serving blitz-website at http://{addr}").replace("[::]", "localhost");
    println!("{msg}");

    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}

async fn dx_route_cached(render_fn: fn() -> Element) -> impl IntoResponse {
    static CACHE: LazyLock<DashMap<usize, Bytes>> = LazyLock::new(|| DashMap::new());

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
) -> impl IntoResponse {
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
