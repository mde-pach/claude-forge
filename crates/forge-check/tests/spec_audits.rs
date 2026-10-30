//! §11 (audits of tool targets): the project's own tool run on the rule's examples, with stand-in
//! tools in the project's `.venv/bin` and `node_modules/.bin` that answer as the real ones do.

use std::path::Path;
use std::sync::Once;

use forge_check::probe::audit_target;
use forge_check::tools::LocalTools;
use forge_packs::model::{Rule, Target, ToolTarget};
use forge_render::report::{
    AuditResult, AuditStatus, Enforcement, enforcement_summary, format_audit, format_audits,
};
use serde_json::json;

/// The tool versions Forge learns are cached in its state: a directory of this test's.
fn home() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let d = tempfile::tempdir().unwrap().keep();
        // SAFETY: set once, before any test of this file reads the environment.
        unsafe { std::env::set_var("FORGE_HOME", d) };
    });
}

fn tools() -> LocalTools {
    LocalTools {
        path: Some("/nonexistent".into()),
        ..Default::default()
    }
}

fn exe(dir: &Path, rel: &str, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    let f = dir.join(rel);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(&f, script).unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A ruff of `version` that reports `S307` for stdin holding `eval(` (`always`: for anything).
fn ruff(dir: &Path, version: &str, always: bool) {
    let test = if always { "true" } else { "grep -q 'eval('" };
    exe(
        dir,
        ".venv/bin/ruff",
        &format!(
            "#!/bin/sh\ncase \"$1\" in --version) echo 'ruff {version}'; exit 0 ;; esac\nif {test}; then echo '[{{\"code\": \"S307\"}}]'; exit 1; fi\necho '[]'\n"
        ),
    );
}

fn rule(tool: &str, expects: &str, lang: &str, m: &str, n: &str) -> (Rule, ToolTarget) {
    let r: Rule = serde_json::from_value(json!({
        "id": "acme/r", "description": "A rule.",
        "scope": {"language": lang},
        "enforce": [{"target": "tool", "tool": tool, "expects": expects, "snippet": "the snippet"}],
        "examples": {"match": [m], "no_match": [n]},
    }))
    .unwrap();
    let Target::Tool(t) = r.enforce[0].clone() else {
        unreachable!()
    };
    (r, t)
}

fn audit(dir: &Path, r: &(Rule, ToolTarget)) -> AuditResult {
    audit_target(&tools(), dir, &r.0, &r.1, "")
}

#[test]
fn ruff_satisfied_unsatisfied_and_noisy() {
    home();
    let r = rule("ruff", "S307", "python", "eval(x)", "x = 1");
    let d = tempfile::tempdir().unwrap();
    ruff(d.path(), "0.6.9", false);
    let a = audit(d.path(), &r);
    assert_eq!(a.status, AuditStatus::Satisfied, "{a:?}");
    assert_eq!(format_audit(&a), "forge: acme/r ruff S307: satisfied");
    // a config that does not select the code: the match example is not reported
    let d = tempfile::tempdir().unwrap();
    exe(
        d.path(),
        ".venv/bin/ruff",
        "#!/bin/sh\ncase \"$1\" in --version) echo 'ruff 0.6.9'; exit 0 ;; esac\necho '[]'\n",
    );
    let a = audit(d.path(), &r);
    assert_eq!(a.status, AuditStatus::Unsatisfied);
    assert_eq!(
        format_audit(&a),
        "forge: acme/r ruff S307: unsatisfied (S307 not reported for: eval(x))\n  add to the ruff config: the snippet"
    );
    // reported for the no_match example too
    let d = tempfile::tempdir().unwrap();
    ruff(d.path(), "0.6.9", true);
    let a = audit(d.path(), &r);
    assert_eq!(a.status, AuditStatus::Unsatisfied);
    assert_eq!(
        a.detail.as_deref(),
        Some("S307 reported for a no_match example: x = 1")
    );
}

/// The installed release decides first: a code it does not have, or a release no range
/// covers, is `unavailable` (never a snippet).
#[test]
fn the_installed_release_decides_first() {
    home();
    let d = tempfile::tempdir().unwrap();
    ruff(d.path(), "0.15.0", false);
    let a = audit(
        d.path(),
        &rule("ruff", "AIR303", "python", "eval(x)", "x = 1"),
    );
    assert_eq!(a.status, AuditStatus::Unavailable);
    assert_eq!(
        a.detail.as_deref(),
        Some("AIR303 needs ruff ≥ 0.16.0; this project has 0.15.0")
    );
    assert!(a.snippet.is_none());
    let d = tempfile::tempdir().unwrap();
    ruff(d.path(), "0.1.0", false);
    let a = audit(
        d.path(),
        &rule("ruff", "S307", "python", "eval(x)", "x = 1"),
    );
    assert_eq!(a.status, AuditStatus::Unavailable);
    assert_eq!(
        a.detail.as_deref(),
        Some("no Forge adapter covers ruff 0.1.0 (Forge drives ruff 0.1.4 and later)")
    );
}

/// A configured tool that is not installed is skipped, never satisfied.
#[test]
fn a_tool_not_installed_is_skipped() {
    home();
    if std::env::var_os("VIRTUAL_ENV").is_some() {
        eprintln!("skipped: VIRTUAL_ENV is set (its ruff would be found)");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let a = audit(
        d.path(),
        &rule("ruff", "S307", "python", "eval(x)", "x = 1"),
    );
    assert_eq!(a.status, AuditStatus::Skipped);
    assert_eq!(
        a.detail.as_deref(),
        Some(
            "ruff is configured but not installed (node_modules/.bin, .venv/bin or the project's Python environment)"
        )
    );
}

/// A diagnostic at warning level does not enforce a rule: the checks do not block on warnings.
#[test]
fn a_warning_is_unsatisfied() {
    home();
    let d = tempfile::tempdir().unwrap();
    exe(
        d.path(),
        "node_modules/.bin/biome",
        "#!/bin/sh\ncase \"$1\" in --version) echo 'Version: 2.6.3'; exit 0 ;; esac\necho '{\"diagnostics\": [{\"category\": \"lint/suspicious/noExplicitAny\", \"severity\": \"warning\"}]}'\n",
    );
    let r = rule(
        "biome",
        "lint/suspicious/noExplicitAny",
        "typescript",
        "let a: any",
        "let a: unknown",
    );
    let a = audit(d.path(), &r);
    assert_eq!(a.status, AuditStatus::Unsatisfied, "{a:?}");
    assert_eq!(
        a.detail.as_deref(),
        Some(
            "lint/suspicious/noExplicitAny is only a warning in your biome config, and Forge's checks do not block on warnings (e.g. let a: any)"
        )
    );
    // the example files were written inside the project and removed afterwards
    let left: Vec<_> = std::fs::read_dir(d.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .filter(|n| n != "node_modules")
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

/// ty is Forge's pinned tool: where the project leaves ty to its defaults, Forge's runs set the
/// rule's level (`--error <rule>`), so the audit does too and the rule is satisfied with no config
/// to add. Where the project configures ty, its config decides, and the snippet is written for
/// the file it uses.
#[test]
fn ty_without_a_project_config_runs_at_the_rules_levels() {
    home();
    let bin = tempfile::tempdir().unwrap();
    // reports redundant-cast for a cast() example: as an error only when asked to
    exe(
        bin.path(),
        "ty",
        "#!/bin/sh\ncase \"$1\" in --version) echo 'ty 0.0.86'; exit 0 ;; esac\nfor a; do case $a in *.py) f=$a ;; esac; done\ngrep -q 'cast(' \"$f\" || { echo '[]'; exit 0; }\nsev=minor; case \" $* \" in *' --error redundant-cast '*) sev=major ;; esac\necho \"[{\\\"check_name\\\": \\\"redundant-cast\\\", \\\"severity\\\": \\\"$sev\\\"}]\"; exit 1\n",
    );
    let tools = LocalTools {
        path: Some(bin.path().to_string_lossy().into_owned()),
        ..Default::default()
    };
    let r: Rule = serde_json::from_value(json!({
        "id": "acme/cast", "description": "No redundant cast.",
        "scope": {"language": "python"},
        "enforce": [{"target": "tool", "tool": "ty", "expects": "redundant-cast",
            "snippet": "[tool.ty.rules] redundant-cast = \"error\""}],
        "examples": {"match": ["from typing import cast\nx = cast(int, 1)\n"], "no_match": ["x = 1\n"]},
    }))
    .unwrap();
    let Target::Tool(t) = r.enforce[0].clone() else {
        unreachable!()
    };
    // no ty config (a plain-scripts project, or a pyproject.toml without [tool.ty])
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("pyproject.toml"), "[project]\nname = \"x\"\n").unwrap();
    let a = audit_target(&tools, d.path(), &r, &t, "");
    assert_eq!(a.status, AuditStatus::Satisfied, "{a:?}");
    // [tool.ty]: the project's levels; the rule is only a warning there
    std::fs::write(
        d.path().join("pyproject.toml"),
        "[project]\nname = \"x\"\n\n[tool.ty.rules]\npossibly-unresolved-reference = \"error\"\n",
    )
    .unwrap();
    let a = audit_target(&tools, d.path(), &r, &t, "");
    assert_eq!(a.status, AuditStatus::Unsatisfied, "{a:?}");
    assert_eq!(
        a.snippet.as_deref(),
        Some("[tool.ty.rules] redundant-cast = \"error\"")
    );
    // ty.toml: its tables have no tool.ty prefix
    std::fs::write(d.path().join("ty.toml"), "[rules]\n").unwrap();
    let a = audit_target(&tools, d.path(), &r, &t, "");
    assert_eq!(a.status, AuditStatus::Unsatisfied, "{a:?}");
    assert_eq!(
        a.snippet.as_deref(),
        Some("[rules] redundant-cast = \"error\"")
    );
}

/// The config the unsatisfied audits lack is one block per tool and directory, merged by table,
/// not one line per rule.
#[test]
fn unsatisfied_audits_suggest_one_block() {
    let a = |rule: &str, tool: &str, expects: &str, snippet: &str| AuditResult {
        rule: rule.into(),
        tool: tool.into(),
        expects: expects.into(),
        status: AuditStatus::Unsatisfied,
        dir: None,
        detail: Some(format!("{expects} not reported for: x")),
        snippet: Some(snippet.into()),
    };
    let audits = [
        a(
            "python/no-useless-cast",
            "ty",
            "redundant-cast",
            "[tool.ty.rules] redundant-cast = \"error\"",
        ),
        a(
            "python/no-deprecated",
            "ty",
            "deprecated",
            "[tool.ty.rules] deprecated = \"error\"",
        ),
        a(
            "python/a",
            "ruff",
            "S110",
            "[tool.ruff.lint] extend-select = [\"S110\", \"S112\"]",
        ),
        a(
            "python/b",
            "ruff",
            "TID251",
            "[tool.ruff.lint]\nextend-select = [\"TID251\"]\n\n[tool.ruff.lint.flake8-tidy-imports.banned-api]\n\"x.y\".msg = \"no\"\n",
        ),
    ];
    assert_eq!(
        format_audits(&audits),
        "forge: python/no-useless-cast ty redundant-cast: unsatisfied (redundant-cast not reported for: x)
forge: python/no-deprecated ty deprecated: unsatisfied (deprecated not reported for: x)
forge: python/a ruff S110: unsatisfied (S110 not reported for: x)
forge: python/b ruff TID251: unsatisfied (TID251 not reported for: x)
forge: add to the ty config:
  [tool.ty.rules]
  redundant-cast = \"error\"
  deprecated = \"error\"
forge: add to the ruff config:
  [tool.ruff.lint]
  extend-select = [\"S110\", \"S112\", \"TID251\"]
  [tool.ruff.lint.flake8-tidy-imports.banned-api]
  \"x.y\".msg = \"no\""
    );
}

#[test]
fn enforcement_summary_counts_reasons() {
    let e = |id: &str, enforced: bool, why: &[&str]| Enforcement {
        id: id.into(),
        enforced,
        by: Vec::new(),
        not_enforced: why.iter().map(|s| s.to_string()).collect(),
    };
    let s = enforcement_summary(&[
        e("a/x", true, &[]),
        e("a/y", false, &["ruff codes not selected"]),
        e("a/z", false, &["ruff codes not selected"]),
        e("a/w", false, &["vitest not installed"]),
    ]);
    assert_eq!(
        s,
        "4 active, 1 enforced, 3 not enforced (2 ruff codes not selected, 1 vitest not installed)"
    );
}
