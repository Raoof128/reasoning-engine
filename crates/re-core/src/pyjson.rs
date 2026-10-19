//! Byte-exact port of Python's `json.dumps` for the options this project uses.
//!
//! Python hashes and stores JSON text (`stable_json_hash`, `SQLite` payloads,
//! run-pack artifacts), so the Rust port must emit the same bytes. `serde_json`
//! differs in three ways that matter: it writes `1e-5` where Python writes
//! `1e-05` (and `1e16` for `1e+16`), it never escapes non-ASCII, and its
//! separators differ. This module reproduces `json.dumps` with
//! `ensure_ascii=True` (Python's default), `sort_keys`, custom separators and
//! `indent`.

use std::fmt::Write as _;

use serde_json::{Number, Value};

/// Options mirroring the `json.dumps` keyword arguments we use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DumpOptions {
    /// `indent=` (`None` writes everything on one line).
    pub indent: Option<usize>,
    /// `sort_keys=`.
    pub sort_keys: bool,
    /// Separator between items (`separators[0]`).
    pub item_separator: &'static str,
    /// Separator between a key and its value (`separators[1]`).
    pub key_separator: &'static str,
    /// `ensure_ascii=`.
    pub ensure_ascii: bool,
}

impl DumpOptions {
    /// `json.dumps(v, sort_keys=True)`.
    #[must_use]
    pub const fn sorted() -> Self {
        Self {
            indent: None,
            sort_keys: true,
            item_separator: ", ",
            key_separator: ": ",
            ensure_ascii: true,
        }
    }

    /// `json.dumps(v, sort_keys=True, separators=(",", ":"))`.
    #[must_use]
    pub const fn compact_sorted() -> Self {
        Self {
            item_separator: ",",
            key_separator: ":",
            ..Self::sorted()
        }
    }

    /// `json.dumps(v, indent=n, sort_keys=True)`; Python switches the item
    /// separator to `","` whenever an indent is given.
    #[must_use]
    pub const fn indented_sorted(indent: usize) -> Self {
        Self {
            indent: Some(indent),
            item_separator: ",",
            ..Self::sorted()
        }
    }
}

/// Serialise `value` exactly as Python's `json.dumps` would.
#[must_use]
pub fn dumps(value: &Value, options: &DumpOptions) -> String {
    let mut out = String::new();
    write_value(&mut out, value, options, 0);
    out
}

fn newline(out: &mut String, options: &DumpOptions, level: usize) {
    if let Some(indent) = options.indent {
        out.push('\n');
        out.extend(std::iter::repeat_n(' ', indent * level));
    }
}

fn write_value(out: &mut String, value: &Value, options: &DumpOptions, level: usize) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => write_number(out, n),
        Value::String(s) => write_string(out, s, options.ensure_ascii),
        Value::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(options.item_separator);
                }
                newline(out, options, level + 1);
                write_value(out, item, options, level + 1);
            }
            newline(out, options, level);
            out.push(']');
        }
        Value::Object(map) => {
            if map.is_empty() {
                out.push_str("{}");
                return;
            }
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            if options.sort_keys {
                // Python compares str by code point; UTF-8 byte order agrees.
                entries.sort_by(|a, b| a.0.cmp(b.0));
            }
            out.push('{');
            for (i, (key, item)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push_str(options.item_separator);
                }
                newline(out, options, level + 1);
                write_string(out, key, options.ensure_ascii);
                out.push_str(options.key_separator);
                write_value(out, item, options, level + 1);
            }
            newline(out, options, level);
            out.push('}');
        }
    }
}

fn write_number(out: &mut String, n: &Number) {
    if let Some(i) = n.as_i64() {
        let _ = write!(out, "{i}");
    } else if let Some(u) = n.as_u64() {
        let _ = write!(out, "{u}");
    } else if let Some(f) = n.as_f64() {
        out.push_str(&python_float_repr(f));
    }
}

/// Python's `float.__repr__`: shortest round-trip digits, positional
/// notation when the decimal exponent is in `-4..16`, otherwise scientific
/// with a signed, at-least-two-digit exponent.
#[must_use]
pub fn python_float_repr(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    // Rust's `{:e}` prints the shortest digits that round-trip, e.g. "1.25e-7".
    // For finite values it always has the form `-?d(.d+)?e-?d+`.
    let sci = format!("{value:e}");
    let Some((mantissa, exponent)) = sci.split_once('e') else {
        return sci;
    };
    let Ok(exponent) = exponent.parse::<i32>() else {
        return sci;
    };
    let (sign, mantissa) = mantissa
        .strip_prefix('-')
        .map_or(("", mantissa), |m| ("-", m));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();

    if (-4..16).contains(&exponent) {
        // Digits before the decimal point: exponent + 1, in -3..=16.
        let point = exponent + 1;
        let body = if point <= 0 {
            format!("0.{}{digits}", "0".repeat(point.unsigned_abs() as usize))
        } else {
            let point = point.unsigned_abs() as usize;
            if digits.len() <= point {
                format!("{digits}{}.0", "0".repeat(point - digits.len()))
            } else {
                format!("{}.{}", &digits[..point], &digits[point..])
            }
        };
        format!("{sign}{body}")
    } else {
        let (first, rest) = digits.split_at(1);
        let fraction = if rest.is_empty() {
            String::new()
        } else {
            format!(".{rest}")
        };
        let exp_sign = if exponent < 0 { '-' } else { '+' };
        format!(
            "{sign}{first}{fraction}e{exp_sign}{:02}",
            exponent.unsigned_abs()
        )
    }
}

fn write_string(out: &mut String, s: &str, ensure_ascii: bool) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            // ensure_ascii escapes everything outside ' '..='~' (DEL included);
            // otherwise only control characters are escaped.
            c if c < ' ' || (ensure_ascii && c > '~') => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    let _ = write!(out, "\\u{unit:04x}");
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_repr_matches_python_examples() {
        for (value, expected) in [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (1e-5, "1e-05"),
            (1e-4, "0.0001"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (-1.5e-7, "-1.5e-07"),
            (2.5e300, "2.5e+300"),
            (12.5, "12.5"),
        ] {
            assert_eq!(python_float_repr(value), expected, "{value:e}");
        }
    }

    proptest::proptest! {
        #[test]
        fn float_repr_round_trips(bits in proptest::prelude::any::<u64>()) {
            let value = f64::from_bits(bits);
            if value.is_finite() {
                let parsed: f64 = python_float_repr(value).parse().unwrap();
                proptest::prop_assert_eq!(parsed.to_bits(), value.to_bits());
            }
        }
    }

    #[test]
    fn ensure_ascii_off_keeps_non_ascii_and_del() {
        let v = Value::String("caf\u{e9}\u{7f}\u{1}".into());
        let opts = DumpOptions {
            ensure_ascii: false,
            ..DumpOptions::sorted()
        };
        assert_eq!(dumps(&v, &opts), "\"caf\u{e9}\u{7f}\\u0001\"");
    }
}
