//! Dependencies a directory declares: package.json, pyproject.toml (PEP 621, dependency
//! groups, uv's dev dependencies and Poetry's tables), requirements files and Pipfile. Each
//! name maps to the lowest version its specifier admits (`""` when unknown).

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::versions::{coerce, from_pep440, from_poetry, from_range};

pub type Deps = BTreeMap<String, String>;

/// PyPI's name normalisation (PEP 503): lower case, runs of `-_.` as one `-`.
pub fn norm_py(name: &str) -> String {
    static RUN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[-_.]+").expect("regex"));
    RUN.replace_all(&name.to_lowercase(), "-").into_owned()
}

/// A requirement string (`fastapi[all]>=0.110 ; python_version>"3"`): its normalised name and version.
pub fn parse_requirement(req: &str) -> Option<(String, String)> {
    static REQ: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^\s*([A-Za-z0-9][A-Za-z0-9._-]*)\s*(?:\[[^\]]*\])?\s*(.*)$").expect("regex")
    });
    let c = REQ.captures(req)?;
    Some((norm_py(&c[1]), from_pep440(&c[2])))
}

/// What a package.json declares, the package manager it pins included (corepack's
/// `packageManager`, `devEngines.packageManager`).
pub fn npm_deps(pkg: &Value) -> Deps {
    let mut out = Deps::new();
    for key in [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ] {
        if let Some(m) = pkg.get(key).and_then(Value::as_object) {
            for (name, range) in m {
                if let Some(r) = range.as_str() {
                    out.insert(name.clone(), from_range(r));
                }
            }
        }
    }
    if let Some((name, version)) = package_manager(pkg) {
        let v = from_range(&version);
        out.insert(name, if v.is_empty() { coerce(&version) } else { v });
    }
    out
}

/// The package manager a package.json pins: `packageManager` (`pnpm@9.1.0+sha…`), else
/// `devEngines.packageManager` (`{name, version}`).
pub fn package_manager(pkg: &Value) -> Option<(String, String)> {
    static SPEC: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(@?[^@\s]+)@([^+\s]*)").expect("regex"));
    let dev = pkg.pointer("/devEngines/packageManager").and_then(|p| {
        let name = p.get("name")?.as_str()?;
        Some(format!(
            "{name}@{}",
            p.get("version").and_then(Value::as_str).unwrap_or("")
        ))
    });
    let specs = [
        pkg.get("packageManager")
            .and_then(Value::as_str)
            .map(str::to_owned),
        dev,
    ];
    let mut found = None;
    for spec in specs.into_iter().flatten() {
        if let Some(c) = SPEC.captures(&spec) {
            // the later one wins, as in the TypeScript reader (both set the same map entry)
            found = Some((c[1].to_string(), c[2].to_string()));
        }
    }
    found
}

fn strings(v: Option<&Value>) -> impl Iterator<Item = &str> {
    v.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

/// What a pyproject.toml declares: `[project]` dependencies and optional ones, dependency
/// groups, `[tool.uv] dev-dependencies`, and Poetry's dependency tables.
pub fn pyproject_deps(toml: &Value) -> Deps {
    let mut out = Deps::new();
    let mut reqs: Vec<&str> = Vec::new();
    reqs.extend(strings(toml.pointer("/project/dependencies")));
    if let Some(opt) = toml
        .pointer("/project/optional-dependencies")
        .and_then(Value::as_object)
    {
        for v in opt.values() {
            reqs.extend(strings(Some(v)));
        }
    }
    if let Some(groups) = toml.get("dependency-groups").and_then(Value::as_object) {
        for v in groups.values() {
            reqs.extend(strings(Some(v)));
        }
    }
    reqs.extend(strings(toml.pointer("/tool/uv/dev-dependencies")));
    for r in reqs {
        if let Some((name, v)) = parse_requirement(r) {
            out.insert(name, v);
        }
    }
    for (name, v) in poetry_deps(toml) {
        out.entry(name).or_insert(v);
    }
    out
}

/// `[tool.poetry.dependencies]`, `[tool.poetry.dev-dependencies]` and
/// `[tool.poetry.group.<g>.dependencies]`, without `python` itself.
pub fn poetry_deps(toml: &Value) -> Deps {
    let mut out = Deps::new();
    let Some(poetry) = toml.pointer("/tool/poetry") else {
        return out;
    };
    let mut tables: Vec<&Value> = Vec::new();
    tables.extend(poetry.get("dependencies"));
    tables.extend(poetry.get("dev-dependencies"));
    if let Some(groups) = poetry.get("group").and_then(Value::as_object) {
        for g in groups.values() {
            tables.extend(g.get("dependencies"));
        }
    }
    for t in tables {
        for (name, spec) in t.as_object().into_iter().flatten() {
            if name.eq_ignore_ascii_case("python") {
                continue;
            }
            out.insert(norm_py(name), table_spec(spec));
        }
    }
    out
}

/// A Poetry or Pipenv dependency value: a constraint string, or a table with `version`
/// (a list of tables, for multiple constraints: the first).
fn table_spec(spec: &Value) -> String {
    match spec {
        Value::String(s) => from_poetry(s),
        Value::Object(o) => o
            .get("version")
            .and_then(Value::as_str)
            .map(from_poetry)
            .unwrap_or_default(),
        Value::Array(a) => a.first().map(table_spec).unwrap_or_default(),
        _ => String::new(),
    }
}

/// Whether `[tool.poetry]` declares dependencies (beyond `python`).
pub fn poetry_declares_deps(toml: &Value) -> bool {
    !poetry_deps(toml).is_empty()
}

/// Pipfile's `[packages]` and `[dev-packages]`.
pub fn pipfile_deps(toml: &Value) -> Deps {
    let mut out = Deps::new();
    for key in ["packages", "dev-packages"] {
        for (name, spec) in toml
            .get(key)
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            out.insert(norm_py(name), table_spec(spec));
        }
    }
    out
}

