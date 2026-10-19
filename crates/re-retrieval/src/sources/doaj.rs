//! DOAJ: Directory of Open Access Journals articles (port of `doaj.py`).

use serde_json::{Map, Value};

use super::{HttpSource, parse_json_body};
use crate::base::{
    RecordInput, build_record, coerce_year, get, get_obj, items, opt_text, quote, relevance_score,
    text, truthy,
};
use crate::result::RetrievalResult;

/// Production endpoint.
pub const ENDPOINT: &str = "https://doaj.org/api/search/articles";
const NAME: &str = "doaj";

/// The DOAJ article search.
#[derive(Debug, Clone)]
pub struct Doaj {
    endpoint: String,
}

impl Default for Doaj {
    fn default() -> Self {
        Self::new()
    }
}

impl Doaj {
    /// The production source.
    #[must_use]
    pub fn new() -> Self {
        Self {
            endpoint: ENDPOINT.into(),
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
    items(get(payload, "results"))
        .enumerate()
        .map(|(i, item)| {
            let position = i + 1;
            let bibjson = get_obj(item, "bibjson");
            let journal = get_obj(bibjson, "journal");
            let authors = items(get(bibjson, "author"))
                .map(|a| text(get(a, "name")))
                .filter(|a| !a.is_empty())
                .collect();
            // Last DOI identifier wins; first link with a URL wins, as in Python.
            let doi = items(get(bibjson, "identifier"))
                .filter(|id| text(get(id, "type")).to_lowercase() == "doi")
                .map(|id| get(id, "id").clone())
                .last()
                .unwrap_or(Value::Null);
            let url = items(get(bibjson, "link"))
                .map(|link| get(link, "url"))
                .find(|u| truthy(u))
                .and_then(opt_text);
            let mut metadata = Map::new();
            metadata.insert("doaj_id".into(), get(item, "id").clone());
            build_record(RecordInput {
                title: text(get(bibjson, "title")),
                authors,
                year: coerce_year(get(bibjson, "year")),
                publisher: text(get(journal, "publisher")),
                venue: text(get(journal, "title")),
                doi,
                url,
                score: relevance_score(position, limit, None),
                snippet: text(get(bibjson, "abstract")),
                licence_notes: Some("Open access (DOAJ).".into()),
                metadata,
                ..RecordInput::new(run_id, NAME, query, position)
            })
        })
        .collect()
}

impl HttpSource for Doaj {
    fn name(&self) -> &'static str {
        NAME
    }

    fn build_url(&self, query: &str, limit: usize) -> String {
        format!("{}/{}?pageSize={limit}", self.endpoint, quote(query, ""))
    }

    fn parse_body(&self, run_id: &str, query: &str, limit: usize, body: &[u8]) -> RetrievalResult {
        parse_json_body(NAME, body, |v| parse(run_id, query, limit, v))
    }
}
