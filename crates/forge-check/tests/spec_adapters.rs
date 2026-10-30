//! §16 of the specification: the adapter tables are generated from `adapters/*.yaml` as
//! forge-check loads them (between the `<!-- generated:adapters -->` markers of
//! `spec/16-tool-adapters.md`), and the behaviours §16 states that need no real tool.
//!
//! Regenerate the tables: `FORGE_BLESS=1 cargo test -p forge-check --test spec_adapters`.

use std::fmt::Write as _;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use forge_check::adapters::{Adapter, DoCtx, OpLookup, Pick, Runner, resolve, run_do};
use forge_check::tools::{ToolCmd, ToolResolver};
use forge_core::NoteKind;

const BEGIN: &str = "<!-- generated:adapters -->";
const END: &str = "<!-- /generated:adapters -->";

fn spec_file() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/16-tool-adapters.md")
}

fn bless() -> bool {
    std::env::var_os("FORGE_BLESS").is_some_and(|v| !v.is_empty() && v != "0")
}

fn code(s: &str) -> String {
    let fence = if s.contains('`') { "``" } else { "`" };
    format!("{fence}{}{fence}", s.replace('|', "\\|"))
}

fn runner(a: &Adapter) -> String {
    match (&a.pinned, a.runner) {
        (Some(p), _) => format!("Forge's pin {}", code(p)),
        (None, Runner::Pinned) => "Forge's pinned release".into(),
        (None, Runner::Path) => "`PATH`".into(),
        (None, Runner::Project) => "the project's own".into(),
    }
}

fn version_from(a: &Adapter) -> String {
    if a.pinned.is_some() {
        return "the pin".into();
    }
    match &a.package {
        Some(p) => format!("{}'s `package.json`", code(p)),
        None => code(&format!("{} {}", a.tool, a.version_args())),
    }
}

fn usage(a: &Adapter) -> String {
    match &a.usage {
        None => "none (no signature distinct from findings)".into(),
        Some(u) => format!(
            "exit {}, a line matching {}",
            u.exit
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join(" or "),
            code(&u.pattern)
        ),
    }
}

fn pick(a: &Adapter) -> &'static str {
    match a.pick {
        None => "",
        Some(Pick::Opentofu) => "`tofu` where the repository uses OpenTofu",
        Some(Pick::KustomizeOrKubectl) => "`kubectl` when `kustomize` is not on `PATH`",
    }
}

fn range_label(r: &forge_check::adapters::Range) -> String {
    match &r.to {
        Some(to) => format!("{} – {to}", r.from),
        None => format!("{} –", r.from),
    }
}

/// The generated part of §16.
fn render(all: &[Adapter]) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "<!-- Generated from crates/forge-check/adapters/*.yaml by crates/forge-check/tests/spec_adapters.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-check --test spec_adapters -->\n"
    );
    let _ = writeln!(
        s,
        "| Tool | Runs | Version read from | Proven up to | Usage error | Pick |"
    );
    let _ = writeln!(s, "|---|---|---|---|---|---|");
    for a in all {
        let _ = writeln!(
            s,
            "| [{t}](#{t}) | {} | {} | {} | {} | {} |",
            runner(a),
            version_from(a),
            code(&a.verified),
            usage(a),
            pick(a),
            t = a.tool
        );
    }
    let _ = writeln!(s, "\n{} adapters.", all.len());
    for a in all {
        let _ = writeln!(s, "\n#### {}\n", a.tool);
        let _ = write!(s, "| Operation |");
        for r in &a.ranges {
            let _ = write!(s, " {} |", range_label(r));
        }
        let _ = write!(s, "\n|---|");
        for _ in &a.ranges {
            let _ = write!(s, "---|");
        }
        s.push('\n');
        for op in a.ops() {
            let _ = write!(s, "| {} |", code(&op));
            for r in &a.ranges {
                let cellv = match r.ops.get(&op) {
                    Some(Some(args)) => code(args),
                    Some(None) => "cannot".into(),
                    None => "—".into(),
                };
                let _ = write!(s, " {cellv} |");
            }
            s.push('\n');
        }
        s.push('\n');
        for r in &a.ranges {
            let _ = writeln!(s, "- {}: {}", range_label(r), r.source.trim());
        }
        if let Some(c) = &a.codes {
            let _ = writeln!(
                s,
                "- Codes: the history of {} codes Forge's rules expect (§16.3); {} lists the release's rules.",
                c.history.len(),
                code(&c.op)
            );
        }
        if let Some(d) = &a.direct {
            let _ = writeln!(
                s,
                "- A check command runs it itself (§16.1) when it matches {}.",
                code(d)
            );
        }
    }
    s
}

