use std::borrow::Cow;

use dioxus::prelude::*;
use dioxus_html_macro::html;
use string_cache::DefaultAtom;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SectionLevel {
    H3,
    H4,
}

#[component]
pub fn Section(
    heading: String,
    description: Option<DefaultAtom>,
    level: Option<SectionLevel>,
    section_key: DefaultAtom,
    subsection_key: Option<DefaultAtom>,
    children: Element,
) -> Element {
    let level = level.unwrap_or_else(|| {
        if subsection_key.is_some() {
            SectionLevel::H4
        } else {
            SectionLevel::H3
        }
    });

    rsx! {
        section {
            "data-toc-section": true,
            id: if let Some(subsection_key) = subsection_key { "section-{ section_key }-subsection-{ subsection_key }" } else { "section-{ section_key }" },
            {
                match level {
                    SectionLevel::H3 => html!(< h3 > "{heading}" </ h3 >),
                    SectionLevel::H4 => html!(< h4 > "{heading}" </ h4 >),
                }
            }

            p { class: "group-description",
                if let Some(desc) = description {
                    {desc}
                }
            }
            {children}
        }
    }
}
