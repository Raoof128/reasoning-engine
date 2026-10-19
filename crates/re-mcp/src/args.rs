//! Tool arguments, read the way `FastMCP` reads them.
//!
//! `FastMCP` first "pre-parses" JSON: for a parameter not typed plain `str`,
//! a string value that `json.loads` turns into a list, object or null is
//! replaced by that value (strings, numbers and booleans are left alone).
//! Pydantic then validates every parameter against its type hint in lax
//! mode, collecting all failures: `int` takes `true`, `3.0`, `"3"` and
//! `" 3.0 "`; `float` takes `true` and numeric strings (including
//! `"nan"`); `str` takes only strings; a union reports one error per
//! member. A missing argument with a default gets the default; an explicit
//! `null` does not.
//!
//! [`check`] reproduces that pass and its message layout,
//! `N validation error(s) for <tool>Arguments` then `<loc>\n  <message>`
//! per failure, without pydantic's version-specific `[type=...]` and
//! documentation-URL suffixes. Tool bodies run only after it succeeds.

use std::fmt::Write as _;

use serde_json::{Map, Value};

use crate::error::ToolError;
use crate::schema::{Kind, ToolSpec};

/// A `list[...] | str` argument before `_coerce_json_array` runs.
#[derive(Debug, Clone, PartialEq)]
pub enum ListOrStr {
    /// A native JSON array.
    List(Vec<Value>),
    /// A string that should hold a JSON array.
    Str(String),
}

/// `FastMCP`'s `pre_parse_json` for one non-`str` parameter.
fn pre_parse(value: &Value) -> Value {
    if let Value::String(s) = value
        && let Ok(parsed @ (Value::Array(_) | Value::Object(_) | Value::Null)) =
            serde_json::from_str::<Value>(s)
    {
        return parsed;
    }
    value.clone()
}

/// Python's `float(str)` for the strings pydantic accepts: surrounding
/// whitespace, signs, exponents, `inf`/`infinity`/`nan` in any case.
fn parse_float(text: &str) -> Option<f64> {
    let text = re_core::pystr::py_strip(text);
    let unsigned = text
        .strip_prefix(['+', '-'])
        .unwrap_or(text)
        .to_ascii_lowercase();
    if matches!(unsigned.as_str(), "inf" | "infinity" | "nan") {
        let value = if unsigned == "nan" {
            f64::NAN
        } else {
            f64::INFINITY
        };
        return Some(if text.starts_with('-') { -value } else { value });
    }
    // Rust also accepts spellings Python rejects (the special values were
    // handled above), so require a digit.
    if !text.bytes().any(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn saturate(value: f64) -> i64 {
    // Out-of-range values fail every range check downstream anyway.
    #[allow(clippy::cast_possible_truncation, reason = "saturating by design")]
    let truncated = value as i64;
    truncated
}

fn as_int(value: &Value) -> Result<i64, &'static str> {
    match value {
        Value::Bool(b) => Ok(i64::from(*b)),
        Value::Number(n) => match (n.as_i64(), n.as_f64()) {
            (Some(i), _) => Ok(i),
            (None, Some(f)) if f.fract() == 0.0 => Ok(saturate(f)),
            _ => Err("Input should be a valid integer, got a number with a fractional part"),
        },
        Value::String(s) => {
            let text = re_core::pystr::py_strip(s);
            if let Ok(i) = text.parse::<i64>() {
                return Ok(i);
            }
            match parse_float(text) {
                Some(f) if f.is_finite() && f.fract() == 0.0 => Ok(saturate(f)),
                _ => Err("Input should be a valid integer, unable to parse string as an integer"),
            }
        }
        _ => Err("Input should be a valid integer"),
    }
}

/// Pydantic's lax `bool`: booleans, `0`/`1`, and the usual words.
fn as_bool(value: &Value) -> Result<bool, &'static str> {
    match value {
        Value::Bool(b) => Ok(*b),
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.abs() < f64::EPSILON => Ok(false),
            Some(f) if (f - 1.0).abs() < f64::EPSILON => Ok(true),
            _ => Err("Input should be a valid boolean, unable to interpret input"),
        },
        Value::String(s) => match re_core::pystr::py_strip(s).to_lowercase().as_str() {
            "0" | "off" | "f" | "false" | "n" | "no" => Ok(false),
            "1" | "on" | "t" | "true" | "y" | "yes" => Ok(true),
            _ => Err("Input should be a valid boolean, unable to interpret input"),
        },
        _ => Err("Input should be a valid boolean"),
    }
}

fn as_float(value: &Value) -> Result<f64, &'static str> {
    match value {
        Value::Bool(b) => Ok(f64::from(u8::from(*b))),
        Value::Number(n) => n.as_f64().ok_or("Input should be a valid number"),
        Value::String(s) => parse_float(s)
            .ok_or("Input should be a valid number, unable to parse string as a number"),
        _ => Err("Input should be a valid number"),
    }
}

