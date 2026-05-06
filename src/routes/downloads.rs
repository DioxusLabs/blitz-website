use std::{ops::Deref, sync::Arc};

use dioxus::prelude::*;
use humansize::{format_size, BINARY};

use crate::{components::Page, downloads::DownloadLink};

#[derive(Clone)]
pub struct ArcDownloadLinks(pub Arc<[DownloadLink]>);
impl PartialEq for ArcDownloadLinks {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Deref for ArcDownloadLinks {
    type Target = [DownloadLink];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[component]
pub fn DownloadsPage(links: ArcDownloadLinks) -> Element {
    rsx! {
        Page { title: "Downloads".into(),
            h1 {
                "Downloads"
            }
            p {
                dangerous_inner_html: r#"Downloads for the Blitz Browser"#
            }
            DownloadsTable { links }
        }
    }
}
#[component]
pub fn DownloadsTable(links: ArcDownloadLinks) -> Element {
    rsx!(
        table {
            width: "100%",
            tr {
                th { "Platform" }
                th { "Architecture" }
                th { "Download Link" }
                th { "File size" }
            }
            {
                links.iter().map(|link| {

                    let size_fmt = format_size(link.size_in_bytes, BINARY);

                    rsx!(
                        tr {
                            td {
                                "{link.platform}"
                            }
                            td {
                                "{link.arch}"
                            }
                            td {
                                a {
                                    href: "{link.url}",
                                    "{link.filename}"
                                }
                            }
                            td {
                                "{size_fmt}"
                            }
                        }
                    )
                })
            }
        }
    )
}
