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
                "Web pages rendered by Blitz, compiled to WebAssembly and drawn onto a canvas "
                "with WebGL2. Pages are fetched through this site and laid out and painted by "
                "Blitz: your browser's own HTML and CSS engine isn't used. Switch to "
                "\"Native\" to compare with your browser's rendering of the same page."
            }
            CommitInfoDisplay { commit_info, label: "Built from commit:" }
            form { class: "demo-toolbar", id: "demo-form", "data-demo-src": "{index_url}",
                div { class: "demo-nav",
                    button {
                        class: "demo-icon-button",
                        id: "demo-back",
                        r#type: "button",
                        disabled: true,
                        title: "Back",
                        "aria-label": "Back",
                        "←"
                    }
                    button {
                        class: "demo-icon-button",
                        id: "demo-forward",
                        r#type: "button",
                        disabled: true,
                        title: "Forward",
                        "aria-label": "Forward",
                        "→"
                    }
                    button {
                        class: "demo-icon-button",
                        id: "demo-reload",
                        r#type: "button",
                        disabled: true,
                        title: "Reload",
                        "aria-label": "Reload",
                        "↻"
                    }
                }
                input {
                    class: "demo-url",
                    id: "demo-url",
                    r#type: "text",
                    inputmode: "url",
                    spellcheck: "false",
                    required: true,
                    "aria-label": "Page URL",
                    value: DEFAULT_START_PAGE,
                }
                button { class: "demo-button", r#type: "submit", "Go" }
                div {
                    class: "demo-views",
                    role: "group",
                    "aria-label": "Renderer",
                    button {
                        class: "demo-view",
                        r#type: "button",
                        "data-view": "blitz",
                        "aria-pressed": "true",
                        "Blitz"
                    }
                    button {
                        class: "demo-view",
                        r#type: "button",
                        "data-view": "native",
                        "aria-pressed": "false",
                        "Native"
                    }
                }
            }
            p { class: "demo-status-line", id: "demo-status-line", role: "status" }
            div { class: "demo-frame", id: "demo-frame",
                div { class: "demo-overlay", id: "demo-overlay",
                    p { id: "demo-overlay-text",
                        "Blitz is a {format_size(download_size, DECIMAL)} download, so it isn't loaded until you ask."
                    }
                    button {
                        class: "demo-button",
                        id: "demo-load",
                        r#type: "button",
                        "Load Blitz ({format_size(download_size, DECIMAL)})"
                    }
                }
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
