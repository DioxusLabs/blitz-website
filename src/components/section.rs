use std::borrow::Cow;

use dioxus::prelude::*;
use dioxus_html_macro::html;
use string_cache::DefaultAtom;

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum SectionLevel {
    H2,
    H3,
    H4,
    H5,
    H6,
}

#[component]
pub fn AnchorHeader(level: SectionLevel, target: &'static str, children: Element) -> Element {
    rsx!({
        match level {
            SectionLevel::H2 => html!(<h2><a style="color: inherit" href="#{target}" id="{target}">{children}</a></h2>),
            SectionLevel::H3 => html!(<h3><a style="color: inherit" href="#{target}" id="{target}">{children}</a></h3>),
            SectionLevel::H4 => html!(<h4><a style="color: inherit" href="#{target}" id="{target}">{children}</a></h4>),
            SectionLevel::H5 => html!(<h5><a style="color: inherit" href="#{target}" id="{target}">{children}</a></h5>),
            SectionLevel::H6 => html!(<h6><a style="color: inherit" href="#{target}" id="{target}">{children}</a></h6>),
        }
    })
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
                (Some(_key), Some(_subkey)) => "section-{ _key }-subsection-{ _subkey }",
                (Some(_key), None) => "section-{ _key }",
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
