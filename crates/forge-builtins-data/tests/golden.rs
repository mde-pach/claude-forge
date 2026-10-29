//! Golden tests: every pack example of [`examples`](./examples.rs) and the corner cases below,
//! run through the original shell function (templates/engine/*.sh, sourced with Forge's
//! default roles) and through the Rust builtin. Both must flag the same things: the same
//! `path` / `path:line` locations, and output (or none) alike. The wording is the port's own
//! (findings are `path:line message`; the original's "skipped" lines are notes now), so lines
//! are compared by what they point at, not word for word.
//!
//! The original's `forge_do` is the adapters' command for the tools these builtins run
//! (squawk and sqlfluff through uvx, the machine's alembic). Cases needing a missing tool are
//! skipped, and so is the whole test once templates/engine is gone.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use common::SQUAWK_VERSION;
use common::*;
use forge_builtins_data::Mode;

fn engine_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../templates/engine")
}

/// The original engine: the modules these builtins come from and what they use.
fn engine() -> Option<String> {
    let dir = engine_dir();
    let mut s = String::new();
    for m in [
        "core",
        "tools",
        "sql",
        "python",
        "env",
        "release",
        "docs",
        "repository",
        "tests",
        "advice",
    ] {
        s.push_str(&std::fs::read_to_string(dir.join(format!("{m}.sh"))).ok()?);
        s.push('\n');
    }
    s = s.replace("@@SQUAWK_VERSION@@", SQUAWK_VERSION);
    s.push_str(include_str!("fixtures/roles.sh"));
    s.push_str(&format!(
        r#"
# the tools are there (the shims below run them)
forge_has_tool() {{ return 0; }}
# the adapters' commands for the tools these functions run
forge_do() {{
  case "$1 $2" in
    "squawk lint") shift 2; uvx -q --from squawk-cli=={SQUAWK_VERSION} squawk --reporter gcc -c "$@" ;;
    "sqlfluff lint") shift 2; uvx -q --from sqlfluff==3.4.2 sqlfluff lint --format github-annotation-native --nocolor "$@" ;;
    "alembic upgrade-sql") shift 2; alembic upgrade "$@" --sql ;;
    *) return 0 ;;
  esac
}}
"#
    ));
    Some(s)
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The original function on a case: its output lines (Forge's own `forge:` lines left out).
fn original(engine: &str, case: &Case, r: &Repo) -> Vec<String> {
    if !case.triggered(r) {
        return vec![];
    }
    let tmp = r.dir.path();
    let lines = |v: &[String]| v.iter().map(|l| format!("{l}\n")).collect::<String>();
    let all = case.mode == Mode::All;
    let changed = if all { vec![] } else { r.changed.clone() };
    let existing: Vec<String> = changed
        .iter()
        .filter(|p| r.root.join(p).is_file())
        .cloned()
        .collect();
    let inbase: Vec<String> = changed
        .iter()
        .filter(|p| r.base.join(p).is_file())
        .cloned()
        .collect();
    let deleted: Vec<String> = inbase
        .iter()
        .filter(|p| !r.root.join(p).is_file())
        .cloned()
        .collect();
    let mut added = String::new();
    for p in &existing {
        match r.added_lines(p) {
            None => added.push_str(&format!("{p}\t*\n")),
            Some(ls) => {
                for l in ls {
                    added.push_str(&format!("{p}\t{l}\n"));
                }
            }
        }
    }
    std::fs::write(tmp.join("changed"), lines(&changed)).unwrap();
    std::fs::write(tmp.join("existing"), lines(&existing)).unwrap();
    std::fs::write(tmp.join("inbase"), lines(&inbase)).unwrap();
    std::fs::write(tmp.join("deleted"), lines(&deleted)).unwrap();
    std::fs::write(tmp.join("added"), added).unwrap();
    let files = case.check_files(r);
    let (name, args) = call(&case.cmd, &files).unwrap();
    let script = format!(
        "{engine}\nmode={}\nFORGE_TMP={}\nFORGE_FILES={}\nFORGE_SNAP=''\nc_id={}\nforge_{name} {}\n",
        if all { "--all" } else { "--changed" },
        sh_quote(&tmp.to_string_lossy()),
        sh_quote(&files.join("\n")),
        sh_quote(&case.rule),
        args.iter()
            .map(|a| sh_quote(a))
            .collect::<Vec<_>>()
            .join(" "),
    );
    let out = Command::new("sh")
        .arg("-c")
        .arg(script)
        .current_dir(&r.root)
        .env_remove("CLAUDECODE")
        .env_remove("FORGE_FOR_AGENT")
        .env_remove("DATABASE_URL")
        .output()
        .expect("sh");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with("forge:"))
        .map(str::to_owned)
        .collect()
}

