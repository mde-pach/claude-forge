//! The installed packages ty is told to check as Any (`untyped_python.py`, SPECS §15.2): those
//! that ship no typing (PEP 561: no py.typed, no .pyi, no `<name>-stubs` installed). ty infers
//! such a package's types from its source, where mypy treats it as Any; on a library with
//! runtime magic that gives errors on correct code.
//!
//! The site-packages directories are read directly. A module ty resolves without that
//! environment is left out: one of the standard library stubs ty bundles, or the project's own
//! code; which ones those are is asked from the pinned ty itself (a probe file checked with an
//! empty environment).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::util::{get, obj, parse_toml, regex_static};

/// What [`untyped_modules`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Untyped {
    /// The directories the answer depends on (site-packages and ty's first-party roots): a change
    /// there recomputes it.
    pub watch: Vec<PathBuf>,
    /// Each untyped top-level module, or namespace package portion (`a.b`), sorted.
    pub modules: Vec<String>,
}

/// The untyped modules of the environment `sites` (in import order) for the project at `root`,
/// checked by `ty` with Python `py_version` (`3.12`).
pub fn untyped_modules(
    root: &Path,
    py_version: &str,
    sites: &[PathBuf],
    ty: &Path,
) -> Result<Untyped, String> {
    let sites: Vec<PathBuf> = sites.iter().filter(|s| s.is_dir()).cloned().collect();
    let (config, project) = ty_config(root);
    let mut seen: BTreeMap<String, bool> = BTreeMap::new();
    for site in &sites {
        let mut found = vec![];
        units(site, "", 0, &sites, &mut found);
        for (name, typed) in found {
            seen.entry(name).or_insert(typed);
        }
    }
    let untyped: Vec<String> = seen
        .iter()
        .filter(|(_, t)| !**t)
        .map(|(n, _)| n.clone())
        .collect();
    let elsewhere =
        resolved_without_environment(root, py_version, &untyped, &config, &project, ty)?;
    let mut watch = sites.clone();
    watch.extend(roots(root, &config));
    Ok(Untyped {
        watch,
        modules: untyped
            .into_iter()
            .filter(|n| !elsewhere.contains(n))
            .collect(),
    })
}

const SKIP_SUFFIXES: &[&str] = &[
    ".dist-info",
    ".egg-info",
    ".data",
    ".pth",
    ".egg-link",
    ".egg",
    "-stubs",
];

fn is_identifier(s: &str) -> bool {
    let mut c = s.chars();
    c.next().is_some_and(|f| f == '_' || f.is_alphabetic())
        && c.all(|x| x == '_' || x.is_alphanumeric())
}

fn has_init(d: &Path) -> bool {
    d.join("__init__.py").is_file() || d.join("__init__.pyi").is_file()
}

/// A package directory with py.typed, or with a .pyi anywhere in it.
fn ships_typing(d: &Path) -> bool {
    if d.join("py.typed").is_file() {
        return true;
    }
    let mut todo = vec![d.to_path_buf()];
    while let Some(dir) = todo.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let path = e.path();
            if path.is_dir() {
                if name != "__pycache__" {
                    todo.push(path);
                }
            } else if name == "py.typed" || name.ends_with(".pyi") {
                return true;
            }
        }
    }
    false
}

/// A stub-only package (`<top>-stubs`) covers the module in some site directory.
fn stubbed(dotted: &str, sites: &[PathBuf]) -> bool {
    let (top, rest) = dotted.split_once('.').unwrap_or((dotted, ""));
    sites.iter().any(|site| {
        let mut base = site.join(format!("{top}-stubs"));
        if !rest.is_empty() {
            for p in rest.split('.') {
                base = base.join(p);
            }
        }
        base.is_dir() || PathBuf::from(format!("{}.pyi", base.display())).is_file()
    })
}

fn units(site: &Path, prefix: &str, depth: usize, sites: &[PathBuf], out: &mut Vec<(String, bool)>) {
    let Ok(rd) = std::fs::read_dir(site) else { return };
    let mut entries: Vec<String> = rd
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    entries.sort();
    let ext = regex_static(r"^([A-Za-z_][A-Za-z0-9_]*)\..*\.(so|pyd)$|^([A-Za-z_][A-Za-z0-9_]*)\.(so|pyd)$");
    for entry in entries {
        let path = site.join(&entry);
        if entry.starts_with("__") || SKIP_SUFFIXES.iter().any(|s| entry.ends_with(s)) {
            continue;
        }
        if path.is_dir() {
            if !is_identifier(&entry) {
                continue;
            }
            let name = format!("{prefix}{entry}");
            if has_init(&path) {
                let typed = ships_typing(&path) || stubbed(&name, sites);
                out.push((name, typed));
            } else if depth < 2 {
                // a namespace package: each portion is its own distribution's
                if path.join("py.typed").is_file() {
                    out.push((name, true));
                } else {
                    units(&path, &format!("{name}."), depth + 1, sites, out);
                }
            }
        } else if let Some(stem) = entry.strip_suffix(".py").filter(|s| is_identifier(s)) {
            let name = format!("{prefix}{stem}");
            let typed = site.join(format!("{entry}i")).is_file() || stubbed(&name, sites);
            out.push((name, typed));
        } else if let Some(stem) = entry.strip_suffix(".pyi").filter(|s| is_identifier(s)) {
            out.push((format!("{prefix}{stem}"), true));
        } else if let Some(c) = ext.captures(&entry) {
            let stem = c.get(1).or_else(|| c.get(3)).map_or("", |m| m.as_str());
            let name = format!("{prefix}{stem}");
            let typed = site.join(format!("{stem}.pyi")).is_file() || stubbed(&name, sites);
            out.push((name, typed));
        }
    }
}

