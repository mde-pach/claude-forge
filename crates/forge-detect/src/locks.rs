//! Resolved versions from the lock files at the repository root: they resolve the versions
//! of the root and of every workspace package. Each reader returns name → version for the
//! packages the lock resolves at top level; a lock that does not parse resolves nothing.

use std::path::Path;

use serde_json::Value;

use crate::deps::{Deps, norm_py, parse_toml};

type Reader = fn(&str) -> Deps;

/// npm versions: `bun.lock`, `pnpm-lock.yaml`, `yarn.lock`, `package-lock.json` (the first
/// that has a package wins).
pub fn npm_versions(repo: &Path) -> Deps {
    let mut out = Deps::new();
    let read = |f: &str| std::fs::read_to_string(repo.join(f)).ok();
    let sources: [(&str, Reader); 4] = [
        ("bun.lock", bun_lock),
        ("pnpm-lock.yaml", pnpm_lock),
        ("yarn.lock", yarn_lock),
        ("package-lock.json", package_lock),
    ];
    for (file, reader) in sources {
        if let Some(text) = read(file) {
            for (k, v) in reader(&text) {
                out.entry(k).or_insert(v);
            }
        }
    }
    out
}

/// PyPI versions (normalised names): `uv.lock`, `poetry.lock`, `pdm.lock`, `Pipfile.lock`.
pub fn pypi_versions(repo: &Path) -> Deps {
    let mut out = Deps::new();
    let read = |f: &str| std::fs::read_to_string(repo.join(f)).ok();
    for file in ["uv.lock", "poetry.lock", "pdm.lock"] {
        if let Some(text) = read(file) {
            for (k, v) in toml_packages(&text) {
                out.entry(k).or_insert(v);
            }
        }
    }
    if let Some(text) = read("Pipfile.lock") {
        for (k, v) in pipfile_lock(&text) {
            out.entry(k).or_insert(v);
        }
    }
    out
}

/// `[[package]]` tables with `name` and `version` (uv, Poetry and PDM locks).
pub fn toml_packages(text: &str) -> Deps {
    let mut out = Deps::new();
    let Ok(v) = parse_toml(text) else { return out };
    for p in v
        .get("package")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let (Some(name), Some(version)) = (
            p.get("name").and_then(Value::as_str),
            p.get("version").and_then(Value::as_str),
        ) {
            out.entry(norm_py(name))
                .or_insert_with(|| version.to_string());
        }
    }
    out
}

/// Pipfile.lock: `default` and `develop`, `{"version": "==1.2.3"}`.
pub fn pipfile_lock(text: &str) -> Deps {
    let mut out = Deps::new();
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return out;
    };
    for section in ["default", "develop"] {
        for (name, p) in v
            .get(section)
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            if let Some(ver) = p.get("version").and_then(Value::as_str) {
                out.entry(norm_py(name))
                    .or_insert_with(|| ver.trim_start_matches('=').to_string());
            }
        }
    }
    out
}

/// bun.lock (JSONC): `packages: {name: ["name@1.2.3", …]}`, nested resolutions skipped.
pub fn bun_lock(text: &str) -> Deps {
    let mut out = Deps::new();
    let Ok(v) = crate::jsonc::parse(text) else {
        return out;
    };
    for (name, entry) in v
        .get("packages")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        if name.contains('/') && !name.starts_with('@') {
            continue;
        }
        let spec = entry.get(0).and_then(Value::as_str).unwrap_or("");
        if let Some(at) = spec.rfind('@').filter(|&at| at > 0) {
            out.insert(name.clone(), spec[at + 1..].to_string());
        }
    }
    out
}

/// package-lock.json: v2/v3 `packages["node_modules/<name>"]`, v1 `dependencies`.
pub fn package_lock(text: &str) -> Deps {
    let mut out = Deps::new();
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return out;
    };
    for (key, p) in v
        .get("packages")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        let Some(name) = key.strip_prefix("node_modules/") else {
            continue;
        };
        if name.contains("/node_modules/") {
            continue;
        }
        if let Some(ver) = p.get("version").and_then(Value::as_str) {
            out.insert(name.to_string(), ver.to_string());
        }
    }
    if out.is_empty() {
        for (name, p) in v
            .get("dependencies")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            if let Some(ver) = p.get("version").and_then(Value::as_str) {
                out.insert(name.clone(), ver.to_string());
            }
        }
    }
    out
}

