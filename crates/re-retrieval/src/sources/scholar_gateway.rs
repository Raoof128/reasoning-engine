//! Scholar Gateway MCP connector: optional and token-gated (port of
//! `scholar_gateway.py`, behind the `scholar-gateway` feature).
//!
//! Without `SCHOLAR_GATEWAY_LIVE=1` it answers from the offline mock, like
//! Python. The access token is read per call and never logged.

use std::time::Duration;

use re_model::clock::utc_now;
use re_model::records::{EvidenceRecord, RetrievalError};
use serde_json::{Map, Value, json};

use super::mock::{MockScholarGateway, NAME};
use super::{BoxFuture, SourceAdapter};
use crate::base::{get, items, opt_text, py_str};
use crate::error::AdapterError;
use crate::result::RetrievalResult;

/// Production MCP endpoint.
pub const SCHOLAR_GATEWAY_MCP_URL: &str = "https://connector.scholargateway.ai/mcp";
const TOKEN_VAR: &str = "SCHOLAR_GATEWAY_ACCESS_TOKEN";

/// The live Scholar Gateway adapter.
#[derive(Debug, Clone)]
pub struct ScholarGateway {
    endpoint: String,
    client: reqwest::Client,
}

fn error(
    error_type: &str,
    message: impl Into<String>,
    retryable: bool,
    metadata: Map<String, Value>,
) -> RetrievalResult {
    RetrievalResult::failed(RetrievalError {
        error_type: error_type.into(),
        message: message.into(),
        retryable,
        metadata,
    })
}

/// `item.get(key, default)`: the default only when the key is absent.
fn get_or<'a>(item: &'a Value, key: &str, default: &'a Value) -> &'a Value {
    item.as_object().and_then(|m| m.get(key)).unwrap_or(default)
}

/// Python's `float(value)` on a JSON value.
fn py_float(value: &Value) -> Result<f64, String> {
    match value {
        Value::Number(n) => n
            .as_f64()
            .ok_or_else(|| format!("could not convert {n} to float")),
        Value::Bool(b) => Ok(f64::from(u8::from(*b))),
        Value::String(s) => s
            .trim()
            .parse()
            .map_err(|_| format!("could not convert string to float: '{s}'")),
        other => Err(format!(
            "float() argument must be a string or a real number, not {other}"
        )),
    }
}

/// Map a JSON-RPC response to evidence, like Python's list comprehension.
fn parse(run_id: &str, query: &str, raw: &Value) -> Result<Vec<EvidenceRecord>, String> {
    let empty = Value::Object(Map::new());
    let result = get_or(raw, "result", &empty);
    let content = get_or(result, "content", &Value::Null);
    if !raw.is_object() || !result.is_object() || !(content.is_array() || content.is_null()) {
        return Err("unexpected response shape".into());
    }
    items(content)
        .zip(1i64..)
        .map(|(item, index)| {
            if !item.is_object() {
                return Err("result item is not an object".into());
            }
            let untitled = Value::String("Untitled".into());
            let wiley = Value::String("Wiley".into());
            let blank = Value::String(String::new());
            let zero = json!(0.0);
            let mut metadata = Map::new();
            metadata.insert("raw".into(), item.clone());
            Ok(EvidenceRecord {
                evidence_id: format!("ev_{index:04}"),
                run_id: run_id.to_string(),
                source_adapter: NAME.into(),
                source_type: "peer_reviewed_article".into(),
                title: py_str(get_or(item, "title", &untitled)),
                authors: items(get(item, "authors")).map(py_str).collect(),
                year: get(item, "year").as_i64(),
                publisher: py_str(get_or(item, "publisher", &wiley)),
                venue: py_str(get_or(item, "venue", &blank)),
                doi: opt_text(get(item, "doi")),
                url: opt_text(get(item, "url")),
                retrieved_at: utc_now(),
                query: query.to_string(),
                rank: index,
                score: py_float(get_or(item, "score", &zero))?,
                snippet: py_str(get_or(item, "snippet", &blank)),
                licence_notes: opt_text(get(item, "licence_notes")),
                risk_flags: Vec::new(),
                metadata,
            })
        })
        .collect()
}

impl ScholarGateway {
    /// The production connector.
    ///
    /// # Errors
    /// If the TLS backend cannot initialise.
    pub fn new() -> Result<Self, AdapterError> {
        Self::with_endpoint(SCHOLAR_GATEWAY_MCP_URL)
    }

