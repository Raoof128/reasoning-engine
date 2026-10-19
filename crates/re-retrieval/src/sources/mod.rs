//! Source adapters (port of `verifiable/sources/*.py`).

use std::future::Future;
use std::pin::Pin;

use reqwest::header::HeaderMap;
use serde_json::{Map, Value};

use crate::base::{DEFAULT_CONTACT_EMAIL, contact_email};
use crate::client::{HttpClient, decode_json};
use crate::result::RetrievalResult;

pub mod arxiv;
pub mod crossref;
pub mod dblp;
pub mod doaj;
pub mod europepmc;
pub mod mock;
pub mod openalex;
#[cfg(feature = "scholar-gateway")]
pub mod scholar_gateway;
pub mod semantic_scholar;

/// A boxed, sendable future, so adapters can live behind `dyn`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Anything that can search for evidence.
pub trait SourceAdapter: Send + Sync {
    /// Canonical source name, e.g. `openalex`.
    fn name(&self) -> &'static str;
    /// Search; failures are returned in the result, never raised.
    fn search<'a>(
        &'a self,
        run_id: &'a str,
        query: &'a str,
        limit: usize,
    ) -> BoxFuture<'a, RetrievalResult>;
}

/// A source that fetches one URL and parses the body. Both halves are pure,
/// which is what makes parity testable on recorded responses.
pub trait HttpSource: Send + Sync {
    /// Canonical source name.
    fn name(&self) -> &'static str;
    /// The exact request URL Python would build.
    fn build_url(&self, query: &str, limit: usize) -> String;
    /// Extra request headers.
    fn headers(&self) -> Vec<(&'static str, String)> {
        Vec::new()
    }
    /// Turn a response body into evidence or an error.
    fn parse_body(&self, run_id: &str, query: &str, limit: usize, body: &[u8]) -> RetrievalResult;
    /// Operational data worth keeping from response headers.
    fn diagnostics(&self, _headers: &HeaderMap) -> Map<String, Value> {
        Map::new()
    }
}

/// Settings Python reads from the environment on every call; the port reads
/// them once so adapters stay pure and tests stay independent of `env`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceConfig {
    /// `REASONING_ENGINE_CONTACT_EMAIL`, sent as `mailto`.
    pub contact_email: String,
    /// `S2_API_KEY`, sent as `x-api-key`.
    pub semantic_scholar_api_key: Option<String>,
    /// `OPENALEX_API_KEY`, sent as `api_key` (not in Python; raises limits).
    pub openalex_api_key: Option<String>,
}

impl Default for SourceConfig {
    fn default() -> Self {
        Self {
            contact_email: DEFAULT_CONTACT_EMAIL.to_string(),
            semantic_scholar_api_key: None,
            openalex_api_key: None,
        }
    }
}

impl SourceConfig {
    /// Read the configuration from the environment.
    #[must_use]
    pub fn from_env() -> Self {
        let non_empty = |var: &str| std::env::var(var).ok().filter(|v| !v.is_empty());
        Self {
            contact_email: contact_email(),
            semantic_scholar_api_key: non_empty("S2_API_KEY"),
            openalex_api_key: non_empty("OPENALEX_API_KEY"),
        }
    }
}

/// The free sources, in Python's `FREE_ADAPTERS` order.
pub const FREE_SOURCES: [&str; 7] = [
    "openalex",
    "crossref",
    "arxiv",
    "semantic_scholar",
    "europepmc",
    "doaj",
    "dblp",
];

/// Build a free source by name.
#[must_use]
pub fn free_source(name: &str, config: &SourceConfig) -> Option<Box<dyn HttpSource>> {
    let source: Box<dyn HttpSource> = match name {
        "openalex" => Box::new(openalex::OpenAlex::new(config)),
        "crossref" => Box::new(crossref::Crossref::new(config)),
        "arxiv" => Box::new(arxiv::Arxiv::new()),
        "semantic_scholar" => Box::new(semantic_scholar::SemanticScholar::new(config)),
        "europepmc" => Box::new(europepmc::EuropePmc::new()),
        "doaj" => Box::new(doaj::Doaj::new()),
        "dblp" => Box::new(dblp::Dblp::new()),
        _ => return None,
    };
    Some(source)
}

/// Adapts an [`HttpSource`] into a [`SourceAdapter`] by adding the fetch.
pub struct HttpAdapter {
    source: Box<dyn HttpSource>,
    client: HttpClient,
}

impl HttpAdapter {
    /// Wrap `source`, fetching with `client`.
    #[must_use]
    pub fn new(source: Box<dyn HttpSource>, client: HttpClient) -> Self {
        Self { source, client }
    }
}

impl SourceAdapter for HttpAdapter {
    fn name(&self) -> &'static str {
        self.source.name()
    }

    fn search<'a>(
        &'a self,
        run_id: &'a str,
        query: &'a str,
        limit: usize,
    ) -> BoxFuture<'a, RetrievalResult> {
        Box::pin(async move {
            let url = self.source.build_url(query, limit);
            match self.client.get(&url, &self.source.headers()).await {
                Ok(fetched) => {
                    let mut result = self.source.parse_body(run_id, query, limit, &fetched.body);
                    result.diagnostics = self.source.diagnostics(&fetched.headers);
                    // For the cost ledger (Rust-only, never serialised into evidence).
                    result
                        .diagnostics
                        .insert("bytes".into(), fetched.body.len().into());
                    result
                }
                Err(error) => RetrievalResult::failed(error.to_retrieval_error(self.source.name())),
            }
        })
    }
}

/// Shared `parse_body` for JSON sources: decode, then hand the value over.
pub(crate) fn parse_json_body(
    name: &'static str,
    body: &[u8],
    parse: impl FnOnce(&Value) -> Vec<re_model::records::EvidenceRecord>,
) -> RetrievalResult {
    match decode_json(body) {
        Ok(value) => RetrievalResult::ok(parse(&value)),
        Err(error) => RetrievalResult::failed(error.to_retrieval_error(name)),
    }
}
