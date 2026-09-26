//! Client for the wpt.fyi API: run discovery and raw report download.

use std::io::Read;
use std::sync::LazyLock;

use reqwest::Client;
use serde::Deserialize;

pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// A test run record from the wpt.fyi runs API.
#[derive(Deserialize)]
pub struct WptFyiRun {
    pub id: i64,
    pub browser_name: String,
    pub browser_version: String,
    pub os_name: Option<String>,
    pub full_revision_hash: String,
    pub time_end: Option<String>,
    pub raw_results_url: String,
}

/// Fetch the latest master run for each of the given products
/// (e.g. `"safari[experimental]"`).
pub async fn fetch_latest_runs(
    client: &Client,
    products: &[&str],
) -> Result<Vec<WptFyiRun>, Error> {
    let url = format!(
        "https://wpt.fyi/api/runs?label=master&products={}&max-count=1",
        products.join(",")
    );
    Ok(client
        .get(url)
        .send()
        .await?
        .json::<Vec<WptFyiRun>>()
        .await?)
}

/// Download a run's raw wptreport.json. The reports are stored gzip-encoded
/// (~17-20 MB); the compressed bytes are returned so decompression can happen
/// while stream-ingesting (see [`report_reader`]).
///
/// Downloaded over HTTP/1.1: Google Cloud Storage streams these reports as
/// many small HTTP/2 DATA frames, which trips h2's (>= 0.4.16) "excessive
/// small DATA frames" protection and aborts the connection with
/// `GOAWAY ENHANCE_YOUR_CALM (too_many_data_frames)` partway through.
pub async fn fetch_raw_report(raw_results_url: &str) -> Result<Vec<u8>, Error> {
    static CLIENT: LazyLock<Client> =
        LazyLock::new(|| Client::builder().http1_only().build().unwrap());
    Ok(CLIENT
        .get(raw_results_url)
        .header("Accept-Encoding", "gzip")
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?
        .to_vec())
}

/// A streaming reader of the decompressed report bytes returned by
/// [`fetch_raw_report`].
pub fn report_reader(bytes: &[u8]) -> Box<dyn Read + '_> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        Box::new(flate2::read::GzDecoder::new(bytes))
    } else {
        // Already decompressed by the HTTP client
        Box::new(bytes)
    }
}
