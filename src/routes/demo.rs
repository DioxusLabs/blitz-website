use std::sync::LazyLock;

use dioxus::prelude::*;
use fxhash::hash32;
use humansize::{format_size, DECIMAL};

use crate::{
    components::{CommitInfoDisplay, Page},
    github::CommitInfo,
};

/// Content hash of the demo script, appended to its URL so browsers don't
/// keep serving a cached copy after it changes (as for index.css)
static DEMO_SCRIPT_HASH: LazyLock<u32> =
    LazyLock::new(|| hash32(include_str!("../../static/demo.js")));

const DEFAULT_START_PAGE: &str = "https://en.wikipedia.org/wiki/Web_browser_engine";

#[component]
pub fn DemoPage(index_url: String, download_size: u64, commit_info: Option<CommitInfo>) -> Element {
    rsx! {
        Page { title: "Demo".into(),
            h1 { "Demo" }
            p {
                "The Blitz browser, compiled to WebAssembly and drawn onto a canvas with "
                "WebGL2. Pages are fetched through this site and laid out and painted by "
                "Blitz: your browser's own HTML and CSS engine isn't used."
            }
            CommitInfoDisplay { commit_info, label: "Built from commit:" }
            form { class: "demo-form", id: "demo-form", "data-demo-src": "{index_url}",
                input {
                    class: "demo-url",
                    id: "demo-url",
                    r#type: "text",
                    inputmode: "url",
                    spellcheck: "false",
                    required: true,
                    "aria-label": "Start page",
                    value: DEFAULT_START_PAGE,
                }
                button { class: "demo-button", r#type: "submit",
                    "Load Blitz ({format_size(download_size, DECIMAL)})"
                }
            }
            div { class: "demo-frame", id: "demo-frame",
                p { class: "demo-status", id: "demo-status",
                    "Enter a start page and load the browser to try it."
                }
            }
            p { class: "demo-links", id: "demo-links", hidden: true,
                a { id: "demo-open-link", target: "_blank", "Open full page" }
            }
            script { src: "/static/demo.js?{*DEMO_SCRIPT_HASH}" }
        }
    }
}

#[component]
pub fn DemoUnavailablePage() -> Element {
    rsx! {
        Page { title: "Demo".into(),
            h1 { "Demo" }
            p { "The demo is currently unavailable" }
        }
    }
}
