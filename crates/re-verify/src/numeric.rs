//! Numeric consistency between claims and evidence (`trust` feature, plan
//! section 6.6).
//!
//! A `nom` grammar pulls quantities out of free text: integers and
//! decimals (with thousands separators and Persian or Arabic-Indic
//! digits), percentages, ranges (`12-15%`, `12 to 15`), confidence
//! intervals (`95% CI 1.2-1.8`), comparatives (`reduced by 30%`,
//! `increased from 4 to 9`) and units (`ms`, `s`, `%`, `x`, `points`).
//! Seconds become milliseconds; everything else keeps its unit.
//!
//! [`enforce`] applies the rule: a quantitative claim's numbers must appear,
//! within 2% relative tolerance and with a compatible unit, in the evidence
//! it cites. When none does, the verdict drops one step: `supported` to
//! `partially_supported`, and `partially_supported` (weak overlap already)
//! to `unsupported`, which cites nothing and so blocks the quality gate. An
//! invented figure on loosely related evidence is not partial support. Bare years (1900-2100 with
//! no unit) and confidence levels are ignored on the claim side, so
//! "in 2024" or "95% CI" can never be the number that matches.

use std::fmt::Write as _;

use nom::branch::alt;
use nom::bytes::complete::{tag, tag_no_case, take_while_m_n};
use nom::character::complete::{char, digit1, space0, space1};
use nom::combinator::{not, opt, peek, recognize};
use nom::multi::many0;
use nom::sequence::{pair, preceded};
use nom::{IResult, Parser};
use re_model::records::{ClaimRecord, EvidenceRecord, VerificationRecord};

/// Relative tolerance for a match.
pub const TOLERANCE: f64 = 0.02;
/// Rationale when the cap applies.
pub const RATIONALE: &str = "numeric value not found in evidence";

/// A normalised unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// No unit.
    None,
    /// Percent.
    Percent,
    /// Percentage points.
    Points,
    /// Milliseconds (seconds are converted).
    Millis,
    /// A multiplier (`3x`, `3×`, `3-fold`).
    Times,
}

/// Where a number came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// A plain value.
    Value,
    /// One end of a range or interval.
    Bound,
    /// A confidence level, e.g. the `95` in `95% CI`.
    Level,
}

/// One extracted quantity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quantity {
    /// The normalised value.
    pub value: f64,
    /// Its unit.
    pub unit: Unit,
    /// Its role.
    pub role: Role,
}

impl Quantity {
    fn new(value: f64, unit: Unit, role: Role) -> Self {
        Self { value, unit, role }
    }

    /// A bare year (1900-2100, integral, no unit).
    #[must_use]
    pub fn is_year(&self) -> bool {
        self.unit == Unit::None
            && self.value.fract() == 0.0
            && (1900.0..=2100.0).contains(&self.value)
    }
}

type Res<'a, T> = IResult<&'a str, T>;

/// `,234`: exactly three digits, so `1,2` is two numbers, not twelve.
fn thousands(input: &str) -> Res<'_, &str> {
    recognize((
        char(','),
        take_while_m_n(3, 3, |c: char| c.is_ascii_digit()),
        not(peek(digit1)),
    ))
    .parse(input)
}

/// `1,234.5`, `-3`, `.5` as a number.
fn number(input: &str) -> Res<'_, f64> {
    let (rest, text) = recognize((
        opt(alt((char('-'), char('−'), char('+')))),
        alt((
            recognize((digit1, many0(thousands), opt(pair(char('.'), digit1)))),
            recognize(pair(char('.'), digit1)),
        )),
    ))
    .parse(input)?;
    let cleaned: String = text.replace([',', '+'], "").replace('−', "-");
    match cleaned.parse::<f64>() {
        Ok(value) => Ok((rest, value)),
        Err(_) => Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Float,
        ))),
    }
}