/// A requirements file's requirements (options, includes, editable installs and URLs skipped).
pub fn requirements_deps(text: &str) -> Deps {
    let mut out = Deps::new();
    for raw in text.lines() {
        let line = match raw.find(" #") {
            Some(i) => &raw[..i],
            None => raw,
        }
        .trim();
        let line = line.split(';').next().unwrap_or("");
        let line = line.split(" @ ").next().unwrap_or("").trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('-') || line.contains("://")
        {
            continue;
        }
        if let Some((name, v)) = parse_requirement(line) {
            out.insert(name, v);
        }
    }
    out
}

/// The requirements files of a directory: `requirements*.txt` there and `requirements/*.txt`.
pub fn requirements_files(own: &[String]) -> impl Iterator<Item = &String> {
    own.iter().filter(|f| {
        let name_ok = |n: &str| n.ends_with(".txt");
        (f.starts_with("requirements") && !f.contains('/') && name_ok(f))
            || f.strip_prefix("requirements/")
                .is_some_and(|rest| !rest.contains('/') && name_ok(rest))
    })
}

/// A TOML file as JSON (`None` when missing or not TOML).
pub fn read_toml(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    parse_toml(&text).ok()
}

pub fn parse_toml(text: &str) -> Result<Value, toml::de::Error> {
    let t: toml::Table = toml::from_str(text)?;
    Ok(toml_to_json(toml::Value::Table(t)))
}

pub fn toml_to_json(v: toml::Value) -> Value {
    match v {
        toml::Value::String(s) => Value::String(s),
        toml::Value::Integer(i) => Value::from(i),
        toml::Value::Float(f) => Value::from(f),
        toml::Value::Boolean(b) => Value::Bool(b),
        toml::Value::Datetime(d) => Value::String(d.to_string()),
        toml::Value::Array(a) => Value::Array(a.into_iter().map(toml_to_json).collect()),
        toml::Value::Table(t) => {
            Value::Object(t.into_iter().map(|(k, v)| (k, toml_to_json(v))).collect())
        }
    }
}

/// A JSON (or JSONC) file (`None` when missing or invalid).
pub fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    crate::jsonc::parse(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pyproject_and_poetry() {
        let t = parse_toml(
            r#"
[project]
dependencies = ["FastAPI[all]>=0.110", "sqlalchemy==2.0.30"]
[project.optional-dependencies]
x = ["httpx~=0.27"]
[dependency-groups]
dev = ["pytest>=8", {include-group = "x"}]
[tool.poetry.dependencies]
python = "^3.12"
Django = "^5.0"
celery = {version = "~5.3", extras = ["redis"]}
[tool.poetry.group.test.dependencies]
pytest-django = "4.8.0"
"#,
        )
        .unwrap();
        let d = pyproject_deps(&t);
        assert_eq!(d["fastapi"], "0.110.0");
        assert_eq!(d["sqlalchemy"], "2.0.30");
        assert_eq!(d["httpx"], "0.27.0");
        assert_eq!(d["pytest"], "8.0.0");
        assert_eq!(d["django"], "5.0.0");
        assert_eq!(d["celery"], "5.3.0");
        assert_eq!(d["pytest-django"], "4.8.0");
        assert!(!d.contains_key("python"));
    }

    #[test]
    fn requirements_and_npm() {
        let d = requirements_deps(
            "# c\n-r base.txt\nrequests==2.31.0 ; python_version > '3'\nflask  # web\n-e .\ngit+https://x/y\npkg @ https://x\n",
        );
        assert_eq!(d["requests"], "2.31.0");
        assert_eq!(d["flask"], "");
        assert_eq!(d["pkg"], "");
        assert_eq!(d.len(), 3);
        let n = npm_deps(
            &json!({"dependencies": {"next": "^15.1.0", "x": "workspace:*"}, "packageManager": "pnpm@9.1.0+sha256.abc"}),
        );
        assert_eq!(n["next"], "15.1.0");
        assert_eq!(n["x"], "");
        assert_eq!(n["pnpm"], "9.1.0");
    }
}
