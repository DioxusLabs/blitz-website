//! `/proxy?url=<percent-encoded absolute URL>`: fetches a page or
//! subresource for the wasm Blitz browser demo, which is served from this
//! origin and can't fetch most sites directly (they send no CORS headers).
//!
//! Hardened against use as an open proxy:
//! - Only same-origin requests from browsers (`Sec-Fetch-Site`), and no CORS
//!   headers, so other sites can't use it
//! - GET only, http(s) on the default ports, public addresses only (checked
//!   at DNS resolution and on every redirect, so neither DNS rebinding nor a
//!   redirect can reach the private network)
//! - Per-client request, byte and concurrency budgets, a global concurrency
//!   cap and hourly byte budget, timeouts and a response size cap
//! - No cookies or credentials in either direction, and responses are
//!   sandboxed with CSP so proxied HTML can't run script on this origin

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{ConnectInfo, Query};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use dashmap::DashMap;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::{redirect, Url};
use serde::Deserialize;
use tokio::sync::Semaphore;

const MAX_RESPONSE_BYTES: usize = 5 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;

/// Per-client budgets, per minute (a page load is typically tens of
/// requests: the document plus its stylesheets, images and fonts)
const CLIENT_WINDOW: Duration = Duration::from_secs(60);
const CLIENT_MAX_REQUESTS: u32 = 300;
const CLIENT_MAX_BYTES: u64 = 100 * 1024 * 1024;
const CLIENT_MAX_IN_FLIGHT: u32 = 6;

/// Global limits: responses are buffered, so this bounds memory at
/// `MAX_IN_FLIGHT * MAX_RESPONSE_BYTES`, and the hourly budget bounds egress
const MAX_IN_FLIGHT: usize = 16;
const GLOBAL_WINDOW: Duration = Duration::from_secs(60 * 60);
const GLOBAL_MAX_BYTES: u64 = 5 * 1024 * 1024 * 1024;

static IN_FLIGHT: Semaphore = Semaphore::const_new(MAX_IN_FLIGHT);
static CLIENT_BUDGETS: LazyLock<DashMap<IpAddr, ClientBudget>> = LazyLock::new(DashMap::new);
static GLOBAL_BUDGET: Mutex<Option<(Instant, u64)>> = Mutex::new(None);

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .dns_resolver(PublicOnlyResolver)
        .no_proxy()
        .referer(false)
        .redirect(redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= MAX_REDIRECTS {
                attempt.error("too many redirects")
            } else if let Err(err) = check_url(attempt.url()) {
                attempt.error(err)
            } else {
                attempt.follow()
            }
        }))
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .user_agent("Mozilla/5.0 (compatible; BlitzDemoProxy/0.1; +https://blitz.is/demo)")
        .build()
        .unwrap()
});

#[derive(Deserialize)]
pub struct ProxyQuery {
    url: String,
}

/// Which route a request came in on
#[derive(Clone, Copy, PartialEq)]
enum Route {
    /// `/proxy`: the wasm renderer's fetches of pages and subresources
    Fetch,
    /// `/proxy/frame`: the document for the demo's native (iframe) view
    Frame,
}

/// Whether a request may use the proxy at all. Both routes go through here,
/// so this is the one place to add an allowlist or token check.
fn authorize(
    headers: &HeaderMap,
    url: &str,
    route: Route,
) -> Result<Url, (StatusCode, &'static str)> {
    // Browsers always send Sec-Fetch-Site: anything but same-origin is
    // another site, or someone using the proxy from the address bar
    if headers
        .get("sec-fetch-site")
        .is_some_and(|site| site != "same-origin")
    {
        return Err((
            StatusCode::FORBIDDEN,
            "The proxy only serves the Blitz demo on this site",
        ));
    }
    // The native view's document is only ever loaded into the demo's iframe
    if route == Route::Frame
        && headers
            .get("sec-fetch-dest")
            .is_some_and(|dest| dest != "iframe")
    {
        return Err((
            StatusCode::FORBIDDEN,
            "The proxy only serves the Blitz demo on this site",
        ));
    }

    let Ok(url) = Url::parse(url) else {
        return Err((StatusCode::BAD_REQUEST, "Invalid URL"));
    };
    if let Err(err) = check_url(&url) {
        return Err((StatusCode::FORBIDDEN, err));
    }
    Ok(url)
}

