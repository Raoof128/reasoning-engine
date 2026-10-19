//! DBLP computer-science bibliography (port of `dblp.py`).
//!
//! DBLP serves an anti-bot challenge page (HTTP 202, HTML) to some networks,
//! including datacenter IPs. Like Python, that surfaces as
//! `malformed_response`; see the `dblp__bot_challenge` fixture.

use serde_json::{Map, Value};

use super::{HttpSource, parse_json_body};
use crate::base::{
    RecordInput, build_record, coerce_year, get, get_obj, opt_text, or, py_str, py_strip,
    relevance_score, text, truthy, urlencode,
};
use crate::result::RetrievalResult;

/// Production endpoint.
pub const ENDPOINT: &str = "https://dblp.org/search/publ/api";
const NAME: &str = "dblp";

/// The DBLP publication search.
#[derive(Debug, Clone)]
pub struct Dblp {
    endpoint: String,
}

impl Default for Dblp {
    fn default() -> Self {
        Self::new()
    }
}

impl Dblp {
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

fn source_type(kind: &Value) -> &'static str {
    match kind.as_str() {
        Some("Conference and Workshop Papers") => "conference_paper",
        Some("Informal and Other Publications") => "preprint",
        _ => "peer_reviewed_article",
    }
}

/// DBLP sends one author as an object and several as a list.
fn authors(info: &Value) -> Vec<String> {
    let author = get(get_obj(info, "authors"), "author");
    let entries: Vec<&Value> = match author {
        Value::Object(_) => vec![author],
        Value::Array(list) => list.iter().collect(),
        _ => Vec::new(),
    };
    entries
        .into_iter()
        .map(|entry| match entry {
            Value::Object(_) => {
                let t = get(entry, "text");
                if t.is_null() {
                    String::new()
                } else {
                    py_strip(&py_str(t)).to_string()
                }
            }
            other if truthy(other) => py_strip(&py_str(other)).to_string(),
            _ => String::new(),
        })
        .filter(|name| !name.is_empty())
        .collect()
}

fn parse(
    run_id: &str,
    query: &str,
    limit: usize,
    payload: &Value,
) -> Vec<re_model::records::EvidenceRecord> {
    let hits = get(get_obj(get_obj(payload, "result"), "hits"), "hit");
    crate::base::items(hits)
        .enumerate()
        .map(|(i, hit)| {
            let position = i + 1;
            let info = get_obj(hit, "info");
            let mut metadata = Map::new();
            metadata.insert("dblp_key".into(), get(info, "key").clone());
            metadata.insert("type".into(), get(info, "type").clone());
            build_record(RecordInput {
                source_type: source_type(get(info, "type")),
                title: text(get(info, "title")),
                authors: authors(info),
                year: coerce_year(get(info, "year")),
                venue: text(get(info, "venue")),
                doi: get(info, "doi").clone(),
                url: opt_text(or(get(info, "ee"), get(info, "url"))),
                score: relevance_score(position, limit, None),
                snippet: text(get(info, "title")),
                metadata,
                ..RecordInput::new(run_id, NAME, query, position)
            })
        })
        .collect()
}

impl HttpSource for Dblp {
    fn name(&self) -> &'static str {
        NAME
    }

    fn build_url(&self, query: &str, limit: usize) -> String {
        let params = [
            ("q", query.to_string()),
            ("format", "json".to_string()),
            ("h", limit.to_string()),
        ];
        format!("{}?{}", self.endpoint, urlencode(&params))
    }

    fn parse_body(&self, run_id: &str, query: &str, limit: usize, body: &[u8]) -> RetrievalResult {
        parse_json_body(NAME, body, |v| parse(run_id, query, limit, v))
    }
}
