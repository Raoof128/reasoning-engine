//! Python `str` whitespace semantics.
//!
//! Python's `str.isspace` is Unicode `White_Space` plus the four ASCII
//! separators `\x1c`..`\x1f`; Rust's `char::is_whitespace` omits those four,
//! so `trim` and `split_whitespace` disagree with `strip` and `split`.

/// Python's `str.isspace` for one character.
#[must_use]
pub fn py_is_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// Python's `str.strip()`.
#[must_use]
pub fn py_strip(text: &str) -> &str {
    text.trim_matches(py_is_space)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_python_whitespace() {
        assert_eq!(py_strip("\u{1c} a b\u{85}\u{3000}"), "a b");
        assert_eq!(py_strip("\u{200b}x"), "\u{200b}x");
    }
}
