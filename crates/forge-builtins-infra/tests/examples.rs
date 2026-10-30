//! Every pack example of a rule whose check calls one of these builtins holds when run through
//! the builtin, in a repository holding the example, with the context the checker would give it
//! (base copies, changed files, the check's files, accepted changes). See tests/common for the
//! tools (`FORGE_TEST_TOOLS`).

mod common;

use common::{Case, cases, ctx, gate, repo, tools};
use forge_builtins_infra::{Options, names};
use forge_core::Report;

fn run(case: &Case, opts: &Options) -> (Report, bool) {
    let r = repo(case);
    let Some(files) = gate(case, &r, opts) else {
        return (Report::default(), false);
    };
    let ctx = ctx(case, &r);
    let extra = common::check(Some(case.rule.clone()), files);
    let report = forge_builtins_infra::run(opts, &case.builtin, &ctx, &extra, &case.args)
        .expect("a builtin of this crate");
    (report, true)
}

#[test]
fn pack_examples_hold() {
    let (opts, _dirs, mode) = tools();
    let all = cases();
    let mut failures = vec![];
    let mut used = std::collections::BTreeSet::new();
    for case in &all {
        let (report, ran) = run(case, &opts);
        used.insert(case.builtin.clone());
        let found = !report.findings.is_empty();
        let text = report.agent_text(200);
        for (_, term) in forge_core::internals::find(&text) {
            failures.push(format!("{}: agent text names {term}: {text}", case.name));
        }
        if text.contains("skipped") {
            failures.push(format!("{}: agent text says skipped: {text}", case.name));
        }
        let missing = report
            .notes
            .iter()
            .any(|n| n.kind == forge_core::NoteKind::MissingTool);
        if mode != "fake" && missing {
            eprintln!("{}: not checked here: {:?}", case.name, report.notes);
            continue;
        }
        if found != case.expect || (mode == "fake" && !report.notes.is_empty()) {
            failures.push(format!(
                "{}: expected {}, got (ran: {ran})\n{text}\nnotes: {:?}",
                case.name,
                if case.expect { "findings" } else { "none" },
                report.notes
            ));
        }
    }
    eprintln!(
        "{} pack examples of {} builtins, tools: {mode}",
        all.len(),
        used.len()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    // every builtin has pack examples
    let unused: Vec<&str> = names().into_iter().filter(|n| !used.contains(*n)).collect();
    assert!(
        unused.is_empty(),
        "builtins no pack example runs: {unused:?}"
    );
}
