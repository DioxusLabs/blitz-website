use dioxus::prelude::*;
use jiff::Timestamp;

use crate::{
    components::Page,
    wpt_runs::{ActiveRun, LatestRun},
};

use super::wpt_compare::{product_color, product_label};

/// Display name for a browser channel, e.g. ("safari", "preview") ->
/// "Safari Technology Preview"
fn channel_label(browser: &str, channel: &str) -> String {
    let channel = match channel {
        "preview" => "Technology Preview".to_string(),
        other => product_label(other),
    };
    format!("{} {channel}", product_label(browser))
}

fn short_sha(sha: &str) -> &str {
    &sha[..sha.len().min(10)]
}

fn format_time(time: Timestamp) -> String {
    time.strftime("%a %d %b %H:%M").to_string()
}

/// A duration in seconds, e.g. "2h 05m" or "12m"
fn format_duration(secs: i64) -> String {
    let secs = secs.max(0);
    let (days, hours, mins) = (secs / 86400, secs / 3600 % 24, secs / 60 % 60);
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {mins:02}m")
    } else {
        format!("{mins}m")
    }
}

fn format_ago(now: Timestamp, time: Timestamp) -> String {
    let secs = now.as_second() - time.as_second();
    if secs < 60 {
        "just now".to_string()
    } else {
        format!("{} ago", format_duration(secs))
    }
}

#[component]
fn BrowserName(browser: String, channel: String) -> Element {
    rsx! {
        span {
            display: "inline-block",
            width: "0.7em",
            height: "0.7em",
            margin_right: "0.4em",
            border_radius: "50%",
            background: product_color(&browser),
        }
        {channel_label(&browser, &channel)}
    }
}

#[component]
fn CommitLink(sha: String) -> Element {
    rsx! {
        a {
            href: format!("https://github.com/web-platform-tests/wpt/commit/{sha}"),
            target: "_blank",
            code { {short_sha(&sha)} }
        }
    }
}

#[component]
pub fn WptRunsPage(
    latest: Vec<LatestRun>,
    active: Option<Result<Vec<ActiveRun>, String>>,
    fetched_at: Timestamp,
) -> Element {
    let now = Timestamp::now();
    let has_github_token = active.is_some();

    rsx! {
        Page { title: "WPT runs".into(),
            h1 { "WPT runs" }
            p {
                class: "introduction",
                dangerous_inner_html: r#"
                The latest <a href="https://github.com/web-platform-tests/wpt" target="_blank">Web Platform Tests</a> master run on
                <a href="https://wpt.fyi/runs" target="_blank">wpt.fyi</a> for each browser in the WPT comparison, most recently added first,
                and the runs that are still in progress."#
            }
            hr {}
            p {
                font_size: "smaller",
                a { href: "/wpt", "WPT comparison" }
                " | All times are UTC. Updated {format_ago(now, fetched_at)}."
            }

            h2 { margin_bottom: "0.4em", "In progress" }
            match active {
                None => rsx! {
                    p { padding: "20px", color: "#666", "In-progress runs aren't shown because this server has no GitHub token configured. They are read from the WPT repository's CI checks on GitHub." }
                },
                Some(Err(err)) => rsx! {
                    p { "In-progress runs are unavailable right now ({err})." }
                },
                Some(Ok(active)) if active.is_empty() => rsx! {
                    p { "No runs are in progress." }
                },
                Some(Ok(active)) => rsx! {
                    table {
                        width: "100%",
                        tr {
                            th { "Browser" }
                            th { "WPT commit" }
                            th { "Started" }
                            th { "Progress" }
                        }
                        for run in active {
                            tr {
                                td { BrowserName { browser: run.browser.clone(), channel: run.channel.clone() } }
                                td { CommitLink { sha: run.revision.clone() } }
                                td {
                                    match run.started_at {
                                        Some(started_at) => rsx! {
                                            "{format_time(started_at)} ({format_ago(now, started_at)})"
                                        },
                                        None => rsx! { "Queued" },
                                    }
                                }
                                td {
                                    match run.finished_at {
                                        Some(finished_at) => rsx! {
                                            "Tests finished {format_ago(now, finished_at)}, waiting for wpt.fyi"
                                        },
                                        None => rsx! {
                                            progress {
                                                max: "{run.chunks_total}",
                                                value: "{run.chunks_completed}",
                                                margin_right: "0.5em",
                                                vertical_align: "middle",
                                            }
                                            "{run.chunks_completed}/{run.chunks_total} chunks done"
                                            if run.chunks_running > 0 {
                                                ", {run.chunks_running} running"
                                            }
                                            if run.chunks_queued() > 0 {
                                                ", {run.chunks_queued()} queued"
                                            }
                                        },
                                    }
                                }
                            }
                        }
                    }
                },
            }
            if has_github_token {
                p {
                    font_size: "smaller",
                    "Found from the CI check runs on recent commits to the WPT repository. Ladybird is tested outside that CI, so its runs only show up once they are on wpt.fyi."
                }
            }

            h2 { margin_bottom: "0.4em", "Latest runs" }
            table {
                width: "100%",
                tr {
                    th { "Browser" }
                    th { "WPT commit" }
                    th { "Started" }
                    th { "Took" }
                    th { "Added to wpt.fyi" }
                    th { "Upload delay" }
                }
                for run in latest {
                    tr {
                        td {
                            a {
                                href: format!("https://wpt.fyi/results/?run_id={}", run.id),
                                target: "_blank",
                                BrowserName { browser: run.browser.clone(), channel: run.channel.clone() }
                            }
                            div {
                                font_size: "smaller",
                                opacity: "0.7",
                                {run.browser_version.clone()}
                            }
                        }
                        td { CommitLink { sha: run.revision.clone() } }
                        td { {format_time(run.time_start)} }
                        td { {format_duration(run.time_end.as_second() - run.time_start.as_second())} }
                        td { "{format_time(run.created_at)} ({format_ago(now, run.created_at)})" }
                        td { {format_duration(run.created_at.as_second() - run.time_end.as_second())} }
                    }
                }
            }
            p {
                font_size: "smaller",
                "Upload delay is the time from a run's tests finishing to its results appearing on wpt.fyi."
            }
        }
    }
}
