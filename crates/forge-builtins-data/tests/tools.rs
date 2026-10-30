//! Builtins that run tools: what they ask of the checker's adapters, and that a missing tool or
//! a failed run is a note for the person (the check passes), never agent text.

mod common;

use std::path::Path;
use std::sync::{Arc, Mutex};

use common::*;
use forge_builtins_data::{Mode, Options, ToolOutput, ToolRun, builtins};
use forge_check::builtins::BuiltinTools;
use forge_core::{NoteKind, Report};

/// Answers each (tool, op) with a canned run and records the calls.
struct Fake {
    answers: Vec<(&'static str, &'static str, ToolRun)>,
    calls: Mutex<Vec<String>>,
}

impl BuiltinTools for Fake {
    fn pinned(&self, tool: &str) -> Result<Vec<String>, String> {
        Err(format!("{tool} is not installed"))
    }

    fn run(&self, _root: &Path, _cwd: &Path, tool: &str, op: &str, args: &[String]) -> ToolRun {
        self.calls
            .lock()
            .unwrap()
            .push(format!("{tool} {op} {}", args.join(" ")));
        self.answers
            .iter()
            .find(|(t, o, _)| *t == tool && *o == op)
            .map(|(_, _, r)| r.clone())
            .unwrap_or_else(|| ToolRun::Missing(format!("{tool} is not installed")))
    }
}

fn ran(status: i32, stdout: &str) -> ToolRun {
    ToolRun::Ran(ToolOutput {
        status,
        stdout: stdout.into(),
        stderr: String::new(),
    })
}

fn run_with(
    fake: Arc<Fake>,
    name: &str,
    args: &[&str],
    head: &[(&str, &str)],
    now: &[(&str, &str)],
) -> Report {
    let f = |p: &[(&str, &str)]| {
        p.iter()
            .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
            .collect()
    };
    let r = repo(&f(head), &f(now));
    let opts = Options {
        tools: fake,
        ..Options::default()
    };
    let check = facts(Mode::Changed, r.existing());
    let all = builtins(opts);
    let b = all.iter().find(|b| b.name() == name).unwrap();
    let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
    b.run(&r.ctx("test/rule", vec![]), &check, &args)
}

const REV: &str =
    "revision = \"0002\"\ndown_revision = \"0001\"\n\ndef upgrade() -> None:\n    pass\n";

#[test]
fn alembic_sql_fails_to_render_is_a_note() {
    let fake = Arc::new(Fake {
        answers: vec![(
            "alembic",
            "upgrade-sql",
            ToolRun::Ran(ToolOutput {
                status: 255,
                stdout: String::new(),
                stderr: "FAILED: no config\n".into(),
            }),
        )],
        calls: Mutex::new(vec![]),
    });
    let rep = run_with(
        fake.clone(),
        "squawk_alembic",
        &["alembic/versions/0002_x.py"],
        &[],
        &[("alembic/versions/0002_x.py", REV)],
    );
    assert!(rep.findings.is_empty());
    assert_eq!(rep.notes.len(), 1);
    assert!(rep.notes[0].text.contains("FAILED: no config"));
    assert_eq!(
        fake.calls.lock().unwrap()[0],
        "alembic upgrade-sql 0001:0002"
    );
}

#[test]
fn squawk_missing_is_a_note() {
    let fake = Arc::new(Fake {
        answers: vec![(
            "alembic",
            "upgrade-sql",
            ran(0, "ALTER TABLE t RENAME COLUMN a TO b;\n"),
        )],
        calls: Mutex::new(vec![]),
    });
    let rep = run_with(
        fake,
        "squawk_alembic",
        &["alembic/versions/0002_x.py"],
        &[],
        &[("alembic/versions/0002_x.py", REV)],
    );
    assert!(rep.findings.is_empty());
    assert_eq!(rep.notes[0].kind, NoteKind::MissingTool);
    let rep = run_with(
        Arc::new(Fake {
            answers: vec![],
            calls: Mutex::new(vec![]),
        }),
        "squawk_files",
        &["db/migrations/1.sql"],
        &[],
        &[(
            "db/migrations/1.sql",
            "ALTER TABLE t RENAME COLUMN a TO b;\n",
        )],
    );
    assert!(rep.findings.is_empty() && rep.notes[0].kind == NoteKind::MissingTool);
}

