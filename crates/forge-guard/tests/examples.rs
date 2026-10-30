//! Every guard rule's pack examples, through `forge_guard::examples` (what `forge test` runs):
//! a `match` example gets one of the rule's decisions, a `no_match` example gets none.

mod common;

use common::*;
use forge_core::examples::ExampleVerdict;

#[test]
fn guard_rules_pass_their_pack_examples() {
    let mut n = 0;
    let mut failures = Vec::new();
    for r in guard_rules() {
        for res in forge_guard::examples::run(r) {
            n += 1;
            match &res.verdict {
                ExampleVerdict::Pass => {}
                ExampleVerdict::Fail { why } => failures.push(format!("{}: {why}", res.label())),
                ExampleVerdict::Skip { note } => {
                    failures.push(format!("{}: skipped: {}", res.label(), note.text));
                }
            }
        }
    }
    assert!(n > 600, "{n} examples");
    assert!(failures.is_empty(), "{failures:#?}");
}
