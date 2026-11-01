//! Python libraries and packaging (`python.sh`): type stubs that mirror a module, uv's release
//! cooldown.

use std::collections::BTreeSet;

use forge_check::builtins::{RunMode, ToolRun};
use forge_core::{Finding, NoteKind, Report};

use crate::Run;
use crate::util::{
    basename, dirname, duration_seconds, duration_text, has_word, py_word, re, toml_get, version_ge,
};

/// `forge_py_public`: the public names of a Python module: the entries of `__all__` when it
/// has one, else its top-level def and class names without a leading underscore.
fn py_public(text: &str) -> BTreeSet<String> {
    let (mut all, mut inall) = (false, false);
    let mut a = BTreeSet::new();
    let mut d = BTreeSet::new();
    for l in text.lines() {
        if re(r"^__all__[ \t]*(:[^=]*)?\+?=").is_match(l) {
            all = true;
            inall = true;
        }
        if inall {
            for c in re(r#"["']([A-Za-z_][A-Za-z0-9_]*)["']"#).captures_iter(l) {
                a.insert(c[1].to_owned());
            }
            if l.contains(']') || l.contains(')') {
                inall = false;
            }
            continue;
        }
        if let Some(c) =
            re(r"^(?:(?:async[ \t]+)?def|class)[ \t]+([A-Za-z][A-Za-z0-9_]*)").captures(l)
        {
            d.insert(c[1].to_owned());
        }
    }
    if all { a } else { d }
}

/// Every top-level def and class name, private ones included.
fn py_defined(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter_map(|l| {
            re(r"^(?:(?:async[ \t]+)?def|class)[ \t]+([A-Za-z_][A-Za-z0-9_]*)")
                .captures(l)
                .map(|c| c[1].to_owned())
        })
        .collect()
}

/// `forge_py_module`: the module path inside its top-level package ("experimental/client" for
/// pkg/experimental/client.py and for pkg-api/pkg_api/experimental/client.pyi): directories
/// with an `__init__.py` or `__init__.pyi` count as packages, the topmost one is dropped.
fn py_module(run: &Run<'_>, path: &str) -> String {
    let mut d = dirname(path);
    let b = basename(path);
    let mut m = b
        .strip_suffix(".pyi")
        .or_else(|| b.strip_suffix(".py"))
        .unwrap_or(b)
        .to_owned();
    let mut top = String::new();
    while d != "."
        && d != "/"
        && (run.exists(&format!("{d}/__init__.py")) || run.exists(&format!("{d}/__init__.pyi")))
    {
        if !top.is_empty() {
            m = format!("{top}/{m}");
        }
        top = basename(&d).to_owned();
        d = dirname(&d);
    }
    m
}

/// `forge_stub_sync`: for each changed module that gained public names, each .pyi stub in the
/// repository with the same module path mentions every new name.
pub fn stub_sync(run: &Run<'_>, _args: &[String]) -> Report {
    let mut r = Report::default();
    let stubs: Vec<(String, String)> = run
        .ls_files(&["--cached", "--others", "--exclude-standard", "--", "*.pyi"])
        .into_iter()
        .filter(|s| run.exists(s))
        .map(|s| (py_module(run, &s), s))
        .collect();
    if stubs.is_empty() {
        return r;
    }
    for f in run.files().iter().filter(|f| f.ends_with(".py")) {
        let module = py_module(run, f);
        let mine: Vec<&String> = stubs
            .iter()
            .filter(|(m, _)| *m == module)
            .map(|(_, s)| s)
            .collect();
        if mine.is_empty() {
            continue;
        }
        let now = py_public(&run.read(f).unwrap_or_default());
        let then: BTreeSet<String> = match run.base(f) {
            Some(b) => py_public(&b).into_iter().chain(py_defined(&b)).collect(),
            None => BTreeSet::new(),
        };
        let new: Vec<&String> = now.iter().filter(|n| !then.contains(*n)).collect();
        if new.is_empty() {
            continue;
        }
        for s in mine {
            let text = run.read(s).unwrap_or_default();
            let missing: Vec<&str> = new
                .iter()
                .filter(|n| !has_word(&text, n, py_word))
                .map(|n| n.as_str())
                .collect();
            if !missing.is_empty() {
                // the stub is not part of the change: a finding about the change as a whole
                r.findings.push(run.finding(format!(
                    "The stub {s} does not have {}, new in {f}; add it there too.",
                    missing.join(", ")
                )));
            }
        }
    }
    r
}