pub async fn proxy_route(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<ProxyQuery>,
) -> Response {
    let fetched = match authorize(&headers, &query.url, Route::Fetch) {
        Ok(url) => fetch(peer, &headers, url).await,
        Err((status, message)) => return error(status, message),
    };
    let fetched = match fetched {
        Ok(fetched) => fetched,
        Err(response) => return response,
    };

    let mut response = Response::new(Body::from(fetched.body));
    *response.status_mut() = fetched.status;
    let response_headers = response.headers_mut();
    response_headers.insert(
        header::CONTENT_TYPE,
        fetched
            .content_type
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    response_headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox; default-src 'none'"),
    );
    response_headers.insert(
        "cross-origin-resource-policy",
        HeaderValue::from_static("same-origin"),
    );
    insert_common_headers(response_headers);
    // The URL after redirects, for resolving relative URLs in the document
    if let Ok(final_url) = HeaderValue::from_str(fetched.final_url.as_str()) {
        response_headers.insert("x-proxy-final-url", final_url);
    }
    response
}

/// `/proxy/frame?url=`: the page's document for the native view, with a
/// `<base>` so its subresources load straight from the site, and a script
/// that reports loads and hands link clicks to the demo page. It runs in a
/// CSP sandbox without `allow-same-origin`, so it gets an opaque origin and
/// can't read this site's storage or call the proxy.
pub async fn proxy_frame_route(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<ProxyQuery>,
) -> Response {
    let fetched = match authorize(&headers, &query.url, Route::Frame) {
        Ok(url) => fetch(peer, &headers, url).await,
        Err((status, message)) => return error(status, message),
    };
    let fetched = match fetched {
        Ok(fetched) => fetched,
        Err(response) => return response,
    };
    let is_html = fetched
        .content_type
        .as_ref()
        .and_then(|ct| ct.to_str().ok())
        .is_some_and(|ct| {
            let mime = ct.split(';').next().unwrap_or_default().trim();
            mime.eq_ignore_ascii_case("text/html")
                || mime.eq_ignore_ascii_case("application/xhtml+xml")
        });
    if !is_html {
        return error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "The native view only shows HTML pages",
        );
    }

    let body = inject_frame_head(&fetched.body, &fetched.final_url);
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = fetched.status;
    let response_headers = response.headers_mut();
    response_headers.insert(header::CONTENT_TYPE, fetched.content_type.unwrap());
    response_headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox allow-scripts; frame-ancestors 'self'"),
    );
    insert_common_headers(response_headers);
    response
}

fn insert_common_headers(headers: &mut HeaderMap) {
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=300"),
    );
}

/// Reports loads to the demo page and hands it link clicks, so navigation
/// stays in the demo's history and goes through the proxy
const FRAME_SCRIPT: &str = r#"<script>
addEventListener("load", () => parent.postMessage({type: "frame:loaded", url: document.baseURI, title: document.title}, "*"));
addEventListener("click", (event) => {
  const link = event.target.closest && event.target.closest("a[href]");
  if (!link || event.defaultPrevented || event.button !== 0 || !/^https?:/.test(link.href)) return;
  event.preventDefault();
  parent.postMessage({type: "frame:navigate", url: link.href}, "*");
});
</script>"#;