    /// A connector at `endpoint` (tests).
    ///
    /// # Errors
    /// If the TLS backend cannot initialise.
    pub fn with_endpoint(endpoint: impl Into<String>) -> Result<Self, AdapterError> {
        let client = reqwest::Client::builder()
            .user_agent(crate::base::user_agent())
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| AdapterError::new("adapter_misconfigured", e.to_string(), false))?;
        Ok(Self {
            endpoint: endpoint.into(),
            client,
        })
    }

    /// Live mode is on and a token is present.
    #[must_use]
    pub fn is_configured() -> bool {
        std::env::var("SCHOLAR_GATEWAY_LIVE").is_ok_and(|v| v == "1")
            && std::env::var(TOKEN_VAR).is_ok_and(|v| !v.is_empty())
    }

    async fn search_live(&self, run_id: &str, query: &str, limit: usize) -> RetrievalResult {
        let Some(token) = std::env::var(TOKEN_VAR).ok().filter(|t| !t.is_empty()) else {
            let mut metadata = Map::new();
            metadata.insert("env_var".into(), TOKEN_VAR.into());
            return error(
                "auth_required",
                "Set SCHOLAR_GATEWAY_ACCESS_TOKEN or complete Scholar Gateway OAuth setup.",
                false,
                metadata,
            );
        };
        let payload = json!({
            "jsonrpc": "2.0",
            "id": "semantic_search",
            "method": "tools/call",
            "params": {"name": "semantic_search", "arguments": {"query": query, "limit": limit}},
        });
        let sent = self
            .client
            .post(&self.endpoint)
            .bearer_auth(token)
            .header(reqwest::header::ACCEPT, "application/json")
            .json(&payload)
            .send()
            .await;
        let response = match sent {
            Ok(response) => response,
            Err(e) => return error("unavailable", e.without_url().to_string(), true, Map::new()),
        };
        let status = response.status().as_u16();
        if !response.status().is_success() {
            let (error_type, retryable) = match status {
                401 => ("auth_required", false),
                429 => ("rate_limited", true),
                _ => ("unavailable", true),
            };
            return error(
                error_type,
                format!("Scholar Gateway HTTP {status}"),
                retryable,
                Map::new(),
            );
        }
        let raw: Value = match response.bytes().await.map(|b| serde_json::from_slice(&b)) {
            Ok(Ok(raw)) => raw,
            Ok(Err(e)) => return error("unavailable", e.to_string(), true, Map::new()),
            Err(e) => return error("unavailable", e.without_url().to_string(), true, Map::new()),
        };
        match parse(run_id, query, &raw) {
            Ok(evidence) if evidence.is_empty() => error(
                "empty_result",
                "Scholar Gateway returned no results.",
                false,
                Map::new(),
            ),
            Ok(evidence) => RetrievalResult::ok(evidence),
            Err(message) => error("malformed_response", message, false, Map::new()),
        }
    }
}

impl SourceAdapter for ScholarGateway {
    fn name(&self) -> &'static str {
        NAME
    }

    fn search<'a>(
        &'a self,
        run_id: &'a str,
        query: &'a str,
        limit: usize,
    ) -> BoxFuture<'a, RetrievalResult> {
        Box::pin(async move {
            if std::env::var("SCHOLAR_GATEWAY_LIVE").is_ok_and(|v| v == "1") {
                self.search_live(run_id, query, limit).await
            } else {
                MockScholarGateway::search_now(run_id, query, limit)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_follows_python_defaults() {
        let raw = json!({"result": {"content": [
            {"title": "T", "authors": ["A"], "year": 2024, "doi": "10.1/x", "score": "0.5"},
            {"title": null, "score": 1}
        ]}});
        let records = parse("run", "q", &raw).expect("parses");
        assert_eq!(records[0].publisher, "Wiley");
        assert_eq!(records[0].score, 0.5);
        assert_eq!(records[1].title, "None");
        assert_eq!(records[1].evidence_id, "ev_0002");
    }

    #[test]
    fn bad_scores_are_malformed() {
        let raw = json!({"result": {"content": [{"score": null}]}});
        assert!(parse("run", "q", &raw).is_err());
    }
}
