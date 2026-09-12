//! Fetching (and caching) of WPT test source code from wpt.live

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use reqwest::Client;

pub type SourceResult = Result<Arc<str>, String>;

/// Fetched sources are cached on disk under the system temp directory, one
/// file per (revision, path), so a crawler walking tens of thousands of test
/// pages costs disk and (reclaimable) page cache rather than process heap.
/// Sources are cheap to refetch, so the cache needn't survive restarts, and
/// the temp directory is on the machine's root disk rather than a tmpfs (if
/// it were, this would be RAM again; `TMPDIR` can redirect it).
static CACHE_DIR: LazyLock<PathBuf> =
    LazyLock::new(|| std::env::temp_dir().join("blitz-wpt-sources"));

/// The revision whose sources are currently being cached. Only the latest
/// WPT revision is ever requested (it changes with each new Blitz run), so
/// when a new one is seen the other revisions' directories are deleted.
static CACHED_REVISION: Mutex<Option<String>> = Mutex::new(None);

fn cache_path(revision: &str, path: &str) -> PathBuf {
    // Paths are hashed so the file name is short and free of path syntax; a
    // 64-bit hash over the ~30k tests of a revision has a negligible
    // collision probability
    CACHE_DIR
        .join(revision)
        .join(format!("{:016x}", fxhash::hash64(path)))
}

async fn store_source(revision: &str, file: &PathBuf, source: &str) -> std::io::Result<()> {
    let stale_dirs = {
        let mut cached = CACHED_REVISION.lock().unwrap();
        if cached.as_deref() != Some(revision) {
            *cached = Some(revision.to_string());
            true
        } else {
            false
        }
    };
    if stale_dirs {
        if let Ok(mut entries) = tokio::fs::read_dir(&*CACHE_DIR).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if entry.file_name() != revision {
                    let _ = tokio::fs::remove_dir_all(entry.path()).await;
                }
            }
        }
    }

    if let Some(dir) = file.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    // Write to a unique temporary name and rename into place, so a
    // concurrent reader never sees a partially written file
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let tmp = file.with_extension(format!(
        "tmp{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    tokio::fs::write(&tmp, source).await?;
    tokio::fs::rename(&tmp, file).await
}

/// Fetch the source code of a WPT test file from the web-platform-tests GitHub
/// repository at the given revision. `path` should be an absolute path like
/// `/css/css-flexbox/foo.html` (without any query string).
/// Successful fetches are cached on disk (keyed by revision and path).
pub async fn fetch_test_source(revision: &str, path: &str) -> SourceResult {
    let file = cache_path(revision, path);
    if let Ok(text) = tokio::fs::read_to_string(&file).await {
        return Ok(Arc::from(text));
    }

    static CLIENT: LazyLock<Client> = LazyLock::new(|| {
        Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap()
    });

    let url = format!("https://raw.githubusercontent.com/web-platform-tests/wpt/{revision}{path}");
    let response = match CLIENT.get(&url).send().await {
        Ok(response) => response,
        Err(err) => return Err(format!("Failed to fetch {url}: {err}")),
    };

    if !response.status().is_success() {
        return Err(format!(
            "wpt.live returned HTTP {} for {url}",
            response.status()
        ));
    }

    let text = match response.text().await {
        Ok(text) => text,
        Err(err) => return Err(format!("Failed to read response body from {url}: {err}")),
    };

    if let Err(err) = store_source(revision, &file, &text).await {
        println!("Failed to cache test source {path}: {err}");
    }

    Ok(Arc::from(text))
}

#[derive(Debug, Clone, PartialEq)]
pub struct RefLink {
    /// Either "match" or "mismatch"
    pub rel: String,
    /// The ref's path, resolved relative to the test's path
    pub href: String,
}

/// Extract `<link rel="match">` and `<link rel="mismatch">` references from a
/// test's source, resolving their hrefs relative to `test_path`.
pub fn parse_ref_links(source: &str, test_path: &str) -> Vec<RefLink> {
    let mut refs = Vec::new();
    let lower = source.to_ascii_lowercase();

    let mut pos = 0;
    while let Some(idx) = lower[pos..].find("<link") {
        let start = pos + idx;
        let end = lower[start..]
            .find('>')
            .map(|e| start + e)
            .unwrap_or(lower.len());
        let tag = &source[start..end];

        if let Some(rel) = get_attr(tag, "rel") {
            let rel = rel.to_ascii_lowercase();
            if rel == "match" || rel == "mismatch" {
                if let Some(href) = get_attr(tag, "href") {
                    if !href.is_empty() {
                        refs.push(RefLink {
                            rel,
                            href: resolve_href(test_path, href),
                        });
                    }
                }
            }
        }

        pos = end;
    }

    refs
}

/// Extract the value of an attribute from the source text of an HTML tag.
fn get_attr<'a>(tag: &'a str, attr_name: &str) -> Option<&'a str> {
    let lower = tag.to_ascii_lowercase();

    let mut search_from = 0;
    loop {
        let idx = lower[search_from..].find(attr_name)? + search_from;

        // Ensure the match is a standalone attribute name preceded by whitespace
        // and followed (optionally after whitespace) by '='
        let preceded_ok = tag[..idx]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_whitespace());
        let after = tag[idx + attr_name.len()..].trim_start();
        if !preceded_ok || !after.starts_with('=') {
            search_from = idx + attr_name.len();
            continue;
        }

        let value = after[1..].trim_start();
        let mut chars = value.chars();
        return match chars.next() {
            Some(quote @ ('"' | '\'')) => {
                let rest = &value[1..];
                rest.find(quote).map(|end| &rest[..end])
            }
            Some(_) => Some(
                value
                    .split(|c: char| c.is_ascii_whitespace())
                    .next()
                    .unwrap(),
            ),
            None => None,
        };
    }
}

/// Resolve a (possibly relative) href against the absolute path of a test file.
fn resolve_href(base_path: &str, href: &str) -> String {
    if href.starts_with('/') {
        return href.to_string();
    }

    let base_dir = base_path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
    let mut segments: Vec<&str> = base_dir.split('/').filter(|s| !s.is_empty()).collect();

    // Split off any query string before resolving path segments
    let (href_path, query) = match href.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (href, None),
    };

    for segment in href_path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }

    let mut resolved = format!("/{}", segments.join("/"));
    if let Some(query) = query {
        resolved.push('?');
        resolved.push_str(query);
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_match_link() {
        let source = r#"<html><head><link rel="match" href="foo-ref.html"></head></html>"#;
        let refs = parse_ref_links(source, "/css/css-flexbox/foo.html");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].rel, "match");
        assert_eq!(refs[0].href, "/css/css-flexbox/foo-ref.html");
    }

    #[test]
    fn parses_mismatch_and_relative_links() {
        let source = "<link rel=mismatch href='../refs/bar-ref.html'>";
        let refs = parse_ref_links(source, "/css/css-grid/sub/test.html");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].rel, "mismatch");
        assert_eq!(refs[0].href, "/css/css-grid/refs/bar-ref.html");
    }

    #[test]
    fn ignores_other_links() {
        let source = r#"<link rel="author" href="mailto:foo@example.com"><link rel="help" href="http://example.com">"#;
        let refs = parse_ref_links(source, "/css/foo.html");
        assert!(refs.is_empty());
    }

    #[test]
    fn resolves_absolute_href() {
        assert_eq!(
            resolve_href("/css/css-flexbox/foo.html", "/common/ref.html"),
            "/common/ref.html"
        );
    }
}
