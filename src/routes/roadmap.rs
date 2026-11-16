use std::sync::LazyLock;

use dioxus::prelude::*;
use serde::Deserialize;

use crate::components::Page;

const REPO_URL: &str = "https://github.com/DioxusLabs/blitz";
const ROADMAP_ISSUE_URL: &str = "https://github.com/DioxusLabs/blitz/issues/119";
const CONTRIBUTING_URL: &str = "https://github.com/DioxusLabs/blitz/blob/main/CONTRIBUTING.MD";

/// "Coming next" and "backlog" are curated by hand from the unchecked items of
/// the roadmap issue (`ROADMAP_ISSUE_URL`) and open PRs, "recently completed"
/// from PRs merged since the latest release, and the release
/// history from the commits between each version bump (versions follow
/// `blitz-dom`, the only crate published at every version). Items link to
/// their issues/PRs, or to commits for work that landed without a PR.
/// Titles, descriptions and highlights are trusted HTML.
static ROADMAP: LazyLock<Roadmap> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../data/roadmap.json"))
        .expect("invalid data/roadmap.json")
});

/// Newest first
static RELEASE_HISTORY: LazyLock<Vec<Release>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../data/release-history.json"))
        .expect("invalid data/release-history.json")
});

#[derive(Deserialize)]
struct Roadmap {
    next: Vec<RoadmapItem>,
    backlog: Vec<RoadmapItem>,
    /// Merged, but not yet in a release
    completed: Vec<RoadmapItem>,
}

#[derive(Deserialize)]
struct RoadmapItem {
    title: String,
    description: String,
    /// The roadmap milestone the item belongs to (e.g. "1.0"), if any
    milestone: Option<String>,
    #[serde(default)]
    in_progress: bool,
    #[serde(default)]
    links: Vec<GitRef>,
}

#[derive(Deserialize)]
struct Release {
    version: String,
    /// `YYYY-MM-DD`
    date: String,
    highlights: Vec<Highlight>,
}

#[derive(Deserialize)]
struct Highlight {
    text: String,
    #[serde(default)]
    links: Vec<GitRef>,
}

/// An issue or PR number, or a commit hash (for work that landed without one),
/// or an issue or PR in another repository (e.g. a Parley dependency)
#[derive(Deserialize)]
#[serde(untagged)]
enum GitRef {
    Number(u32),
    Commit(String),
    External { repo: String, number: u32 },
}

impl GitRef {
    fn label(&self) -> String {
        match self {
            GitRef::Number(number) => format!("#{number}"),
            GitRef::Commit(hash) => hash.chars().take(7).collect(),
            GitRef::External { repo, number } => {
                let name = repo.rsplit('/').next().unwrap_or(repo);
                format!("{name}#{number}")
            }
        }
    }

    /// GitHub redirects `/issues/N` to `/pull/N` for PRs
    fn url(&self) -> String {
        match self {
            GitRef::Number(number) => format!("{REPO_URL}/issues/{number}"),
            GitRef::Commit(hash) => format!("{REPO_URL}/commit/{hash}"),
            GitRef::External { repo, number } => {
                format!("https://github.com/{repo}/issues/{number}")
            }
        }
    }
}

impl Release {
    /// The minor version series, e.g. "0.3" for "0.3.0-beta.1"
    fn series(&self) -> &str {
        let end = self
            .version
            .match_indices('.')
            .nth(1)
            .map_or(self.version.len(), |(i, _)| i);
        &self.version[..end]
    }

    fn is_prerelease(&self) -> bool {
        self.version.contains('-')
    }

    fn display_date(&self) -> String {
        self.date
            .parse::<jiff::civil::Date>()
            .map_or_else(|_| self.date.clone(), |date| date.strftime("%-d %b %Y").to_string())
    }
}

#[component]
pub fn RoadmapPage() -> Element {
    let roadmap = &*ROADMAP;
    let completed_description = match RELEASE_HISTORY.first() {
        Some(release) => format!("Merged since {}, not yet released.", release.version),
        None => "Merged, not yet released.".to_string(),
    };
    rsx! {
        Page {
            title: "Roadmap".into(),

            div { class: "roadmap",
                div { class: "roadmap-hero",
                    h1 { "Roadmap" }
                    p { "Keep up with what the Blitz team is building next." }
                    div { class: "roadmap-hero__actions",
                        a {
                            class: "roadmap-button",
                            href: CONTRIBUTING_URL,
                            target: "_blank",
                            rel: "noopener",
                            "Contribute to Blitz"
                            ChevronRightIcon {}
                        }
                        a {
                            class: "roadmap-button",
                            href: ROADMAP_ISSUE_URL,
                            target: "_blank",
                            rel: "noopener",
                            "Roadmap issue"
                            ChevronRightIcon {}
                        }
                    }
                }

                div { class: "roadmap-grid",
                    {roadmap_column("Backlog", "Other planned work.", &roadmap.backlog, true)}
                    {roadmap_column("Coming Next", "What we plan to land next.", &roadmap.next, true)}
                    {roadmap_column("Recently Completed", &completed_description, &roadmap.completed, false)}
                }

                {release_history(&RELEASE_HISTORY)}
            }
        }
    }
}

