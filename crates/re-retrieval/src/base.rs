//! Shared helpers (port of `verifiable/sources/base.py`) plus the Python
//! value semantics adapters rely on: truthiness, `x or y`, `str(x)`.

use std::fmt::Write as _;
use std::sync::LazyLock;

use re_core::pyjson::python_float_repr;
use re_model::clock::utc_now;
use re_model::records::EvidenceRecord;
use regex::Regex;
use serde_json::{Map, Value};

static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").expect("valid regex"));
static DECIMAL_DIGIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d$").expect("valid regex"));

/// Default contact address for polite-pool `User-Agent` and `mailto`.
pub const DEFAULT_CONTACT_EMAIL: &str = "reasoning-engine@example.org";
/// Snippet length cap, in characters.
pub const SNIPPET_LIMIT: usize = 1500;
/// Title length cap, in characters.
pub const TITLE_LIMIT: usize = 500;

/// `REASONING_ENGINE_CONTACT_EMAIL`, or the default.
#[must_use]
pub fn contact_email() -> String {
    std::env::var("REASONING_ENGINE_CONTACT_EMAIL")
        .unwrap_or_else(|_| DEFAULT_CONTACT_EMAIL.to_string())
}

/// Descriptive `User-Agent` with a contact address, as `OpenAlex` and
/// Crossref ask for.
#[must_use]
pub fn user_agent() -> String {
    format!(
        "reasoning-engine/{} (mailto:{})",
        env!("CARGO_PKG_VERSION"),
        contact_email()
    )
}

/// Python's `round(x, ndigits)` for small `ndigits`: correctly rounded to a
/// decimal string, then parsed back. Rust's `{:.N}` and `CPython` both round
/// the exact binary value half-to-even, so this matches bit for bit.
#[must_use]
pub fn py_round(x: f64, ndigits: usize) -> f64 {
    format!("{x:.ndigits$}").parse().unwrap_or(x)
}

/// Blend within-source rank with log-scaled citations into `[0, 1]`.
#[must_use]
pub fn relevance_score(position: usize, limit: usize, citations: Option<f64>) -> f64 {
    let pos = (1.0 - (position as f64 - 1.0) / limit.max(1) as f64).max(0.0);
    let cite = match citations {
        Some(c) if c > 0.0 => (c.ln_1p() / 1000f64.ln()).min(1.0),
        _ => 0.0,
    };
    py_round(0.6 * pos + 0.4 * cite, 4)
}

/// Python truthiness of a JSON value.
#[must_use]
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Python's `a or b`.
#[must_use]
pub fn or<'a>(a: &'a Value, b: &'a Value) -> &'a Value {
    if truthy(a) { a } else { b }
}

static NULL: Value = Value::Null;
static EMPTY_OBJECT: LazyLock<Value> = LazyLock::new(|| Value::Object(Map::new()));

/// `value.get(key)` on an object, or `null` for anything else or a missing key.
#[must_use]
pub fn get<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.as_object().and_then(|m| m.get(key)).unwrap_or(&NULL)
}

/// Python's `value.get(key) or {}`: the child if it is a non-empty object,
/// else an empty object.
#[must_use]
pub fn get_obj<'a>(value: &'a Value, key: &str) -> &'a Value {
    let child = get(value, key);
    if child.is_object() && truthy(child) {
        child
    } else {
        &EMPTY_OBJECT
    }
}

/// The elements of an array value, or nothing for any other value.
pub fn items(value: &Value) -> impl Iterator<Item = &Value> {
    value.as_array().into_iter().flatten()
}

/// Python's `str(value)` for JSON scalars; containers become their JSON text.
#[must_use]
pub fn py_str(value: &Value) -> String {
    match value {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::String(s) => s.clone(),
        Value::Number(n) => match (n.as_i64(), n.as_u64(), n.as_f64()) {
            (Some(i), _, _) => i.to_string(),
            (_, Some(u), _) => u.to_string(),
            (_, _, Some(f)) => python_float_repr(f),
            _ => n.to_string(),
        },
        other => other.to_string(),
    }
}

/// A text field after Python's `x or ""`: empty for falsy values.
#[must_use]
pub fn text(value: &Value) -> String {
    if truthy(value) {
        py_str(value)
    } else {
        String::new()
    }
}

