//! Python imports a project declares but the environment Forge type-checks with lacks (§10.4).
//!
//! ty reports `unresolved-import` for a module it cannot find. When the module comes from a
//! dependency the project declares (pyproject.toml, requirements files, Pipfile, Poetry's
//! tables), the code is right and the environment is not set up: that is the person's to fix,
//! a note, never a finding. An import of a package the project does not declare stays a finding.
//!
//! A distribution's import names come from its installed metadata when the project's
//! environment has it (`top_level.txt`, else the top-level entries of `RECORD`), else from its
//! name: lower case with `-` and `.` as `_`, plus a table of the common names that differ.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use forge_core::{Note, NoteKind};
use forge_detect::deps::{
    norm_py, pipfile_deps, pyproject_deps, read_toml, requirements_deps, requirements_files,
};
use regex::Regex;

/// Distributions whose import name is not their normalised name.
const IMPORT_NAMES: &[(&str, &[&str])] = &[
    ("beautifulsoup4", &["bs4"]),
    ("pyyaml", &["yaml"]),
    ("pillow", &["PIL"]),
    ("scikit-learn", &["sklearn"]),
    ("scikit-image", &["skimage"]),
    ("python-dateutil", &["dateutil"]),
    ("opencv-python", &["cv2"]),
    ("opencv-python-headless", &["cv2"]),
    ("opencv-contrib-python", &["cv2"]),
    ("python-dotenv", &["dotenv"]),
    ("pyjwt", &["jwt"]),
    ("psycopg2-binary", &["psycopg2"]),
    ("protobuf", &["google"]),
    ("attrs", &["attr", "attrs"]),
    ("python-multipart", &["multipart", "python_multipart"]),
    ("pymupdf", &["fitz", "pymupdf"]),
    ("pycryptodome", &["Crypto"]),
];

/// The import names the project's declared Python dependencies provide.
#[derive(Debug, Default, Clone)]
pub struct Declared {
    /// Import name (top-level module) → the distribution that provides it.
    by_module: BTreeMap<String, String>,
}

impl Declared {
    /// What the project at `root` declares, mapped through its environment's metadata.
    pub fn of(root: &Path) -> Declared {
        let dists = declared(root);
        if dists.is_empty() {
            return Declared::default();
        }
        let installed = installed_names(root);
        let mut by_module = BTreeMap::new();
        for d in dists {
            let names: Vec<String> = match installed.get(&d) {
                Some(n) if !n.is_empty() => n.iter().cloned().collect(),
                _ => heuristic(&d),
            };
            for n in names {
                by_module.entry(n).or_insert_with(|| d.clone());
            }
        }
        Declared { by_module }
    }

    /// The declared distribution providing `module` (`bs4.element` → `beautifulsoup4`).
    pub fn provider(&self, module: &str) -> Option<&str> {
        let top = module.split('.').next().unwrap_or(module);
        if top.is_empty() {
            return None;
        }
        self.by_module
            .get(top)
            .or_else(|| {
                // the heuristic names are lower case; a module's own case may differ
                let lower = top.to_lowercase();
                self.by_module.get(&lower)
            })
            .map(String::as_str)
    }

    /// `output` (a type checker's diagnostics, one per line) without the unresolved imports of
    /// declared dependencies, and those dependencies (`dist (module)`, each once).
    pub fn split(&self, output: &str) -> (String, Vec<String>) {
        let mut kept = String::new();
        let mut missing: Vec<String> = Vec::new();
        for line in output.lines() {
            if let Some(m) = unresolved_module(line)
                && let Some(d) = self.provider(m)
            {
                let top = m.split('.').next().unwrap_or(m);
                let item = if top == d {
                    d.to_string()
                } else {
                    format!("{d} ({top})")
                };
                if !missing.contains(&item) {
                    missing.push(item);
                }
                continue;
            }
            kept.push_str(line);
            kept.push('\n');
        }
        (kept, missing)
    }
}

/// The note for dependencies the environment lacks.
pub fn missing_note(missing: &[String], rule: &str) -> Note {
    let n = Note::new(
        NoteKind::Environment,
        format!(
            "dependencies are not installed in the environment Forge type-checks with: {}; their imports are not checked until they are",
            missing.join(", ")
        ),
    );
    if rule.is_empty() { n } else { n.for_rule(rule) }
}

