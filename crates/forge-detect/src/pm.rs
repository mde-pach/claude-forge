//! The package managers a directory uses, as facts for `when:` conditions (`python.pm`,
//! `js.pm`). They follow the project's own files, so the rules text they select is true on
//! every machine.
//!
//! Python (`python.pm`), first match wins:
//!
//! | Files                                                                  | Fact     |
//! |------------------------------------------------------------------------|----------|
//! | `uv.lock`                                                              | `uv`     |
//! | `poetry.lock`                                                          | `poetry` |
//! | `pdm.lock`                                                             | `pdm`    |
//! | `Pipfile.lock` or `Pipfile`                                            | `pipenv` |
//! | `pyproject.toml` whose `[tool.poetry]` declares dependencies          | `poetry` |
//! | `pyproject.toml` with a `[project]` table or dependency groups, no lock | `uv`     |
//! | `requirements*.txt` (no pyproject, or one holding only tool config)    | `pip`    |
//! | `setup.py` / `setup.cfg` (setuptools without pyproject metadata)       | `pip`    |
//! | none of these (plain scripts, or no Python)                            | `none`   |
//!
//! A workspace package also sees the locks at the repository root (a uv or Poetry workspace
//! has one lock).
//!
//! JavaScript (`js.pm`): the pinned manager (`packageManager`, `devEngines.packageManager`,
//! the package's then the root's), else the lock file (`bun.lock`/`bun.lockb` → bun,
//! `pnpm-lock.yaml` → pnpm, `yarn.lock` → yarn, `package-lock.json`/`npm-shrinkwrap.json` →
//! npm, the package's directory then the root), else `npm` when there is a package.json, else `none`.

use std::path::Path;

use serde_json::Value;

