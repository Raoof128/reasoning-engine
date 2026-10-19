//! The Persian report (`fa` feature, plan Phase 12): the same content as
//! [`crate::service::build_report`], right to left, for Persian queries.
//!
//! Statuses and the gate verdict are translated; run ids, profiles and
//! modes stay as they are, since tools read them. Sections added after
//! this one (scrutiny, references) stay in English, left to right, outside
//! the right-to-left block. `REASONING_ENGINE_PERSIAN_DIGITS=1` writes the
//! query and claims with Persian digits.

use re_model::records::{ClaimRecord, ResearchRun, VerificationRecord};
use serde_json::Value;

use crate::service::final_status;

/// Persian for a verification status; unknown statuses pass through.
#[must_use]
pub fn status(status: &str) -> &str {
    match status {
        "supported" => "پشتیبانی‌شده",
        "partially_supported" => "پشتیبانی جزئی",
        "contradicted" => "نقض‌شده",
        "unsupported" => "بدون پشتیبانی",
        "needs_more_evidence" => "نیازمند شواهد بیشتر",
        "not_verifiable" => "راستی‌آزمایی‌نشدنی",
        "hypothesis" => "فرضیه",
        "unverified" => "راستی‌آزمایی‌نشده",
        other => other,
    }
}

/// Persian for a quality-gate verdict; unknown verdicts pass through.
#[must_use]
pub fn gate_result(result: &str) -> &str {
    match result {
        "pass" => "پذیرفته",
        "pass_with_warnings" => "پذیرفته با هشدار",
        "blocked" => "متوقف",
        other => other,
    }
}

/// The Persian Markdown report.
#[must_use]
pub fn build_report(
    run: &ResearchRun,
    claims: &[ClaimRecord],
    verifications: &[VerificationRecord],
    gate: &Value,
    persian_digits: bool,
) -> String {
    let text = |s: &str| {
        if persian_digits {
            re_core::fa::persian_digits(s)
        } else {
            s.to_string()
        }
    };
    let result = gate
        .get("result")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut lines = vec![
        r#"<div dir="rtl" lang="fa">"#.to_string(),
        String::new(),
        format!("# گزارش پژوهش `{}`", run.run_id),
        String::new(),
        format!("- پرسش: {}", text(&run.query)),
        format!("- نمایه: `{}`", run.profile),
        format!("- حالت: `{}`", run.mode),
        format!("- دروازهٔ کیفیت: {} (`{result}`)", gate_result(result)),
        String::new(),
        "## ادعاها".to_string(),
        String::new(),
    ];
    for claim in claims {
        let status = final_status(claim, verifications);
        lines.push(format!(
            "- [{}] {}",
            self::status(status),
            text(&claim.text)
        ));
    }
    lines.push(String::new());
    lines.push("</div>".to_string());
    lines.join("\n") + "\n"
}