/// What a line points at: the repository paths it names, with their line (0 and none alike).
fn locations(r: &Repo, lines: &[String]) -> BTreeSet<String> {
    let loc = regex::Regex::new(r"^(.+?)(?::(\d+))?(?::(\d+))?:?$").unwrap();
    let mut out = BTreeSet::new();
    for l in lines {
        for w in l.split_whitespace() {
            let w = w.trim_matches(|c: char| matches!(c, '(' | ')' | ',' | ';' | '"' | '\''));
            let Some(c) = loc.captures(w) else { continue };
            let p = c[1].trim_end_matches('/');
            let known = r.root.join(p).exists()
                || r.changed.iter().any(|x| x == p)
                || r.head.contains_key(p);
            if !known || p.is_empty() || p == "." {
                continue;
            }
            match c.get(2).map(|m| m.as_str()) {
                Some(n) if n != "0" => out.insert(format!("{p}:{n}")),
                _ => out.insert(p.to_owned()),
            };
        }
    }
    out
}

fn files(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect()
}

fn own(name: &str, cmd: &str, mode: Mode, head: &[(&str, &str)], now: &[(&str, &str)]) -> Case {
    Case {
        name: name.into(),
        rule: "test/rule".into(),
        cmd: cmd.into(),
        scope: None,
        when_changed: None,
        lint: false,
        mode,
        head: files(head),
        files: files(now),
        want: None,
    }
}

const REV1: &str = "revision = \"0001\"\ndown_revision = None\n\n\ndef upgrade() -> None:\n    op.create_table(\"users\", sa.Column(\"id\", sa.Integer()))\n";
const REV2: &str = "revision = \"0002\"\ndown_revision = \"0001\"\n\n\ndef upgrade() -> None:\n    op.add_column(\"posts\", sa.Column(\"author_id\", sa.Integer(), sa.ForeignKey(\"users.id\")))\n    op.alter_column(\"users\", \"nick\", nullable=False)\n    op.execute(\"UPDATE users SET nick = 'x'\")\n\n\ndef downgrade() -> None:\n    pass\n";
const DRIZZLE: &str = "export default { dialect: \"postgresql\", schema: [\"./src/db/*.ts\"], out: \"./migrations\" };\n";