/// The module an `unresolved-import` diagnostic names (ty's text: ``error[unresolved-import]
/// Cannot resolve imported module `bs4` ``). Relative imports are never a dependency's.
pub fn unresolved_module(line: &str) -> Option<&str> {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\[unresolved-import\] Cannot resolve imported module `([^`]+)`")
            .expect("regex")
    });
    let m = RE.captures(line)?.get(1)?.as_str();
    (!m.starts_with('.')).then_some(m)
}

/// The distributions `root` declares, PEP 503-normalised.
fn declared(root: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if let Some(t) = read_toml(&root.join("pyproject.toml")) {
        out.extend(pyproject_deps(&t).into_keys());
    }
    if let Some(t) = read_toml(&root.join("Pipfile")) {
        out.extend(pipfile_deps(&t).into_keys());
    }
    let mut own: Vec<String> = Vec::new();
    for (dir, prefix) in [
        (root.to_path_buf(), ""),
        (root.join("requirements"), "requirements/"),
    ] {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                own.push(format!("{prefix}{}", e.file_name().to_string_lossy()));
            }
        }
    }
    for f in requirements_files(&own) {
        let text = std::fs::read_to_string(root.join(f)).unwrap_or_default();
        out.extend(requirements_deps(&text).into_keys());
    }
    out
}

/// A distribution's import names by its name alone.
fn heuristic(dist: &str) -> Vec<String> {
    if let Some((_, names)) = IMPORT_NAMES.iter().find(|(d, _)| *d == dist) {
        return names.iter().map(|n| n.to_string()).collect();
    }
    vec![dist.to_lowercase().replace(['-', '.'], "_")]
}

/// The site-packages directories of the project's environment (`VIRTUAL_ENV`, else `.venv`).
fn site_packages(root: &Path) -> Vec<PathBuf> {
    let mut prefixes: Vec<PathBuf> = Vec::new();
    if let Some(v) = std::env::var_os("VIRTUAL_ENV").filter(|v| !v.is_empty()) {
        prefixes.push(PathBuf::from(v));
    }
    prefixes.push(root.join(".venv"));
    let mut out = Vec::new();
    for p in prefixes {
        for lib in ["lib", "lib64"] {
            if let Ok(rd) = std::fs::read_dir(p.join(lib)) {
                for e in rd.flatten() {
                    let site = e.path().join("site-packages");
                    if e.file_name().to_string_lossy().starts_with("python") && site.is_dir() {
                        out.push(site);
                    }
                }
            }
        }
        let win = p.join("Lib/site-packages");
        if win.is_dir() {
            out.push(win);
        }
    }
    out
}

/// Installed distributions (normalised) → their top-level import names, from their metadata.
fn installed_names(root: &Path) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for site in site_packages(root) {
        let Ok(rd) = std::fs::read_dir(&site) else {
            continue;
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(stem) = name.strip_suffix(".dist-info") else {
                continue;
            };
            let dist = norm_py(stem.split('-').next().unwrap_or(stem));
            let names = out.entry(dist).or_default();
            if let Ok(t) = std::fs::read_to_string(e.path().join("top_level.txt")) {
                names.extend(
                    t.lines()
                        .map(str::trim)
                        .filter(|l| !l.is_empty())
                        .map(str::to_owned),
                );
                continue;
            }
            let record = std::fs::read_to_string(e.path().join("RECORD")).unwrap_or_default();
            for line in record.lines() {
                let path = line.split(',').next().unwrap_or("");
                let first = path.split('/').next().unwrap_or("");
                if first.is_empty()
                    || first == ".."
                    || first == "__pycache__"
                    || first.ends_with(".dist-info")
                    || first.ends_with(".data")
                    || first.ends_with(".pth")
                {
                    continue;
                }
                let module = if path.contains('/') {
                    first
                } else if let Some(m) = first.strip_suffix(".py") {
                    m
                } else if first.ends_with(".so") || first.ends_with(".pyd") {
                    first.split('.').next().unwrap_or(first)
                } else {
                    continue;
                };
                names.insert(module.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, path: &str, text: &str) {
        let p = root.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn declared_modules_by_name_and_table() {
        let d = tempfile::tempdir().unwrap();
        write(
            d.path(),
            "requirements.txt",
            "beautifulsoup4==4.12.3\nPyYAML>=6\nrequests\n",
        );
        write(
            d.path(),
            "pyproject.toml",
            "[tool.poetry.dependencies]\npython = \"^3.12\"\nscikit-learn = \"^1.5\"\n",
        );
        let x = Declared::of(d.path());
        assert_eq!(x.provider("bs4"), Some("beautifulsoup4"));
        assert_eq!(x.provider("bs4.element"), Some("beautifulsoup4"));
        assert_eq!(x.provider("yaml"), Some("pyyaml"));
        assert_eq!(x.provider("requests"), Some("requests"));
        assert_eq!(x.provider("sklearn"), Some("scikit-learn"));
        assert_eq!(x.provider("numpy"), None);
    }

    #[test]
    fn installed_metadata_names_the_modules() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "requirements.txt", "Odd-Dist==1.0\n");
        write(
            d.path(),
            ".venv/lib/python3.12/site-packages/odd_dist-1.0.dist-info/RECORD",
            "oddmod/__init__.py,sha256=x,1\nodd_dist-1.0.dist-info/METADATA,,\n",
        );
        let x = Declared::of(d.path());
        assert_eq!(x.provider("oddmod"), Some("odd-dist"));
        assert_eq!(x.provider("odd_dist"), None);
    }

    #[test]
    fn split_keeps_undeclared_imports_and_other_errors() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "requirements.txt", "beautifulsoup4\n");
        let out = "app.py:2:6: error[unresolved-import] Cannot resolve imported module `bs4`\n\
                   app.py:3:8: error[unresolved-import] Cannot resolve imported module `numpy`\n\
                   app.py:4:1: error[invalid-assignment] Object of type `str` is not assignable\n\
                   app.py:5:1: error[unresolved-import] Cannot resolve imported module `.local`\n";
        let (kept, missing) = Declared::of(d.path()).split(out);
        assert_eq!(missing, ["beautifulsoup4 (bs4)"]);
        assert!(!kept.contains("`bs4`"), "{kept}");
        assert!(
            kept.contains("`numpy`") && kept.contains("invalid-assignment"),
            "{kept}"
        );
        assert!(kept.contains("`.local`"), "{kept}");
    }
}