/// Errors for a `list[T] | str` union, one per failing member, as pydantic
/// reports them.
fn union_errors(
    field: &str,
    value: &Value,
    member: &str,
    item_ok: fn(&Value) -> bool,
    item_message: &str,
) -> Vec<(String, String)> {
    let string_error = (
        format!("{field}.str"),
        "Input should be a valid string".to_string(),
    );
    match value {
        Value::String(_) => Vec::new(),
        Value::Array(items) => {
            let mut errors: Vec<(String, String)> = items
                .iter()
                .enumerate()
                .filter(|(_, item)| !item_ok(item))
                .map(|(i, _)| (format!("{field}.{member}.{i}"), item_message.to_string()))
                .collect();
            if !errors.is_empty() {
                errors.push(string_error);
            }
            errors
        }
        _ => vec![
            (
                format!("{field}.{member}"),
                "Input should be a valid list".to_string(),
            ),
            string_error,
        ],
    }
}

/// Pydantic's validation pass over a tool's arguments.
///
/// # Errors
/// Every failing parameter, formatted as pydantic formats them.
pub fn check(spec: &ToolSpec, map: &Map<String, Value>) -> Result<(), ToolError> {
    let mut errors: Vec<(String, String)> = Vec::new();
    for param in spec.params {
        let field = param.name;
        let Some(raw) = map.get(field) else {
            if param.default.is_none() {
                errors.push((field.to_string(), "Field required".to_string()));
            }
            continue;
        };
        let value = if param.kind == Kind::Str {
            raw.clone()
        } else {
            pre_parse(raw)
        };
        match param.kind {
            Kind::Str if !value.is_string() => {
                errors.push((field.into(), "Input should be a valid string".into()));
            }
            Kind::Int => {
                if let Err(message) = as_int(&value) {
                    errors.push((field.into(), message.into()));
                }
            }
            Kind::Float => {
                if let Err(message) = as_float(&value) {
                    errors.push((field.into(), message.into()));
                }
            }
            Kind::Bool => {
                if let Err(message) = as_bool(&value) {
                    errors.push((field.into(), message.into()));
                }
            }
            Kind::StrListOrStr => errors.extend(union_errors(
                field,
                &value,
                "list[str]",
                Value::is_string,
                "Input should be a valid string",
            )),
            Kind::DictListOrStr => errors.extend(union_errors(
                field,
                &value,
                "list[dict[any,any]]",
                Value::is_object,
                "Input should be a valid dictionary",
            )),
            Kind::Str | Kind::AnyOrNull => {}
        }
    }
    if errors.is_empty() {
        return Ok(());
    }
    let plural = if errors.len() == 1 { "" } else { "s" };
    let mut message = format!(
        "{} validation error{plural} for {}Arguments",
        errors.len(),
        spec.name
    );
    for (loc, text) in errors {
        let _ = write!(message, "\n{loc}\n  {text}");
    }
    Err(ToolError(message))
}

/// One tool call's arguments, read after [`check`] has passed, so the
/// getters fail only if a tool body asks for something its signature does
/// not declare.
#[derive(Debug, Clone, Copy)]
pub struct Args<'a> {
    tool: &'a str,
    map: &'a Map<String, Value>,
}

impl<'a> Args<'a> {
    /// Arguments for `tool`.
    #[must_use]
    pub fn new(tool: &'a str, map: &'a Map<String, Value>) -> Self {
        Self { tool, map }
    }

    fn fail(&self, field: &str, message: &str) -> ToolError {
        ToolError(format!(
            "1 validation error for {}Arguments\n{field}\n  {message}",
            self.tool
        ))
    }

