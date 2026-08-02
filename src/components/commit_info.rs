use dioxus::prelude::*;

use crate::github::CommitInfo;

fn format_commit_date(timestamp: &str) -> String {
    timestamp
        .strip_suffix('Z')
        .and_then(|timestamp| timestamp.split_once('T'))
        .map(|(date, _time)| format!("{date}"))
        .unwrap_or_else(|| timestamp.to_string())
}

#[component]
pub fn CommitInfoDisplay(commit_info: Option<CommitInfo>) -> Element {
    let Some(commit_info) = commit_info else {
        return rsx! {};
    };
    let short_sha = commit_info.sha.get(..7).unwrap_or(&commit_info.sha);
    let message = commit_info
        .message
        .as_deref()
        .and_then(|message| message.lines().next());

    rsx! {
        p {
            // font_size: "smaller",
            span {
                font_size: "smaller",
                color: "#666",
                "Data from:"
            }
            br {},
            a {
                href: "https://github.com/DioxusLabs/blitz/commit/{commit_info.sha}",
                target: "_blank",
                "{short_sha}"
            }
            if let Some(message) = message {
                " {message}"
            }
            if let Some(timestamp) = commit_info.timestamp.as_deref() {
                span {
                    color: "#666",
                    " ({format_commit_date(timestamp)})"
                }
            }
        }
    }
}
