//! Exact port of the `CPython` `html.unescape` function (`Lib/html/__init__.py`).
//!
//! Third-party decoders differ from Python on legacy references without a
//! semicolon (`&copy`), on `&#0;` (U+FFFD) and on the Windows-1252 remap of
//! `&#x80;`-`&#x9f;`. Parity needs the exact algorithm and tables, so the
//! tables in `html_entities.rs` are generated from `CPython` itself.

use std::borrow::Cow;
use std::sync::LazyLock;

use regex::{Captures, Regex};

use crate::html_entities::{HTML5, INVALID_CHARREFS, INVALID_CODEPOINTS};

static CHARREF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"&(#[0-9]+;?|#[xX][0-9a-fA-F]+;?|[^\t\n\x0c <&#;]{1,32};?)")
        .expect("valid charref regex")
});

fn lookup_named(name: &str) -> Option<&'static str> {
    HTML5
        .binary_search_by(|(key, _)| key.as_bytes().cmp(name.as_bytes()))
        .ok()
        .map(|i| HTML5[i].1)
}

/// Parse digits in `radix`; `None` means larger than any code point, which
/// Python maps to U+FFFD just like an out-of-range value.
fn parse_codepoint(digits: &str, radix: u32) -> Option<u32> {
    let mut value: u32 = 0;
    for ch in digits.chars() {
        let digit = ch.to_digit(radix)?;
        value = value.checked_mul(radix)?.checked_add(digit)?;
    }
    Some(value)
}

fn replace_numeric(body: &str) -> Cow<'static, str> {
    let (digits, radix) = match body.as_bytes().get(1) {
        Some(b'x' | b'X') => (&body[2..], 16),
        _ => (&body[1..], 10),
    };
    let Some(num) = parse_codepoint(digits.trim_end_matches(';'), radix) else {
        return Cow::Borrowed("\u{fffd}");
    };
    if let Ok(i) = INVALID_CHARREFS.binary_search_by_key(&num, |(cp, _)| *cp) {
        return Cow::Borrowed(INVALID_CHARREFS[i].1);
    }
    if (0xD800..=0xDFFF).contains(&num) || num > 0x0010_FFFF {
        return Cow::Borrowed("\u{fffd}");
    }
    if INVALID_CODEPOINTS.binary_search(&num).is_ok() {
        return Cow::Borrowed("");
    }
    char::from_u32(num).map_or(Cow::Borrowed("\u{fffd}"), |c| Cow::Owned(c.to_string()))
}

fn replace_named(body: &str) -> String {
    if let Some(value) = lookup_named(body) {
        return value.to_string();
    }
    // Longest matching prefix, as the HTML5 spec defines. Python slices by
    // characters: for x in range(len(s) - 1, 1, -1): s[:x].
    let boundaries: Vec<usize> = body.char_indices().map(|(i, _)| i).collect();
    for x in (2..boundaries.len()).rev() {
        let split = boundaries[x];
        if let Some(value) = lookup_named(&body[..split]) {
            return format!("{value}{}", &body[split..]);
        }
    }
    format!("&{body}")
}

/// Decode named and numeric character references exactly like Python.
#[must_use]
pub fn unescape(text: &str) -> Cow<'_, str> {
    if !text.contains('&') {
        return Cow::Borrowed(text);
    }
    CHARREF.replace_all(text, |caps: &Captures<'_>| {
        let body = &caps[1];
        if body.starts_with('#') {
            replace_numeric(body).into_owned()
        } else {
            replace_named(body)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_match_cpython_sizes() {
        assert_eq!(HTML5.len(), 2231);
        assert_eq!(INVALID_CHARREFS.len(), 34);
        assert_eq!(INVALID_CODEPOINTS.len(), 126);
        assert!(
            HTML5
                .windows(2)
                .all(|w| w[0].0.as_bytes() < w[1].0.as_bytes())
        );
    }

    #[test]
    fn matches_python_on_edge_cases() {
        // Expected values printed by CPython 3.13 html.unescape.
        let cases = [
            ("no entities", "no entities"),
            ("&amp;&lt;&gt;&quot;&#39;", "&<>\"'"),
            ("&copy &amp &nbsp;x", "\u{a9} & \u{a0}x"),
            ("&#0; &#x80; &#x9F; &#13;", "\u{fffd} \u{20ac} \u{178} \r"),
            (
                "&#xD800; &#1114112; &#99999999999999999999;",
                "\u{fffd} \u{fffd} \u{fffd}",
            ),
            ("&#1; &#xFFFE; &#65;&#x42;", "  AB"),
            (
                "&NotEqualTilde; &notit; &unknown; &",
                "\u{2242}\u{338} \u{ac}it; &unknown; &",
            ),
            ("&amp;lt;", "&lt;"),
        ];
        for (input, expected) in cases {
            assert_eq!(unescape(input), expected, "input {input:?}");
        }
    }
}