/// Corners of each builtin beyond the pack examples.
fn own_cases() -> Vec<Case> {
    vec![
        own(
            "alembic --all: the head revision only",
            "forge_alembic_fk_index",
            Mode::All,
            &[
                (
                    "alembic/env.py",
                    "context.configure(render_as_batch=True)\n",
                ),
                ("alembic/versions/0001_users.py", REV1),
                ("alembic/versions/0002_posts.py", REV2),
            ],
            &[],
        ),
        own(
            "alembic batch --all",
            "forge_alembic_batch",
            Mode::All,
            &[
                (
                    "alembic/env.py",
                    "context.configure(render_as_batch=True)\n",
                ),
                ("alembic/versions/0001_users.py", REV1),
                ("alembic/versions/0002_posts.py", REV2),
            ],
            &[],
        ),
        own(
            "alembic data downgrade --all",
            "forge_alembic_data_downgrade",
            Mode::All,
            &[
                ("alembic/versions/0001_users.py", REV1),
                ("alembic/versions/0002_posts.py", REV2),
            ],
            &[],
        ),
        own(
            "prisma: a dropped table recreated elsewhere, a renamed column dropped later",
            "forge_prisma_drops {files}",
            Mode::Changed,
            &[(
                "prisma/migrations/1_init/migration.sql",
                "CREATE TABLE \"A\" (\"id\" INT, \"x\" TEXT);\nCREATE TABLE \"B\" (\"id\" INT);\n",
            )],
            &[(
                "prisma/migrations/2_next/migration.sql",
                "ALTER TABLE \"A\" RENAME COLUMN \"x\" TO \"y\";\nALTER TABLE \"A\" DROP COLUMN \"y\";\nDROP TABLE IF EXISTS \"B\";\nCREATE TABLE \"new_A\" (\"id\" INT);\nDROP TABLE \"A\";\nALTER TABLE \"new_A\" RENAME TO \"A\";\n",
            )],
        ),
        own(
            "drizzle: a glob schema, another out folder",
            "forge_drizzle_migration_present",
            Mode::Changed,
            &[
                ("drizzle.config.ts", DRIZZLE),
                (
                    "src/db/users.ts",
                    "export const users = pgTable(\"users\", {});\n",
                ),
            ],
            &[
                (
                    "src/db/users.ts",
                    "export const users = pgTable(\"users\", { id: serial(\"id\") });\n",
                ),
                (
                    "migrations/0001_x.sql",
                    "ALTER TABLE users ADD COLUMN id serial;\n",
                ),
            ],
        ),
        own(
            "drizzle applied: a deleted migration",
            "forge_drizzle_applied",
            Mode::Changed,
            &[
                ("drizzle.config.ts", DRIZZLE),
                ("migrations/0000_init.sql", "CREATE TABLE users ();\n"),
                ("migrations/meta/0000_snapshot.json", "{}\n"),
            ],
            &[("migrations/meta/0000_snapshot.json", "{\"a\": 1}\n")],
        ),
        own(
            "models registered through alembic/env.py",
            "forge_models_registered",
            Mode::Changed,
            &[
                ("app/models/__init__.py", "from .user import User\n"),
                (
                    "app/models/user.py",
                    "class User(Base):\n    __tablename__ = \"users\"\n",
                ),
                ("alembic/env.py", "from app.models import invoice  # noqa\n"),
            ],
            &[(
                "app/models/invoice.py",
                "class Invoice(Base):\n    __tablename__ = \"invoices\"\n\n\nclass Line(Base):\n    __tablename__ = \"lines\"\n",
            )],
        ),
        own(
            "models not registered: two classes",
            "forge_models_registered",
            Mode::Changed,
            &[
                (
                    "app/models/__init__.py",
                    "from app.models.user import User\n",
                ),
                (
                    "app/models/user.py",
                    "class User(Base):\n    __tablename__ = \"users\"\n",
                ),
            ],
            &[(
                "app/models/invoice.py",
                "class Invoice(Base):\n    __tablename__ = \"invoices\"\n\n\nclass Line(Base):\n    __tablename__ = \"lines\"\n",
            )],
        ),
        own(
            "uv cooldown in hours, too short",
            "forge_uv_cooldown",
            Mode::Changed,
            &[],
            &[(
                "uv.toml",
                "exclude-newer = \"48 hours\"\nrequired-version = \"==0.9.2\"\n",
            )],
        ),
        own(
            "version: nested versions do not count, the top-level one does",
            "forge_version_changed",
            Mode::Changed,
            &[(
                "packages/a/package.json",
                "{\"config\": {\"version\": \"1\"}, \"version\": \"1.0.0\"}\n",
            )],
            &[(
                "packages/a/package.json",
                "{\"config\": {\"version\": \"2\"}, \"version\": \"1.1.0\"}\n",
            )],
        ),
        own(
            "release notes: a pnpm workspace, a private root",
            "forge_release_notes @python-tests",
            Mode::Changed,
            &[
                (".changeset/config.json", "{}\n"),
                ("pnpm-lock.yaml", "lockfileVersion: '9.0'\n"),
                ("package.json", "{\"private\": true}\n"),
                ("packages/a/package.json", "{\"name\": \"a\"}\n"),
                ("packages/a/src/x.ts", "export const x = 1;\n"),
            ],
            &[
                ("packages/a/src/x.ts", "export const x = 2;\n"),
                ("src/root.ts", "export const r = 1;\n"),
            ],
        ),
        own(
            "stub: __all__ grows, two stubs",
            "forge_stub_sync",
            Mode::Changed,
            &[
                ("pkg/__init__.py", ""),
                ("pkg/core.py", "__all__ = [\"run\"]\n\n\ndef run(): ...\n"),
                ("stubs/pkg/__init__.pyi", ""),
                ("stubs/pkg/core.pyi", "def run(): ...\n"),
                ("other/pkg/__init__.pyi", ""),
                ("other/pkg/core.pyi", "def run(): ...\ndef stop(): ...\n"),
            ],
            &[(
                "pkg/core.py",
                "__all__ = [\"run\", \"stop\", \"pause\"]\n\n\ndef run(): ...\ndef stop(): ...\ndef pause(): ...\n",
            )],
        ),
        own(
            "docs: a removed function documented in a fence",
            "forge_docs_mentions",
            Mode::Changed,
            &[
                (
                    "src/api.ts",
                    "export function fetchOrders() {}\nexport const LIMIT_ROWS = 5;\n",
                ),
                (
                    "docs/api.md",
                    "# API\n\n```ts\nawait fetchOrders();\n```\n\nSee `LIMIT_ROWS`.\n",
                ),
            ],
            &[(
                "src/api.ts",
                "export function loadOrders() {}\nexport const LIMIT_ROWS = 6;\n",
            )],
        ),
        own(
            "adr: only the front matter status changed",
            "forge_adr_edits",
            Mode::Changed,
            &[(
                "docs/adr/0001-x.md",
                "---\nstatus: accepted\n---\n# X\n\nWe do X.\n",
            )],
            &[(
                "docs/adr/0001-x.md",
                "---\nstatus: superseded\n---\n# X\n\nWe do X.\n",
            )],
        ),
        own(
            "env example: a prefix in the template, a required alias",
            "forge_env_example py",
            Mode::Changed,
            &[
                ("svc/.env.example", "APP_DB=\n"),
                (
                    "svc/app/settings.py",
                    "class S(BaseSettings):\n    model_config = SettingsConfigDict(env_prefix=\"app_\")\n    db: str\n",
                ),
            ],
            &[(
                "svc/app/settings.py",
                "class S(BaseSettings):\n    model_config = SettingsConfigDict(env_prefix=\"app_\")\n    db: str\n    token: str = Field(alias=\"API_TOKEN\")\n    level: str = Field(default=\"info\")\n",
            )],
        ),
        own(
            "squawk: newest migration of the role with --all",
            "forge_squawk_role @sql-migrations",
            Mode::All,
            &[
                (
                    "supabase/migrations/20260101_a.sql",
                    "ALTER TABLE t RENAME COLUMN a TO b;\n",
                ),
                (
                    "supabase/migrations/20260102_b.sql",
                    "ALTER TABLE t ADD COLUMN c int;\nALTER TABLE t ALTER COLUMN c TYPE bigint;\n",
                ),
            ],
            &[],
        ),
    ]
}