/// A unit after a number, or none. Word units must end at a word boundary.
/// Never fails: no unit is `(Unit::None, 1.0)` with nothing consumed.
fn unit(input: &str) -> (&str, (Unit, f64)) {
    let word_end = |rest: &str| !rest.starts_with(|c: char| c.is_alphanumeric());
    let attempts: [(&str, Unit, f64, bool); 15] = [
        ("%", Unit::Percent, 1.0, false),
        (" %", Unit::Percent, 1.0, false),
        (" درصد", Unit::Percent, 1.0, true),
        (" percentage points", Unit::Points, 1.0, true),
        (" percentage point", Unit::Points, 1.0, true),
        (" percent", Unit::Percent, 1.0, true),
        (" per cent", Unit::Percent, 1.0, true),
        (" points", Unit::Points, 1.0, true),
        (" pp", Unit::Points, 1.0, true),
        (" ms", Unit::Millis, 1.0, true),
        ("ms", Unit::Millis, 1.0, true),
        (" seconds", Unit::Millis, 1000.0, true),
        (" s", Unit::Millis, 1000.0, true),
        ("×", Unit::Times, 1.0, false),
        ("-fold", Unit::Times, 1.0, true),
    ];
    for (text, unit, scale, needs_end) in attempts {
        if let Ok((rest, _)) = tag_no_case::<_, _, nom::error::Error<&str>>(text).parse(input)
            && (!needs_end || word_end(rest))
        {
            return (rest, (unit, scale));
        }
    }
    // `3x` and `3 x` but not `3 xylophones`.
    if let Ok((rest, _)) = (space0::<_, nom::error::Error<&str>>, tag_no_case("x")).parse(input)
        && word_end(rest)
    {
        return (rest, (Unit::Times, 1.0));
    }
    (input, (Unit::None, 1.0))
}

/// A number with its unit, scaled.
fn measure(input: &str) -> Res<'_, (f64, Unit)> {
    let (rest, value) = number(input)?;
    let (rest, (unit, scale)) = unit(rest);
    Ok((rest, (value * scale, unit)))
}

fn range_separator(input: &str) -> Res<'_, &str> {
    alt((
        recognize((space0, alt((tag("-"), tag("–"), tag("—"))), space0)),
        recognize((space1, tag_no_case("to"), space1)),
    ))
    .parse(input)
}

/// `a-b`, `a to b`, `a% - b%`. A unit on either end applies to both.
fn range(input: &str) -> Res<'_, Vec<Quantity>> {
    let (rest, (low, low_unit)) = measure(input)?;
    let (rest, _) = range_separator(rest)?;
    let (rest, (high, high_unit)) = measure(rest)?;
    let unit = if high_unit == Unit::None {
        low_unit
    } else {
        high_unit
    };
    // `12-15%`: the first end is 12 percent, not 12.
    Ok((
        rest,
        vec![
            Quantity::new(low, unit, Role::Bound),
            Quantity::new(high, unit, Role::Bound),
        ],
    ))
}

/// `95% CI 1.2-1.8`, `95% CI: 1.2 to 1.8`, `95% confidence interval 1.2-1.8`.
fn interval(input: &str) -> Res<'_, Vec<Quantity>> {
    let (rest, (level, _)) = measure(input)?;
    let (rest, _) = (
        space0,
        alt((tag_no_case("ci"), tag_no_case("confidence interval"))),
        opt(char(':')),
        space0,
        opt(char('(')),
    )
        .parse(rest)?;
    let (rest, mut bounds) = range(rest)?;
    let (rest, _) = opt(char(')')).parse(rest)?;
    bounds.insert(0, Quantity::new(level, Unit::Percent, Role::Level));
    Ok((rest, bounds))
}

/// `from 4 to 9` (the comparative's two values).
fn from_to(input: &str) -> Res<'_, Vec<Quantity>> {
    let (rest, (a, a_unit)) = preceded((tag_no_case("from"), space1), measure).parse(input)?;
    let (rest, (b, b_unit)) = preceded((space1, tag_no_case("to"), space1), measure).parse(rest)?;
    let unit = if b_unit == Unit::None { a_unit } else { b_unit };
    let a_unit = if a_unit == Unit::None { unit } else { a_unit };
    Ok((
        rest,
        vec![
            Quantity::new(a, a_unit, Role::Value),
            Quantity::new(b, unit, Role::Value),
        ],
    ))
}

fn single(input: &str) -> Res<'_, Vec<Quantity>> {
    let (rest, (value, unit)) = measure(input)?;
    Ok((rest, vec![Quantity::new(value, unit, Role::Value)]))
}

/// One numeric expression at the start of `input`.
fn expression(input: &str) -> Res<'_, Vec<Quantity>> {
    alt((from_to, interval, range, single)).parse(input)
}

/// The ASCII digit for `0..=9`.
fn digit(value: u32) -> char {
    char::from_digit(value, 10).unwrap_or('0')
}

