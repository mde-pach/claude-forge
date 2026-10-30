//! Every pack example of a rule whose check calls one of this crate's builtins holds when the
//! builtin runs on it as the checker would run it (pre-commit: the change against HEAD, the
//! check's files the in-scope changed ones, a lint check's findings attributed to the change):
//! a `match` example finds something, a `no_match` one nothing. Agent text never names Forge's
//! internals, and never says "skipped".
//!
//! A tool the machine lacks (uvx for Squawk and sqlfluff) skips the examples that need it.

mod common;

use common::*;

#[test]
fn pack_examples_hold() {
    let uvx = have("uvx", &["--version"]);
    let (mut ran, mut skipped) = (0, vec![]);
    let mut failures = vec![];
    for case in pack_cases() {
        if case.needs_uvx() && !uvx {
            skipped.push(case.name.clone());
            continue;
        }
        let r = repo(&case.head, &case.files);
        let out = port(&case, &r);
        for t in &out.texts {
            assert!(
                forge_core::internals::find(t).is_empty(),
                "{}: {t}",
                case.name
            );
            assert!(!t.contains("skipped"), "{}: {t}", case.name);
        }
        ran += 1;
        let want = case.want.expect("pack examples say what they expect");
        if want == out.counted.is_empty() {
            failures.push(format!(
                "{}: expected {}, got {:?} (notes {:?})",
                case.name,
                if want { "a finding" } else { "nothing" },
                out.counted,
                out.notes
            ));
        }
    }
    eprintln!("{ran} examples ran; skipped (no uvx): {skipped:?}");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(ran >= 100, "only {ran} examples ran");
}
