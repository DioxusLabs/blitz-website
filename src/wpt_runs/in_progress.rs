//! Runs that are still in progress, for the `/wpt/runs` page.
//!
//! Chrome, Firefox and Servo (Taskcluster), Safari (GitHub Actions) and Edge
//! (Azure Pipelines) all run the tests as check runs on the commit of the
//! WPT repository they are testing, so in-progress runs are found by
//! looking at the check runs of recent master commits and the `epochs/*`
//! branch heads (which the daily and hourly runs are triggered from).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use jiff::Timestamp;
use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize};
use tokio::task::JoinSet;

use super::{parse_time, Error, LatestRun};

/// Browsers whose in-progress runs are tracked (Ladybird runs outside the
/// WPT repository's CI, so it isn't visible until it reaches wpt.fyi)
const TRACKED_BROWSERS: &[&str] = &["chrome", "edge", "firefox", "safari", "servo"];

const WPT_REPO_API: &str = "https://api.github.com/repos/web-platform-tests/wpt";

/// How far back to look for master commits that may still be being tested
const RECENT_COMMITS_SECS: i64 = 8 * 3600;
/// How long after its tests finish a run is shown as waiting for wpt.fyi
const AWAITING_UPLOAD_SECS: i64 = 4 * 3600;
/// Runs that started longer ago than this are assumed to be stuck
const STALE_RUN_SECS: i64 = 12 * 3600;

#[derive(Clone, PartialEq)]
pub struct ActiveRun {
    pub browser: String,
    pub channel: String,
    pub revision: String,
    pub started_at: Option<Timestamp>,
    /// When the last chunk finished, once they all have
    pub finished_at: Option<Timestamp>,
    pub chunks_total: usize,
    pub chunks_completed: usize,
    pub chunks_running: usize,
}

impl ActiveRun {
    pub fn chunks_queued(&self) -> usize {
        self.chunks_total - self.chunks_completed - self.chunks_running
    }
}

/// The in-progress runs, or why they couldn't be loaded. `None` when no
/// `GITHUB_TOKEN` is configured, in which case GitHub isn't queried at all
pub(super) async fn load(
    client: Client,
    latest: &[LatestRun],
) -> Option<Result<Arc<Vec<ActiveRun>>, Arc<str>>> {
    let token = std::env::var("GITHUB_TOKEN")
        .ok()
        .filter(|token| !token.is_empty())?;
    let github = GithubApi {
        client,
        token: Some(Arc::from(token)),
    };
    Some(
        fetch_active_runs(&github, latest)
            .await
            .map(Arc::new)
            .map_err(|err| {
                println!("Error fetching in-progress WPT runs: {err}");
                Arc::from(err.to_string())
            }),
    )
}

fn parse_optional_time(time: Option<&str>) -> Option<Timestamp> {
    time.and_then(|time| time.parse().ok())
}

