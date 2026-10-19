//! Persian text handling (`fa` feature, plan Phase 12).
//!
//! The same Persian word reaches the engine in several spellings: Arabic
//! yeh and kaf from Arabic keyboards, optional short vowels and tatweel,
//! three digit sets, and a zero-width non-joiner (ZWNJ) that some writers
//! type as a space or leave out. [`normalize`] picks one form so terms
//! match; [`terms`] then drops the ZWNJ so `می‌شود`, `میشود` and
//! `می شود` agree as far as a single token can.

/// Zero-width non-joiner, the Persian half-space.
pub const ZWNJ: char = '\u{200C}';

/// Normalise Persian text: Persian yeh and kaf, heh for teh marbuta and
/// heh with yeh, no short vowels or tatweel, ASCII digits, and one ZWNJ
/// with no spaces around it in place of any run of joiners and spaces that
/// contains one. Other text passes through unchanged.
#[must_use]
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    let mut joined = false;
    for c in text.chars() {
        let c = match c {
            // Arabic yeh and alef maksura -> Persian yeh.
            '\u{064A}' | '\u{0649}' => '\u{06CC}',
            // Arabic kaf -> Persian kaf (keheh).
            '\u{0643}' => '\u{06A9}',
            // Teh marbuta and heh with yeh above -> heh.
            '\u{0629}' | '\u{06C0}' => '\u{0647}',
            // Arabic-Indic and Persian digits -> ASCII.
            '\u{0660}'..='\u{0669}' => digit(c, 0x0660),
            '\u{06F0}'..='\u{06F9}' => digit(c, 0x06F0),
            // Short vowels, superscript alef, tatweel and other invisible
            // joiners are dropped.
            '\u{064B}'..='\u{065F}'
            | '\u{0670}'
            | '\u{0640}'
            | '\u{200B}'
            | '\u{200D}'
            | '\u{FEFF}' => continue,
            other => other,
        };
        if c == ZWNJ {
            joined = true;
            pending_space = false;
            continue;
        }
        if c == ' ' {
            pending_space = true;
            continue;
        }
        if joined {
            if !out.is_empty() {
                out.push(ZWNJ);
            }
        } else if pending_space && !out.is_empty() {
            out.push(' ');
        }
        joined = false;
        pending_space = false;
        out.push(c);
    }
    if pending_space && !joined {
        out.push(' ');
    }
    out
}

fn digit(c: char, zero: u32) -> char {
    char::from_digit(u32::from(c) - zero, 10).unwrap_or(c)
}

/// Whether `c` is an Arabic-script letter (Persian and Arabic blocks).
#[must_use]
pub fn is_arabic_script(c: char) -> bool {
    matches!(c, '\u{0600}'..='\u{06FF}' | '\u{0750}'..='\u{077F}' | '\u{FB50}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}')
        && c.is_alphabetic()
}

/// Whether most of the letters in `text` are Arabic script: Persian text,
/// for routing to the multilingual model and the Persian report.
#[must_use]
pub fn is_persian(text: &str) -> bool {
    let (mut script, mut letters) = (0usize, 0usize);
    for c in text.chars().filter(|c| c.is_alphabetic()) {
        letters += 1;
        if is_arabic_script(c) {
            script += 1;
        }
    }
    letters > 0 && script * 2 > letters
}

/// Arabic-script terms of at least three letters in normalised `text`,
/// with ZWNJ removed so the joined and unjoined spellings agree.
#[must_use]
pub fn terms(text: &str) -> Vec<String> {
    normalize(text)
        .split(|c: char| !(is_arabic_script(c) || c == ZWNJ))
        .map(|t| t.replace(ZWNJ, ""))
        .filter(|t| t.chars().count() >= 3)
        .collect()
}

/// ASCII digits as Persian digits, for reports that ask for them.
#[must_use]
pub fn persian_digits(text: &str) -> String {
    text.chars()
        .map(|c| match c.to_digit(10) {
            Some(d) if c.is_ascii_digit() => char::from_u32(0x06F0 + d).unwrap_or(c),
            _ => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arabic_forms_digits_and_diacritics_are_normalised() {
        // Arabic yeh, Arabic kaf, a fatha, tatweel and Persian digits.
        assert_eq!(normalize("كتابِ علمي ـ۱۲"), "کتاب علمی 12");
        assert_eq!(normalize("مدرسة"), "مدرسه");
        assert_eq!(normalize("٣٤ نفر"), "34 نفر");
        assert_eq!(normalize("plain English 42"), "plain English 42");
    }

    #[test]
    fn half_spaces_collapse_to_one_zwnj() {
        let joined = "می\u{200C}شود";
        assert_eq!(normalize("می \u{200C} شود"), joined);
        assert_eq!(normalize("می\u{200C}\u{200C}شود"), joined);
        assert_eq!(normalize(joined), joined);
        assert_eq!(normalize("دو  واژه "), "دو واژه ");
    }

    #[test]
    fn spellings_agree_as_terms() {
        assert_eq!(terms("می\u{200C}شود"), ["میشود"]);
        assert_eq!(terms("میشود"), ["میشود"]);
        assert_eq!(
            terms("خواب روزانه، توان بعدازظهر را بهبود می‌بخشد."),
            ["خواب", "روزانه", "توان", "بعدازظهر", "بهبود", "میبخشد"]
        );
        assert_eq!(terms("naps improve power"), Vec::<String>::new());
    }

    #[test]
    fn persian_is_detected_by_its_letters() {
        assert!(is_persian("خواب روزانه توان را بهبود می‌بخشد"));
        assert!(is_persian("اثر DPO بر مدل‌های زبانی"));
        assert!(!is_persian("Daytime naps improve power"));
        assert!(!is_persian("12 34"));
    }

    #[test]
    fn digits_can_be_shown_in_persian() {
        assert_eq!(persian_digits("run 2026-10"), "run ۲۰۲۶-۱۰");
    }
}