/// An optional field passed through as-is in Python: `None` for `null`.
/// Non-string values are stringified (Python would keep the raw value; the
/// typed record cannot).
#[must_use]
pub fn opt_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::Array(a) => a.first().map(py_str),
        other => Some(py_str(other)),
    }
}

/// A number usable as a citation count, like Python's `if c and c > 0`.
#[must_use]
pub fn citations(value: &Value) -> Option<f64> {
    value.as_f64()
}

/// `_TAG_RE.sub(" ", text)`.
#[must_use]
pub fn strip_tags(text: &str) -> String {
    TAG.replace_all(text, " ").into_owned()
}

pub use re_core::pystr::{py_is_space, py_strip};

/// `" ".join(text.split())[:limit]`, counting characters.
#[must_use]
pub fn clip_snippet(text: &str, limit: usize) -> String {
    text.split(py_is_space)
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(limit)
        .collect()
}

fn decimal_value(c: char) -> Option<u32> {
    if let Some(d) = c.to_digit(10) {
        return Some(d);
    }
    let is_digit = |ch: char| DECIMAL_DIGIT.is_match(ch.encode_utf8(&mut [0; 4]));
    if !is_digit(c) {
        return None;
    }
    // Unicode decimal digits come in contiguous runs of ten starting at zero.
    let mut start = c as u32;
    while let Some(prev) = start.checked_sub(1).and_then(char::from_u32) {
        if !is_digit(prev) {
            break;
        }
        start -= 1;
    }
    Some((c as u32 - start) % 10)
}

/// Best-effort year: ints in `1000..=2200`, or the first four characters of
/// `str(value)` when they are decimal digits (Unicode digits included, as
/// Python's `int()` accepts them). Booleans are rejected.
#[must_use]
pub fn coerce_year(value: &Value) -> Option<i64> {
    let in_range = |y: i64| (1000..=2200).contains(&y).then_some(y);
    match value {
        Value::Null | Value::Bool(_) => None,
        Value::Number(n) if n.is_i64() || n.is_u64() => n.as_i64().and_then(in_range),
        other => {
            let text = py_str(other);
            let head: Vec<char> = py_strip(&text).chars().take(4).collect();
            if head.len() < 4 {
                return None;
            }
            let mut year = 0i64;
            for c in head {
                year = year * 10 + i64::from(decimal_value(c)?);
            }
            in_range(year)
        }
    }
}

/// Lowercase, strip `https://doi.org/`, `http://doi.org/` and `doi:` (each
/// checked once, in that order), and treat empty as missing.
#[must_use]
pub fn normalize_doi(doi: &Value) -> Option<String> {
    if !truthy(doi) {
        return None;
    }
    let mut text = py_strip(&py_str(doi)).to_lowercase();
    for prefix in ["https://doi.org/", "http://doi.org/", "doi:"] {
        if let Some(rest) = text.strip_prefix(prefix) {
            text = rest.to_string();
        }
    }
    (!text.is_empty()).then_some(text)
}

/// Inputs for [`build_record`], mirroring Python's keyword arguments.
#[derive(Debug, Clone)]
pub struct RecordInput {
    /// Run the evidence belongs to.
    pub run_id: String,
    /// Adapter name.
    pub source_adapter: &'static str,
    /// Query that found it.
    pub query: String,
    /// 1-based position within this source.
    pub rank: usize,
    /// Publication type.
    pub source_type: &'static str,
    /// Raw title.
    pub title: String,
    /// Author names.
    pub authors: Vec<String>,
    /// Publication year.
    pub year: Option<i64>,
    /// Publisher.
    pub publisher: String,
    /// Journal or venue.
    pub venue: String,
    /// Raw DOI value, normalised by [`build_record`].
    pub doi: Value,
    /// Landing page.
    pub url: Option<String>,
    /// Relevance score.
    pub score: f64,
    /// Raw snippet.
    pub snippet: String,
    /// Licence information.
    pub licence_notes: Option<String>,
    /// Adapter-specific extras.
    pub metadata: Map<String, Value>,
}