fn iso_time(time: Timestamp) -> String {
    time.strftime("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[derive(Clone)]
struct GithubApi {
    client: Client,
    token: Option<Arc<str>>,
}

impl GithubApi {
    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, Error> {
        let mut request = self
            .client
            .get(format!("{WPT_REPO_API}{path}"))
            .header("user-agent", "Blitz website")
            .header("accept", "application/vnd.github+json");
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(format!("GitHub API returned {} for {path}", response.status()).into());
        }
        Ok(response.json().await?)
    }
}

#[derive(Deserialize)]
struct CheckSuite {
    id: u64,
    head_sha: String,
    status: String,
    updated_at: String,
    latest_check_runs_count: usize,
    app: CheckApp,
}

#[derive(Deserialize)]
struct CheckApp {
    slug: String,
}

#[derive(Clone, Deserialize)]
struct CheckRun {
    name: String,
    status: String,
    started_at: Option<String>,
    completed_at: Option<String>,
}

/// The GitHub apps that run the tests themselves (as opposed to wpt.fyi's
/// own checks, which report results once they have been ingested)
const TEST_RUNNER_APPS: &[&str] = &[
    "community-tc-integration",
    "github-actions",
    "azure-pipelines",
];

async fn fetch_active_runs(
    github: &GithubApi,
    latest: &[LatestRun],
) -> Result<Vec<ActiveRun>, Error> {
    #[derive(Deserialize)]
    struct Commit {
        sha: String,
    }
    #[derive(Deserialize)]
    struct GitRef {
        object: GitObject,
    }
    #[derive(Deserialize)]
    struct GitObject {
        sha: String,
    }
    #[derive(Deserialize)]
    struct CheckSuites {
        check_suites: Vec<CheckSuite>,
    }

    let now = Timestamp::now();
    let since = iso_time(Timestamp::from_second(
        now.as_second() - RECENT_COMMITS_SECS,
    )?);
    let commits: Vec<Commit> = github
        .get(&format!("/commits?sha=master&since={since}&per_page=100"))
        .await?;
    let epochs: Vec<GitRef> = github.get("/git/matching-refs/heads/epochs/").await?;

    let mut seen = HashSet::new();
    let shas: Vec<String> = commits
        .into_iter()
        .map(|commit| commit.sha)
        .chain(epochs.into_iter().map(|git_ref| git_ref.object.sha))
        .filter(|sha| seen.insert(sha.clone()))
        .collect();

    let mut tasks = JoinSet::new();
    for sha in shas {
        let github = github.clone();
        tasks.spawn(async move {
            github
                .get::<CheckSuites>(&format!("/commits/{sha}/check-suites?per_page=100"))
                .await
        });
    }
    // Suites that are still running, or finished recently enough that their
    // results may not have reached wpt.fyi yet
    let recent_cutoff = now.as_second() - AWAITING_UPLOAD_SECS;
    let mut suites = Vec::new();
    while let Some(result) = tasks.join_next().await {
        for suite in result??.check_suites {
            let recent = parse_time(&suite.updated_at)?.as_second() >= recent_cutoff;
            if TEST_RUNNER_APPS.contains(&suite.app.slug.as_str())
                && suite.latest_check_runs_count > 0
                && (suite.status != "completed" || recent)
            {
                suites.push(suite);
            }
        }
    }

    // A completed suite's check runs don't change, so they are only fetched once
    static COMPLETED_SUITES: std::sync::Mutex<Option<HashMap<u64, Arc<Vec<CheckRun>>>>> =
        std::sync::Mutex::new(None);
    let completed_cache = COMPLETED_SUITES.lock().unwrap().clone().unwrap_or_default();
    let mut suite_check_runs = Vec::new();
    let mut tasks = JoinSet::new();
    for suite in suites {
        let completed = suite.status == "completed";
        if let Some(runs) = completed_cache.get(&suite.id).filter(|_| completed) {
            suite_check_runs.push((suite.id, completed, suite.head_sha, runs.clone()));
            continue;
        }
        let github = github.clone();
        tasks.spawn(async move {
            let runs = fetch_suite_check_runs(&github, &suite).await?;
            Ok::<_, Error>((suite.id, completed, suite.head_sha, Arc::new(runs)))
        });
    }
    while let Some(result) = tasks.join_next().await {
        suite_check_runs.push(result??);
    }
    *COMPLETED_SUITES.lock().unwrap() = Some(
        suite_check_runs
            .iter()
            .filter(|(_, completed, _, _)| *completed)
            .map(|(id, _, _, runs)| (*id, runs.clone()))
            .collect(),
    );

    let mut active = summarize_check_runs(
        suite_check_runs
            .iter()
            .map(|(_, _, sha, runs)| (sha.as_str(), runs.as_slice())),
        latest,
        now,
    );
    // Only the channels shown on the comparison pages (which `latest` is
    // limited to)
    active.retain(|run| {
        latest
            .iter()
            .any(|latest| latest.browser == run.browser && latest.channel == run.channel)
    });
    Ok(active)
}

/// Group check runs (with the commit they ran on) into the runs they are
/// chunks of, keeping those that are in progress or waiting for wpt.fyi
fn summarize_check_runs<'a>(
    suites: impl Iterator<Item = (&'a str, &'a [CheckRun])>,
    latest: &[LatestRun],
    now: Timestamp,
) -> Vec<ActiveRun> {
    let recent_cutoff = now.as_second() - AWAITING_UPLOAD_SECS;
    let mut groups: HashMap<(String, String, String), ActiveRun> = HashMap::new();
    for (sha, check_runs) in suites {
        for check_run in check_runs {
            let Some((browser, channel)) = chunk_product(&check_run.name) else {
                continue;
            };
            let run = groups
                .entry((sha.to_string(), browser.clone(), channel.clone()))
                .or_insert_with(|| ActiveRun {
                    browser,
                    channel,
                    revision: sha.to_string(),
                    started_at: None,
                    finished_at: None,
                    chunks_total: 0,
                    chunks_completed: 0,
                    chunks_running: 0,
                });
            run.chunks_total += 1;
            match check_run.status.as_str() {
                "completed" => run.chunks_completed += 1,
                "in_progress" => run.chunks_running += 1,
                _ => {}
            }
            if let Some(started_at) = parse_optional_time(check_run.started_at.as_deref()) {
                run.started_at = Some(run.started_at.map_or(started_at, |t| t.min(started_at)));
            }
            if let Some(completed_at) = parse_optional_time(check_run.completed_at.as_deref()) {
                run.finished_at = Some(
                    run.finished_at
                        .map_or(completed_at, |t| t.max(completed_at)),
                );
            }
        }
    }

    let mut active: Vec<ActiveRun> = groups
        .into_values()
        .filter_map(|mut run| {
            if run.chunks_completed < run.chunks_total {
                run.finished_at = None;
                let stale = run
                    .started_at
                    .is_some_and(|t| t.as_second() < now.as_second() - STALE_RUN_SECS);
                return (!stale).then_some(run);
            }
            // Finished: only shown until wpt.fyi has the run (or it has
            // been long enough that it presumably never will)
            let finished_at = run.finished_at?;
            if finished_at.as_second() < recent_cutoff {
                return None;
            }
            let uploaded = latest.iter().any(|latest| {
                latest.browser == run.browser
                    && latest.channel == run.channel
                    && (latest.revision == run.revision || latest.created_at >= finished_at)
            });
            (!uploaded).then_some(run)
        })
        .collect();
    active.sort_by_key(|run| (run.finished_at.is_some(), std::cmp::Reverse(run.started_at)));
    active
}