/// Insert a `<base>` for the page's real URL and [`FRAME_SCRIPT`] at the start
/// of the document's `<head>` (or of the document, if it has no `<head>` tag)
fn inject_frame_head(html: &[u8], url: &Url) -> Vec<u8> {
    let href = url
        .as_str()
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;");
    let injected = format!(r#"<base href="{href}">{FRAME_SCRIPT}"#);

    let lower = html.to_ascii_lowercase();
    let after_head = lower
        .windows(5)
        .position(|w| w == b"<head")
        .filter(|&i| matches!(lower.get(i + 5), Some(b'>' | b' ' | b'\t' | b'\n' | b'\r')))
        .and_then(|i| {
            lower[i..]
                .iter()
                .position(|&b| b == b'>')
                .map(|j| i + j + 1)
        });

    let mut out = Vec::with_capacity(html.len() + injected.len());
    match after_head {
        Some(i) => {
            out.extend_from_slice(&html[..i]);
            out.extend_from_slice(injected.as_bytes());
            out.extend_from_slice(&html[i..]);
        }
        None => {
            out.extend_from_slice(injected.as_bytes());
            out.extend_from_slice(html);
        }
    }
    out
}

struct Fetched {
    status: StatusCode,
    content_type: Option<HeaderValue>,
    /// The URL after redirects
    final_url: Url,
    body: Vec<u8>,
}

/// Fetch `url` within the per-client and global budgets
async fn fetch(peer: SocketAddr, headers: &HeaderMap, url: Url) -> Result<Fetched, Response> {
    let client = budget_key(client_ip(headers, peer));
    let Some(_client_slot) = ClientSlot::acquire(client) else {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            [(header::RETRY_AFTER, "60")],
            "Too many proxy requests; try again in a minute",
        )
            .into_response());
    };
    if !global_budget_available() {
        return Err(error(
            StatusCode::SERVICE_UNAVAILABLE,
            "The proxy has reached its hourly limit; try again later",
        ));
    }
    let Ok(Ok(_slot)) = tokio::time::timeout(Duration::from_secs(5), IN_FLIGHT.acquire()).await
    else {
        return Err(error(
            StatusCode::SERVICE_UNAVAILABLE,
            "The proxy is busy; try again in a moment",
        ));
    };

    let accept = headers
        .get(header::ACCEPT)
        .cloned()
        .unwrap_or(HeaderValue::from_static("*/*"));
    let mut upstream = match CLIENT.get(url).header(header::ACCEPT, accept).send().await {
        Ok(upstream) => upstream,
        Err(err) => {
            tracing::info!("Proxy fetch failed: {err}");
            return Err(error(StatusCode::BAD_GATEWAY, "Couldn't fetch the URL"));
        }
    };

    let content_type = upstream.headers().get(header::CONTENT_TYPE).cloned();
    if !is_allowed_content_type(content_type.as_ref().and_then(|ct| ct.to_str().ok())) {
        return Err(error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "This content type can't be proxied",
        ));
    }
    if upstream
        .content_length()
        .is_some_and(|len| len > MAX_RESPONSE_BYTES as u64)
    {
        return Err(error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "The response is too large",
        ));
    }

    let status = upstream.status();
    let final_url = upstream.url().clone();
    let mut body = Vec::new();
    loop {
        match upstream.chunk().await {
            Ok(Some(chunk)) => {
                if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
                    record_bytes(client, body.len());
                    return Err(error(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "The response is too large",
                    ));
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(err) => {
                tracing::info!("Proxy fetch failed: {err}");
                record_bytes(client, body.len());
                return Err(error(StatusCode::BAD_GATEWAY, "Couldn't fetch the URL"));
            }
        }
    }
    record_bytes(client, body.len());

    Ok(Fetched {
        status,
        content_type,
        final_url,
        body,
    })
}

fn error(status: StatusCode, message: &'static str) -> Response {
    (status, message).into_response()
}

/// Only http(s) URLs on the default ports, without credentials, whose host
/// isn't a private address or name (resolved names are checked by
/// [`PublicOnlyResolver`])
fn check_url(url: &Url) -> Result<(), &'static str> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Only http and https URLs can be proxied");
    }
    if !matches!(url.port_or_known_default(), Some(80 | 443)) {
        return Err("Only the default ports can be proxied");
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("URLs with credentials can't be proxied");
    }
    let Some(host) = url.host_str() else {
        return Err("Invalid URL");
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = host.parse::<IpAddr>() {
        if !is_public_ip(ip) {
            return Err("Private addresses can't be proxied");
        }
    } else {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        if !host.contains('.')
            || host.ends_with(".localhost")
            || host.ends_with(".internal")
            || host.ends_with(".local")
        {
            return Err("Private addresses can't be proxied");
        }
    }
    Ok(())
}

