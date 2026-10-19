//! Europe PMC: biomedical and life-sciences literature (port of `europepmc.py`).

use serde_json::{Map, Value};

use super::{HttpSource, parse_json_body};
use re_model::records::QualityHints;

use crate::base::{
    RecordInput, build_record, citations, coerce_year, get, get_obj, items, opt_text, py_str,
    relevance_score, text, truthy, urlencode,
};
use crate::result::RetrievalResult;

/// Production endpoint.
pub const ENDPOINT: &str = "https://www.ebi.ac.uk/europepmc/webservices/rest/search";
const NAME: &str = "europepmc";

/// The Europe PMC REST search.
#[derive(Debug, Clone)]
pub struct EuropePmc {
    endpoint: String,
}

impl Default for EuropePmc {
    fn default() -> Self {
        Self::new()
    }
}

impl EuropePmc {
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
    items(get(get_obj(payload, "resultList"), "result"))
        .enumerate()
        .map(|(i, item)| {
            let position = i + 1;
            let journal = text(get(
                get_obj(get_obj(item, "journalInfo"), "journal"),
                "title",
            ));
            let author_string = text(get(item, "authorString"));
            let authors = author_string
                .trim_end_matches('.')
                .split(',')
                .map(str::trim)
                .filter(|a| !a.is_empty())
                .map(str::to_string)
                .collect();
            let (source, id) = (get(item, "source"), get(item, "id"));
            let url = (truthy(source) && truthy(id)).then(|| {
                format!(
                    "https://europepmc.org/abstract/{}/{}",
                    py_str(source),
                    py_str(id)
                )
            });
            let cited = get(item, "citedByCount");
            let mut metadata = Map::new();
            metadata.insert("cited_by".into(), cited.clone());
            metadata.insert("pmid".into(), get(item, "pmid").clone());
            let mut record = build_record(RecordInput {
                title: text(get(item, "title")),
                authors,
                year: coerce_year(get(item, "pubYear")),
                venue: journal,
                doi: get(item, "doi").clone(),
                url,
                score: relevance_score(position, limit, citations(cited)),
                snippet: text(get(item, "abstractText")),
                metadata,
                ..RecordInput::new(run_id, NAME, query, position)
            });
            let pub_types = get(get_obj(item, "pubTypeList"), "pubType");
            let publication_types: Vec<String> = match pub_types {
                Value::Array(types) => types.iter().filter_map(opt_text).collect(),
                other => opt_text(other).into_iter().collect(),
            };
            let retracted = publication_types
                .iter()
                .any(|t| t.to_ascii_lowercase().contains("retracted publication"));
            record.hints = QualityHints {
                publication_types,
                venue_type: None,
                retracted: Some(retracted),
                open_access: opt_text(get(item, "isOpenAccess")).map(|v| v == "Y"),
                pmcid: opt_text(get(item, "pmcid")),
                pdf_url: None,
            };
            record
        })
        .collect()
}

impl HttpSource for EuropePmc {
    fn name(&self) -> &'static str {
        NAME
    }

    fn build_url(&self, query: &str, limit: usize) -> String {
        let params = [
            ("query", query.to_string()),
            ("format", "json".to_string()),
            ("pageSize", limit.to_string()),
            ("resultType", "core".to_string()),
        ];
        format!("{}?{}", self.endpoint, urlencode(&params))
    }

    fn parse_body(&self, run_id: &str, query: &str, limit: usize, body: &[u8]) -> RetrievalResult {
        parse_json_body(NAME, body, |v| parse(run_id, query, limit, v))
    }
}