/// `forge_uv_cooldown`: [tool.uv] (or uv.toml) exclude-newer is a relative duration of at least
/// the cooldown, and required-version asks for uv >= 0.9.17 (older uv ignores relative values).
/// In a change, only what the change broke: a setting the base already lacked is the project's
/// setup, which `forge apply` offers and `forge status` reports, not the agent's to fix (§10).
pub fn uv_cooldown(run: &Run<'_>, _args: &[String]) -> Report {
    // compared with the base here: its findings are the change's as they are
    run.check.mark_attributed();
    let (file, section) = if run.exists("uv.toml") {
        ("uv.toml", "")
    } else {
        ("pyproject.toml", "tool.uv")
    };
    let text = run.read(file).unwrap_or_default();
    let min = run.opts.cooldown.as_secs();
    let now = cooldown_problems(&text, file, section, min);
    let before = if run.all() {
        Vec::new()
    } else if run.changed().iter().any(|c| c == file) {
        // a file the change created had nothing before
        run.base(file)
            .map(|b| cooldown_problems(&b, file, section, min))
            .unwrap_or_default()
    } else {
        now.clone()
    };
    let mut r = Report::default();
    for (key, message) in now {
        if !before.iter().any(|(k, _)| *k == key) {
            r.findings.push(run.finding(message));
        }
    }
    r
}

/// What a uv config lacks of the cooldown: (setting, the finding's text).
fn cooldown_problems(
    text: &str,
    file: &str,
    section: &str,
    min: u64,
) -> Vec<(&'static str, String)> {
    let shown = if section.is_empty() {
        "top level"
    } else {
        section
    };
    let mut out = Vec::new();
    let v = toml_get(text, section, "exclude-newer").unwrap_or_default();
    if duration_seconds(&v).is_none_or(|n| n < min) {
        let example = duration_text(min.max(604_800));
        out.push((
            "exclude-newer",
            format!(
                "{file}: [{shown}] exclude-newer is {}; set a relative duration of at least {}, e.g. exclude-newer = \"{example}\", so a freshly published (possibly compromised) version is not resolved.",
                if v.is_empty() { "unset" } else { &v },
                duration_text(min)
            ),
        ));
    }
    let req = toml_get(text, section, "required-version").unwrap_or_default();
    let lower = re(r"^[ \t]*(?:>=|==|~=|>)[ \t]*([0-9][0-9.]*)")
        .captures(&req)
        .map(|c| c[1].to_owned());
    if lower.is_none_or(|m| !version_ge(&m, "0.9.17")) {
        out.push((
            "required-version",
            format!(
                "{file}: [{shown}] required-version is {}; set required-version = \">=0.9.17\" (older uv ignores a relative exclude-newer).",
                if req.is_empty() { "unset" } else { &req }
            ),
        ));
    }
    out
}

/// The last non-empty line of a tool's output: its error.
fn last_line(text: &str) -> &str {
    text.lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
}

/// `forge_uv_lock`: uv.lock matches pyproject.toml, and Forge keeps it so (§10.5): the lock is
/// derived, so a stale one is relocked rather than reported. In the agent's turn, the relock is
/// advice; in a commit (`--changed`), a refusal to commit again with the new lock; in CI (`--all`,
/// `--since`), nothing is written and a stale or missing lock is a finding. A change is never held
/// to a lock the project never had: creating it is setup, which `forge apply` offers (§3.5).
pub fn uv_lock(run: &Run<'_>, _args: &[String]) -> Report {
    run.check.mark_attributed();
    let mut r = Report::default();
    let root = run.root();
    let lock_now = run.exists("uv.lock");
    let ci = run.all() || run.check.mode == RunMode::Since;
    if !ci {
        let lock_before = if run.changed().iter().any(|c| c == "uv.lock") {
            run.base_path("uv.lock").is_some()
        } else {
            lock_now
        };
        if !lock_now && !lock_before {
            return r;
        }
    } else if !lock_now {
        r.findings.push(run.finding(
            "uv.lock is missing: create it with `uv lock` and commit it, so every install resolves the same versions.",
        ));
        return r;
    }
    match run.opts.tools.run(root, root, "uv", "lock-check", &[]) {
        ToolRun::Ran(o) if o.status == 0 => return r,
        ToolRun::Ran(_) => {}
        ToolRun::Missing(why) => {
            r.notes
                .push(run.note(NoteKind::MissingTool, format!("skipped: {why}")));
            return r;
        }
        ToolRun::Unsupported(why) => {
            r.notes.push(run.note(NoteKind::NotEnforced, why));
            return r;
        }
    }
    if ci {
        r.findings.push(run.finding(
            "uv.lock does not match pyproject.toml: run `uv lock` and commit the result.",
        ));
        return r;
    }
    match run.opts.tools.run(root, root, "uv", "lock", &[]) {
        ToolRun::Ran(o) if o.status == 0 => {
            let text = "uv.lock was updated to match pyproject.toml.";
            if run.check.mode == RunMode::Changed {
                r.findings
                    .push(run.finding(format!("{text} Add it and commit again.")));
            } else {
                r.findings.push(Finding::advice(run.ctx.rule.clone(), text));
            }
        }
        ToolRun::Ran(o) => {
            let why = last_line(&o.stderr);
            r.findings.push(run.finding(format!(
                "pyproject.toml does not resolve, so uv.lock cannot be updated: {}",
                if why.is_empty() {
                    last_line(&o.stdout)
                } else {
                    why
                }
            )));
        }
        ToolRun::Missing(why) => r
            .notes
            .push(run.note(NoteKind::MissingTool, format!("skipped: {why}"))),
        ToolRun::Unsupported(why) => r.notes.push(run.note(NoteKind::NotEnforced, why)),
    }
    r
}