/// Resolves names to their public addresses only
struct PublicOnlyResolver;

impl Resolve for PublicOnlyResolver {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((name.as_str(), 0))
                .await?
                .filter(|addr| is_public_ip(addr.ip()))
                .collect();
            if addrs.is_empty() {
                return Err("the host has no public addresses".into());
            }
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || a == 0
        // Shared address space (carrier-grade NAT)
        || (a == 100 && (64..128).contains(&b))
        // IETF protocol assignments
        || (a == 192 && b == 0 && c == 0)
        // Benchmarking
        || (a == 198 && (b == 18 || b == 19))
        // Reserved
        || a >= 240)
}

fn is_public_ip(ip: IpAddr) -> bool {
    let ip = match ip {
        IpAddr::V4(ip) => return is_public_ipv4(ip),
        IpAddr::V6(ip) => ip,
    };
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_public_ipv4(v4);
    }
    let s = ip.segments();
    let embedded_v4 = |high: u16, low: u16| Ipv4Addr::from(((high as u32) << 16) | low as u32);
    // NAT64 and 6to4 addresses embed an IPv4 address
    if s[0] == 0x64 && s[1] == 0xff9b && s[2..6] == [0; 4] {
        return is_public_ipv4(embedded_v4(s[6], s[7]));
    }
    if s[0] == 0x2002 {
        return is_public_ipv4(embedded_v4(s[1], s[2]));
    }
    !(ip.is_multicast()
        // Unspecified, loopback and IPv4-compatible
        || s[0..6] == [0; 6]
        // Unique local, which includes Fly's private network
        || (s[0] & 0xfe00) == 0xfc00
        // Link local
        || (s[0] & 0xffc0) == 0xfe80
        // Teredo and documentation
        || (s[0] == 0x2001 && (s[1] == 0 || s[1] == 0x0db8)))
}

fn is_allowed_content_type(content_type: Option<&str>) -> bool {
    let Some(content_type) = content_type else {
        return true;
    };
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    mime.starts_with("text/")
        || mime.starts_with("image/")
        || mime.starts_with("font/")
        || matches!(
            mime.as_str(),
            "" | "application/xhtml+xml"
                | "application/xml"
                | "application/javascript"
                | "application/x-javascript"
                | "application/json"
                | "application/octet-stream"
                | "application/font-woff"
                | "application/x-font-woff"
                | "application/font-sfnt"
                | "application/x-font-ttf"
                | "application/x-font-otf"
                | "application/x-font-truetype"
                | "application/x-font-opentype"
                | "application/vnd.ms-fontobject"
        )
}

/// The client's address. Behind Fly's proxy every connection comes from
/// Fly, which passes the real client address in `Fly-Client-IP`.
fn client_ip(headers: &HeaderMap, peer: SocketAddr) -> IpAddr {
    if std::env::var_os("FLY_APP_NAME").is_some() {
        if let Some(ip) = headers
            .get("fly-client-ip")
            .and_then(|ip| ip.to_str().ok())
            .and_then(|ip| ip.parse().ok())
        {
            return ip;
        }
    }
    peer.ip()
}

/// IPv6 clients are budgeted per /64, since they typically have a whole one
fn budget_key(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => {
                let s = ip.segments();
                IpAddr::V6(Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0))
            }
        },
        ip => ip,
    }
}

struct ClientBudget {
    window_start: Instant,
    requests: u32,
    bytes: u64,
    in_flight: u32,
}

/// A request counted against a client's budget, released on drop
struct ClientSlot(IpAddr);