fn roadmap_column(
    heading: &str,
    description: &str,
    items: &[RoadmapItem],
    show_milestones: bool,
) -> Element {
    let items = items
        .iter()
        .filter(|item| item.in_progress)
        .chain(items.iter().filter(|item| !item.in_progress));
    rsx! {
        section { class: "roadmap-column",
            div { class: "roadmap-column__header",
                span { class: "roadmap-column__title", "{heading}" }
            }
            p { class: "roadmap-column__description", "{description}" }
            div { class: "roadmap-column__items",
                for item in items {
                    div {
                        class: "roadmap-card",
                        class: if !item.links.is_empty() { "roadmap-card--linked" },
                        div { class: "roadmap-card__header",
                            // The title links to the item's first issue/PR, and is
                            // stretched over the whole card (see roadmap.scss)
                            if let Some(link) = item.links.first() {
                                h3 { class: "roadmap-card__title",
                                    a {
                                        class: "roadmap-card__link",
                                        href: link.url(),
                                        target: "_blank",
                                        rel: "noopener",
                                        dangerous_inner_html: "{item.title}",
                                    }
                                }
                            } else {
                                h3 { class: "roadmap-card__title", dangerous_inner_html: "{item.title}" }
                            }
                            if item.in_progress {
                                span { class: "roadmap-tag roadmap-tag--in-progress", "In progress" }
                            } else if let Some(milestone) = item.milestone.as_ref().filter(|_| show_milestones) {
                                span { class: "roadmap-tag", title: "Roadmap milestone", "{milestone}" }
                            }
                        }
                        p { class: "roadmap-card__description", dangerous_inner_html: "{item.description}" }
                        if !item.links.is_empty() {
                            div { class: "roadmap-card__links", {ref_links(&item.links)} }
                        }
                    }
                }
            }
        }
    }
}

fn release_history(history: &[Release]) -> Element {
    // Consecutive releases grouped by minor version series
    let mut series: Vec<(&str, Vec<&Release>)> = Vec::new();
    for release in history {
        match series.last_mut() {
            Some((name, releases)) if *name == release.series() => releases.push(release),
            _ => series.push((release.series(), vec![release])),
        }
    }

    rsx! {
        section { class: "roadmap-history",
            h2 { "Release History" }
            p { class: "group-description",
                "Major features that landed in each Blitz release, including alphas and betas."
            }
            for (name, releases) in series {
                h3 { class: "roadmap-history__series", "Blitz {name}" }
                ol { class: "roadmap-timeline",
                    for release in releases {
                        li {
                            class: "roadmap-release",
                            class: if !release.is_prerelease() { "roadmap-release--stable" },
                            div { class: "roadmap-release__meta",
                                span { class: "roadmap-release__version", "{release.version}" }
                                time { class: "roadmap-release__date", datetime: "{release.date}",
                                    "{release.display_date()}"
                                }
                            }
                            ul { class: "roadmap-release__highlights",
                                for highlight in &release.highlights {
                                    li {
                                        span { dangerous_inner_html: "{highlight.text}" }
                                        if !highlight.links.is_empty() {
                                            " ("
                                            for (i , link) in highlight.links.iter().enumerate() {
                                                if i > 0 {
                                                    ", "
                                                }
                                                {ref_link(link, "roadmap-release__link")}
                                            }
                                            ")"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Issue/PR/commit links as a comma-separated list (on cards)
fn ref_links(links: &[GitRef]) -> Element {
    rsx! {
        for (i , link) in links.iter().enumerate() {
            if i > 0 {
                ", "
            }
            {ref_link(link, "roadmap-card__ref")}
        }
    }
}

fn ref_link(link: &GitRef, class: &str) -> Element {
    rsx! {
        a {
            class: "{class}",
            class: if matches!(link, GitRef::Commit(_)) { "roadmap-ref--commit" },
            href: link.url(),
            target: "_blank",
            rel: "noopener",
            {link.label()}
        }
    }
}

#[component]
fn ChevronRightIcon() -> Element {
    rsx! {
        svg {
            width: "16",
            height: "16",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            path { d: "m9 18 6-6-6-6" }
        }
    }
}
