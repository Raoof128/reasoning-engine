//! Tool arguments, read the way `FastMCP`'s pydantic models read them.
//!
//! Pydantic validates arguments in lax mode before a tool body runs, so the
//! Python server accepts more than its type hints suggest: `int` takes
//! `true`, `3.0`, `"3"` and `" 3.0 "`; `float` takes `true` and numeric
//! strings (including `"nan"`); `str` takes only strings. A missing
//! argument with a default gets the default; an explicit `null` does not.
//! Failures use pydantic's first line and field layout:
//! `1 validation error for <tool>Arguments\n<field>\n  <message>`.

use serde_json::{Map, Value};

use crate::error::ToolError;

/// A `list[...] | str` argument before `_coerce_json_array` runs.
#[derive(Debug, Clone, PartialEq)]
pub enum ListOrStr {
    /// A native JSON array.
    List(Vec<Value>),
    /// A string that should hold a JSON array.
    Str(String),
}

/// One tool call's arguments.
#[derive(Debug, Clone, Copy)]
pub struct Args<'a> {
    tool: &'a str,
    map: &'a Map<String, Value>,
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
    // Rust also accepts "inf"/"nan" spellings Python rejects; those were
    // handled above, so require a digit here.
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

    fn string(&self, field: &str, value: &Value) -> Result<String, ToolError> {
        value
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| self.fail(field, "Input should be a valid string"))
    }

    fn int(&self, field: &str, value: &Value) -> Result<i64, ToolError> {
        match value {
            Value::Bool(b) => Ok(i64::from(*b)),
            Value::Number(n) => match (n.as_i64(), n.as_f64()) {
                (Some(i), _) => Ok(i),
                (None, Some(f)) if f.fract() == 0.0 => Ok(saturate(f)),
                _ => Err(self.fail(
                    field,
                    "Input should be a valid integer, got a number with a fractional part",
                )),
            },
            Value::String(s) => {
                let text = re_core::pystr::py_strip(s);
                if let Ok(i) = text.parse::<i64>() {
                    return Ok(i);
                }
                match parse_float(text) {
                    Some(f) if f.is_finite() && f.fract() == 0.0 => Ok(saturate(f)),
                    _ => Err(self.fail(
                        field,
                        "Input should be a valid integer, unable to parse string as an integer",
                    )),
                }
            }
            _ => Err(self.fail(field, "Input should be a valid integer")),
        }
    }

    fn float(&self, field: &str, value: &Value) -> Result<f64, ToolError> {
        match value {
            Value::Bool(b) => Ok(f64::from(u8::from(*b))),
            Value::Number(n) => n
                .as_f64()
                .ok_or_else(|| self.fail(field, "Input should be a valid number")),
            Value::String(s) => parse_float(s).ok_or_else(|| {
                self.fail(
                    field,
                    "Input should be a valid number, unable to parse string as a number",
                )
            }),
            _ => Err(self.fail(field, "Input should be a valid number")),
        }
    }

    /// A required `str`.
    ///
    /// # Errors
    /// Missing or not a string.
    pub fn str(&self, field: &str) -> Result<String, ToolError> {
        self.string(field, self.required(field)?)
    }

    /// An optional `str` with a default.
    ///
    /// # Errors
    /// Present but not a string.
    pub fn str_or(&self, field: &str, default: &str) -> Result<String, ToolError> {
        self.map
            .get(field)
            .map_or_else(|| Ok(default.to_string()), |v| self.string(field, v))
    }

    /// An optional `int` with a default.
    ///
    /// # Errors
    /// Present but not coercible to an integer.
    pub fn int_or(&self, field: &str, default: i64) -> Result<i64, ToolError> {
        self.map
            .get(field)
            .map_or(Ok(default), |v| self.int(field, v))
    }

    /// A required `float`.
    ///
    /// # Errors
    /// Missing or not coercible to a number.
    pub fn float_req(&self, field: &str) -> Result<f64, ToolError> {
        self.float(field, self.required(field)?)
    }

    fn list_or_str(
        &self,
        field: &str,
        value: &Value,
        item_is_ok: fn(&Value) -> bool,
        item: &str,
    ) -> Result<ListOrStr, ToolError> {
        match value {
            Value::String(s) => Ok(ListOrStr::Str(s.clone())),
            Value::Array(items) => match items.iter().position(|v| !item_is_ok(v)) {
                None => Ok(ListOrStr::List(items.clone())),
                Some(index) => Err(self.fail(
                    &format!("{field}.{item}.{index}"),
                    "Input should be a valid item",
                )),
            },
            _ => Err(self.fail(field, "Input should be a valid list or string")),
        }
    }

    /// A required `list[str] | str`.
    ///
    /// # Errors
    /// Missing, or neither a list of strings nor a string.
    pub fn str_list_or_str(&self, field: &str) -> Result<ListOrStr, ToolError> {
        self.list_or_str(field, self.required(field)?, Value::is_string, "list[str]")
    }

    /// An optional `list[dict] | str` with a string default.
    ///
    /// # Errors
    /// Present but neither a list of objects nor a string.
    pub fn dict_list_or_str_or(&self, field: &str, default: &str) -> Result<ListOrStr, ToolError> {
        self.map.get(field).map_or_else(
            || Ok(ListOrStr::Str(default.to_string())),
            |v| self.list_or_str(field, v, Value::is_object, "list[dict[any,any]]"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn args(value: &Value) -> Args<'_> {
        Args::new("t", value.as_object().expect("object"))
    }

    #[test]
    fn ints_coerce_like_pydantic() {
        let v = json!({"a": true, "b": 3.0, "c": " 3 ", "d": "3.0", "e": 3.5, "f": "x", "g": null});
        let a = args(&v);
        assert_eq!(a.int_or("a", 0), Ok(1));
        assert_eq!(a.int_or("b", 0), Ok(3));
        assert_eq!(a.int_or("c", 0), Ok(3));
        assert_eq!(a.int_or("d", 0), Ok(3));
        assert_eq!(a.int_or("missing", 7), Ok(7));
        for bad in ["e", "f", "g"] {
            assert!(a.int_or(bad, 0).is_err(), "{bad}");
        }
        assert_eq!(
            a.int_or("g", 0).unwrap_err().0,
            "1 validation error for tArguments\ng\n  Input should be a valid integer"
        );
    }

    #[test]
    fn floats_coerce_like_pydantic() {
        let v = json!({"a": true, "b": 1, "c": " .5 ", "d": "NaN", "e": "-Infinity", "f": "x", "g": null, "h": "infx"});
        let a = args(&v);
        assert_eq!(a.float_req("a"), Ok(1.0));
        assert_eq!(a.float_req("b"), Ok(1.0));
        assert_eq!(a.float_req("c"), Ok(0.5));
        assert!(a.float_req("d").expect("nan").is_nan());
        assert_eq!(a.float_req("e"), Ok(f64::NEG_INFINITY));
        for bad in ["f", "g", "h", "missing"] {
            assert!(a.float_req(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn strings_are_strict_and_defaults_only_fill_absent_keys() {
        let v = json!({"a": 1, "b": null, "c": "x"});
        let a = args(&v);
        assert!(a.str("a").is_err());
        assert!(a.str_or("b", "d").is_err());
        assert_eq!(a.str_or("missing", "d"), Ok("d".into()));
        assert_eq!(
            a.str("missing").unwrap_err().0,
            "1 validation error for tArguments\nmissing\n  Field required"
        );
    }

    #[test]
    fn unions_accept_lists_or_strings() {
        let v =
            json!({"a": ["x"], "b": "[\"x\"]", "c": ["x", 1], "d": 5, "e": [{"u": 1}], "f": [1]});
        let a = args(&v);
        assert_eq!(
            a.str_list_or_str("a"),
            Ok(ListOrStr::List(vec![json!("x")]))
        );
        assert_eq!(a.str_list_or_str("b"), Ok(ListOrStr::Str("[\"x\"]".into())));
        assert!(a.str_list_or_str("c").is_err());
        assert!(a.str_list_or_str("d").is_err());
        assert!(a.dict_list_or_str_or("e", "[]").is_ok());
        assert!(a.dict_list_or_str_or("f", "[]").is_err());
        assert_eq!(
            a.dict_list_or_str_or("missing", "[]"),
            Ok(ListOrStr::Str("[]".into()))
        );
    }
}
