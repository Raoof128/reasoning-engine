//! Recorded HTTP for replayable runs (plan Phase 7).
//!
//! A [`Tape`] in record mode keeps every response the [`crate::HttpClient`]
//! sees: status, filtered headers and body, or the mapped error. In replay
//! mode the client answers from the tape and never touches the network; a
//! request that was not recorded fails closed with `replay_miss`.
//!
//! Entries are keyed by `sha256("GET " + url)` after secrets and contact
//! details are removed from the query string (`api_key`, `key`, `mailto`,
//! `email`, anything containing `token`), so a recording made with an API
//! key replays on a machine without one. Response headers pass a deny-list
//! (`authorization`, `cookie`, `set-cookie`, `x-api-key`, anything
//! containing `token`); request headers are never stored. Bodies are
//! scrubbed of the configured secrets (contact address, API keys), because
//! some providers echo the request back: DBLP's JSON carries the caller's
//! `User-Agent` (with its `mailto`) and IP address.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;
use std::sync::{Mutex, PoisonError};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use re_core::hashing::sha256_prefixed;
use re_model::records::RETRIEVAL_ERROR_TYPES;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::client::Fetched;
use crate::error::AdapterError;

/// Query parameters that never reach a recording or its key.
const SECRET_PARAMS: [&str; 5] = ["api_key", "apikey", "key", "mailto", "email"];
/// Response headers that are never recorded.
const SECRET_HEADERS: [&str; 4] = ["authorization", "cookie", "set-cookie", "x-api-key"];

/// Whether a tape records live traffic or serves it back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapeMode {
    /// Fetch live and keep every outcome.
    Record,
    /// Serve recorded outcomes only.
    Replay,
}

/// What one request produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Outcome {
    /// A 2xx response.
    Response {
        /// HTTP status.
        status: u16,
        /// Response headers after the deny-list, lowercase names.
        headers: BTreeMap<String, String>,
        /// The body when it is valid UTF-8.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
        /// The body as base64 otherwise.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body_base64: Option<String>,
    },
    /// The error the client mapped the request to.
    Error {
        /// Retrieval error type.
        error_type: String,
        /// Message (already free of credentials).
        message: String,
        /// Whether a retry might succeed.
        retryable: bool,
        /// Extra context such as `retry_after_secs`.
        #[serde(default)]
        extra: Map<String, Value>,
    },
}

/// One recorded request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recording {
    /// Always `GET`.
    pub method: String,
    /// The URL with secret query parameters removed.
    pub url: String,
    /// What came back.
    pub outcome: Outcome,
}

/// `url` without secret or contact query parameters.
#[must_use]
pub fn redact_url(url: &str) -> String {
    let (base, rest) = url.split_once('?').unwrap_or((url, ""));
    let (query, fragment) = rest.split_once('#').unwrap_or((rest, ""));
    let kept: Vec<&str> = query
        .split('&')
        .filter(|pair| {
            let name = pair
                .split('=')
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            !pair.is_empty() && !SECRET_PARAMS.contains(&name.as_str()) && !name.contains("token")
        })
        .collect();
    let mut out = base.to_string();
    if !kept.is_empty() {
        out.push('?');
        out.push_str(&kept.join("&"));
    }
    if !fragment.is_empty() {
        out.push('#');
        out.push_str(fragment);
    }
    out
}

/// The file stem a request is stored under: hex SHA-256 of the method and
/// redacted URL.
#[must_use]
pub fn request_key(method: &str, url: &str) -> String {
    let hash = sha256_prefixed(format!("{method} {}", redact_url(url)).as_bytes());
    hash.trim_start_matches("sha256:").to_string()
}

fn is_secret_header(name: &str) -> bool {
    SECRET_HEADERS.contains(&name) || name.contains("token")
}

fn static_error_type(name: &str) -> &'static str {
    RETRIEVAL_ERROR_TYPES
        .iter()
        .copied()
        .find(|t| *t == name)
        .unwrap_or("unavailable")
}

impl Outcome {
    fn from_result(result: &Result<Fetched, AdapterError>) -> Self {
        match result {
            Ok(fetched) => {
                let headers = fetched
                    .headers
                    .iter()
                    .filter(|(name, _)| !is_secret_header(name.as_str()))
                    .filter_map(|(name, value)| {
                        Some((name.as_str().to_string(), value.to_str().ok()?.to_string()))
                    })
                    .collect();
                let (body, body_base64) = match std::str::from_utf8(&fetched.body) {
                    Ok(text) => (Some(text.to_string()), None),
                    Err(_) => (None, Some(STANDARD.encode(&fetched.body))),
                };
                Self::Response {
                    status: 200,
                    headers,
                    body,
                    body_base64,
                }
            }
            Err(error) => Self::Error {
                error_type: error.error_type.to_string(),
                message: error.message.clone(),
                retryable: error.retryable,
                extra: error.extra.clone(),
            },
        }
    }

