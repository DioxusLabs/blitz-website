//! The latest wpt.fyi master run for each browser channel, for the
//! `/wpt/runs` page.

use std::collections::HashMap;
use std::sync::Arc;

use jiff::Timestamp;
use reqwest::Client;
use serde::Deserialize;

use crate::cache::{Cache, Cached, RefreshOutcome};
use crate::wpt_compare;

pub type Error = Box<dyn std::error::Error + Send + Sync>;

pub static WPT_RUNS_CACHE: Cache<WptRunsEntry> = Cache::new();

/// The wpt.fyi products shown on the comparison pages
fn product_specs() -> impl Iterator<Item = &'static str> {
    wpt_compare::PRODUCTS.iter().copied().filter(|spec| {
        let product = spec.split('[').next().unwrap();
        !wpt_compare::HIDDEN_PRODUCTS.contains(&product)
    })
}

/// wpt.fyi run labels that name a release channel
const CHANNELS: &[&str] = &["stable", "beta", "dev", "canary", "nightly", "preview"];

#[derive(Clone)]
pub struct WptRunsEntry {
    /// The latest run for each browser channel, newest on wpt.fyi first
    pub latest: Arc<Vec<LatestRun>>,
    pub fetched_at: Timestamp,
}

#[derive(Clone, PartialEq)]
pub struct LatestRun {
    pub id: i64,
    pub browser: String,
    pub channel: String,
    pub browser_version: String,
    pub revision: String,
    pub time_start: Timestamp,
    pub time_end: Timestamp,
    /// When the run was added to wpt.fyi
    pub created_at: Timestamp,
}

/// Fetch the latest runs. Only one refresh runs at a time;
/// calls that arrive while one is in flight wait for it instead.
pub async fn load_wpt_runs(
    _existing: Option<Arc<Cached<WptRunsEntry>>>,
) -> RefreshOutcome<WptRunsEntry> {
    static REFRESH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let Ok(_guard) = REFRESH_LOCK.try_lock() else {
        let _guard = REFRESH_LOCK.lock().await;
        return RefreshOutcome::Unchanged;
    };

    let client = Client::new();
    let latest = match fetch_latest_runs(&client).await {
        Ok(latest) => latest,
        Err(err) => {
            println!("Error fetching latest wpt.fyi runs: {err}");
            return RefreshOutcome::Failed;
        }
    };
    RefreshOutcome::Updated(WptRunsEntry {
        latest: Arc::new(latest),
        fetched_at: Timestamp::now(),
    })
}

fn parse_time(time: &str) -> Result<Timestamp, Error> {
    time.parse::<Timestamp>()
        .map_err(|err| format!("invalid timestamp {time:?}: {err}").into())
}

async fn fetch_latest_runs(client: &Client) -> Result<Vec<LatestRun>, Error> {
    #[derive(Deserialize)]
    struct ApiRun {
        id: i64,
        browser_name: String,
        browser_version: String,
        full_revision_hash: String,
        labels: Vec<String>,
        time_start: String,
        time_end: String,
        created_at: String,
    }

    // Runs are returned newest-started first: the most recently added run is
    // almost always the first, but a run that started earlier can finish (and
    // be added) later. `max-count` applies to each product separately
    let mut url = String::from("https://wpt.fyi/api/runs?label=master&max-count=5");
    for spec in product_specs() {
        url.push_str("&product=");
        url.push_str(&spec.replace('[', "%5B").replace(']', "%5D"));
    }
    let runs: Vec<ApiRun> = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let mut latest: HashMap<(String, String), LatestRun> = HashMap::new();
    for run in runs {
        let Some(channel) = run
            .labels
            .iter()
            .find(|label| CHANNELS.contains(&label.as_str()))
        else {
            continue;
        };
        let run = LatestRun {
            id: run.id,
            browser: run.browser_name,
            channel: channel.clone(),
            browser_version: run.browser_version,
            revision: run.full_revision_hash,
            time_start: parse_time(&run.time_start)?,
            time_end: parse_time(&run.time_end)?,
            created_at: parse_time(&run.created_at)?,
        };
        let key = (run.browser.clone(), run.channel.clone());
        match latest.get(&key) {
            Some(existing) if existing.created_at >= run.created_at => {}
            _ => {
                latest.insert(key, run);
            }
        }
    }

    let mut latest: Vec<LatestRun> = latest.into_values().collect();
    latest.sort_by_key(|run| std::cmp::Reverse(run.created_at));
    Ok(latest)
}