async fn fetch_suite_check_runs(
    github: &GithubApi,
    suite: &CheckSuite,
) -> Result<Vec<CheckRun>, Error> {
    #[derive(Deserialize)]
    struct CheckRuns {
        total_count: usize,
        check_runs: Vec<CheckRun>,
    }

    let mut runs = Vec::new();
    for page in 1..=10 {
        let response: CheckRuns = github
            .get(&format!(
                "/check-suites/{}/check-runs?filter=latest&per_page=100&page={page}",
                suite.id
            ))
            .await?;
        let done = response.check_runs.is_empty();
        runs.extend(response.check_runs);
        if done || runs.len() >= response.total_count {
            break;
        }
    }
    Ok(runs)
}

/// The `(browser, channel)` a check run is a test chunk of, if it is one.
/// Channels are named as in wpt.fyi's run labels.
///
/// - Taskcluster: `wpt-chrome-canary-testharness-3`
/// - GitHub Actions: `All Tests: Safari Technology Preview / testharness: 3 (of 16)`
/// - Azure Pipelines: `Azure Pipelines (all tests: Edge Dev 3)`
fn chunk_product(name: &str) -> Option<(String, String)> {
    let (browser, channel) = if let Some(rest) = name.strip_prefix("wpt-") {
        let parts: Vec<&str> = rest.split('-').collect();
        if parts.len() < 4 || parts.last()?.parse::<u32>().is_err() {
            return None;
        }
        (parts[0].to_string(), parts[1].to_string())
    } else if let Some(rest) = name.strip_prefix("All Tests: ") {
        let (workflow, job) = rest.split_once(" / ")?;
        if !job.contains(" (of ") {
            return None;
        }
        if workflow.contains("Technology Preview") {
            ("safari".to_string(), "preview".to_string())
        } else if workflow.starts_with("Safari") {
            ("safari".to_string(), "stable".to_string())
        } else {
            return None;
        }
    } else {
        let rest = name.strip_prefix("Azure Pipelines (all tests: ")?;
        let words: Vec<&str> = rest.trim_end_matches(')').split(' ').collect();
        let [browser, channel, chunk] = words[..] else {
            return None;
        };
        chunk.parse::<u32>().ok()?;
        (browser.to_lowercase(), channel.to_lowercase())
    };
    TRACKED_BROWSERS
        .contains(&browser.as_str())
        .then_some((browser, channel))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_run(
        name: &str,
        status: &str,
        started: Option<&str>,
        completed: Option<&str>,
    ) -> CheckRun {
        CheckRun {
            name: name.to_string(),
            status: status.to_string(),
            started_at: started.map(str::to_string),
            completed_at: completed.map(str::to_string),
        }
    }

    #[test]
    fn summarizes_check_runs() {
        let now: Timestamp = "2026-09-29T16:00:00Z".parse().unwrap();
        let running = [
            check_run(
                "wpt-chrome-canary-testharness-1",
                "completed",
                Some("2026-09-29T14:00:00Z"),
                Some("2026-09-29T15:00:00Z"),
            ),
            check_run(
                "wpt-chrome-canary-testharness-2",
                "in_progress",
                Some("2026-09-29T14:05:00Z"),
                None,
            ),
            check_run("wpt-chrome-canary-testharness-3", "queued", None, None),
            check_run(
                "wpt-decision-task",
                "completed",
                Some("2026-09-29T13:55:00Z"),
                Some("2026-09-29T13:58:00Z"),
            ),
            // Finished, and not yet on wpt.fyi
            check_run(
                "wpt-servo-nightly-testharness-1",
                "completed",
                Some("2026-09-29T12:00:00Z"),
                Some("2026-09-29T15:30:00Z"),
            ),
            // Finished, and already on wpt.fyi
            check_run(
                "wpt-firefox-nightly-testharness-1",
                "completed",
                Some("2026-09-29T13:00:00Z"),
                Some("2026-09-29T14:00:00Z"),
            ),
        ];
        let stale = [check_run(
            "wpt-servo-nightly-testharness-1",
            "in_progress",
            Some("2026-09-28T12:00:00Z"),
            None,
        )];
        let uploaded = LatestRun {
            id: 1,
            browser: "firefox".to_string(),
            channel: "nightly".to_string(),
            browser_version: "159.0a1".to_string(),
            revision: "aaa".to_string(),
            time_start: "2026-09-29T13:00:00Z".parse().unwrap(),
            time_end: "2026-09-29T14:00:00Z".parse().unwrap(),
            created_at: "2026-09-29T14:30:00Z".parse().unwrap(),
        };

        let active = summarize_check_runs(
            [("aaa", &running[..]), ("bbb", &stale[..])].into_iter(),
            &[uploaded],
            now,
        );
        let summary: Vec<_> = active
            .iter()
            .map(|run| {
                (
                    format!("{} {} {}", run.browser, run.channel, run.revision),
                    (
                        run.chunks_completed,
                        run.chunks_running,
                        run.chunks_queued(),
                    ),
                    run.finished_at.is_some(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("chrome canary aaa".to_string(), (1, 1, 1), false),
                ("servo nightly aaa".to_string(), (1, 0, 0), true),
            ]
        );
    }

    fn product(name: &str) -> Option<String> {
        chunk_product(name).map(|(browser, channel)| format!("{browser} {channel}"))
    }

    #[test]
    fn parses_chunk_names() {
        assert_eq!(
            product("wpt-chrome-canary-testharness-3"),
            Some("chrome canary".into())
        );
        assert_eq!(
            product("wpt-firefox-nightly-print-reftest-1"),
            Some("firefox nightly".into())
        );
        assert_eq!(
            product("wpt-servo-nightly-wdspec-2"),
            Some("servo nightly".into())
        );
        assert_eq!(product("wpt-firefox_android-nightly-testharness-1"), None);
        assert_eq!(product("wpt-decision-task"), None);
        assert_eq!(
            product("All Tests: Safari Technology Preview / testharness: 3 (of 16)"),
            Some("safari preview".into())
        );
        assert_eq!(
            product("All Tests: Safari (stable) / reftest: 1 (of 6)"),
            Some("safari stable".into())
        );
        assert_eq!(
            product("All Tests: Safari Technology Preview / Merge results artifacts"),
            None
        );
        assert_eq!(
            product("Azure Pipelines (all tests: Edge Dev 3)"),
            Some("edge dev".into())
        );
        assert_eq!(
            product("Azure Pipelines (wpt.fyi hook: edge-dev-results)"),
            None
        );
        assert_eq!(product("wpt.fyi - chrome[experimental]"), None);
    }
}