/// Persian and Arabic-Indic digits to ASCII, `٫` to `.`, `٬` to `,`.
fn ascii_digits(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '۰'..='۹' => digit(u32::from(c) - u32::from('۰')),
            '٠'..='٩' => digit(u32::from(c) - u32::from('٠')),
            '٫' => '.',
            '٬' => ',',
            other => other,
        })
        .collect()
}

/// Every quantity in `text`, in order.
#[must_use]
pub fn extract(text: &str) -> Vec<Quantity> {
    let text = ascii_digits(text);
    let mut out = Vec::new();
    let mut rest = text.as_str();
    // The two characters before `rest`.
    let mut previous: (Option<char>, Option<char>) = (None, None);
    while let Some(c) = rest.chars().next() {
        // Start only at a word boundary: not inside `v2`, `CO2` or `GPT-4`.
        let boundary = match previous {
            (_, None) => true,
            (before, Some('-' | '−')) => before.is_none_or(|b| !b.is_alphanumeric()),
            (_, Some(p)) => !p.is_alphanumeric() && p != '.',
        };
        let starts = c.is_ascii_digit()
            || ((c == '-' || c == '−' || c == '.')
                && rest[c.len_utf8()..].starts_with(|d: char| d.is_ascii_digit()))
            || (c == 'f' || c == 'F');
        if boundary
            && starts
            && let Ok((after, found)) = expression(rest)
            && after.len() < rest.len()
        {
            out.extend(found);
            let mut consumed = rest[..rest.len() - after.len()].chars().rev();
            let last = consumed.next();
            previous = (consumed.next().or(previous.1), last);
            rest = after;
            continue;
        }
        previous = (previous.1, Some(c));
        rest = &rest[c.len_utf8()..];
    }
    out
}

fn compatible(a: Unit, b: Unit) -> bool {
    a == b
        || matches!(
            (a, b),
            (Unit::Percent, Unit::Points) | (Unit::Points, Unit::Percent)
        )
}

/// Whether two quantities agree within [`TOLERANCE`].
#[must_use]
pub fn matches(a: &Quantity, b: &Quantity) -> bool {
    if !compatible(a.unit, b.unit) {
        return false;
    }
    let scale = a.value.abs().max(b.value.abs());
    scale == 0.0 || (a.value - b.value).abs() <= TOLERANCE * scale
}

/// The claim's quantities that a match can rest on: no years, no levels.
#[must_use]
pub fn claim_quantities(text: &str) -> Vec<Quantity> {
    extract(text)
        .into_iter()
        .filter(|q| q.role != Role::Level && !q.is_year())
        .collect()
}

/// Whether any of the claim's numbers appears in `evidence_text`. `None`
/// when the claim has no checkable number.
#[must_use]
pub fn supported_by(claim_text: &str, evidence_text: &str) -> Option<bool> {
    let wanted = claim_quantities(claim_text);
    if wanted.is_empty() {
        return None;
    }
    let found = extract(evidence_text);
    Some(wanted.iter().any(|w| found.iter().any(|f| matches(w, f))))
}