    fn required(&self, field: &str) -> Result<&'a Value, ToolError> {
        self.map
            .get(field)
            .ok_or_else(|| self.fail(field, "Field required"))
    }

    /// A required `str`.
    ///
    /// # Errors
    /// Missing or not a string.
    pub fn str(&self, field: &str) -> Result<String, ToolError> {
        self.required(field)?
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| self.fail(field, "Input should be a valid string"))
    }

    /// An optional `str` with a default.
    ///
    /// # Errors
    /// Present but not a string.
    pub fn str_or(&self, field: &str, default: &str) -> Result<String, ToolError> {
        match self.map.get(field) {
            None => Ok(default.to_string()),
            Some(Value::String(s)) => Ok(s.clone()),
            Some(_) => Err(self.fail(field, "Input should be a valid string")),
        }
    }

    /// An optional `int` with a default.
    ///
    /// # Errors
    /// Present but not coercible to an integer.
    pub fn int_or(&self, field: &str, default: i64) -> Result<i64, ToolError> {
        self.map.get(field).map_or(Ok(default), |v| {
            as_int(&pre_parse(v)).map_err(|m| self.fail(field, m))
        })
    }

    /// An optional `bool` with a default.
    ///
    /// # Errors
    /// Not coercible to a boolean.
    pub fn bool_or(&self, field: &str, default: bool) -> Result<bool, ToolError> {
        self.map.get(field).map_or(Ok(default), |v| {
            as_bool(&pre_parse(v)).map_err(|m| self.fail(field, m))
        })
    }

    /// A required `float`.
    ///
    /// # Errors
    /// Missing or not coercible to a number.
    pub fn float_req(&self, field: &str) -> Result<f64, ToolError> {
        as_float(&pre_parse(self.required(field)?)).map_err(|m| self.fail(field, m))
    }

    fn list_or_str(&self, field: &str, value: &Value) -> Result<ListOrStr, ToolError> {
        match pre_parse(value) {
            Value::String(s) => Ok(ListOrStr::Str(s)),
            Value::Array(items) => Ok(ListOrStr::List(items)),
            _ => Err(self.fail(field, "Input should be a valid list")),
        }
    }

    /// A required `list[...] | str`.
    ///
    /// # Errors
    /// Missing, or neither a list nor a string.
    pub fn str_list_or_str(&self, field: &str) -> Result<ListOrStr, ToolError> {
        self.list_or_str(field, self.required(field)?)
    }

    /// An optional `list[...] | str` with a string default.
    ///
    /// # Errors
    /// Present but neither a list nor a string.
    pub fn dict_list_or_str_or(&self, field: &str, default: &str) -> Result<ListOrStr, ToolError> {
        self.map.get(field).map_or_else(
            || Ok(ListOrStr::Str(default.to_string())),
            |v| self.list_or_str(field, v),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::find;
    use serde_json::json;

    fn checked(tool: &str, args: &Value) -> Result<(), String> {
        check(find(tool).expect("tool"), args.as_object().expect("object")).map_err(|e| e.0)
    }

    #[test]
    fn ints_and_floats_coerce_like_pydantic() {
        let v = json!({"a": true, "b": 3.0, "c": " 3 ", "d": "3.0", "e": 3.5, "f": "x", "g": null, "h": "null"});
        let a = Args::new("t", v.as_object().expect("object"));
        assert_eq!(a.int_or("a", 0), Ok(1));
        assert_eq!(a.int_or("b", 0), Ok(3));
        assert_eq!(a.int_or("c", 0), Ok(3));
        assert_eq!(a.int_or("d", 0), Ok(3));
        assert_eq!(a.int_or("missing", 7), Ok(7));
        for bad in ["e", "f", "g", "h"] {
            assert!(a.int_or(bad, 0).is_err(), "{bad}");
        }
        let v = json!({"a": true, "c": " .5 ", "d": "NaN", "e": "-Infinity", "f": "x", "g": null, "h": "infx"});
        let a = Args::new("t", v.as_object().expect("object"));
        assert_eq!(a.float_req("a"), Ok(1.0));
        assert_eq!(a.float_req("c"), Ok(0.5));
        assert!(a.float_req("d").expect("nan").is_nan());
        assert_eq!(a.float_req("e"), Ok(f64::NEG_INFINITY));
        for bad in ["f", "g", "h", "missing"] {
            assert!(a.float_req(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn check_reports_every_failure_like_pydantic() {
        assert_eq!(
            checked(
                "consensus_candidates",
                &json!({"session_id": "s", "top_k": "3"})
            ),
            Ok(())
        );
        assert_eq!(
            checked("init_research_session", &json!({})),
            Err(
                "1 validation error for init_research_sessionArguments\nquery\n  Field required"
                    .into()
            )
        );
        // A JSON-array string is pre-parsed; each bad item and the `str`
        // member then fail, as in pydantic's output.
        assert_eq!(
            checked("register_branch", &json!({"session_id": "s", "trace": "[\"ok\", 5]"})),
            Err("2 validation errors for register_branchArguments\ntrace.list[str].1\n  Input should be a valid string\ntrace.str\n  Input should be a valid string".into())
        );
        assert_eq!(
            checked("register_branch", &json!({"session_id": 1, "trace": 5, "sources": [1], "depth": "x"})),
            Err("6 validation errors for register_branchArguments\nsession_id\n  Input should be a valid string\ntrace.list[str]\n  Input should be a valid list\ntrace.str\n  Input should be a valid string\nsources.list[dict[any,any]].0\n  Input should be a valid dictionary\nsources.str\n  Input should be a valid string\ndepth\n  Input should be a valid integer, unable to parse string as an integer".into())
        );
        // Strings that are not JSON arrays reach _coerce_json_array.
        assert_eq!(
            checked(
                "evidence_gap_questions_tool",
                &json!({"query": "q", "claims": "[1,"})
            ),
            Ok(())
        );
        assert_eq!(
            checked(
                "evidence_gap_questions_tool",
                &json!({"query": "q", "claims": "\"x\""})
            ),
            Ok(())
        );
        assert!(
            checked(
                "evidence_gap_questions_tool",
                &json!({"query": "q", "claims": "null"})
            )
            .is_err()
        );
        assert_eq!(
            checked(
                "get_scholar_auth_status",
                &json!({"dummy": [1, {"x": null}]})
            ),
            Ok(())
        );
    }
}