#[test]
fn typeorm_pending() {
    let ds = [(
        "forge.yaml",
        "files: {typeorm-datasource: {set: [src/data-source.ts]}}\n",
    )];
    // no data source named: a setup note
    let none = Arc::new(Fake {
        answers: vec![],
        calls: Mutex::new(vec![]),
    });
    let rep = run_with(
        none,
        "typeorm_pending",
        &["@typeorm-datasource", "@esm-modules"],
        &ds,
        &[("src/user.entity.ts", "export class User {}\n")],
    );
    assert!(rep.findings.is_empty());
    assert_eq!(rep.notes[0].kind, NoteKind::Setup);
    assert!(forge_core::internals::find(&rep.notes[0].text).len() == 1);

    // the data source as the role names it (the checker passes forge.yaml's roles)
    let fake = Arc::new(Fake {
        answers: vec![
            ("typeorm-ts-node-commonjs", "migrate", ran(0, "ok\n")),
            (
                "typeorm-ts-node-commonjs",
                "migrations-check",
                ran(
                    1,
                    "Unexpected changes in database schema were found in check mode:\nALTER TABLE ...\n",
                ),
            ),
        ],
        calls: Mutex::new(vec![]),
    });
    let r = repo(
        &[(
            "src/data-source.ts".to_owned(),
            "export default ds;\n".to_owned(),
        )]
        .into(),
        &[(
            "src/user.entity.ts".to_owned(),
            "export class User {}\n".to_owned(),
        )]
        .into(),
    );
    let mut facts = facts(Mode::Changed, r.existing());
    facts.roles = forge_check::scope::Roles::from_map(
        [
            (
                "typeorm-datasource".to_owned(),
                vec!["src/data-source.ts".to_owned()],
            ),
            ("esm-modules".to_owned(), vec!["**/*.mts".to_owned()]),
        ]
        .into(),
    );
    let opts = Options {
        tools: fake.clone(),
        ..Options::default()
    };
    let all = builtins(opts);
    let b = all.iter().find(|b| b.name() == "typeorm_pending").unwrap();
    let rep = b.run(
        &r.ctx("test/rule", vec![]),
        &facts,
        &["@typeorm-datasource".into(), "@esm-modules".into()],
    );
    assert_eq!(rep.findings.len(), 1, "{rep:?}");
    assert!(
        rep.findings[0]
            .message
            .contains("typeorm-ts-node-commonjs migration:generate -d src/data-source.ts")
    );
    assert_eq!(
        fake.calls.lock().unwrap()[0],
        "typeorm-ts-node-commonjs migrate src/data-source.ts"
    );
}

#[test]
fn sqlfluff_missing_is_a_note() {
    let rep = run_with(
        Arc::new(Fake {
            answers: vec![],
            calls: Mutex::new(vec![]),
        }),
        "sqlfluff",
        &["q.sql"],
        &[(".sqlfluff", "[sqlfluff]\ndialect = postgres\n")],
        &[("q.sql", "select 1\n")],
    );
    assert!(rep.findings.is_empty());
    assert_eq!(rep.notes[0].kind, NoteKind::MissingTool);
}

/// Audit 4.7: a removed parameter counts in a fenced block only where code uses it.
#[test]
fn docs_parameter_in_a_fence_needs_code() {
    let fake = || {
        Arc::new(Fake {
            answers: vec![],
            calls: Mutex::new(vec![]),
        })
    };
    let head = [
        (
            "app/client.py",
            "def connect(host: str, retries: int = 3) -> None:\n    pass\n",
        ),
        ("README.md", "# App\n\n```yaml\nretries: 5\n```\n"),
    ];
    let now = [(
        "app/client.py",
        "def connect(host: str) -> None:\n    pass\n",
    )];
    let rep = run_with(fake(), "docs_mentions", &[], &head, &now);
    let text: Vec<String> = rep.findings.iter().map(|f| f.agent_text()).collect();
    assert!(!text.iter().any(|t| t.contains("README.md:4")), "{text:?}");
    let head = [
        (
            "app/client.py",
            "def connect(host: str, retries: int = 3) -> None:\n    pass\n",
        ),
        (
            "README.md",
            "# App\n\n```python\nconnect(\"db\", retries=5)\n```\n",
        ),
    ];
    let rep = run_with(fake(), "docs_mentions", &[], &head, &now);
    let text: Vec<String> = rep.findings.iter().map(|f| f.agent_text()).collect();
    // (the signature changed too: connect is reported first)
    assert!(text.iter().any(|t| t.contains("README.md:4 ")), "{text:?}");
}