/// Drop quantitative claims whose numbers are not in their cited evidence
/// one step (`supported` to `partially_supported`, `partially_supported` to
/// `unsupported`). Returns how many were downgraded.
pub fn enforce(
    claims: &mut [ClaimRecord],
    verifications: &mut [VerificationRecord],
    evidence: &[EvidenceRecord],
) -> usize {
    let mut capped = 0;
    for verification in verifications.iter_mut() {
        let downgraded = match verification.support_status.as_str() {
            "supported" => "partially_supported",
            "partially_supported" => "unsupported",
            _ => continue,
        };
        let Some(claim) = claims
            .iter_mut()
            .find(|c| c.claim_id == verification.claim_id)
        else {
            continue;
        };
        if claim.claim_type != "quantitative" {
            continue;
        }
        let cited = evidence
            .iter()
            .filter(|e| verification.evidence_ids.contains(&e.evidence_id))
            .fold(String::new(), |mut text, e| {
                let _ = writeln!(text, "{}\n{}", e.title, e.snippet);
                text
            });
        if supported_by(&claim.text, &cited) == Some(false) {
            verification.support_status = downgraded.into();
            verification.support_rationale =
                format!("{} ({RATIONALE})", verification.support_rationale);
            verification.missing_evidence = Some(RATIONALE.into());
            verification.requires_human_review = true;
            claim.status = downgraded.into();
            if downgraded == "unsupported" {
                // Like the lexical verifier's own `unsupported`: no citation.
                verification.evidence_ids.clear();
                claim.evidence_ids.clear();
            }
            capped += 1;
        }
    }
    capped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(value: f64, unit: Unit) -> (f64, Unit) {
        (value, unit)
    }

    fn parsed(text: &str) -> Vec<(f64, Unit)> {
        extract(text)
            .into_iter()
            .map(|x| (x.value, x.unit))
            .collect()
    }

    #[test]
    fn thirty_phrases_parse_as_expected() {
        use Unit::{Millis, None as N, Percent, Points, Times};
        let cases: [(&str, Vec<(f64, Unit)>); 30] = [
            ("accuracy was 12%", vec![q(12.0, Percent)]),
            ("accuracy was 12 %", vec![q(12.0, Percent)]),
            ("rose to 12.5 percent", vec![q(12.5, Percent)]),
            ("fell 3 per cent", vec![q(3.0, Percent)]),
            ("between 12-15%", vec![q(12.0, Percent), q(15.0, Percent)]),
            (
                "between 12% and 15%",
                vec![q(12.0, Percent), q(15.0, Percent)],
            ),
            ("12 to 15 points", vec![q(12.0, Points), q(15.0, Points)]),
            ("a 2.5 pp gain", vec![q(2.5, Points)]),
            ("up 4 percentage points", vec![q(4.0, Points)]),
            (
                "OR 1.4 (95% CI 1.2-1.8)",
                vec![q(1.4, N), q(95.0, Percent), q(1.2, N), q(1.8, N)],
            ),
            (
                "95% CI: 0.8 to 1.1",
                vec![q(95.0, Percent), q(0.8, N), q(1.1, N)],
            ),
            (
                "95% confidence interval 2–4",
                vec![q(95.0, Percent), q(2.0, N), q(4.0, N)],
            ),
            ("latency reduced by 30%", vec![q(30.0, Percent)]),
            ("increased from 4 to 9", vec![q(4.0, N), q(9.0, N)]),
            (
                "increased from 40% to 55%",
                vec![q(40.0, Percent), q(55.0, Percent)],
            ),
            (
                "dropped from 120 ms to 80 ms",
                vec![q(120.0, Millis), q(80.0, Millis)],
            ),
            ("took 1.5 s", vec![q(1500.0, Millis)]),
            ("took 2 seconds", vec![q(2000.0, Millis)]),
            ("p95 under 250ms", vec![q(250.0, Millis)]),
            ("a 3x speed-up", vec![q(3.0, Times)]),
            ("a 3 x speed-up", vec![q(3.0, Times)]),
            ("3× faster", vec![q(3.0, Times)]),
            ("a 10-fold rise", vec![q(10.0, Times)]),
            (
                "1,234 samples, not 1,2",
                vec![q(1234.0, N), q(1.0, N), q(2.0, N)],
            ),
            ("an effect of -0.4", vec![q(-0.4, N)]),
            ("about .5 of runs", vec![q(0.5, N)]),
            ("نرخ ۳۰ درصد کاهش یافت", vec![q(30.0, Percent)]),
            ("GPT-4 and CO2 are not numbers here", vec![]),
            ("3 xylophones", vec![q(3.0, N)]),
            (
                "in 2024, 7 of 10 sites",
                vec![q(2024.0, N), q(7.0, N), q(10.0, N)],
            ),
        ];
        for (text, expected) in cases {
            assert_eq!(parsed(text), expected, "{text}");
        }
    }

    #[test]
    fn matching_uses_tolerance_units_and_ignores_years_and_levels() {
        assert_eq!(
            supported_by("improved accuracy by 12%", "a 12.2% gain"),
            Some(true)
        );
        assert_eq!(
            supported_by("improved accuracy by 12%", "a 13% gain"),
            Some(false)
        );
        assert_eq!(
            supported_by("cut latency to 1.5 s", "p50 of 1500 ms"),
            Some(true)
        );
        assert_eq!(
            supported_by("cut latency by 12%", "12 ms faster"),
            Some(false),
            "units"
        );
        assert_eq!(
            supported_by("in 2024 the rate rose", "published 2024"),
            None,
            "years"
        );
        assert_eq!(
            supported_by("OR 1.4 (95% CI 1.2-1.8)", "the 95% CI excluded the null"),
            Some(false),
            "a confidence level alone is not a match"
        );
        assert_eq!(supported_by("no numbers here", "12%"), None);
    }
}