impl ClientSlot {
    fn acquire(client: IpAddr) -> Option<Self> {
        let now = Instant::now();
        if CLIENT_BUDGETS.len() > 10_000 {
            CLIENT_BUDGETS.retain(|_, budget| {
                budget.in_flight > 0 || now.duration_since(budget.window_start) < CLIENT_WINDOW
            });
        }
        let mut budget = CLIENT_BUDGETS.entry(client).or_insert(ClientBudget {
            window_start: now,
            requests: 0,
            bytes: 0,
            in_flight: 0,
        });
        if now.duration_since(budget.window_start) >= CLIENT_WINDOW {
            budget.window_start = now;
            budget.requests = 0;
            budget.bytes = 0;
        }
        if budget.requests >= CLIENT_MAX_REQUESTS
            || budget.bytes >= CLIENT_MAX_BYTES
            || budget.in_flight >= CLIENT_MAX_IN_FLIGHT
        {
            return None;
        }
        budget.requests += 1;
        budget.in_flight += 1;
        Some(Self(client))
    }
}

impl Drop for ClientSlot {
    fn drop(&mut self) {
        if let Some(mut budget) = CLIENT_BUDGETS.get_mut(&self.0) {
            budget.in_flight = budget.in_flight.saturating_sub(1);
        }
    }
}

fn global_budget_available() -> bool {
    let mut budget = GLOBAL_BUDGET.lock().unwrap();
    let now = Instant::now();
    match &mut *budget {
        Some((start, _)) if now.duration_since(*start) < GLOBAL_WINDOW => {}
        budget => *budget = Some((now, 0)),
    }
    budget.is_some_and(|(_, bytes)| bytes < GLOBAL_MAX_BYTES)
}

fn record_bytes(client: IpAddr, bytes: usize) {
    if let Some(mut budget) = CLIENT_BUDGETS.get_mut(&client) {
        budget.bytes += bytes as u64;
    }
    if let Some((_, total)) = &mut *GLOBAL_BUDGET.lock().unwrap() {
        *total += bytes as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(url: &str) -> Result<(), &'static str> {
        check_url(&Url::parse(url).unwrap())
    }

    #[test]
    fn allows_public_urls() {
        assert!(check("https://example.com/").is_ok());
        assert!(check("http://example.com:80/a?b").is_ok());
        assert!(check("https://93.184.215.14/").is_ok());
        assert!(check("https://[2606:2800:21f:cb07:6820:80da:af6b:8b2c]/").is_ok());
    }

    #[test]
    fn rejects_private_and_unusual_urls() {
        for url in [
            "file:///etc/passwd",
            "ftp://example.com/",
            "https://example.com:8080/",
            "https://user:pass@example.com/",
            "http://localhost/",
            "http://foo.localhost/",
            "http://blitz-website.internal/",
            "http://printer.local/",
            "http://intranet/",
            "http://127.0.0.1/",
            "http://2130706433/",
            "http://0x7f.1/",
            "http://10.0.0.1/",
            "http://172.16.0.1/",
            "http://192.168.1.1/",
            "http://169.254.169.254/latest/meta-data/",
            "http://100.64.0.1/",
            "http://0.0.0.0/",
            "http://[::1]/",
            "http://[::]/",
            "http://[::ffff:127.0.0.1]/",
            "http://[fdaa::3]/",
            "http://[fe80::1]/",
            "http://[64:ff9b::a00:1]/",
            "http://[2002:c0a8:101::]/",
        ] {
            assert!(check(url).is_err(), "{url} should be rejected");
        }
    }

    #[test]
    fn frame_head_injection() {
        let url = Url::parse("https://example.com/a?b=1&c=\"").unwrap();
        let out = |html: &str| String::from_utf8(inject_frame_head(html.as_bytes(), &url)).unwrap();
        let base = r#"<base href="https://example.com/a?b=1&amp;c=%22">"#;
        assert!(out("<html><HEAD lang=en><title>t</title>")
            .starts_with(&format!("<html><HEAD lang=en>{base}")));
        assert!(out("<header>x</header>").starts_with(base));
        assert!(out("<p>no head</p>").ends_with("</script><p>no head</p>"));
    }

    #[test]
    fn content_types() {
        assert!(is_allowed_content_type(Some("text/html; charset=utf-8")));
        assert!(is_allowed_content_type(Some("image/png")));
        assert!(is_allowed_content_type(Some("font/woff2")));
        assert!(is_allowed_content_type(None));
        assert!(!is_allowed_content_type(Some("video/mp4")));
        assert!(!is_allowed_content_type(Some("application/zip")));
    }
}
