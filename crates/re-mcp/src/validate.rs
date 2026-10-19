//! Port of `validation.py` plus `_coerce_json_array` from `server.py`. Every
//! message matches Python's text, because clients see these strings.

use re_core::pystr::py_strip;
use re_store::sessions::SourceInput;
use serde_json::Value;

use crate::args::ListOrStr;
use crate::error::ToolError;
use crate::limits::{MAX_SOURCE_FIELD_CHARS, MAX_SOURCES};

fn fail(message: String) -> ToolError {
    ToolError(message)
}

/// `validate_text`: strip, reject empty unless allowed, cap code points.
///
/// # Errors
/// Empty or too long.
pub fn text(
    value: &str,
    field: &str,
    max_chars: usize,
    allow_empty: bool,
) -> Result<String, ToolError> {
    let cleaned = py_strip(value);
    if !allow_empty && cleaned.is_empty() {
        return Err(fail(format!("{field} must not be empty")));
    }
    if cleaned.chars().count() > max_chars {
        return Err(fail(format!("{field} exceeds {max_chars} characters")));
    }
    Ok(cleaned.to_string())
}

/// `validate_text` on an arbitrary JSON value (list items).
///
/// # Errors
/// Not a string, empty or too long.
pub fn text_value(value: &Value, field: &str, max_chars: usize) -> Result<String, ToolError> {
    let s = value
        .as_str()
        .ok_or_else(|| fail(format!("{field} must be a string")))?;
    text(s, field, max_chars, false)
}

/// `validate_score` after the value is a float.
///
/// # Errors
/// Outside `[0, 1]`, including NaN.
pub fn score(value: f64, field: &str) -> Result<f64, ToolError> {
    if (0.0..=1.0).contains(&value) {
        Ok(value)
    } else {
        Err(fail(format!("{field} must be between 0.0 and 1.0")))
    }
}

/// `validate_score` on a raw JSON value: Python's `float(value)` first.
///
/// # Errors
/// Not a number, or outside `[0, 1]`.
pub fn score_value(value: &Value, field: &str) -> Result<f64, ToolError> {
    let not_number = || fail(format!("{field} must be a number"));
    let number = match value {
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::Number(n) => n.as_f64().ok_or_else(not_number)?,
        Value::String(s) => {
            let t = py_strip(s);
            let lower = t.trim_start_matches(['+', '-']).to_ascii_lowercase();
            let special = matches!(lower.as_str(), "inf" | "infinity" | "nan");
            if !special && !t.bytes().any(|b| b.is_ascii_digit()) {
                return Err(not_number());
            }
            t.parse::<f64>().map_err(|_| not_number())?
        }
        _ => return Err(not_number()),
    };
    score(number, field)
}

/// `validate_non_negative_int`.
///
/// # Errors
/// Negative or above `max_value`.
pub fn non_negative_int(value: i64, field: &str, max_value: i64) -> Result<i64, ToolError> {
    if value < 0 {
        return Err(fail(format!("{field} must be non-negative")));
    }
    if value > max_value {
        return Err(fail(format!("{field} must be <= {max_value}")));
    }
    Ok(value)
}

/// `validate_limited_int`.
///
/// # Errors
/// Outside `[min_value, max_value]`.
pub fn limited_int(
    value: i64,
    field: &str,
    min_value: i64,
    max_value: i64,
) -> Result<i64, ToolError> {
    if (min_value..=max_value).contains(&value) {
        Ok(value)
    } else {
        Err(fail(format!(
            "{field} must be between {min_value} and {max_value}"
        )))
    }
}

/// `json.JSONDecodeError.pos` for a serde error: the character offset of
/// its line and column, or the input length when the input ran out.
fn char_pos(raw: &str, error: &serde_json::Error) -> usize {
    if error.to_string().starts_with("EOF while parsing a string") {
        return unterminated_string_start(raw).unwrap_or(0);
    }
    if error.is_eof() {
        return raw.chars().count();
    }
    let line_start: usize = raw
        .split_inclusive('\n')
        .take(error.line().saturating_sub(1))
        .map(|l| l.chars().count())
        .sum();
    line_start + error.column().saturating_sub(1)
}

/// Character offset of the opening quote of a string that never closes.
fn unterminated_string_start(raw: &str) -> Option<usize> {
    let (mut open, mut escaped) = (None, false);
    for (index, c) in raw.chars().enumerate() {
        match (open, c) {
            (Some(_), _) if escaped => escaped = false,
            (Some(_), '\\') => escaped = true,
            (Some(_), '"') => open = None,
            (None, '"') => open = Some(index),
            _ => {}
        }
    }
    open
}

