//! Every pack rule whose check calls one of this crate's builtins: its examples hold when the
//! builtin runs on them, in a fixture repository with the context the checker builds (a turn:
//! the changed files, their base copies, the added lines; lint attribution to added lines).
//!
//! A builtin that needs a pinned tool which is not installed writes a note: its examples are
//! counted as not run, never as passed. `FORGE_TEST_INSTALL_TOOLS=1` installs the pinned tools
//! into Forge's cache first.

mod common;

use common::{not_run, options, pack_cases, run_port};
use forge_builtins_code::code_builtins;

#[test]
fn pack_examples_hold() {
    let opts = options();
    let names: Vec<&str> = code_builtins(&opts).iter().map(|b| b.name()).collect();
    let (rules, cases) = pack_cases(&names);
    let (mut ran, mut skipped, mut failures) = (0, vec![], vec![]);
    for r in &rules {
        if r.examples.is_none() {
            failures.push(format!("{}: no examples", r.id));
        }
    }
    for case in &cases {
        let repo = case.repo();
        let (lines, notes) = run_port(&opts, case, &repo);
        if let Some(n) = not_run(&notes) {
            skipped.push(format!("{}: {}", case.label, n.text));
            continue;
        }
        ran += 1;
        for l in &lines {
            assert!(
                forge_core::internals::find(l).is_empty(),
                "{}: internals in {l:?}",
                case.label
            );
        }
        if case.want == Some(lines.is_empty()) {
            failures.push(format!(
                "{}: {} (findings {lines:?}, notes {notes:?})",
                case.label,
                if lines.is_empty() {
                    "nothing found"
                } else {
                    "found"
                }
            ));
        }
    }
    eprintln!(
        "{} rules, {ran} examples run, {} not run:\n  {}",
        rules.len(),
        skipped.len(),
        skipped.join("\n  ")
    );
    assert!(
        rules.len() >= 13,
        "{} rules use this crate's builtins",
        rules.len()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
