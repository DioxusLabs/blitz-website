use std::{ops::Deref, sync::Arc};

use dioxus::prelude::*;
use humansize::{format_size, BINARY};

use crate::{
    components::{CommitInfoDisplay, Page},
    downloads::DownloadLink,
    github::CommitInfo,
};

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
pub fn DownloadsPage(links: ArcDownloadLinks, commit_info: Option<CommitInfo>) -> Element {
    rsx! {
        <Page title={"Downloads".into()}>
            <h1>"Downloads"</h1>
            <p dangerous_inner_html=r#"Downloads for the Blitz Browser"# />
            <CommitInfoDisplay commit_info label="Builds for commit:" />
            <DownloadsTable links />
        </Page>
    }
}
#[component]
pub fn DownloadsUnavailablePage() -> Element {
    rsx! {
        <Page title={"Downloads".into()}>
            <h1>"Downloads"</h1>
            <p>"Downloads are currently unavailable"</p>
        </Page>
    }
}

#[component]
pub fn DownloadsTable(links: ArcDownloadLinks) -> Element {
    rsx!(
        <table width="100%">
            <tr>
                <th>"Platform"</th>
                <th>"Architecture"</th>
                <th>"Download Link"</th>
                <th>"File size"</th>
            </tr>
            {
                links
                    .iter()
                    .map(|link| {
                        let size_fmt = format_size(link.size_in_bytes, BINARY);
                        rsx! {
                            <tr>
                                <td>"{link.platform}"</td>
                                <td>"{link.arch}"</td>
                                <td>
                                    <a href="{link.url}">"{link.filename}"</a>
                                </td>
                                <td>"{size_fmt}"</td>
                            </tr>
                        }
                    })
            }
        </table>
    )
}