/// `json.JSONDecodeError.msg` for the common serde errors.
fn python_json_message(error: &serde_json::Error) -> String {
    let detail = error.to_string();
    let serde_message = detail.split(" at line ").next().unwrap_or(&detail);
    match serde_message {
        "expected value" | "EOF while parsing a value" => "Expecting value",
        "EOF while parsing a string" => "Unterminated string starting at",
        "EOF while parsing a list"
        | "expected `,` or `]`"
        | "EOF while parsing an object"
        | "expected `,` or `}`" => "Expecting ',' delimiter",
        "key must be a string" => "Expecting property name enclosed in double quotes",
        "expected `:`" => "Expecting ':' delimiter",
        "trailing characters" => "Extra data",
        other => other,
    }
    .to_string()
}

/// `_coerce_json_array`: native arrays pass, strings must parse to arrays.
///
/// # Errors
/// Invalid JSON (`Invalid JSON in '<field>': ... (pos N)`), or not an array.
pub fn json_array(value: ListOrStr, field: &str) -> Result<Vec<Value>, ToolError> {
    match value {
        ListOrStr::List(items) => Ok(items),
        ListOrStr::Str(raw) => match serde_json::from_str::<Value>(&raw) {
            Ok(Value::Array(items)) => Ok(items),
            Ok(_) => Err(fail(format!("{field} must be a JSON array"))),
            Err(e) => Err(fail(format!(
                "Invalid JSON in '{field}': {} (pos {})",
                python_json_message(&e),
                char_pos(&raw, &e)
            ))),
        },
    }
}

/// `validate_string_list` (never `allow_empty` in the server).
///
/// # Errors
/// Empty, too many items, or an invalid item.
pub fn string_list(
    value: &[Value],
    field: &str,
    max_items: usize,
    max_item_chars: usize,
) -> Result<Vec<String>, ToolError> {
    if value.is_empty() {
        return Err(fail(format!("{field} must not be empty")));
    }
    if value.len() > max_items {
        return Err(fail(format!(
            "{field} must contain at most {max_items} items"
        )));
    }
    value
        .iter()
        .enumerate()
        .map(|(index, item)| text_value(item, &format!("{field}[{index}]"), max_item_chars))
        .collect()
}

/// `urlparse(url)` has an `http`/`https` scheme and a non-empty netloc.
/// `urlsplit` drops tabs and newlines first and lowercases the scheme.
fn is_http_url(url: &str) -> bool {
    let cleaned: String = url
        .chars()
        .filter(|c| !matches!(c, '\t' | '\r' | '\n'))
        .collect();
    let Some((scheme, rest)) = cleaned.split_once(':') else {
        return false;
    };
    let valid_scheme = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !valid_scheme || !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
        return false;
    }
    rest.strip_prefix("//").is_some_and(|after| {
        !after
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default()
            .is_empty()
    })
}

/// Python's `str(source.get(key, ""))`.
fn field_text(source: &serde_json::Map<String, Value>, key: &str) -> String {
    source
        .get(key)
        .map_or_else(String::new, re_retrieval::base::py_str)
}

