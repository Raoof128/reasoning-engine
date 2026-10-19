//! Semantic Scholar paper search (port of `semantic_scholar.py`).

use serde_json::{Map, Value};

use super::{HttpSource, SourceConfig, parse_json_body};
use crate::base::{
    RecordInput, build_record, citations, coerce_year, get, get_obj, items, opt_text,
    relevance_score, text, urlencode,
};
use crate::result::RetrievalResult;

/// Production endpoint.
pub const ENDPOINT: &str = "https://api.semanticscholar.org/graph/v1/paper/search";
const NAME: &str = "semantic_scholar";
const FIELDS: &str =
    "title,abstract,year,authors,venue,externalIds,citationCount,url,publicationTypes";

/// The Semantic Scholar Graph API search.
#[derive(Debug, Clone)]
pub struct SemanticScholar {
    endpoint: String,
    api_key: Option<String>,
}

impl SemanticScholar {
    /// A source using `config`.
    #[must_use]
    pub fn new(config: &SourceConfig) -> Self {
        Self {
            endpoint: ENDPOINT.into(),
            api_key: config.semantic_scholar_api_key.clone(),
        }
    }

    /// Point at a different endpoint (tests).
    #[must_use]
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }
}

fn parse(
    run_id: &str,
    query: &str,
    limit: usize,
    payload: &Value,
) -> Vec<re_model::records::EvidenceRecord> {
    items(get(payload, "data"))
        .enumerate()
        .map(|(i, item)| {
            let position = i + 1;
            let external = get_obj(item, "externalIds");
            let authors = items(get(item, "authors"))
                .map(|a| text(get(a, "name")))
                .filter(|a| !a.is_empty())
                .collect();
            let preprint = items(get(item, "publicationTypes")).any(|t| t == "Preprint");
            let cited = get(item, "citationCount");
            let mut metadata = Map::new();
            metadata.insert("cited_by".into(), cited.clone());
            metadata.insert("external_ids".into(), external.clone());
            build_record(RecordInput {
                source_type: if preprint {
                    "preprint"
                } else {
                    "peer_reviewed_article"
                },
                title: text(get(item, "title")),
                authors,
                year: coerce_year(get(item, "year")),
                venue: text(get(item, "venue")),
                doi: get(external, "DOI").clone(),
                url: opt_text(get(item, "url")),
                score: relevance_score(position, limit, citations(cited)),
                snippet: text(get(item, "abstract")),
                metadata,
                ..RecordInput::new(run_id, NAME, query, position)
            })
        })
        .collect()
}

impl HttpSource for SemanticScholar {
    fn name(&self) -> &'static str {
        NAME
    }

    fn build_url(&self, query: &str, limit: usize) -> String {
        let params = [
            ("query", query.to_string()),
            ("limit", limit.to_string()),
            ("fields", FIELDS.to_string()),
        ];
        format!("{}?{}", self.endpoint, urlencode(&params))
    }

    fn headers(&self) -> Vec<(&'static str, String)> {
        self.api_key
            .iter()
            .map(|k| ("x-api-key", k.clone()))
            .collect()
    }

    fn parse_body(&self, run_id: &str, query: &str, limit: usize, body: &[u8]) -> RetrievalResult {
        parse_json_body(NAME, body, |v| parse(run_id, query, limit, v))
    }
}