#[test]
fn spec_16_tables_are_generated_from_the_adapters() {
    let all = Adapter::all().expect("adapters load");
    let body = render(&all);
    let path = spec_file();
    let text = std::fs::read_to_string(&path).expect("spec/16-tool-adapters.md");
    let (Some(b), Some(e)) = (text.find(BEGIN), text.find(END)) else {
        panic!("spec/16-tool-adapters.md needs the {BEGIN} ... {END} markers");
    };
    let want = format!("{}{BEGIN}\n{body}{}", &text[..b], &text[e..]);
    if text != want {
        if bless() {
            std::fs::write(&path, &want).unwrap();
        } else {
            panic!(
                "spec/16-tool-adapters.md is out of date with crates/forge-check/adapters. Regenerate: FORGE_BLESS=1 cargo test -p forge-check --test spec_adapters"
            );
        }
    }
}

/// §16.2: ranges are consecutive and oldest first, the last one open; every range names the
/// same operations (an operation a range cannot do is `null`, never missing); a pinned
/// adapter has one range, starting at its pin.
#[test]
fn ranges_are_consecutive_and_name_every_operation() {
    for a in Adapter::all().unwrap() {
        let ops = a.ops();
        for r in &a.ranges {
            let mut mine: Vec<String> = r.ops.keys().cloned().collect();
            mine.sort();
            assert_eq!(
                mine, ops,
                "{} {}: operations differ between ranges",
                a.tool, r.from
            );
            assert!(
                !r.source.trim().is_empty(),
                "{} {}: no source",
                a.tool,
                r.from
            );
        }
        if let Some(pin) = &a.pinned {
            assert_eq!(a.runner, Runner::Pinned, "{}", a.tool);
            assert_eq!(
                &a.verified, pin,
                "{}: a pinned tool is proven on its pin",
                a.tool
            );
            assert_eq!(a.ranges.len(), 1, "{}", a.tool);
            assert_eq!(&a.ranges[0].from, pin, "{}", a.tool);
        }
        // the proven release is covered by a range
        let probe = ops.first().map(String::as_str).unwrap_or("");
        assert!(
            !matches!(a.lookup(&a.verified, probe), OpLookup::NotCovered { .. }),
            "{}: verified {} is not covered",
            a.tool,
            a.verified
        );
    }
}

// ---- Loading: a check command names the operation, never the tool's command line (§16.1)

const PACK: &str =
    "id: acme\ndescription: Acme's conventions.\nlevel: project\ndetect: {always: true}\n";

fn load_check(run: &str) -> Result<forge_packs::Loaded, forge_packs::LoadError> {
    let dir = tempfile::tempdir().unwrap();
    let rule = format!(
        "id: acme/lint\ndescription: Lint.\nscope: {{language: any}}\nenforce:\n  - target: check\n    stage: lint\n    run: {}\n",
        serde_json::to_string(run).unwrap()
    );
    for (p, t) in [
        ("acme/pack.yaml", PACK),
        ("acme/rules/lint.yaml", rule.as_str()),
    ] {
        let p = dir.path().join(p);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, t).unwrap();
    }
    let disk = forge_packs::tree::Disk(dir.path().to_path_buf());
    forge_packs::loader::load_packs(&forge_packs::loader::Place {
        tree: &disk,
        prefix: "",
        shown: "",
        source: forge_packs::PackSource::Local,
    })
}