/// `validate_sources`.
///
/// # Errors
/// Too many sources, a non-object, a non-http URL, or a bad score.
pub fn sources(value: &[Value]) -> Result<Vec<SourceInput>, ToolError> {
    if value.len() > MAX_SOURCES {
        return Err(fail(format!(
            "sources must contain at most {MAX_SOURCES} items"
        )));
    }
    value
        .iter()
        .enumerate()
        .map(|(index, source)| {
            let source = source
                .as_object()
                .ok_or_else(|| fail(format!("sources[{index}] must be an object")))?;
            let url = text(
                &field_text(source, "url"),
                &format!("sources[{index}].url"),
                MAX_SOURCE_FIELD_CHARS,
                true,
            )?;
            if !url.is_empty() && !is_http_url(&url) {
                return Err(fail(format!("sources[{index}].url must be an http(s) URL")));
            }
            let relevance_score = match source.get("relevance_score") {
                None => 0.0,
                Some(v) => score_value(v, &format!("sources[{index}].relevance_score"))?,
            };
            Ok(SourceInput {
                url,
                title: text(
                    &field_text(source, "title"),
                    &format!("sources[{index}].title"),
                    MAX_SOURCE_FIELD_CHARS,
                    true,
                )?,
                excerpt: text(
                    &field_text(source, "excerpt"),
                    &format!("sources[{index}].excerpt"),
                    MAX_SOURCE_FIELD_CHARS,
                    true,
                )?,
                relevance_score,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn err<T: std::fmt::Debug>(r: Result<T, ToolError>) -> String {
        r.expect_err("should fail").0
    }

    #[test]
    fn messages_match_python() {
        assert_eq!(
            err(text("  ", "query", 5, false)),
            "query must not be empty"
        );
        assert_eq!(
            err(text("abcdef", "query", 5, false)),
            "query exceeds 5 characters"
        );
        assert_eq!(text(" \u{1c}ok ", "q", 5, false), Ok("ok".into()));
        assert_eq!(
            err(score(1.5, "q_score")),
            "q_score must be between 0.0 and 1.0"
        );
        assert_eq!(
            err(score(f64::NAN, "q_score")),
            "q_score must be between 0.0 and 1.0"
        );
        assert_eq!(
            err(non_negative_int(-1, "depth", 3)),
            "depth must be non-negative"
        );
        assert_eq!(err(non_negative_int(4, "depth", 3)), "depth must be <= 3");
        assert_eq!(
            err(limited_int(0, "top_k", 1, 10)),
            "top_k must be between 1 and 10"
        );
        assert_eq!(
            err(string_list(&[], "trace", 3, 5)),
            "trace must not be empty"
        );
        assert_eq!(
            err(string_list(&[json!(1)], "trace", 3, 5)),
            "trace[0] must be a string"
        );
        assert_eq!(
            err(string_list(&vec![json!("a"); 4], "trace", 3, 5)),
            "trace must contain at most 3 items"
        );
    }

    #[test]
    fn json_arrays_and_errors() {
        assert_eq!(
            json_array(ListOrStr::Str("[1]".into()), "t"),
            Ok(vec![json!(1)])
        );
        assert_eq!(
            err(json_array(ListOrStr::Str("{}".into()), "t")),
            "t must be a JSON array"
        );
        // (input, Python's json.JSONDecodeError msg, pos)
        let cases = [
            ("[1,", "Expecting value", 3),
            ("[1", "Expecting ',' delimiter", 2),
            ("[1,x]", "Expecting value", 3),
            ("{\"a\":1", "Expecting ',' delimiter", 6),
            ("[1] x", "Extra data", 4),
            ("", "Expecting value", 0),
            ("{\"a\" 1}", "Expecting ':' delimiter", 5),
            ("[1 2]", "Expecting ',' delimiter", 3),
            (
                "{'a':1}",
                "Expecting property name enclosed in double quotes",
                1,
            ),
            ("[\"unterminated", "Unterminated string starting at", 1),
            ("[\"a\\\"b\", \"c", "Unterminated string starting at", 9),
        ];
        for (raw, message, pos) in cases {
            assert_eq!(
                err(json_array(ListOrStr::Str(raw.into()), "trace")),
                format!("Invalid JSON in 'trace': {message} (pos {pos})"),
                "{raw:?}"
            );
        }
    }

    #[test]
    fn sources_follow_python() {
        let ok =
            sources(&[json!({"url": " https://a.org/x ", "relevance_score": "0.5", "title": 5})])
                .expect("valid");
        assert_eq!(
            (
                ok[0].url.as_str(),
                ok[0].title.as_str(),
                ok[0].relevance_score
            ),
            ("https://a.org/x", "5", 0.5)
        );
        assert_eq!(err(sources(&[json!(1)])), "sources[0] must be an object");
        assert_eq!(
            err(sources(&[json!({"url": "ftp://a"})])),
            "sources[0].url must be an http(s) URL"
        );
        assert_eq!(
            err(sources(&[json!({"url": "https:///nohost"})])),
            "sources[0].url must be an http(s) URL"
        );
        assert_eq!(
            err(sources(&[json!({"relevance_score": null})])),
            "sources[0].relevance_score must be a number"
        );
        assert_eq!(
            err(sources(&[json!({"relevance_score": 2})])),
            "sources[0].relevance_score must be between 0.0 and 1.0"
        );
        assert!(sources(&[json!({"url": "HTTP://h\tost"})]).is_ok());
        assert!(sources(&vec![json!({}); 51]).is_err());
    }
}
