use std::sync::{Arc, LazyLock};

use dioxus::prelude::*;
use fxhash::hash32;
use humansize::{format_size, DECIMAL};

use crate::{
    components::{CommitInfoDisplay, Page},
    github::CommitInfo,
    wasm_demo::WasmDemoApp,
};

/// Content hash of the demo script, appended to its URL so browsers don't
/// keep serving a cached copy after it changes (as for index.css)
static DEMO_SCRIPT_HASH: LazyLock<u32> =
    LazyLock::new(|| hash32(include_str!("../../static/demo.js")));

#[derive(Clone)]
pub struct ArcWasmDemoApps(pub Arc<[WasmDemoApp]>);
impl PartialEq for ArcWasmDemoApps {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[component]
pub fn DemoPage(apps: ArcWasmDemoApps, commit_info: Option<CommitInfo>) -> Element {
    rsx! {
        Page { title: "Demo".into(),
            h1 { "Demo" }
            p {
                "Dioxus Native apps rendered by Blitz, compiled to WebAssembly and drawn onto a "
                "canvas with WebGL2. Your browser's own HTML and CSS engine isn't used."
            }
            CommitInfoDisplay { commit_info, label: "Built from commit:" }
            div { class: "demo-buttons",
                for app in apps.0.iter() {
                    button {
                        class: "demo-button",
                        "data-demo-src": "{app.url}",
                        "Load {app.label} ({format_size(app.download_size, DECIMAL)})"
                    }
                }
            }
            div { class: "demo-frame", id: "demo-frame",
                p { class: "demo-status", id: "demo-status",
                    "Pick an app to download and run it."
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