/// pnpm-lock.yaml: each importer's dependencies (`{specifier, version: "1.2.3(peer@x)"}`,
/// or a plain version in lock format 5), the root importer first.
pub fn pnpm_lock(text: &str) -> Deps {
    let mut out = Deps::new();
    let Ok(parsed) = forge_packs::yaml::parse_strict(text) else {
        return out;
    };
    let v = parsed.value;
    let mut importers: Vec<&Value> = Vec::new();
    if let Some(imp) = v.get("importers").and_then(Value::as_object) {
        importers.extend(imp.get("."));
        importers.extend(imp.iter().filter(|(k, _)| *k != ".").map(|(_, v)| v));
    } else {
        importers.push(&v);
    }
    for imp in importers {
        for key in ["dependencies", "devDependencies", "optionalDependencies"] {
            for (name, d) in imp
                .get(key)
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                let ver = match d {
                    Value::String(s) => s.as_str(),
                    _ => d.get("version").and_then(Value::as_str).unwrap_or(""),
                };
                let ver = ver.split('(').next().unwrap_or("");
                if ver.starts_with(|c: char| c.is_ascii_digit()) {
                    out.entry(name.clone()).or_insert_with(|| ver.to_string());
                }
            }
        }
    }
    out
}

/// yarn.lock, classic (`version "1.2.3"`) and Berry (`version: 1.2.3`): the first block of each name.
pub fn yarn_lock(text: &str) -> Deps {
    let mut out = Deps::new();
    let mut names: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if !line.starts_with(' ') && line.ends_with(':') {
            names = line[..line.len() - 1]
                .split(", ")
                .filter_map(|spec| {
                    let spec = spec.trim().trim_matches('"');
                    let at = spec[1..].find('@')? + 1;
                    Some(spec[..at].to_string())
                })
                .collect();
            continue;
        }
        let t = line.trim_start();
        if let Some(rest) = t
            .strip_prefix("version ")
            .or_else(|| t.strip_prefix("version: "))
        {
            let ver = rest.trim().trim_matches('"');
            for n in names.drain(..) {
                out.entry(n).or_insert_with(|| ver.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readers() {
        assert_eq!(
            yarn_lock(
                "# yarn\n\"@a/b@^1.0.0\", \"@a/b@^1.1\":\n  version \"1.2.3\"\nleft-pad@npm:^1:\n  version: 1.3.0\n"
            ),
            Deps::from([
                ("@a/b".into(), "1.2.3".into()),
                ("left-pad".into(), "1.3.0".into())
            ])
        );
        assert_eq!(
            package_lock(
                r#"{"packages": {"": {}, "node_modules/next": {"version": "15.1.0"}, "node_modules/a/node_modules/b": {"version": "1.0.0"}}}"#
            ),
            Deps::from([("next".into(), "15.1.0".into())])
        );
        assert_eq!(
            pnpm_lock(
                "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      react:\n        specifier: ^19\n        version: 19.0.0(x@1)\n  packages/a:\n    devDependencies:\n      vitest:\n        specifier: ^3\n        version: 3.1.0\n"
            ),
            Deps::from([
                ("react".into(), "19.0.0".into()),
                ("vitest".into(), "3.1.0".into())
            ])
        );
        assert_eq!(
            toml_packages("[[package]]\nname = \"Django\"\nversion = \"5.1.2\"\n"),
            Deps::from([("django".into(), "5.1.2".into())])
        );
        assert_eq!(
            pipfile_lock(r#"{"default": {"requests": {"version": "==2.31.0"}}}"#),
            Deps::from([("requests".into(), "2.31.0".into())])
        );
    }
}
