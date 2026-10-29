//! pytest's filterwarnings turns a framework's next-version deprecations into errors
//! (`pytest_filterwarnings.py`). Arguments: `<entry> <dependency>`.
//!
//! Reads the pytest configuration pytest itself uses (the first of pytest.ini, .pytest.ini,
//! pyproject.toml with `[tool.pytest]` or `[tool.pytest.ini_options]`, tox.ini with `[pytest]`,
//! setup.cfg with `[tool:pytest]`) and reports when its filterwarnings has neither `<entry>` nor a
//! bare "error".

use forge_core::Report;
use forge_core::builtin::{Builtin, BuiltinCtx};
use serde_json::Value;

use crate::util::{Change, Ini, at, could_not_run, get, obj, parse_toml, py_str, strip_script};

pub struct PytestFilterwarnings;

impl Builtin for PytestFilterwarnings {
    fn name(&self) -> &'static str {
        "pytest_filterwarnings"
    }

    fn run(&self, ctx: &BuiltinCtx, args: &[String]) -> Report {
        let (_, args) = strip_script(args);
        let mut r = Report::default();
        let (Some(entry), Some(dep)) = (args.first(), args.get(1)) else {
            r.notes.push(could_not_run(
                &ctx.rule,
                "the pytest filterwarnings check needs an entry and a dependency",
            ));
            return r;
        };
        let ch = Change::new(ctx);
        let (path, entries) = config(&ch);
        let norm: Vec<String> = entries.iter().map(|e| e.replace(' ', "")).collect();
        if norm.contains(&entry.replace(' ', "")) || norm.iter().any(|e| e == "error") {
            return r;
        }
        r.findings.push(at(
            &ctx.rule,
            path,
            0,
            format!("pytest filterwarnings has no \"{entry}\": {dep} deprecations only warn, so tests pass on code the next {dep} release breaks; add it"),
        ));
        r
    }
}

fn ini(ch: &Change, path: &str, section: &str) -> Option<Vec<String>> {
    let p = Ini::parse(&ch.now(path)?).ok()?;
    if !p.has_section(section) {
        return None;
    }
    Some(
        p.get(section, "filterwarnings")
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect(),
    )
}

fn toml(ch: &Change) -> Option<Vec<String>> {
    let doc = parse_toml(&ch.now("pyproject.toml")?).ok()?;
    let tool = obj(get(obj(Some(&doc)), "tool"));
    let Some(Value::Object(pytest)) = tool.get("pytest") else {
        return None;
    };
    // pytest 9 reads [tool.pytest] itself, older versions [tool.pytest.ini_options]
    let opts = if pytest.contains_key("ini_options") {
        obj(pytest.get("ini_options"))
    } else {
        pytest
    };
    let items: Vec<String> = match opts.get("filterwarnings") {
        None => vec![],
        Some(Value::Array(a)) => a.iter().map(py_str).collect(),
        Some(v) => py_str(v).lines().map(str::to_owned).collect(),
    };
    Some(
        items
            .iter()
            .map(|x| x.trim().to_owned())
            .filter(|x| !x.is_empty())
            .collect(),
    )
}

fn config(ch: &Change) -> (&'static str, Vec<String>) {
    for path in ["pytest.ini", ".pytest.ini"] {
        if ch.exists(path) {
            return (path, ini(ch, path, "pytest").unwrap_or_default());
        }
    }
    if ch.exists("pyproject.toml")
        && let Some(e) = toml(ch)
    {
        return ("pyproject.toml", e);
    }
    if ch.exists("tox.ini")
        && let Some(e) = ini(ch, "tox.ini", "pytest")
    {
        return ("tox.ini", e);
    }
    if ch.exists("setup.cfg")
        && let Some(e) = ini(ch, "setup.cfg", "tool:pytest")
    {
        return ("setup.cfg", e);
    }
    ("pyproject.toml", vec![])
}