#[test]
fn a_check_running_an_adapted_tool_itself_is_rejected() {
    for a in Adapter::all().unwrap() {
        let t = &a.tool;
        // the package managers adapt some commands only; `install` is adapted for each
        let direct = if a.direct.is_some() {
            format!("{t} install")
        } else {
            format!("{t} --some-option x")
        };
        let err = load_check(&direct)
            .err()
            .unwrap_or_else(|| panic!("{t}: \"{direct}\" was accepted"));
        assert!(
            err.message
                .contains(&format!("run {t} as \"forge_do {t} <operation>\"")),
            "{t}: {err}"
        );
        // after a pipe or `&&` too
        assert!(load_check(&format!("true && {direct}")).is_err(), "{t}");
        // the adapter's form, and the name inside single quotes (data), are accepted
        let op = a.ops().into_iter().next().unwrap();
        load_check(&format!("forge_do {t} {op} {{files}}"))
            .unwrap_or_else(|e| panic!("{t}: forge_do refused: {e}"));
        load_check(&format!("echo '{direct}'"))
            .unwrap_or_else(|e| panic!("{t}: quoted text refused: {e}"));
    }
    // the loader's patterns are the adapters' own: bun's `direct` counts only its
    // package-manager commands
    assert!(Adapter::load("bun").unwrap().direct.is_some());
    load_check("bun --no-install -e 'console.log(1)'").expect("bun as a runtime");
    assert!(load_check("bun add left-pad").is_err());
}

// ---- forge_do with stand-in tools (§16.3)

/// Forge's state in a directory of the test's own (the tool versions are cached there).
fn forge_home() {
    static HOME: OnceLock<tempfile::TempDir> = OnceLock::new();
    HOME.get_or_init(|| {
        let d = tempfile::tempdir().unwrap();
        // SAFETY: set once, before any test of this file reads it, to the same value.
        unsafe { std::env::set_var("FORGE_HOME", d.path()) };
        d
    });
}

/// Every tool is the stand-in script `bin` (project install and PATH alike).
struct Fake {
    bin: PathBuf,
}

impl ToolResolver for Fake {
    fn project(&self, _: &Path, _: &Path, _: &str) -> Option<ToolCmd> {
        Some(ToolCmd::exe(self.bin.clone()))
    }
    fn on_path(&self, _: &str) -> Option<PathBuf> {
        Some(self.bin.clone())
    }
    fn pinned(&self, name: &str, _: &str) -> Result<ToolCmd, String> {
        Err(format!("{name} is not installed"))
    }
}

struct Absent;

impl ToolResolver for Absent {
    fn pinned(&self, name: &str, _: &str) -> Result<ToolCmd, String> {
        Err(format!("{name} is not installed"))
    }
}

