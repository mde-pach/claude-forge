//! The project's own whole-project tools (`forge_knip`, `forge_depcruise`, `forge_import_linter`,
//! `forge_jscpd`) on their rules' examples, in a turn as the checker runs it at Stop: a fixture
//! repository with the change, its base copies, the turn snapshot, and the findings the builtin
//! attributes itself (`$FORGE_ATTRIBUTED`), taken as they are.
//!
//! The tools are the project's own, which a fixture does not install: with
//! `FORGE_TEST_PROJECT_TOOLS=<dir>`, `<dir>/node_modules` (knip, dependency-cruiser, jscpd,
//! typescript) and `<dir>/.venv` (import-linter) are linked into each fixture, ignored by git as
//! a project's are, and every example must hold, with no note that a tool could not run.
//! Without it, every example must be skipped with a missing-tool note and find nothing: a
//! project that set a tool up without installing it is told, never failed.

mod common;

use std::path::Path;
use std::time::Instant;

use common::{Case, Repo, options, pack_cases};
use forge_builtins_code::Extra;
use forge_builtins_code::ctx::CheckRule;
use forge_check::builtins::RunMode;
use forge_core::NoteKind;

const BUILTINS: &[&str] = &["knip", "depcruise", "import_linter", "jscpd"];

/// The case's repository, `.gitignore` ignoring installed dependencies as a project's does,
/// and the tools linked in.
fn repo(case: &Case, tools: Option<&Path>) -> Repo {
    let mut head = case.head.clone();
    head.entry(".gitignore".into())
        .or_insert_with(|| "node_modules/\n.venv/\n".into());
    let repo = Repo::new(&head, &case.files);
    if let Some(dir) = tools {
        for d in ["node_modules", ".venv"] {
            if dir.join(d).exists() {
                std::os::unix::fs::symlink(dir.join(d), repo.root.join(d)).unwrap();
            }
        }
    }
    repo
}

#[test]
fn project_tools_examples_hold() {
    let tools = std::env::var_os("FORGE_TEST_PROJECT_TOOLS").map(std::path::PathBuf::from);
    let opts = options();
    let (rules, cases) = pack_cases(BUILTINS);
    assert_eq!(
        rules.len(),
        4,
        "{:?}",
        rules.iter().map(|r| &r.id).collect::<Vec<_>>()
    );
    let mut failures = vec![];
    for case in &cases {
        let repo = repo(case, tools.as_deref());
        let state = tempfile::tempdir().unwrap();
        let x = Extra {
            mode: RunMode::Turn,
            snapshot: Some(state.path().join("snapshot")),
            attributed: Some(state.path().join("attributed")),
            check: Some(CheckRule {
                rule: case.rule.clone(),
                dirs: None,
            }),
            ..repo.extra(&case.rule)
        };
        std::fs::create_dir_all(state.path().join("snapshot")).unwrap();
        let ctx = repo.ctx(&case.rule.id);
        let started = Instant::now();
        let report = forge_builtins_code::run(&opts, &case.name, &ctx, &x, &case.args)
            .expect("a builtin of this crate");
        let took = started.elapsed();
        let texts: Vec<String> = report.findings.iter().map(|f| f.agent_text()).collect();
        eprintln!(
            "{} ({:.2}s): {texts:?} {:?}",
            case.label,
            took.as_secs_f64(),
            report.notes.iter().map(|n| &n.text).collect::<Vec<_>>()
        );
        for t in &texts {
            assert!(
                forge_core::internals::find(t).is_empty(),
                "{}: {t}",
                case.label
            );
        }
        if tools.is_none() {
            if !texts.is_empty() || !report.notes.iter().any(|n| n.kind == NoteKind::MissingTool) {
                failures.push(format!(
                    "{}: without the tool, expected a missing-tool note and nothing found, got {texts:?} {:?}",
                    case.label, report.notes
                ));
            }
            continue;
        }
        if let Some(n) = report.notes.iter().find(|n| {
            matches!(
                n.kind,
                NoteKind::MissingTool
                    | NoteKind::ForgeFailure
                    | NoteKind::NotEnforced
                    | NoteKind::Environment
            )
        }) {
            failures.push(format!("{}: note {:?}: {}", case.label, n.kind, n.text));
            continue;
        }
        if case.want == Some(texts.is_empty()) {
            failures.push(format!(
                "{}: expected {}, got {texts:?}",
                case.label,
                if texts.is_empty() {
                    "a finding"
                } else {
                    "nothing"
                }
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(cases.len() >= 16, "{} examples", cases.len());
}
