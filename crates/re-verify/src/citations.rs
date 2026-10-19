//! Reference lists in the profile's citation style (`trust` feature, plan
//! Phase 7), rendered from the run's evidence with `hayagriva` (CSL).
//!
//! | Profile style | CSL style |
//! |---|---|
//! | `harvard_au` | Harvard Cite Them Right (bundled) |
//! | `vancouver` | Vancouver (vendored, patched: see `assets/csl/README.md`) |
//! | `ieee` | IEEE (bundled) |
//! | `chicago` | Chicago author-date (bundled) |
//! | `legal` | AGLC4 (vendored in `assets/csl/`) |
//!
//! The run pack keeps `sources.bib` for compatibility and gains
//! `references.<style>.md`.

use std::fmt::Write as _;

use hayagriva::archive::{ArchivedStyle, locales};
use hayagriva::citationberg::{IndependentStyle, Style};
use hayagriva::{
    BibliographyDriver, BibliographyRequest, BufWriteFormat, CitationItem, CitationRequest,
    io::from_yaml_str,
};
use serde_json::{Map, Value, json};

const AGLC4: &str = include_str!("../assets/csl/australian-guide-to-legal-citation.csl");
const VANCOUVER: &str = include_str!("../assets/csl/vancouver.csl");

/// A loaded CSL style and its display name.
struct Loaded {
    style: IndependentStyle,
    name: &'static str,
}

fn load(citation_style: &str) -> Result<Loaded, String> {
    let (archived, name) = match citation_style {
        "harvard_au" => (
            ArchivedStyle::by_name("harvard-cite-them-right"),
            "Harvard (Cite Them Right)",
        ),

        "ieee" => (ArchivedStyle::by_name("ieee"), "IEEE"),
        "chicago" => (
            ArchivedStyle::by_name("chicago-author-date"),
            "Chicago (author-date)",
        ),
        "legal" => return vendored(AGLC4, "AGLC4"),
        "vancouver" => return vendored(VANCOUVER, "Vancouver"),
        other => return Err(format!("no CSL style for citation_style {other:?}")),
    };
    match archived.map(ArchivedStyle::get) {
        Some(Style::Independent(style)) => Ok(Loaded { style, name }),
        _ => Err(format!("bundled style for {citation_style} is unavailable")),
    }
}

fn vendored(xml: &str, name: &'static str) -> Result<Loaded, String> {
    let style = IndependentStyle::from_xml(xml).map_err(|e| format!("{name}: {e}"))?;
    Ok(Loaded { style, name })
}

/// `"Marcell Szikszai"` to `"Szikszai, Marcell"`; names that already have a
/// comma, or one word, are kept.
fn family_first(name: &str) -> String {
    let name = name.trim();
    if name.contains(',') {
        return name.to_string();
    }
    match name.rsplit_once(' ') {
        Some((given, family)) => format!("{family}, {given}"),
        None => name.to_string(),
    }
}

fn text<'a>(item: &'a Value, key: &str) -> Option<&'a str> {
    item.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// One hayagriva entry (as JSON, which is valid YAML) for an evidence dict.
fn entry(item: &Value) -> Value {
    let mut entry = Map::new();
    entry.insert("type".into(), "article".into());
    entry.insert(
        "title".into(),
        text(item, "title").unwrap_or("Untitled").into(),
    );
    let authors: Vec<Value> = item
        .get("authors")
        .and_then(Value::as_array)
        .map(|names| {
            names
                .iter()
                .filter_map(Value::as_str)
                .map(|n| family_first(n).into())
                .collect()
        })
        .unwrap_or_default();
    if !authors.is_empty() {
        entry.insert("author".into(), Value::Array(authors));
    }
    if let Some(year) = item.get("year").and_then(Value::as_i64) {
        entry.insert("date".into(), year.into());
    }
    if let Some(doi) = text(item, "doi") {
        entry.insert("serial-number".into(), json!({"doi": doi}));
    }
    if let Some(url) = text(item, "url").filter(|u| u.starts_with("http")) {
        entry.insert("url".into(), url.into());
    }
    if let Some(publisher) = text(item, "publisher") {
        entry.insert("publisher".into(), publisher.into());
    }
    if let Some(venue) = text(item, "venue") {
        entry.insert(
            "parent".into(),
            json!({"type": "periodical", "title": venue}),
        );
    }
    Value::Object(entry)
}

