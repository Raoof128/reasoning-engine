//! `OpenAlex`: 250M+ works across every field (port of `openalex.py`).

use reqwest::header::HeaderMap;
use serde_json::{Map, Value};

use super::{HttpSource, SourceConfig, parse_json_body};
use re_model::records::QualityHints;

use crate::base::{
    RecordInput, build_record, citations, coerce_year, get, get_obj, items, opt_text, or,
    relevance_score, text, urlencode,
};
use crate::result::RetrievalResult;

/// Production endpoint.
pub const ENDPOINT: &str = "https://api.openalex.org/works";
const NAME: &str = "openalex";

/// The `OpenAlex` works search.
#[derive(Debug, Clone)]
pub struct OpenAlex {
    endpoint: String,
    contact_email: String,
    api_key: Option<String>,
}

impl OpenAlex {
    /// A source using `config`.
    #[must_use]
    pub fn new(config: &SourceConfig) -> Self {
        Self {
            endpoint: ENDPOINT.into(),
            contact_email: config.contact_email.clone(),
            api_key: config.openalex_api_key.clone(),
        }
    }

    /// Point at a different endpoint (tests).
    #[must_use]
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }
}

/// Rebuild an abstract from `OpenAlex`'s inverted index.
fn reconstruct_abstract(index: &Value) -> String {
    let Some(map) = index.as_object().filter(|m| !m.is_empty()) else {
        return String::new();
    };
    let mut positions: Vec<(i64, &str)> = Vec::new();
    for (word, indexes) in map {
        for idx in items(indexes) {
            if let Some(i) = idx.as_i64() {
                positions.push((i, word));
            }
        }
    }
    positions.sort_by_key(|(i, _)| *i); // stable, like Python's sort
    positions
        .iter()
        .map(|(_, w)| *w)
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse(
    run_id: &str,
    query: &str,
    limit: usize,
    payload: &Value,
) -> Vec<re_model::records::EvidenceRecord> {
    items(get(payload, "results"))
        .enumerate()
        .map(|(i, item)| {
            let position = i + 1;
            let location = get_obj(item, "primary_location");
            let source = get_obj(location, "source");
            let authors = items(get(item, "authorships"))
                .map(|a| text(get(get_obj(a, "author"), "display_name")))
                .filter(|a| !a.is_empty())
                .collect();
            let mut metadata = Map::new();
            metadata.insert("openalex_id".into(), get(item, "id").clone());
            metadata.insert("cited_by".into(), get(item, "cited_by_count").clone());
            let mut record = build_record(RecordInput {
                title: text(or(get(item, "title"), get(item, "display_name"))),
                authors,
                year: coerce_year(get(item, "publication_year")),
                publisher: text(get(source, "host_organization_name")),
                venue: text(get(source, "display_name")),
                doi: get(item, "doi").clone(),
                url: opt_text(or(get(location, "landing_page_url"), get(item, "id"))),
                score: relevance_score(position, limit, citations(get(item, "cited_by_count"))),
                snippet: reconstruct_abstract(get(item, "abstract_inverted_index")),
                metadata,
                ..RecordInput::new(run_id, NAME, query, position)
            });
            let open_access = get_obj(item, "open_access");
            let best = get_obj(item, "best_oa_location");
            record.hints = QualityHints {
                publication_types: opt_text(get(item, "type")).into_iter().collect(),
                venue_type: opt_text(get(source, "type")),
                retracted: get(item, "is_retracted").as_bool(),
                open_access: get(open_access, "is_oa").as_bool(),
                pmcid: None,
                pdf_url: opt_text(get(best, "pdf_url")),
            };
            record
        })
        .collect()
}

impl HttpSource for OpenAlex {
    fn name(&self) -> &'static str {
        NAME
    }

    fn build_url(&self, query: &str, limit: usize) -> String {
        let mut params = vec![
            ("search", query.to_string()),
            ("per_page", limit.to_string()),
            ("mailto", self.contact_email.clone()),
        ];
        if let Some(key) = &self.api_key {
            params.push(("api_key", key.clone()));
        }
        format!("{}?{}", self.endpoint, urlencode(&params))
    }

    fn parse_body(&self, run_id: &str, query: &str, limit: usize, body: &[u8]) -> RetrievalResult {
        parse_json_body(NAME, body, |v| parse(run_id, query, limit, v))
    }

    fn diagnostics(&self, headers: &HeaderMap) -> Map<String, Value> {
        let mut out = Map::new();
        if let Some(used) = headers
            .get("x-ratelimit-credits-used")
            .and_then(|v| v.to_str().ok())
        {
            let value = used
                .trim()
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map_or_else(|| Value::String(used.to_string()), Value::Number);
            out.insert("openalex_credits_used".into(), value);
        }
        out
    }
}
