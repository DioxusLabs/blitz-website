use std::collections::HashMap;

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use string_cache::DefaultAtom;

use crate::components::{Page, Section};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropPopularity {
    pub property_name: DefaultAtom,
    pub day_percentage: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropGroup {
    pub id: DefaultAtom,
    pub name: DefaultAtom,
    pub entries: Vec<PropEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropEntry {
    pub name: DefaultAtom,
    pub status: Option<PropStatus>,
    pub notes: Option<String>,
    #[serde(default)]
    pub percentage: f64,
    pub properties: Option<Vec<DefaultAtom>>,
    pub values: Option<Vec<PropValue>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PropStatus {
    Yes,
    No,
    Partial,
}

impl PropStatus {
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Yes => "✅ Supported",
            Self::No => "❌ Not supported",
            Self::Partial => "⚠️ Partial support",
        }
    }
}

impl PropStatus {
    pub fn class(&self) -> &'static str {
        match self {
            Self::Yes => "css-prop--supported",
            Self::No => "css-prop--not-supported",
            Self::Partial => "css-prop--partial-support",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropValue {
    pub value: DefaultAtom,
    pub status: PropStatus,
    pub notes: Option<String>,
}

static CSS_PROPERTIES: GlobalSignal<Vec<PropGroup>> = Signal::global(|| {
    // Load a hashmap of popularity data
    let raw_css_popularity: &str = include_str!("../../data/css-popularity.json");
    let css_popularity: Vec<PropPopularity> = serde_json5::from_str(&raw_css_popularity).unwrap();
    let css_popularity: HashMap<DefaultAtom, f64> = css_popularity
        .into_iter()
        .map(|prop| (prop.property_name, prop.day_percentage))
        .collect();

    // Load crate data
    let raw_css_prop_groups: &str = include_str!("../../data/css-property-groups.json");
    let mut css_prop_groups: Vec<PropGroup> = serde_json5::from_str(&raw_css_prop_groups).unwrap();

    // Fill in percentages for each entry
    for group in &mut css_prop_groups {
        for entry in &mut group.entries {
            entry.percentage = match &entry.properties {
                Some(props) => *props
                    .iter()
                    .filter_map(|prop_name| css_popularity.get(prop_name))
                    .max_by(|a, b| a.total_cmp(&b))
                    .unwrap_or(&0.0),
                None => *css_popularity.get(&entry.name).unwrap_or(&0.0),
            }
        }
    }

    css_prop_groups
});

#[component]
pub fn CssSupportPage() -> Element {
    rsx! {
        Page { title: "Supported CSS Properties".into(),
            h1 { "Supported CSS Properties" }
            p {
                class: "introduction",
                dangerous_inner_html: r#"
                    This page documents which CSS properties (and for some properties, which values are supported for that property).
                    Properties are grouped into logical feature grouping, and  within each group they are roughly ordered by the percentage of web pages 
                    that use that property.
                "#,
            }
            p {
                class: "introduction",
                dangerous_inner_html: r#"
                    You can generally assume that if the longhand versions of a property are supported then the shorthand version will also be supported and vice-versa.
                "#,
            }
            for group in CSS_PROPERTIES() {

                Section { section_key: group.id.clone(), heading: group.name,
                    // description: group.notes,
                    SupportTable { entries: group.entries }
                }
            }
        }
    }
}

#[component]
pub fn SupportTable(entries: Vec<PropEntry>) -> Element {
    rsx! {

        table {
            class: "full-width fixed-layout",
            style: "background: transparent",
            thead {
                tr {
                    th { style: "color: #666;text-align: right;width: 60px", "% use" }
                    th { style: "color: #666", class: "use-case-column", "Property" }
                    th { style: "color: #666", "Status" }
                }
            }
            tbody { style: "background: transparent",
                for entry in entries {
                    SupportTableRow { entry }
                }
            }
        }
    }
}

#[component]
fn SupportTableRow(entry: PropEntry) -> Element {
    rsx! {
        tr { class: if entry.values.is_none() { entry.status.map(|status| status.class()).unwrap_or("") } else { "css-prop--split-by-value" },
            td { style: "vertical-align: top;color: #666;text-align: right",
                {format!("{:.0}%", entry.percentage * 100.0)}
            }
            td { style: "vertical-align: top;font-weight: bold;text-wrap: nowrap",
                {entry.name.clone()}
            }
            td {
                if let Some(status) = entry.status {
                    {status.icon()}
                }
                if let Some(values) = &entry.values {
                    table { style: "border: 0;width: 100%;background: transparent",
                        tbody { style: "background: transparent",
                            for value in values {
                                PropValueItem {
                                    prop: entry.name.clone(),
                                    value: value.clone(),
                                }
                            }
                        }
                    }
                }
                if let Some(notes) = entry.notes {
                    p {
                        style: "margin: 0;color: #333;font-size: 0.8em",
                        style: if entry.values.is_some() { "margin: 0;color: #333;font-size: 0.8em;margin-top: 6px;" },
                        dangerous_inner_html: notes,
                    }
                }
            }
        }
    }
}

#[component]
fn PropValueItem(prop: DefaultAtom, value: PropValue) -> Element {
    rsx!(
        tr { class: value.status.class(),
            td { style: "vertical-align:top;border: 0;width: 40%", "{prop}:{value.value}" }
            td { style: "border: 0;",
                {value.status.icon()}
                if let Some(notes) = value.notes {
                    p {
                        style: "margin: 2px 0;color: #333;font-size: 0.8em",
                        dangerous_inner_html: notes,
                    }
                }
            }
        }
    )
}