/// The project's ty configuration (`ty.toml`, or `[tool.ty]` in pyproject.toml) and its
/// `[project]` table.
fn ty_config(root: &Path) -> (Value, Value) {
    let empty = || Value::Object(Default::default());
    for (f, in_tool) in [("ty.toml", false), ("pyproject.toml", true)] {
        let Ok(text) = std::fs::read_to_string(root.join(f)) else {
            continue;
        };
        let Ok(data) = parse_toml(&text) else { continue };
        if !in_tool {
            return (data, empty());
        }
        let project = get(obj(Some(&data)), "project").cloned().unwrap_or_else(empty);
        let tool = obj(get(obj(Some(&data)), "tool"));
        return (tool.get("ty").cloned().unwrap_or_else(empty), project);
    }
    (empty(), empty())
}

/// ty's first-party search roots: environment.root, else its layout detection's candidates.
fn roots(root: &Path, config: &Value) -> Vec<PathBuf> {
    let c = obj(Some(config));
    let configured = get(obj(get(c, "environment")), "root")
        .or_else(|| get(obj(get(c, "src")), "root"));
    let names: Vec<String> = match configured {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) if !a.is_empty() => {
            a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()
        }
        _ => [".", "src", "python", "tests"].map(str::to_owned).to_vec(),
    };
    names.iter().map(|n| normpath(&root.join(n))).collect()
}

fn normpath(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            x => out.push(x),
        }
    }
    out
}

/// The names the pinned ty resolves with no site-packages at all.
fn resolved_without_environment(
    root: &Path,
    py_version: &str,
    names: &[String],
    config: &Value,
    project: &Value,
    ty: &Path,
) -> Result<Vec<String>, String> {
    if names.is_empty() {
        return Ok(vec![]);
    }
    let tmp = std::env::temp_dir().join(format!(
        "forge-untyped-{}-{}",
        std::process::id(),
        crate::util::now_secs() as u64
    ));
    let env = tmp.join("env");
    std::fs::create_dir_all(env.join("lib").join(format!("python{py_version}")).join("site-packages"))
        .map_err(|e| e.to_string())?;
    let probe = tmp.join("probe.py");
    let mut src = String::from("# probe\n");
    for (i, n) in names.iter().enumerate() {
        src.push_str(&format!("import {n} as _m{i}; reveal_type(_m{i})\n"));
    }
    std::fs::write(&probe, src).map_err(|e| e.to_string())?;
    let mut cmd = Command::new(ty);
    cmd.arg("check")
        .arg("--project")
        .arg(root)
        .arg("--python")
        .arg(&env)
        .args(["--output-format", "concise", "--exit-zero"]);
    let has_version = get(obj(get(obj(Some(config)), "environment")), "python-version").is_some()
        || get(obj(Some(project)), "requires-python").is_some();
    if !has_version {
        cmd.args(["--python-version", py_version]);
    }
    let out = cmd.arg(&probe).current_dir(root).output();
    let _ = std::fs::remove_dir_all(&tmp);
    let out = out.map_err(|e| format!("ty could not run: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "ty could not run the probe: {}",
            err.trim().lines().next().unwrap_or("")
        ));
    }
    let re = regex_static(r"probe\.py:(\d+):\d+: info\[revealed-type\] Revealed type: `<module '");
    let mut found = vec![];
    for l in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some(c) = re.captures(l)
            && let Ok(i) = c[1].parse::<usize>()
            && i >= 2
            && let Some(n) = names.get(i - 2)
        {
            found.push(n.clone());
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_of_a_site() {
        let d = tempfile::tempdir().unwrap();
        let s = d.path();
        std::fs::create_dir_all(s.join("typed_pkg")).unwrap();
        std::fs::write(s.join("typed_pkg/__init__.py"), "").unwrap();
        std::fs::write(s.join("typed_pkg/py.typed"), "").unwrap();
        std::fs::create_dir_all(s.join("plain/sub")).unwrap();
        std::fs::write(s.join("plain/__init__.py"), "").unwrap();
        std::fs::create_dir_all(s.join("ns/part")).unwrap();
        std::fs::write(s.join("ns/part/__init__.py"), "").unwrap();
        std::fs::write(s.join("mod.py"), "").unwrap();
        std::fs::create_dir_all(s.join("mod-stubs")).unwrap();
        std::fs::create_dir_all(s.join("x-1.0.dist-info")).unwrap();
        let sites = vec![s.to_path_buf()];
        let mut out = vec![];
        units(s, "", 0, &sites, &mut out);
        assert_eq!(
            out,
            [
                ("mod".to_owned(), true),
                ("ns.part".to_owned(), false),
                ("plain".to_owned(), false),
                ("typed_pkg".to_owned(), true)
            ]
        );
    }
}