/// The file name a style's reference list is written to.
#[must_use]
pub fn file_name(citation_style: &str) -> String {
    format!("references.{citation_style}.md")
}

/// The Markdown reference list for `evidence` (evidence dicts, in ledger
/// order) in `citation_style`.
///
/// # Errors
/// An unknown style, or a style or library that cannot be loaded.
pub fn render_references(evidence: &[Value], citation_style: &str) -> Result<String, String> {
    let Loaded { style, name } = load(citation_style)?;
    let mut library = Map::new();
    for (index, item) in evidence.iter().enumerate() {
        let key = text(item, "evidence_id").map_or_else(|| format!("ref{index}"), str::to_string);
        library.insert(key, entry(item));
    }
    let yaml = serde_json::to_string(&Value::Object(library)).map_err(|e| e.to_string())?;
    let library = from_yaml_str(&yaml).map_err(|e| format!("bibliography: {e}"))?;
    let locales = locales();
    let mut driver = BibliographyDriver::new();
    for entry in library.iter() {
        driver.citation(CitationRequest::from_items(
            vec![CitationItem::with_entry(entry)],
            &style,
            &locales,
        ));
    }
    let rendered = driver.finish(BibliographyRequest {
        style: &style,
        locale: None,
        locale_files: &locales,
    });
    let mut out = format!("# References\n\nStyle: {name}\n\n");
    let items = rendered.bibliography.map(|b| b.items).unwrap_or_default();
    if items.is_empty() {
        out.push_str("No evidence was cited.\n");
    }
    for item in items {
        let mut label = String::new();
        if let Some(first) = &item.first_field {
            first
                .write_buf(&mut label, BufWriteFormat::Plain)
                .map_err(|e| e.to_string())?;
        }
        let mut content = String::new();
        item.content
            .write_buf(&mut content, BufWriteFormat::Plain)
            .map_err(|e| e.to_string())?;
        let (label, content) = (label.trim(), content.trim());
        // `1.` already makes a Markdown list item; `[1]` and unlabelled
        // entries need a bullet to stay one per line.
        if label.ends_with('.') && label[..label.len() - 1].chars().all(|c| c.is_ascii_digit()) {
            let _ = writeln!(out, "{label} {content}");
        } else if label.is_empty() {
            let _ = writeln!(out, "- {content}");
        } else {
            let _ = writeln!(out, "- {label} {content}");
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence() -> Vec<Value> {
        vec![
            json!({
                "evidence_id": "ev_0001",
                "title": "From mathematical reasoning to code: generalization of process reward models in test-time scaling",
                "authors": ["Zhengyu Chen", "Yudong Wang", "Teng Xiao"],
                "year": 2025,
                "venue": "arXiv",
                "doi": "10.48550/arXiv.2506.00027",
                "url": "https://arxiv.org/abs/2506.00027",
            }),
            json!({
                "evidence_id": "ev_0002",
                "title": "Every rollout counts: optimal resource allocation for efficient test-time scaling",
                "authors": ["Wang, Xinglin"],
                "year": 2025,
                "venue": "Advances in Neural Information Processing Systems",
            }),
        ]
    }

    #[test]
    fn names_are_family_first() {
        assert_eq!(family_first("Marcell Szikszai"), "Szikszai, Marcell");
        assert_eq!(family_first("David H. Mathews"), "Mathews, David H.");
        assert_eq!(family_first("Wang, Xinglin"), "Wang, Xinglin");
        assert_eq!(family_first("Plato"), "Plato");
    }

    #[test]
    fn harvard_au_renders_like_this() {
        insta::assert_snapshot!(render_references(&evidence(), "harvard_au").unwrap());
    }

    #[test]
    fn vancouver_renders_like_this() {
        insta::assert_snapshot!(render_references(&evidence(), "vancouver").unwrap());
    }

    #[test]
    fn legal_renders_like_this() {
        insta::assert_snapshot!(render_references(&evidence(), "legal").unwrap());
    }

    #[test]
    fn every_profile_style_loads() {
        for profile in re_core::profiles::PROFILES {
            let rendered = render_references(&evidence(), profile.citation_style);
            assert!(rendered.is_ok(), "{}: {rendered:?}", profile.citation_style);
        }
        assert!(render_references(&evidence(), "mla").is_err());
        assert!(
            render_references(&[], "ieee")
                .unwrap()
                .contains("No evidence was cited.")
        );
    }
}
