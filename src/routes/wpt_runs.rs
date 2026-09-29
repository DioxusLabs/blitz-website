use dioxus::prelude::*;
use jiff::Timestamp;

use crate::{
    components::Page,
    wpt_runs::LatestRun,
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
    fetched_at: Timestamp,
) -> Element {
    let now = Timestamp::now();

    rsx! {
        Page { title: "WPT runs".into(),
            h1 { "WPT runs" }
            p {
                class: "introduction",
                dangerous_inner_html: r#"
                The latest <a href="https://github.com/web-platform-tests/wpt" target="_blank">Web Platform Tests</a> master run on
                <a href="https://wpt.fyi/runs" target="_blank">wpt.fyi</a> for each browser in the WPT comparison, most recently added first."#
            }
            hr {}
            p {
                font_size: "smaller",
                a { href: "/wpt", "WPT comparison" }
                " | All times are UTC. Updated {format_ago(now, fetched_at)}."
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
