//! Citation and authorship links from `OpenAlex` (`graph` feature, plan
//! Phase 10).
//!
//! [`LinkFetcher::fetch`] looks up works by DOI, [`BATCH`] per request
//! (`filter=doi:a|b|...`), and keeps each work's `OpenAlex` ID, the works it
//! references and its authors' `OpenAlex` IDs: what the evidence graph needs
//! to tell corroborating sources from one source echoed.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::base::urlencode;
use crate::client::HttpClient;
use crate::error::AdapterError;
use crate::sources::SourceConfig;

/// DOIs per request (`OpenAlex` allows up to 100 values in an OR filter).
pub const BATCH: usize = 50;
/// The `OpenAlex` works endpoint.
pub const ENDPOINT: &str = "https://api.openalex.org/works";

/// One work's links.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkLinks {
    /// Lower-case DOI without a resolver prefix.
    pub doi: String,
    /// `OpenAlex` work ID (`W...`).
    pub id: String,
    /// IDs of the works it cites.
    pub references: Vec<String>,
    /// `OpenAlex` author IDs (`A...`).
    pub authors: Vec<String>,
}

/// Lower-case a DOI and strip `https://doi.org/` and `doi:` prefixes.
#[must_use]
pub fn normalise_doi(doi: &str) -> String {
    let doi = doi.trim().to_lowercase();
    let doi = doi
        .strip_prefix("https://doi.org/")
        .or_else(|| doi.strip_prefix("http://doi.org/"))
        .or_else(|| doi.strip_prefix("doi:"))
        .unwrap_or(&doi);
    doi.to_string()
}

/// `https://openalex.org/W123` to `W123`.
fn short_id(value: &Value) -> Option<String> {
    let id = value.as_str()?;
    let id = id.rsplit('/').next().unwrap_or(id);
    (!id.is_empty()).then(|| id.to_string())
}

/// Parse one page of `/works` results.
///
/// # Errors
/// `malformed_response` for a body that is not the expected JSON.
pub fn parse(body: &[u8]) -> Result<Vec<WorkLinks>, AdapterError> {
    let malformed = |detail: &str| {
        AdapterError::new(
            "malformed_response",
            format!("unreadable OpenAlex works: {detail}"),
            false,
        )
    };
    let json: Value = serde_json::from_slice(body).map_err(|e| malformed(&e.to_string()))?;
    let results = json
        .get("results")
        .and_then(Value::as_array)
        .ok_or_else(|| malformed("no results"))?;
    Ok(results
        .iter()
        .filter_map(|work| {
            let doi = work.get("doi").and_then(Value::as_str).map(normalise_doi)?;
            let id = work.get("id").and_then(short_id)?;
            let references = work
                .get("referenced_works")
                .and_then(Value::as_array)
                .map(|refs| refs.iter().filter_map(short_id).collect())
                .unwrap_or_default();
            let authors = work
                .get("authorships")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(|a| a.get("author").and_then(|a| a.get("id")))
                        .filter_map(short_id)
                        .collect()
                })
                .unwrap_or_default();
            Some(WorkLinks {
                doi,
                id,
                references,
                authors,
            })
        })
        .collect())
}

/// Fetches [`WorkLinks`] by DOI.
#[derive(Debug, Clone)]
pub struct LinkFetcher {
    client: HttpClient,
    endpoint: String,
    contact_email: String,
    api_key: Option<String>,
}

impl LinkFetcher {
    /// A fetcher over `client`, with `config`'s contact address and key.
    #[must_use]
    pub fn new(client: HttpClient, config: &SourceConfig) -> Self {
        Self {
            client,
            endpoint: ENDPOINT.into(),
            contact_email: config.contact_email.clone(),
            api_key: config.openalex_api_key.clone(),
        }
    }

    /// Use another endpoint (tests).
    #[must_use]
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    /// Whether requests go to or come from a tape (then callers must not
    /// answer from their own cache).
    #[must_use]
    pub fn has_tape(&self) -> bool {
        self.client.has_tape()
    }

    /// The request URL for one batch of DOIs.
    #[must_use]
    pub fn build_url(&self, dois: &[String]) -> String {
        let mut params = vec![
            ("filter", format!("doi:{}", dois.join("|"))),
            ("select", "id,doi,referenced_works,authorships".to_string()),
            ("per_page", dois.len().max(1).to_string()),
            ("mailto", self.contact_email.clone()),
        ];
        if let Some(key) = &self.api_key {
            params.push(("api_key", key.clone()));
        }
        format!("{}?{}", self.endpoint, urlencode(&params))
    }

    /// Links for `dois` (normalised and de-duplicated, in order); works
    /// `OpenAlex` does not know are simply absent.
    ///
    /// # Errors
    /// The first failing request.
    pub async fn fetch(&self, dois: &[String]) -> Result<Vec<WorkLinks>, AdapterError> {
        let mut unique: Vec<String> = Vec::new();
        for doi in dois.iter().map(|d| normalise_doi(d)) {
            if !doi.is_empty() && !unique.contains(&doi) {
                unique.push(doi);
            }
        }
        let mut links = Vec::new();
        for batch in unique.chunks(BATCH) {
            let fetched = self.client.get(&self.build_url(batch), &[]).await?;
            links.extend(parse(&fetched.body)?);
        }
        Ok(links)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn works_parse_to_short_ids() {
        let body = br#"{"results": [
            {"id": "https://openalex.org/W1", "doi": "https://doi.org/10.1/ABC",
             "referenced_works": ["https://openalex.org/W2", "https://openalex.org/W3"],
             "authorships": [{"author": {"id": "https://openalex.org/A9"}}, {"author": {}}]},
            {"id": "https://openalex.org/W4", "doi": null}
        ]}"#;
        assert_eq!(
            parse(body).unwrap(),
            [WorkLinks {
                doi: "10.1/abc".into(),
                id: "W1".into(),
                references: vec!["W2".into(), "W3".into()],
                authors: vec!["A9".into()],
            }]
        );
        assert_eq!(parse(b"[]").unwrap_err().error_type, "malformed_response");
    }

    #[test]
    fn urls_batch_dois_in_one_filter() {
        let fetcher = LinkFetcher::new(HttpClient::new().unwrap(), &SourceConfig::default());
        let url = fetcher.build_url(&["10.1/a".into(), "10.2/b".into()]);
        assert!(
            url.starts_with("https://api.openalex.org/works?filter=doi%3A10.1%2Fa%7C10.2%2Fb&"),
            "{url}"
        );
        assert!(url.contains("per_page=2"));
        assert_eq!(normalise_doi(" https://doi.org/10.1/X "), "10.1/x");
        assert_eq!(normalise_doi("doi:10.1/Y"), "10.1/y");
    }
}