    fn to_result(&self) -> Result<Fetched, AdapterError> {
        match self {
            Self::Response {
                headers,
                body,
                body_base64,
                ..
            } => {
                let body = match (body, body_base64) {
                    (Some(text), _) => text.clone().into_bytes(),
                    (None, Some(encoded)) => STANDARD.decode(encoded).map_err(|e| {
                        AdapterError::new("replay_miss", format!("corrupt recording: {e}"), false)
                    })?,
                    (None, None) => Vec::new(),
                };
                let mut map = HeaderMap::new();
                for (name, value) in headers {
                    if let (Ok(name), Ok(value)) = (
                        HeaderName::try_from(name.as_str()),
                        HeaderValue::from_str(value),
                    ) {
                        map.insert(name, value);
                    }
                }
                Ok(Fetched { body, headers: map })
            }
            Self::Error {
                error_type,
                message,
                retryable,
                extra,
            } => Err(AdapterError {
                error_type: static_error_type(error_type),
                message: message.clone(),
                retryable: *retryable,
                extra: extra.clone(),
            }),
        }
    }
}

/// What a scrubbed secret becomes in a recorded body.
pub const SCRUBBED: &str = "[redacted]";

/// Recorded responses for one run.
#[derive(Debug)]
pub struct Tape {
    mode: TapeMode,
    entries: Mutex<BTreeMap<String, Recording>>,
    scrub: Vec<String>,
}

impl Tape {
    /// An empty tape that records.
    #[must_use]
    pub fn recording() -> Self {
        Self {
            mode: TapeMode::Record,
            entries: Mutex::new(BTreeMap::new()),
            scrub: Vec::new(),
        }
    }

    /// Replace each of `secrets` (ignoring any shorter than 6 characters,
    /// which would mangle ordinary text) with [`SCRUBBED`] in recorded
    /// bodies and messages.
    #[must_use]
    pub fn scrubbing(mut self, secrets: impl IntoIterator<Item = String>) -> Self {
        self.scrub = secrets.into_iter().filter(|s| s.len() >= 6).collect();
        // Longest first, so a secret containing another is replaced whole.
        self.scrub.sort_by_key(|s| std::cmp::Reverse(s.len()));
        self
    }

    fn scrubbed(&self, text: &str) -> String {
        self.scrub.iter().fold(text.to_string(), |acc, secret| {
            acc.replace(secret.as_str(), SCRUBBED)
        })
    }

    /// A replaying tape loaded from `dir/<key>.json`.
    ///
    /// # Errors
    /// A file that cannot be read or parsed. A missing directory is an
    /// empty tape (an offline run makes no requests).
    pub fn replaying(dir: &Path) -> io::Result<Self> {
        let mut entries = BTreeMap::new();
        if dir.is_dir() {
            for entry in fs::read_dir(dir)? {
                let path = entry?.path();
                let Some(key) = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .and_then(|n| n.strip_suffix(".json"))
                else {
                    continue;
                };
                let recording: Recording =
                    serde_json::from_str(&fs::read_to_string(&path)?).map_err(io::Error::other)?;
                entries.insert(key.to_string(), recording);
            }
        }
        Ok(Self {
            mode: TapeMode::Replay,
            entries: Mutex::new(entries),
            scrub: Vec::new(),
        })
    }

    /// Record or replay.
    #[must_use]
    pub fn mode(&self) -> TapeMode {
        self.mode
    }

