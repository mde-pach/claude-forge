//! `python/no-dead-code` (`forge_condor`) on its pack examples, with condor's stand-in
//! (`tests/fake_condor.py`), or with the pinned cndr itself when `FORGE_REAL_CONDOR` is set
//! (opt-in: it downloads cndr through uvx). Each example is decided by condor's findings, never
//! by a note that it could not run.

mod common;

use std::path::Path;
use std::sync::Arc;

use common::*;
use forge_builtins_data::{Mode, Options, ToolRun};
use forge_check::builtins::{BuiltinTools, CheckRule};

/// The examples' tools, with ruff able to lint or not.
struct WithRuff(bool);

impl BuiltinTools for WithRuff {
    fn run(&self, root: &Path, cwd: &Path, tool: &str, op: &str, args: &[String]) -> ToolRun {
        LocalProjectTools.run(root, cwd, tool, op, args)
    }

    fn pinned(&self, tool: &str) -> Result<Vec<String>, String> {
        LocalProjectTools.pinned(tool)
    }

    fn available(&self, _root: &Path, _cwd: &Path, tool: &str, op: &str) -> bool {
        self.0 && (tool, op) == ("ruff", "lint")
    }
}

/// The texts `forge_condor` reports for a new unused import, with `checks` active.
fn import_findings(ruff: bool, checks: &[&str]) -> Vec<String> {
    let head = [("app/main.py", "def main():\n    return 1\n\n\nmain()\n")];
    let now = [(
        "app/main.py",
        "import os\n\n\ndef main():\n    return 1\n\n\nmain()\n",
    )];
    let map = |p: &[(&str, &str)]| {
        p.iter()
            .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
            .collect()
    };
    let r = repo(&map(&head), &map(&now));
    let set = forge_packs::load_builtin(forge_packs::Options::default()).unwrap();
    let rule = set
        .packs
        .values()
        .flat_map(|p| &p.rules)
        .find(|r| r.id == "python/no-dead-code")
        .unwrap()
        .clone();
    let mut check = facts(Mode::Changed, r.existing());
    check.check = Some(CheckRule { rule, dirs: None });
    check.checks = checks.iter().map(|c| (*c).to_owned()).collect();
    let opts = Options {
        tools: Arc::new(WithRuff(ruff)),
        ..Options::default()
    };
    let all = forge_builtins_data::builtins(opts);
    let b = all.iter().find(|b| b.name() == "condor").unwrap();
    let report = b.run(&r.ctx("python/no-dead-code", vec![]), &check, &[]);
    assert!(report.notes.is_empty(), "{:?}", report.notes);
    report.findings.iter().map(|f| f.agent_text()).collect()
}

/// An unused import is ruff's F401 when the run lints with ruff, condor's otherwise.
#[test]
fn unused_imports_are_left_to_ruff_when_it_lints() {
    assert!(import_findings(true, &["python/lint", "python/no-dead-code"]).is_empty());
    let own = import_findings(false, &["python/lint", "python/no-dead-code"]);
    assert_eq!(own.len(), 1, "{own:?}");
    assert!(
        own[0].starts_with("app/main.py:1 import `os` is unused"),
        "{own:?}"
    );
    assert_eq!(import_findings(true, &["python/no-dead-code"]).len(), 1);
}

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