impl RecordInput {
    /// Defaults for everything but the identifying fields, like Python's
    /// keyword defaults.
    #[must_use]
    pub fn new(run_id: &str, source_adapter: &'static str, query: &str, rank: usize) -> Self {
        Self {
            run_id: run_id.to_string(),
            source_adapter,
            query: query.to_string(),
            rank,
            source_type: "peer_reviewed_article",
            title: String::new(),
            authors: Vec::new(),
            year: None,
            publisher: String::new(),
            venue: String::new(),
            doi: Value::Null,
            url: None,
            score: 0.0,
            snippet: String::new(),
            licence_notes: None,
            metadata: Map::new(),
        }
    }
}

/// Fill an [`EvidenceRecord`] with normalised values, like Python's
/// `build_record`.
#[must_use]
pub fn build_record(input: RecordInput) -> EvidenceRecord {
    let title = clip_snippet(&input.title, TITLE_LIMIT);
    EvidenceRecord {
        evidence_id: format!("ev_{}_{:04}", input.source_adapter, input.rank),
        run_id: input.run_id,
        source_adapter: input.source_adapter.to_string(),
        source_type: input.source_type.to_string(),
        title: if title.is_empty() {
            "Untitled".into()
        } else {
            title
        },
        authors: input.authors,
        year: input.year,
        publisher: input.publisher,
        venue: input.venue,
        doi: normalize_doi(&input.doi),
        url: input.url,
        retrieved_at: utc_now(),
        query: input.query,
        rank: i64::try_from(input.rank).unwrap_or(i64::MAX),
        score: input.score,
        snippet: clip_snippet(&input.snippet, SNIPPET_LIMIT),
        licence_notes: input.licence_notes,
        risk_flags: Vec::new(),
        metadata: input.metadata,
    }
}

/// Python's `urllib.parse.quote(s, safe=safe)`: UTF-8 percent-encoding with
/// uppercase hex, keeping `A-Za-z0-9_.-~` and the `safe` characters.
#[must_use]
pub fn quote(s: &str, safe: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        let c = byte as char;
        if byte.is_ascii_alphanumeric()
            || b"_.-~".contains(&byte)
            || (byte < 128 && safe.contains(c))
        {
            out.push(c);
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// Python's `urllib.parse.quote_plus(s)`: like [`quote`] with spaces as `+`.
#[must_use]
pub fn quote_plus(s: &str) -> String {
    quote(s, " ").replace(' ', "+")
}

/// Python's `urllib.parse.urlencode(pairs)`.
#[must_use]
pub fn urlencode(pairs: &[(&str, String)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", quote_plus(k), quote_plus(v)))
        .collect::<Vec<_>>()
        .join("&")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn py_round_matches_python_ties_and_signs() {
        // Values printed by Python's round(x, 4).
        assert_eq!(py_round(0.123_45, 4).to_bits(), 0.1235f64.to_bits());
        assert_eq!(py_round(0.000_05, 4).to_bits(), 0.0001f64.to_bits());
        assert_eq!(py_round(0.6, 4).to_bits(), 0.6f64.to_bits());
        assert!(py_round(-0.0, 4).is_sign_negative());
    }

    #[test]
    fn quote_matches_python() {
        assert_eq!(quote("a b/c", ""), "a%20b%2Fc");
        assert_eq!(quote_plus("caf\u{e9} & x"), "caf%C3%A9+%26+x");
        assert_eq!(quote("~._-", ""), "~._-");
    }

    #[test]
    fn coerce_year_cases() {
        assert_eq!(coerce_year(&json!(2025)), Some(2025));
        assert_eq!(coerce_year(&json!(999)), None);
        assert_eq!(coerce_year(&json!(true)), None);
        assert_eq!(coerce_year(&json!(" 2017-06-12T00:00:00Z")), Some(2017));
        assert_eq!(coerce_year(&json!(2024.0)), Some(2024));
        assert_eq!(
            coerce_year(&json!("\u{6f2}\u{6f0}\u{6f2}\u{6f4}")),
            Some(2024)
        );
        assert_eq!(coerce_year(&json!("20x4")), None);
        assert_eq!(coerce_year(&json!(null)), None);
    }

    #[test]
    fn truthiness_and_or() {
        assert!(!truthy(&json!(0)));
        assert!(!truthy(&json!("")));
        assert!(!truthy(&json!({})));
        assert!(truthy(&json!([0])));
        assert_eq!(or(&json!(""), &json!("b")), &json!("b"));
        assert_eq!(or(&json!("a"), &json!("b")), &json!("a"));
    }
}