    fn entries(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Recording>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The recorded outcome for `url`, or `replay_miss`.
    ///
    /// # Errors
    /// The recorded error, or `replay_miss` for an unrecorded request.
    pub fn replay(&self, url: &str) -> Result<Fetched, AdapterError> {
        let key = request_key("GET", url);
        match self.entries().get(&key) {
            Some(recording) => recording.outcome.to_result(),
            None => Err(AdapterError::new(
                "replay_miss",
                format!("no recorded response for GET {}", redact_url(url)),
                false,
            )),
        }
    }

    /// Keep the outcome of a live request (record mode only).
    pub fn record(&self, url: &str, result: &Result<Fetched, AdapterError>) {
        if self.mode != TapeMode::Record {
            return;
        }
        let mut outcome = Outcome::from_result(result);
        match &mut outcome {
            Outcome::Response {
                body: Some(body), ..
            } => *body = self.scrubbed(body),
            Outcome::Error { message, .. } => *message = self.scrubbed(message),
            Outcome::Response { .. } => {}
        }
        let recording = Recording {
            method: "GET".into(),
            url: self.scrubbed(&redact_url(url)),
            outcome,
        };
        self.entries().insert(request_key("GET", url), recording);
    }

    /// Recorded entries by key, in key order.
    #[must_use]
    pub fn snapshot(&self) -> BTreeMap<String, Recording> {
        self.entries().clone()
    }

    /// Every entry as `(<key>.json, indented sorted JSON text)`, in key
    /// order: what a run pack stores under `http/`.
    ///
    /// # Panics
    /// Never: recordings are plain data.
    #[must_use]
    pub fn rendered(&self) -> BTreeMap<String, String> {
        self.snapshot()
            .into_iter()
            .map(|(key, recording)| {
                let value = serde_json::to_value(&recording).expect("recordings serialise");
                let mut text = re_core::pyjson::dumps(
                    &value,
                    &re_core::pyjson::DumpOptions::indented_sorted(2),
                );
                text.push('\n');
                (format!("{key}.json"), text)
            })
            .collect()
    }

    /// Write every entry to `dir/<key>.json`; returns the file names.
    ///
    /// # Errors
    /// Filesystem failures.
    pub fn write_to(&self, dir: &Path) -> io::Result<Vec<String>> {
        let rendered = self.rendered();
        if !rendered.is_empty() {
            fs::create_dir_all(dir)?;
        }
        for (name, text) in &rendered {
            fs::write(dir.join(name), text)?;
        }
        Ok(rendered.into_keys().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_never_reach_the_key_or_the_url() {
        let url = "https://api.openalex.org/works?search=prm&api_key=SECRET&mailto=me%40x.org&per-page=5&access_token=T";
        assert_eq!(
            redact_url(url),
            "https://api.openalex.org/works?search=prm&per-page=5"
        );
        assert_eq!(
            request_key("GET", url),
            request_key(
                "GET",
                "https://api.openalex.org/works?search=prm&per-page=5"
            ),
            "a key recorded with an API key replays without one"
        );
        assert_eq!(redact_url("https://x.org/a"), "https://x.org/a");
        assert_eq!(redact_url("https://x.org/a?key=1"), "https://x.org/a");
        assert_eq!(request_key("GET", "https://x.org/a").len(), 64);
    }

    #[test]
    fn outcomes_round_trip_and_drop_secret_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("set-cookie", HeaderValue::from_static("s=1"));
        headers.insert("x-ratelimit-credits-used", HeaderValue::from_static("3"));
        headers.insert("x-session-token", HeaderValue::from_static("t"));
        let ok: Result<Fetched, AdapterError> = Ok(Fetched {
            body: b"{\"a\": 1}".to_vec(),
            headers,
        });
        let tape = Tape::recording();
        tape.record("https://x.org/a?api_key=K", &ok);
        let mut error = AdapterError::new("rate_limited", "HTTP 429 from provider", true);
        error.extra.insert("retry_after_secs".into(), 7.into());
        tape.record("https://x.org/b", &Err(error.clone()));
        tape.record(
            "https://x.org/bin",
            &Ok(Fetched {
                body: vec![0xff, 0x00],
                headers: HeaderMap::new(),
            }),
        );

        let dir = tempfile::tempdir().expect("tmp");
        let names = tape.write_to(dir.path()).expect("write");
        assert_eq!(names.len(), 3);
        let text: String = names
            .iter()
            .map(|n| fs::read_to_string(dir.path().join(n)).unwrap())
            .collect();
        assert!(
            !text.contains("set-cookie")
                && !text.contains("x-session-token")
                && !text.contains('K')
        );

        let replay = Tape::replaying(dir.path()).expect("load");
        let fetched = replay.replay("https://x.org/a").expect("recorded");
        assert_eq!(fetched.body, b"{\"a\": 1}");
        assert_eq!(fetched.headers["x-ratelimit-credits-used"], "3");
        assert_eq!(replay.replay("https://x.org/b").unwrap_err(), error);
        assert_eq!(
            replay.replay("https://x.org/bin").unwrap().body,
            vec![0xff, 0x00]
        );
        let miss = replay.replay("https://x.org/never").unwrap_err();
        assert_eq!((miss.error_type, miss.retryable), ("replay_miss", false));
        replay.record("https://x.org/c", &ok);
        assert_eq!(replay.snapshot().len(), 3, "a replaying tape never records");
    }

    #[test]
    fn echoed_secrets_are_scrubbed_from_bodies() {
        let tape = Tape::recording().scrubbing(["me@real.example".to_string(), "abc".into()]);
        let body = br#"{"metadata":{"User-Agent":"reasoning-engine/0.2.0 (mailto:me@real.example)"},"hits":"abc"}"#;
        tape.record(
            "https://dblp.org/search?q=x",
            &Ok(Fetched {
                body: body.to_vec(),
                headers: HeaderMap::new(),
            }),
        );
        let text = tape.rendered().into_values().collect::<String>();
        assert!(!text.contains("me@real.example"));
        assert!(text.contains("(mailto:[redacted])"));
        assert!(text.contains("abc"), "short strings are left alone");
    }
}
