//! Crossref: 150M+ DOI-registered works (port of `crossref.py`).

use serde_json::{Map, Value};

use super::{HttpSource, SourceConfig, parse_json_body};
use crate::base::{
    RecordInput, build_record, citations, coerce_year, get, get_obj, items, opt_text, py_str,
    relevance_score, strip_tags, text, truthy, urlencode,
};
use crate::result::RetrievalResult;

/// Production endpoint.
pub const ENDPOINT: &str = "https://api.crossref.org/works";
const NAME: &str = "crossref";
const SELECT: &str = "title,author,DOI,publisher,container-title,abstract,is-referenced-by-count,URL,issued,published";

/// The Crossref works search.
#[derive(Debug, Clone)]
pub struct Crossref {
    endpoint: String,
    contact_email: String,
}

impl Crossref {
    /// A source using `config`.
    #[must_use]
    pub fn new(config: &SourceConfig) -> Self {
        Self {
            endpoint: ENDPOINT.into(),
            contact_email: config.contact_email.clone(),
        }
    }

    /// Point at a different endpoint (tests).
    #[must_use]
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }
}

/// `str(seq[0])` for a non-empty list, else `""`.
fn first(value: &Value) -> String {
    value
        .as_array()
        .and_then(|a| a.first())
        .map(py_str)
        .unwrap_or_default()
}

/// The first date-parts year among Crossref's date fields.
fn year(item: &Value) -> Option<i64> {
    for key in ["published", "published-print", "published-online", "issued"] {
        let parts = get(get_obj(item, key), "date-parts");
        if let Some(first) = parts
            .as_array()
            .and_then(|p| p.first())
            .and_then(Value::as_array)
            && let Some(y) = first.first()
        {
            return coerce_year(y);
        }
    }
    None
}

fn parse(
    run_id: &str,
    query: &str,
    limit: usize,
    payload: &Value,
) -> Vec<re_model::records::EvidenceRecord> {
    items(get(get_obj(payload, "message"), "items"))
        .enumerate()
        .map(|(i, item)| {
            let position = i + 1;
            let authors = items(get(item, "author"))
                .map(|a| {
                    [get(a, "given"), get(a, "family")]
                        .into_iter()
                        .filter(|part| truthy(part))
                        .map(py_str)
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .filter(|a| !a.is_empty())
                .collect();
            let cited = get(item, "is-referenced-by-count");
            let mut metadata = Map::new();
            metadata.insert("cited_by".into(), cited.clone());
            build_record(RecordInput {
                title: first(get(item, "title")),
                authors,
                year: year(item),
                publisher: text(get(item, "publisher")),
                venue: first(get(item, "container-title")),
                doi: get(item, "DOI").clone(),
                url: opt_text(get(item, "URL")),
                score: relevance_score(position, limit, citations(cited)),
                snippet: strip_tags(&text(get(item, "abstract"))),
                metadata,
                ..RecordInput::new(run_id, NAME, query, position)
            })
        })
        .collect()
}

impl HttpSource for Crossref {
    fn name(&self) -> &'static str {
        NAME
    }

    fn build_url(&self, query: &str, limit: usize) -> String {
        let params = [
            ("query", query.to_string()),
            ("rows", limit.to_string()),
            ("mailto", self.contact_email.clone()),
            ("select", SELECT.to_string()),
        ];
        format!("{}?{}", self.endpoint, urlencode(&params))
    }

    fn parse_body(&self, run_id: &str, query: &str, limit: usize, body: &[u8]) -> RetrievalResult {
        parse_json_body(NAME, body, |v| parse(run_id, query, limit, v))
    }
}
