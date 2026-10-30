//! `python/no-dead-code` (`forge_condor`) on its pack examples, with condor's stand-in
//! (`tests/fake_condor.py`), or with the pinned cndr itself when `FORGE_REAL_CONDOR` is set
//! (opt-in: it downloads cndr through uvx). Each example is decided by condor's findings, never
//! by a note that it could not run.

mod common;

use common::*;

#[test]
fn condor_examples_are_decided_by_findings() {
    let real = std::env::var_os("FORGE_REAL_CONDOR").is_some();
    if real && !have("uvx", &["--version"]) {
        panic!("FORGE_REAL_CONDOR is set but uvx is missing");
    }
    let cases: Vec<Case> = pack_cases()
        .into_iter()
        .filter(|c| c.rule == "python/no-dead-code")
        .collect();
    assert!(cases.len() >= 8, "{} examples", cases.len());
    let mut failures = vec![];
    for case in &cases {
        let r = repo(&case.head, &case.files);
        let out = port(case, &r);
        if !out.notes.is_empty() {
            failures.push(format!("{}: notes {:?}", case.name, out.notes));
        }
        if case.want == Some(out.counted.is_empty()) {
            failures.push(format!("{}: got {:?}", case.name, out.counted));
        }
    }
    assert!(
        failures.is_empty(),
        "{} condor:\n{}",
        if real { "real" } else { "stand-in" },
        failures.join("\n")
    );
}
