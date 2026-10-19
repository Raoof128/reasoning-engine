//! HTTP plumbing shared by every adapter (port of `base._request`,
//! `fetch_json` and `fetch_text`).

use std::sync::Arc;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::{ACCEPT, HeaderMap, RETRY_AFTER};
use serde_json::Value;

use crate::base::user_agent;
use crate::error::AdapterError;
use crate::tape::{Tape, TapeMode};

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

/// A pooled HTTP client with the engine's `User-Agent`, optionally
/// recording to or replaying from a [`Tape`].
#[derive(Debug, Clone)]
pub struct HttpClient {
    inner: reqwest::Client,
    tape: Option<Arc<Tape>>,
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
        // Never follow a redirect into loopback or private address space:
        // full-text URLs come from provider metadata (plan Phase 9).
        let redirects = reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 10 {
                attempt.error("too many redirects")
            } else if is_public_url(attempt.url()) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        });
        let inner = reqwest::Client::builder()
            .user_agent(user_agent())
            .timeout(timeout)
            .redirect(redirects)
            .build()
            .map_err(|e| AdapterError::new("adapter_misconfigured", e.to_string(), false))?;
        Ok(Self { inner, tape: None })
    }

    /// Record to, or replay from, `tape`.
    #[must_use]
    pub fn with_tape(mut self, tape: Arc<Tape>) -> Self {
        self.tape = Some(tape);
        self
    }

    /// Whether requests go to or come from a [`Tape`].
    #[must_use]
    pub fn has_tape(&self) -> bool {
        self.tape.is_some()
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
        match &self.tape {
            Some(tape) if tape.mode() == TapeMode::Replay => tape.replay(url),
            Some(tape) => {
                let result = self.fetch(url, headers).await;
                tape.record(url, &result);
                result
            }
            None => self.fetch(url, headers).await,
        }
    }

    /// GET a document such as a paper's full text, with `Accept: accept`,
    /// refusing bodies over `max_bytes` (`unavailable`, not retryable).
    /// Records and replays like [`Self::get`].
    ///
    /// # Errors
    /// The mapped [`AdapterError`].
    pub async fn get_document(
        &self,
        url: &str,
        accept: &str,
        max_bytes: usize,
    ) -> Result<Fetched, AdapterError> {
        let fetch = || self.fetch_with(url, accept, &[], Some(max_bytes));
        match &self.tape {
            Some(tape) if tape.mode() == TapeMode::Replay => tape.replay(url),
            Some(tape) => {
                let result = fetch().await;
                tape.record(url, &result);
                result
            }
            None => fetch().await,
        }
    }

    async fn fetch(&self, url: &str, headers: &[(&str, String)]) -> Result<Fetched, AdapterError> {
        self.fetch_with(url, "application/json", headers, None)
            .await
    }

    async fn fetch_with(
        &self,
        url: &str,
        accept: &str,
        headers: &[(&str, String)],
        max_bytes: Option<usize>,
    ) -> Result<Fetched, AdapterError> {
        let unavailable =
            |e: reqwest::Error| AdapterError::new("unavailable", network_message(&e), true);
        let mut request = self.inner.get(url).header(ACCEPT, accept);
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        let mut response = request.send().await.map_err(unavailable)?;
        let status = response.status();
        let response_headers = response.headers().clone();
        if !status.is_success() {
            return Err(map_status(status, &response_headers));
        }
        let Some(max) = max_bytes else {
            let body = response.bytes().await.map_err(unavailable)?;
            return Ok(Fetched {
                body: body.to_vec(),
                headers: response_headers,
            });
        };
        let too_large = || {
            AdapterError::new(
                "unavailable",
                format!("response is larger than {} MB", max / (1024 * 1024)),
                false,
            )
        };
        if response
            .content_length()
            .is_some_and(|length| length > max as u64)
        {
            return Err(too_large());
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(unavailable)? {
            if body.len() + chunk.len() > max {
                return Err(too_large());
            }
            body.extend_from_slice(&chunk);
        }
        Ok(Fetched {
            body,
            headers: response_headers,
        })
    }
}

/// Whether `url` is `http(s)` to a host that is not loopback, private,
/// link-local, shared, unspecified or a local-only name. A hostname that
/// resolves to a private address is not caught here.
#[must_use]
pub fn is_public_url(url: &reqwest::Url) -> bool {
    use std::net::{IpAddr, Ipv4Addr};
    fn public_v4(ip: Ipv4Addr) -> bool {
        let [a, b, ..] = ip.octets();
        !(ip.is_loopback()
            || ip.is_private()
            || ip.is_link_local()
            || ip.is_unspecified()
            || ip.is_broadcast()
            || ip.is_documentation()
            || a == 0
            || (a == 100 && (64..128).contains(&b)))
    }
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => public_v4(ip),
        Ok(IpAddr::V6(ip)) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                public_v4(v4)
            } else {
                let first = ip.segments()[0];
                !(ip.is_loopback()
                    || ip.is_unspecified()
                    || (first & 0xfe00) == 0xfc00
                    || (first & 0xffc0) == 0xfe80)
            }
        }
        Err(_) => {
            let domain = host.trim_end_matches('.').to_ascii_lowercase();
            !(domain == "localhost"
                || [".localhost", ".local", ".internal", ".home.arpa"]
                    .iter()
                    .any(|suffix| domain.ends_with(suffix)))
        }
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

#[cfg(test)]
mod public_url_tests {
    use super::is_public_url;

    #[test]
    fn internal_hosts_are_refused() {
        for (url, public) in [
            ("https://arxiv.org/pdf/2504.00406", true),
            ("http://93.184.216.34/paper.pdf", true),
            ("https://[2606:4700::1111]/x.pdf", true),
            ("http://127.0.0.1:8080/x", false),
            ("http://10.0.0.5/x", false),
            ("http://169.254.169.254/latest/meta-data/", false),
            ("http://100.64.0.1/x", false),
            ("http://0.0.0.0/x", false),
            ("http://[::1]/x", false),
            ("http://[::ffff:192.168.1.1]/x", false),
            ("http://[fd00::1]/x", false),
            ("http://localhost/x", false),
            ("http://printer.local/x", false),
            ("http://metadata.google.internal/x", false),
            ("file:///etc/passwd", false),
        ] {
            let parsed = reqwest::Url::parse(url).unwrap();
            assert_eq!(is_public_url(&parsed), public, "{url}");
        }
    }
}
