use std::borrow::Cow;

use dioxus::prelude::*;
use dioxus_html_macro::html;
use string_cache::DefaultAtom;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SectionLevel {
    H2,
    H3,
    H4,
    H5,
    H6,
}

#[component]
pub fn Section(
    heading: String,
    description: Option<DefaultAtom>,
    level: Option<SectionLevel>,
    section_key: Option<DefaultAtom>,
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
            id: match (section_key, subsection_key) {
                (Some(key), Some(subkey)) => "section-{ key }-subsection-{ subkey }",
                (Some(key), None) => "section-{ key }",
                _ => "",
            },
            {
                match level {
                    SectionLevel::H2 => html!(< h2 > "{heading}" </ h2 >),
                    SectionLevel::H3 => html!(< h3 > "{heading}" </ h3 >),
                    SectionLevel::H4 => html!(< h4 > "{heading}" </ h4 >),
                    SectionLevel::H5 => html!(< h5 > "{heading}" </ h5 >),
                    SectionLevel::H6 => html!(< h6 > "{heading}" </ h6 >),
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
