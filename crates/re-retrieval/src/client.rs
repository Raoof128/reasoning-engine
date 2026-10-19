//! HTTP plumbing shared by every adapter (port of `base._request`,
//! `fetch_json` and `fetch_text`).

use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::{ACCEPT, HeaderMap, RETRY_AFTER};
use serde_json::Value;

use crate::base::user_agent;
use crate::error::AdapterError;

/// Python's default per-request timeout. Tighter deadlines belong in the
/// aggregator's fan-out policy, where they can be measured (plan section 4).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(25);

/// A successful response.
#[derive(Debug, Clone)]
pub struct Fetched {
    /// Raw body bytes.
    pub body: Vec<u8>,
    /// Response headers, for diagnostics such as rate-limit credits.
    pub headers: HeaderMap,
}

/// A pooled HTTP client with the engine's `User-Agent`.
#[derive(Debug, Clone)]
pub struct HttpClient {
    inner: reqwest::Client,
}

impl HttpClient {
    /// Build a client with [`DEFAULT_TIMEOUT`], overridable by
    /// `REASONING_ENGINE_HTTP_TIMEOUT_SECS`.
    ///
    /// # Errors
    /// If the TLS backend cannot initialise.
    pub fn new() -> Result<Self, AdapterError> {
        let timeout = std::env::var("REASONING_ENGINE_HTTP_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .map_or(DEFAULT_TIMEOUT, Duration::from_secs);
        Self::with_timeout(timeout)
    }

    /// Build a client with an explicit timeout.
    ///
    /// # Errors
    /// If the TLS backend cannot initialise.
    pub fn with_timeout(timeout: Duration) -> Result<Self, AdapterError> {
        let inner = reqwest::Client::builder()
            .user_agent(user_agent())
            .timeout(timeout)
            .build()
            .map_err(|e| AdapterError::new("adapter_misconfigured", e.to_string(), false))?;
        Ok(Self { inner })
    }

    /// GET `url` with `Accept: application/json` plus `headers`.
    ///
    /// Any 2xx is success, as with Python's `urllib` (DBLP answers 202).
    /// 401 maps to `auth_required`, 429 to `rate_limited` (with
    /// `retry_after_secs` when the server sends it), other statuses and
    /// network failures to `unavailable`.
    ///
    /// # Errors
    /// The mapped [`AdapterError`].
    pub async fn get(
        &self,
        url: &str,
        headers: &[(&str, String)],
    ) -> Result<Fetched, AdapterError> {
        let mut request = self.inner.get(url).header(ACCEPT, "application/json");
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        let response = request
            .send()
            .await
            .map_err(|e| AdapterError::new("unavailable", network_message(&e), true))?;
        let status = response.status();
        let response_headers = response.headers().clone();
        if !status.is_success() {
            return Err(map_status(status, &response_headers));
        }
        let body = response
            .bytes()
            .await
            .map_err(|e| AdapterError::new("unavailable", network_message(&e), true))?;
        Ok(Fetched {
            body: body.to_vec(),
            headers: response_headers,
        })
    }
}

fn network_message(error: &reqwest::Error) -> String {
    // reqwest's Display includes the URL; keep it, it carries no secrets
    // because API keys travel in headers (OpenAlex's is a query parameter,
    // so strip query strings to be safe).
    let message = error.to_string();
    match message.split_once('?') {
        Some((head, _)) => format!("{head}?…"),
        None => message,
    }
}

fn map_status(status: StatusCode, headers: &HeaderMap) -> AdapterError {
    match status.as_u16() {
        401 => AdapterError::new("auth_required", "HTTP 401 from provider", false),
        429 => {
            let mut error = AdapterError::new("rate_limited", "HTTP 429 from provider", true);
            if let Some(secs) = headers
                .get(RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok())
            {
                error.extra.insert("retry_after_secs".into(), secs.into());
            }
            error
        }
        code => AdapterError::new("unavailable", format!("HTTP {code} from provider"), true),
    }
}

/// `raw.decode("utf-8")`, mapping failure to `malformed_response`.
///
/// # Errors
/// `malformed_response` for invalid UTF-8.
pub fn decode_text(body: &[u8]) -> Result<&str, AdapterError> {
    std::str::from_utf8(body).map_err(|e| {
        AdapterError::new(
            "malformed_response",
            format!("invalid text body: {e}"),
            false,
        )
    })
}

/// `json.loads(raw.decode("utf-8"))`, mapping failure to `malformed_response`.
///
/// # Errors
/// `malformed_response` for invalid UTF-8 or JSON.
pub fn decode_json(body: &[u8]) -> Result<Value, AdapterError> {
    let text = decode_text(body)?;
    serde_json::from_str(text).map_err(|e| {
        AdapterError::new(
            "malformed_response",
            format!("invalid JSON body: {e}"),
            false,
        )
    })
}