use crate::deps::{package_manager, poetry_declares_deps, read_json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PythonPm {
    Uv,
    Poetry,
    Pdm,
    Pipenv,
    Pip,
    None,
}

impl PythonPm {
    pub fn as_str(self) -> &'static str {
        match self {
            PythonPm::Uv => "uv",
            PythonPm::Poetry => "poetry",
            PythonPm::Pdm => "pdm",
            PythonPm::Pipenv => "pipenv",
            PythonPm::Pip => "pip",
            PythonPm::None => "none",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum JsPm {
    Npm,
    Pnpm,
    Yarn,
    Bun,
    None,
}

impl JsPm {
    pub fn as_str(self) -> &'static str {
        match self {
            JsPm::Npm => "npm",
            JsPm::Pnpm => "pnpm",
            JsPm::Yarn => "yarn",
            JsPm::Bun => "bun",
            JsPm::None => "none",
        }
    }

    fn from_name(n: &str) -> Option<JsPm> {
        Some(match n {
            "npm" => JsPm::Npm,
            "pnpm" => JsPm::Pnpm,
            "yarn" => JsPm::Yarn,
            "bun" => JsPm::Bun,
            _ => return None,
        })
    }
}

/// `dir`'s Python package manager. `pyproject` is its parsed pyproject.toml; `own` its files
/// (relative to `dir`).
pub fn python_pm(dir: &Path, repo: &Path, pyproject: Option<&Value>, own: &[String]) -> PythonPm {
    let here_or_root = |f: &str| dir.join(f).is_file() || repo.join(f).is_file();
    if here_or_root("uv.lock") {
        return PythonPm::Uv;
    }
    if here_or_root("poetry.lock") {
        return PythonPm::Poetry;
    }
    if here_or_root("pdm.lock") {
        return PythonPm::Pdm;
    }
    if dir.join("Pipfile.lock").is_file() || dir.join("Pipfile").is_file() {
        return PythonPm::Pipenv;
    }
    if let Some(t) = pyproject {
        if poetry_declares_deps(t) {
            return PythonPm::Poetry;
        }
        if t.get("project").is_some_and(Value::is_object) || t.get("dependency-groups").is_some() {
            return PythonPm::Uv;
        }
    }
    if crate::deps::requirements_files(own).next().is_some() {
        return PythonPm::Pip;
    }
    if dir.join("setup.py").is_file() || dir.join("setup.cfg").is_file() {
        return PythonPm::Pip;
    }
    PythonPm::None
}

/// `dir`'s JavaScript package manager. `pkg` is its parsed package.json.
pub fn js_pm(dir: &Path, repo: &Path, pkg: Option<&Value>) -> JsPm {
    let root_pkg = if dir == repo {
        None
    } else {
        read_json(&repo.join("package.json"))
    };
    for p in [pkg, root_pkg.as_ref()].into_iter().flatten() {
        if let Some(pm) = package_manager(p).and_then(|(n, _)| JsPm::from_name(&n)) {
            return pm;
        }
    }
    for base in [dir, repo] {
        let has = |f: &str| base.join(f).is_file();
        if has("bun.lock") || has("bun.lockb") {
            return JsPm::Bun;
        }
        if has("pnpm-lock.yaml") {
            return JsPm::Pnpm;
        }
        if has("yarn.lock") {
            return JsPm::Yarn;
        }
        if has("package-lock.json") || has("npm-shrinkwrap.json") {
            return JsPm::Npm;
        }
    }
    if pkg.is_some() { JsPm::Npm } else { JsPm::None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deps::parse_toml;

    fn case(files: &[(&str, &str)]) -> PythonPm {
        let t = tempfile::tempdir().unwrap();
        let mut own = Vec::new();
        for (f, text) in files {
            let p = t.path().join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
            own.push(f.to_string());
        }
        let py = files
            .iter()
            .find(|(f, _)| *f == "pyproject.toml")
            .map(|(_, t)| parse_toml(t).unwrap());
        python_pm(t.path(), t.path(), py.as_ref(), &own)
    }

    /// Every case of the design's "Python packaging" decision.
    #[test]
    fn python_pm_table() {
        let project = "[project]\nname = \"x\"\ndependencies = [\"httpx\"]\n";
        let poetry = "[tool.poetry]\nname = \"x\"\n[tool.poetry.dependencies]\npython = \"^3.12\"\ndjango = \"^5\"\n";
        let tool_only = "[tool.ruff]\nline-length = 100\n";
        for (files, want) in [
            (
                vec![("uv.lock", ""), ("pyproject.toml", project)],
                PythonPm::Uv,
            ),
            (
                vec![("poetry.lock", ""), ("pyproject.toml", poetry)],
                PythonPm::Poetry,
            ),
            (vec![("pyproject.toml", poetry)], PythonPm::Poetry),
            (
                vec![("pdm.lock", ""), ("pyproject.toml", project)],
                PythonPm::Pdm,
            ),
            (
                vec![("Pipfile.lock", "{}"), ("Pipfile", "")],
                PythonPm::Pipenv,
            ),
            (
                vec![("Pipfile", "[packages]\nrequests = \"*\"\n")],
                PythonPm::Pipenv,
            ),
            (vec![("pyproject.toml", project)], PythonPm::Uv),
            (vec![("requirements.txt", "flask\n")], PythonPm::Pip),
            (
                vec![
                    ("requirements-dev.txt", "pytest\n"),
                    ("pyproject.toml", tool_only),
                ],
                PythonPm::Pip,
            ),
            (vec![("requirements/base.txt", "flask\n")], PythonPm::Pip),
            (vec![("pyproject.toml", tool_only)], PythonPm::None),
            (vec![("main.py", "print(1)\n")], PythonPm::None),
            (vec![], PythonPm::None),
            (vec![("setup.py", "")], PythonPm::Pip),
        ] {
            assert_eq!(case(&files), want, "{files:?}");
        }
    }

    #[test]
    fn js_pm_cases() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        assert_eq!(js_pm(d, d, None), JsPm::None);
        let pkg = serde_json::json!({"name": "x"});
        assert_eq!(js_pm(d, d, Some(&pkg)), JsPm::Npm);
        std::fs::write(d.join("pnpm-lock.yaml"), "").unwrap();
        assert_eq!(js_pm(d, d, Some(&pkg)), JsPm::Pnpm);
        let pinned = serde_json::json!({"packageManager": "yarn@4.1.0"});
        assert_eq!(js_pm(d, d, Some(&pinned)), JsPm::Yarn);
        std::fs::write(d.join("bun.lock"), "").unwrap();
        assert_eq!(js_pm(d, d, Some(&pkg)), JsPm::Bun);
    }
}