/// Cases where the port differs from the original on purpose, and why.
const DELIBERATE: &[(&str, &str)] = &[];

#[test]
fn port_matches_the_shell_original() {
    let Some(engine) = engine() else {
        eprintln!("templates/engine is gone: nothing to compare with");
        return;
    };
    let uvx = have("uvx", &["--version"]);
    let mut failures = vec![];
    let mut compared = 0;
    let lead = regex::Regex::new(r"^\s*(?:-->\s*)?([^\s:(]+):([0-9]+)").unwrap();
    for case in pack_cases().into_iter().chain(own_cases()) {
        if case.needs_uvx() && !uvx {
            continue;
        }
        let r = repo(&case.head, &case.files);
        let port = port(&case, &r);
        let orig_lines = original(&engine, &case, &r);
        let orig_loc = locations(&r, &orig_lines);
        // what the checker attributes: a line that starts with path:line
        let named: Vec<forge_core::Finding> = orig_lines
            .iter()
            .filter_map(|l| lead.captures(l))
            .map(|c| forge_core::Finding::error("x", "x").at(&c[1], c[2].parse().ok()))
            .collect();
        let names_file = !named.is_empty();
        let names_change = named.iter().any(|f| r.names_change(f));
        let orig_counted = case.count(&r, orig_lines.clone(), names_file, names_change);
        let port_loc = locations(&r, &port.texts);
        compared += 1;
        if DELIBERATE.iter().any(|(n, _)| *n == case.name) {
            continue;
        }
        if orig_counted.is_empty() != port.counted.is_empty() || orig_loc != port_loc {
            failures.push(format!(
                "{}:\n  original {orig_lines:?}\n  port     {:?}\n  notes    {:?}",
                case.name, port.texts, port.notes
            ));
        }
    }
    eprintln!("{compared} cases compared");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
