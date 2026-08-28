//! Human-readable spec titles and links for WPT test directories, generated
//! by scripts/update-wpt-spec-meta.py from the WPT repo's META.yml files and
//! the w3c/browser-specs registry.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;

#[derive(Deserialize)]
pub struct SpecMeta {
    pub spec: String,
    pub title: Option<String>,
}

static SPEC_META: LazyLock<HashMap<String, SpecMeta>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../data/wpt-spec-meta.json"))
        .expect("invalid data/wpt-spec-meta.json")
});

/// Look up spec metadata for an area path (e.g. "css/css-grid"), falling back
/// to the deepest ancestor directory that has an entry.
pub fn lookup(area: &str) -> Option<&'static SpecMeta> {
    let mut path = area.trim_matches('/');
    loop {
        if let Some(meta) = SPEC_META.get(path) {
            return Some(meta);
        }
        path = path.rsplit_once('/')?.0;
    }
}
