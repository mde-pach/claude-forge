//! Python libraries and packaging (`python.sh`): type stubs that mirror a module, uv's release
//! cooldown.

use std::collections::BTreeSet;

use forge_core::Report;

use crate::Run;
use crate::util::{basename, dirname, duration_seconds, duration_text, has_word, py_word, re, toml_get, version_ge};

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
        if let Some(c) = re(r"^(?:(?:async[ \t]+)?def|class)[ \t]+([A-Za-z][A-Za-z0-9_]*)").captures(l) {
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
    while d != "." && d != "/" && (run.exists(&format!("{d}/__init__.py")) || run.exists(&format!("{d}/__init__.pyi"))) {
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
        let mine: Vec<&String> = stubs.iter().filter(|(m, _)| *m == module).map(|(_, s)| s).collect();
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
pub fn uv_cooldown(run: &Run<'_>, _args: &[String]) -> Report {
    let (file, section) = if run.exists("uv.toml") {
        ("uv.toml", "")
    } else {
        ("pyproject.toml", "tool.uv")
    };
    let shown = if section.is_empty() { "top level" } else { section };
    let text = run.read(file).unwrap_or_default();
    let min = run.opts.cooldown.as_secs();
    let mut r = Report::default();
    let v = toml_get(&text, section, "exclude-newer").unwrap_or_default();
    if duration_seconds(&v).is_none_or(|n| n < min) {
        let example = duration_text(min.max(604_800));
        r.findings.push(run.finding(
            format!(
                "{file}: [{shown}] exclude-newer is {}; set a relative duration of at least {}, e.g. exclude-newer = \"{example}\", so a freshly published (possibly compromised) version is not resolved.",
                if v.is_empty() { "unset" } else { &v },
                duration_text(min)
            ),
        ));
    }
    let req = toml_get(&text, section, "required-version").unwrap_or_default();
    let lower = re(r"^[ \t]*(?:>=|==|~=|>)[ \t]*([0-9][0-9.]*)")
        .captures(&req)
        .map(|c| c[1].to_owned());
    if lower.is_none_or(|m| !version_ge(&m, "0.9.17")) {
        r.findings.push(run.finding(
            format!(
                "{file}: [{shown}] required-version is {}; set required-version = \">=0.9.17\" (older uv ignores a relative exclude-newer).",
                if req.is_empty() { "unset" } else { &req }
            ),
        ));
    }
    r
}
