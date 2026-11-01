//! Python libraries and packaging (`python.sh`): type stubs that mirror a module, uv's release
//! cooldown, pinned requirements.

use std::collections::{BTreeMap, BTreeSet};

use forge_check::builtins::RunMode;
use forge_core::{Finding, Report};

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
/// A `pyproject.toml` the agent's turn created gets the missing settings from Forge (advice):
/// they are mechanical, nothing for the agent to do. In a commit and in CI they are reported.
pub fn uv_cooldown(run: &Run<'_>, _args: &[String]) -> Report {
    // compared with the base here: its findings are the change's as they are
    run.check.mark_attributed();
    let (file, section) = if run.exists("uv.toml") {
        ("uv.toml", "")
    } else {
        ("pyproject.toml", "tool.uv")
    };
    let mut r = Report::default();
    let mut text = run.read(file).unwrap_or_default();
    let created =
        !run.all() && run.changed().iter().any(|c| c == file) && run.base_path(file).is_none();
    if created
        && file == "pyproject.toml"
        && run.check.mode == RunMode::Turn
        && let Some(after) = with_uv_cooldown(&text)
        && std::fs::write(run.path(file), &after).is_ok()
    {
        text = after;
        r.findings.push(Finding::advice(
            run.ctx.rule.clone(),
            "pyproject.toml: added [tool.uv] exclude-newer and required-version.",
        ));
    }
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
    for (key, message) in now {
        if !before.iter().any(|(k, _)| *k == key) {
            r.findings.push(run.finding(message));
        }
    }
    r
}

/// The settings `python/uv/exclude-newer` asks of a project, as Forge adds them.
pub const UV_COOLDOWN: [(&str, &str); 2] = [
    ("exclude-newer", "\"7 days\""),
    ("required-version", "\">=0.9.17\""),
];

/// `pyproject.toml` with the `[tool.uv]` keys of [`UV_COOLDOWN`] it lacks: under its
/// `[tool.uv]` header, else in a new table at the end. A value the project set is never
/// changed. `None`: nothing to add, or a file it cannot read as TOML.
pub fn with_uv_cooldown(text: &str) -> Option<String> {
    let doc: toml::Table = text.parse().ok()?;
    let uv = doc
        .get("tool")
        .and_then(|t| t.get("uv"))
        .and_then(|u| u.as_table());
    let missing: Vec<String> = UV_COOLDOWN
        .iter()
        .filter(|(k, _)| uv.is_none_or(|u| !u.contains_key(*k)))
        .map(|(k, v)| format!("{k} = {v}"))
        .collect();
    if missing.is_empty() {
        return None;
    }
    let header = text
        .lines()
        .position(|l| l.trim().replace(' ', "") == "[tool.uv]");
    let out = match header {
        Some(i) => {
            let mut lines: Vec<&str> = text.lines().collect();
            for (n, m) in missing.iter().enumerate() {
                lines.insert(i + 1 + n, m);
            }
            let mut t = lines.join("\n");
            t.push('\n');
            t
        }
        None => {
            let mut t = text.trim_end().to_string();
            t.push_str("\n\n[tool.uv]\n");
            for m in &missing {
                t.push_str(m);
                t.push('\n');
            }
            t
        }
    };
    out.parse::<toml::Table>().ok().map(|_| out)
}

/// `forge_pinned_requirements`: an entry a change adds to a requirements file pins an exact
/// version where the file does: its base copy pins every entry, or the change created it (an
/// added entry: a line the base copy does not have). A
/// file that writes ranges or bare names keeps its own convention, and the whole project
/// (`--all`) has no change to hold to one.
pub fn pinned_requirements(run: &Run<'_>, _args: &[String]) -> Report {
    run.check.mark_attributed();
    let mut r = Report::default();
    if run.all() {
        return r;
    }
    for f in run.files() {
        let base = run.base(f);
        if base
            .as_deref()
            .is_some_and(|b| forge_detect::deps::requirements_pinned(b) == Some(false))
        {
            continue;
        }
        // the entries the change added: lines its base copy does not have (as many times)
        let mut before: BTreeMap<&str, usize> = BTreeMap::new();
        for l in base.as_deref().unwrap_or("").lines() {
            *before.entry(l.trim()).or_default() += 1;
        }
        let text = run.read(f).unwrap_or_default();
        for (i, line) in text.lines().enumerate() {
            let n = u32::try_from(i + 1).unwrap_or(u32::MAX);
            if let Some(k) = before.get_mut(line.trim()).filter(|k| **k > 0) {
                *k -= 1;
                continue;
            }
            if !forge_detect::deps::unpinned_requirement(line) {
                continue;
            }
            let entry = line.split(" #").next().unwrap_or(line).trim();
            let name = re(r"^[A-Za-z0-9][A-Za-z0-9._-]*(\[[^\]]*\])?")
                .find(entry)
                .map_or(entry, |m| m.as_str());
            let why = if base.is_some() {
                "as the file's other entries are"
            } else {
                "so every install gets the same version"
            };
            r.findings.push(run.at(
                f,
                Some(n),
                format!("`{entry}` has no exact version: pin it (`{name}==<version>`), {why}."),
            ));
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

/// `forge_uv_lock`: uv.lock matches pyproject.toml, kept so as [`crate::lock::keep`] says.
pub fn uv_lock(run: &Run<'_>, _args: &[String]) -> Report {
    crate::lock::keep(run, &crate::lock::UV)
}

#[cfg(test)]
mod tests {
    use super::with_uv_cooldown;

    #[test]
    fn the_uv_cooldown_is_added_where_missing() {
        let plain = "[project]\nname = \"x\"\n";
        assert_eq!(
            with_uv_cooldown(plain).unwrap(),
            "[project]\nname = \"x\"\n\n[tool.uv]\nexclude-newer = \"7 days\"\nrequired-version = \">=0.9.17\"\n"
        );
        // under an existing [tool.uv]; a value the project set is kept
        let some = "[project]\nname = \"x\"\n\n[tool.uv]\nexclude-newer = \"2 days\"\n";
        assert_eq!(
            with_uv_cooldown(some).unwrap(),
            "[project]\nname = \"x\"\n\n[tool.uv]\nrequired-version = \">=0.9.17\"\nexclude-newer = \"2 days\"\n"
        );
        let done = "[tool.uv]\nexclude-newer = \"7 days\"\nrequired-version = \">=0.9.17\"\n";
        assert!(with_uv_cooldown(done).is_none());
        assert!(with_uv_cooldown("not = = toml").is_none());
    }
}
