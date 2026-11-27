//! The snippet cap is one setting for the process, so its test has a
//! binary to itself: changing it beside other tests would change their
//! records.

use re_retrieval::base::{
    LONG_SNIPPET_LIMIT, RecordInput, SNIPPET_LIMIT, build_record, set_snippet_limit, snippet_limit,
    snippet_limit_from_env,
};

fn snippet_of(chars: usize) -> String {
    let record = build_record(RecordInput {
        snippet: "word ".repeat(chars / 5),
        ..RecordInput::new("run_1", "source", "q", 1)
    });
    record.snippet
}

#[test]
fn the_cap_is_1500_until_set_and_is_clamped() {
    assert_eq!(snippet_limit(), SNIPPET_LIMIT);
    assert_eq!(snippet_of(6000).chars().count(), 1500);

    set_snippet_limit(LONG_SNIPPET_LIMIT);
    assert_eq!(snippet_of(6000).chars().count(), 4000);
    assert_eq!(
        snippet_of(1000).chars().count(),
        999,
        "shorter text is kept whole"
    );

    set_snippet_limit(5);
    assert_eq!(snippet_limit(), 200);
    set_snippet_limit(1_000_000);
    assert_eq!(snippet_limit(), 20_000);
    set_snippet_limit(SNIPPET_LIMIT);
}

#[test]
fn the_environment_overrides_the_default_only_with_a_sane_number() {
    let env = |value: Option<&'static str>| {
        move |var: &str| {
            (var == "REASONING_ENGINE_SNIPPET_CHARS")
                .then(|| value.map(str::to_string))
                .flatten()
        }
    };
    assert_eq!(snippet_limit_from_env(env(None), 4000), 4000);
    assert_eq!(snippet_limit_from_env(env(Some("1500")), 4000), 1500);
    assert_eq!(snippet_limit_from_env(env(Some(" 2500 ")), 1500), 2500);
    assert_eq!(snippet_limit_from_env(env(Some("lots")), 4000), 4000);
    assert_eq!(snippet_limit_from_env(env(Some("3")), 4000), 4000);
    assert_eq!(snippet_limit_from_env(env(Some("999999")), 1500), 1500);
}