/// A stand-in script in its own directory: it says `version` and runs `body`.
fn fake(version: &str, body: &str) -> (tempfile::TempDir, Fake) {
    forge_home();
    let d = tempfile::tempdir().unwrap();
    let bin = d.path().join("tool");
    std::fs::write(
        &bin,
        format!(
            "#!/bin/sh\ncase \"$*\" in *--version*|version*) echo 'tool {version}'; exit 0 ;; esac\n{body}\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    (d, Fake { bin })
}

fn do_ctx<'a>(root: &'a Path, tools: &'a dyn ToolResolver) -> DoCtx<'a> {
    DoCtx {
        root,
        cwd: root,
        rule: "acme/lint",
        tools,
        stdin: None,
    }
}

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_release_newer_than_proven_is_driven_as_the_newest_range() {
    let (d, tools) = fake("99.0.0", "exit 0");
    let r = resolve("ruff", "lint", &args(&["a.py"]), &do_ctx(d.path(), &tools)).unwrap();
    assert_eq!(r.version, "99.0.0");
    assert_eq!(
        r.args,
        args(&["check", "--output-format", "concise", "a.py"])
    );
    // an older range's own form
    let (d, tools) = fake("0.1.10", "exit 0");
    let r = resolve("ruff", "lint", &args(&["a.py"]), &do_ctx(d.path(), &tools)).unwrap();
    assert_eq!(r.args, args(&["check", "--output-format", "text", "a.py"]));
}

#[test]
fn a_release_that_cannot_is_not_enforced_and_passes() {
    // terraform before 1.3.0 cannot format several files
    let (d, tools) = fake("1.2.0", "echo SHOULD-NOT-RUN; exit 1");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let o = run_do(
        "terraform",
        "format",
        &args(&["a.tf", "b.tf"]),
        &do_ctx(d.path(), &tools),
        &mut out,
        &mut err,
    );
    assert_eq!(o.status, 0);
    assert!(out.is_empty() && err.is_empty(), "the tool ran");
    assert_eq!(o.notes.len(), 1);
    assert_eq!(o.notes[0].kind, NoteKind::NotEnforced);
    assert_eq!(o.notes[0].rule.as_deref(), Some("acme/lint"));
    assert_eq!(
        o.notes[0].text,
        "not enforced: terraform 1.2.0 cannot format (it needs terraform 1.3.0 or later)"
    );
    // older than every range
    let (d, tools) = fake("0.15.0", "exit 1");
    let o = run_do(
        "terraform",
        "format-check",
        &[],
        &do_ctx(d.path(), &tools),
        &mut out,
        &mut err,
    );
    assert_eq!(o.status, 0);
    assert_eq!(
        o.notes[0].text,
        "not enforced: no Forge adapter covers terraform 0.15.0 (Forge drives terraform 1.0.0 and later)"
    );
}

#[test]
fn a_usage_error_is_a_note_never_a_finding() {
    // ruff's usage error: exit 2 and clap's help line; its output is not passed on
    let (d, tools) = fake(
        "0.6.9",
        "echo \"error: unexpected argument '--output-format' found\" >&2; echo \"For more information, try '--help'.\" >&2; exit 2",
    );
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let o = run_do(
        "ruff",
        "lint",
        &args(&["a.py"]),
        &do_ctx(d.path(), &tools),
        &mut out,
        &mut err,
    );
    assert_eq!(o.status, 0);
    assert!(out.is_empty() && err.is_empty());
    assert_eq!(o.notes[0].kind, NoteKind::NotEnforced);
    assert_eq!(
        o.notes[0].text,
        "skipped: ruff 0.6.9 rejected the command (ruff check --output-format concise a.py): unexpected argument '--output-format' found"
    );
    // a finding (exit 1) is passed on as the tool printed it
    let (d, tools) = fake("0.6.9", "echo 'a.py:1:1: F401 unused'; exit 1");
    let o = run_do(
        "ruff",
        "lint",
        &args(&["a.py"]),
        &do_ctx(d.path(), &tools),
        &mut out,
        &mut err,
    );
    assert_eq!(o.status, 1);
    assert!(o.notes.is_empty());
    assert_eq!(String::from_utf8_lossy(&out), "a.py:1:1: F401 unused\n");
}

#[test]
fn an_absent_tool_is_skipped_with_a_note() {
    forge_home();
    let d = tempfile::tempdir().unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let o = run_do(
        "ruff",
        "lint",
        &[],
        &do_ctx(d.path(), &Absent),
        &mut out,
        &mut err,
    );
    assert_eq!(o.status, 0);
    assert_eq!(o.notes[0].kind, NoteKind::MissingTool);
    assert_eq!(o.notes[0].text, "skipped: ruff is not installed");
    let o = run_do(
        "no-such-tool",
        "lint",
        &[],
        &do_ctx(d.path(), &Absent),
        &mut out,
        &mut err,
    );
    assert_eq!(o.status, 0);
    assert_eq!(o.notes[0].kind, NoteKind::ForgeFailure);
    assert_eq!(o.notes[0].text, "Forge has no adapter for no-such-tool");
}

#[test]
fn caller_arguments_by_rank() {
    // `{args}` places the caller's arguments; without it they follow the adapter's words
    let (d, tools) = fake("0.6.9", "exit 0");
    let r = resolve(
        "ruff",
        "format-stdin",
        &args(&["src/a.py"]),
        &do_ctx(d.path(), &tools),
    )
    .unwrap();
    assert_eq!(
        r.args,
        args(&["format", "--stdin-filename", "src/a.py", "-"])
    );
}
