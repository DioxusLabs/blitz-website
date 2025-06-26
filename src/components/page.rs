use std::{borrow::Cow, env::current_dir, fs::read_to_string};

use dioxus::prelude::*;
use fxhash::hash32;

use super::{TableOfContents, TocSection};

#[component]
pub fn Page(
    title: Cow<'static, str>,
    head: Option<Element>,
    footer: Option<Element>,
    children: Element,
    #[props(default)] transparent_header: bool,
    #[props(default)] noframe: bool,
) -> Element {
    // Register function to get hash of CSS file. Hash doesn't need to be secure as it is
    // purely to prevent the old version of the file being cached when the file it updated
    let cwd = current_dir().unwrap();
    let index_css_path = {
        let mut path = cwd.clone();
        path.push("static/index.css");
        path
    };
    let index_css_contents = read_to_string(index_css_path).unwrap();
    let index_css_hash = hash32(&index_css_contents);

    rsx! {
        div { lang: "en",
            head {
                Fragment {
                    meta { charset: "UTF-8" }
                    meta {
                        name: "viewport",
                        content: "width=device-width, initial-scale=1.0",
                    }
                    title { "Blitz - {title}" }
                    link { href: "/static/normalize.css", rel: "stylesheet" }
                    link {
                        rel: "stylesheet",
                        href: "/static/index.css?{index_css_hash}",
                    }
                    link { rel: "icon", href: "/static/blitz-logo.svg" }
                    {head}
                }
            }
            body {
                Fragment {
                    div {
                        id: "header",
                        class: if transparent_header { "transparent-bg" },
                        a { href: "/", class: "logo-link",
                            img {
                                src: "/static/blitz-logo-with-text3.svg",
                                class: "logo",
                            }
                        }
                        // Logo with HTML text
                        // div {
                        //     style: "display: flex;align-items: center; gap: 20px;",
                        //     a { href: "/",
                        //         img { src: "/static/blitz-logo.svg", class: "logo" }
                        //     }
                        //     div { class: "page-heading-group",
                        //         h1 { class: "page-heading", "Blitz" }
                        //         h2 { class: "page-subheading", "A radically modular web engine" }
                        //     }
                        // }
                        nav {
                            a { href: "/about", "About" }
                            a { href: "/status", "Status" }
                            a {
                                class: "nav-icon",
                                alt: "Github",
                                title: "Github",
                                href: "https://github.com/dioxuslabs/blitz",
                                img { src: "/static/github.svg" }
                            }
                            a {
                                class: "nav-icon",
                                target: "_blank",
                                alt: "Discord",
                                title: "Discord chat",
                                href: "https://discord.gg/AnNPqT95pu",
                                img { src: "/static/discord.svg" }
                            }
                        }
                    }
                    div {
                        id: "main-container",
                        class: if noframe { "main-container--noframe" },
                        {children}
                    }
                    {footer}
                }
            }
        }
    }
}

#[component]
pub fn LeftSidebar(toc_sections: Vec<TocSection>) -> Element {
    rsx! {
        div { id: "left-sidebar",
            TableOfContents { sections: toc_sections }
        }
    }
}

#[component]
pub fn MainContent(
    #[props(default)] prose: bool,
    toc_sections: Option<Vec<TocSection>>,
    children: Element,
) -> Element {
    rsx! {
        div { class: "hflex",
            if let Some(toc_sections) = toc_sections {
                LeftSidebar { toc_sections }
            }
            div {
                id: "content",
                style: if prose { "max-width: 800px;max-width: min(800px, 100%)" },
                {children}
            }
        }
    }
}
