//! What detection sees (`readProjectFacts` in `src/packs.ts`): the root and each workspace
//! package, each with its own dependencies (never merged: next@14 in one package and next@16
//! in another are two facts), files and package managers. A pack is detected when it is
//! detected in one of them.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use forge_packs::glob::{glob_match, glob_to_regex};
use forge_packs::model::{DepSpec, Ecosystem};
use regex::Regex;
use serde_json::Value;

use crate::deps::{
    self, Deps, norm_py, npm_deps, pipfile_deps, pyproject_deps, read_json, read_toml,
    requirements_deps, requirements_files,
};
use crate::locks;
use crate::pm;
use crate::versions::satisfies;

/// Files detection reads the contents of (content_any, yaml_keys) are at most this large.
const DETECT_READ_MAX: u64 = 1024 * 1024;

/// The facts of the root or of one workspace package.
#[derive(Debug)]
pub struct DepFacts {
    /// The package's directory, relative to the repository ("" for the root).
    pub dir: String,
    /// name → resolved or lowest declared version ("" when unknown).
    pub npm: Deps,
    pub pypi: Deps,
    /// Ecosystems the project publishes a library to.
    pub library: BTreeSet<Ecosystem>,
    /// Named facts for `when:` conditions: `python.pm`, `js.pm`.
    pub facts: BTreeMap<String, String>,
    /// Its own files, relative to `dir` (the root's: those outside every workspace package).
    pub files: Vec<String>,
    abs: PathBuf,
    memo: RefCell<HashMap<String, bool>>,
}

/// The root and the workspace packages.
#[derive(Debug)]
pub struct ProjectFacts {
    pub root: DepFacts,
    pub workspaces: Vec<DepFacts>,
}

/// The fact names `when: {facts: …}` may use.
pub const FACT_PYTHON_PM: &str = "python.pm";
pub const FACT_JS_PM: &str = "js.pm";

impl DepFacts {
    fn memo(&self, key: String, compute: impl FnOnce() -> bool) -> bool {
        if let Some(v) = self.memo.borrow().get(&key) {
            return *v;
        }
        let v = compute();
        self.memo.borrow_mut().insert(key, v);
        v
    }

    fn read(&self, rel: &str) -> String {
        let p = self.abs.join(rel);
        match std::fs::metadata(&p) {
            Ok(m) if m.len() <= DETECT_READ_MAX => std::fs::read_to_string(p).unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// A file of its own matches the glob.
    pub fn has_files(&self, glob: &str) -> bool {
        self.memo(format!("f\0{glob}"), || {
            let re = glob_to_regex(glob);
            self.files.iter().any(|f| re.is_match(f).unwrap_or(false))
        })
    }

    /// A file matching one of the globs has a line matching the pattern (detect.content_any).
    pub fn contains(&self, globs: &[String], pattern: &str) -> bool {
        self.memo(format!("c\0{}\0{pattern}", globs.join("\0")), || {
            let res: Vec<_> = globs.iter().map(|g| glob_to_regex(g)).collect();
            let Ok(line) = fancy_regex::Regex::new(&format!("(?m){pattern}")) else {
                return false;
            };
            self.files.iter().any(|f| {
                res.iter().any(|re| re.is_match(f).unwrap_or(false))
                    && line.is_match(&self.read(f)).unwrap_or(false)
            })
        })
    }

    /// A file matching one of the globs outside vendored, generated and environment directories (detect.sources_any).
    pub fn sources(&self, globs: &[String]) -> bool {
        self.memo(format!("s\0{}", globs.join("\0")), || {
            let res: Vec<_> = globs.iter().map(|g| glob_to_regex(g)).collect();
            self.files
                .iter()
                .any(|f| !is_vendored(f) && res.iter().any(|re| re.is_match(f).unwrap_or(false)))
        })
    }

    /// A .yaml / .yml file's first document is a mapping with all these top-level keys (detect.yaml_keys).
    pub fn yaml_keys(&self, keys: &[String]) -> bool {
        self.memo(format!("y\0{}", keys.join("\0")), || {
            self.files
                .iter()
                .filter(|f| f.ends_with(".yaml") || f.ends_with(".yml"))
                .any(|f| {
                    let top = first_document_keys(&self.read(f));
                    keys.iter().all(|k| top.contains(k))
                })
        })
    }

    /// Whether the dependency is declared here, in its range.
    pub fn dep_matches(&self, dep: &DepSpec) -> bool {
        let (map, name) = match dep.ecosystem {
            Ecosystem::Npm => (&self.npm, dep.name.clone()),
            Ecosystem::Pypi => (&self.pypi, norm_py(&dep.name)),
        };
        map.get(&name)
            .is_some_and(|v| satisfies(v, dep.version.as_deref()))
    }

    /// The fact's value here (`python.pm` → `uv`).
    pub fn fact(&self, name: &str) -> Option<&str> {
        self.facts.get(name).map(String::as_str)
    }

    /// The package's directory on disk.
    pub fn path(&self) -> &Path {
        &self.abs
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectError {
    pub file: String,
    pub message: String,
}

impl std::fmt::Display for DetectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.file, self.message)
    }
}

impl std::error::Error for DetectError {}

/// Reads the facts of `repo`, whose files (tracked and untracked, not ignored) are `files`.
pub fn read_project_facts(repo: &Path, files: &[String]) -> Result<ProjectFacts, DetectError> {
    let ws_dirs = workspace_dirs(repo, files);
    let mut own = files_by_package(files, &ws_dirs);
    let has_manage_py = |own: &[String]| {
        own.iter()
            .any(|f| f == "manage.py" || f.ends_with("/manage.py"))
    };
    let published = publish_signals(repo, files);
    let lock_npm = locks::npm_versions(repo);
    let lock_pypi = locks::pypi_versions(repo);
    let root_own = own.remove("").unwrap_or_default();
    let mut root = read_dir_facts(repo, repo, "", root_own, true, &published, has_manage_py)?;
    resolve(&mut root, &lock_npm, &lock_pypi);
    let mut workspaces = Vec::new();
    for dir in ws_dirs {
        let mine = own.remove(&dir).unwrap_or_default();
        let mut w = read_dir_facts(
            repo,
            &repo.join(&dir),
            &dir,
            mine,
            false,
            &published,
            has_manage_py,
        )?;
        resolve(&mut w, &lock_npm, &lock_pypi);
        workspaces.push(w);
    }
    Ok(ProjectFacts { root, workspaces })
}

fn resolve(d: &mut DepFacts, npm: &Deps, pypi: &Deps) {
    for (name, v) in d.npm.iter_mut() {
        if let Some(l) = npm.get(name) {
            *v = l.clone();
        }
    }
    for (name, v) in d.pypi.iter_mut() {
        if let Some(l) = pypi.get(name) {
            *v = l.clone();
        }
    }
}

/// The dependencies and package managers of one directory (`readDeps`). The root's
/// package.json and pyproject.toml must parse; a workspace package's may not.
fn read_dir_facts(
    repo: &Path,
    abs: &Path,
    dir: &str,
    own: Vec<String>,
    strict: bool,
    published: &Published,
    has_manage_py: impl Fn(&[String]) -> bool,
) -> Result<DepFacts, DetectError> {
    let err = |file: &str, message: String| DetectError {
        file: if dir.is_empty() {
            file.to_string()
        } else {
            format!("{dir}/{file}")
        },
        message,
    };
    let mut library = BTreeSet::new();
    let pkg_path = abs.join("package.json");
    let pkg: Option<Value> = if !pkg_path.exists() {
        None
    } else if strict {
        let text =
            std::fs::read_to_string(&pkg_path).map_err(|e| err("package.json", e.to_string()))?;
        Some(
            serde_json::from_str(&text)
                .map_err(|e| err("package.json", format!("not valid JSON: {e}")))?,
        )
    } else {
        read_json(&pkg_path)
    };
    let npm = pkg.as_ref().map(npm_deps).unwrap_or_default();
    if pkg
        .as_ref()
        .is_some_and(|p| is_npm_library(p, published.npm))
    {
        library.insert(Ecosystem::Npm);
    }
    let py_path = abs.join("pyproject.toml");
    let toml: Option<Value> = if !py_path.exists() {
        None
    } else if strict {
        let text =
            std::fs::read_to_string(&py_path).map_err(|e| err("pyproject.toml", e.to_string()))?;
        Some(
            deps::parse_toml(&text)
                .map_err(|e| err("pyproject.toml", format!("not valid TOML: {}", e.message())))?,
        )
    } else {
        read_toml(&py_path)
    };
    let mut pypi = toml.as_ref().map(pyproject_deps).unwrap_or_default();
    if toml
        .as_ref()
        .is_some_and(|t| is_py_library(t, has_manage_py(&own), published.pypi))
    {
        library.insert(Ecosystem::Pypi);
    }
    for f in requirements_files(&own) {
        let text = std::fs::read_to_string(abs.join(f)).unwrap_or_default();
        for (k, v) in requirements_deps(&text) {
            pypi.entry(k).or_insert(v);
        }
    }
    if let Some(pipfile) = read_toml(&abs.join("Pipfile")) {
        for (k, v) in pipfile_deps(&pipfile) {
            pypi.entry(k).or_insert(v);
        }
    }
    let mut facts = BTreeMap::new();
    facts.insert(
        FACT_PYTHON_PM.to_string(),
        pm::python_pm(abs, repo, toml.as_ref(), &own)
            .as_str()
            .to_string(),
    );
    facts.insert(
        FACT_JS_PM.to_string(),
        pm::js_pm(abs, repo, pkg.as_ref()).as_str().to_string(),
    );
    Ok(DepFacts {
        dir: dir.to_string(),
        npm,
        pypi,
        library,
        facts,
        files: own,
        abs: abs.to_path_buf(),
        memo: RefCell::new(HashMap::new()),
    })
}

/// A Python library (`isPyLibrary`): a named project with a build backend, no scripts, not
/// private, no manage.py, and evidence it is published (classifiers, or CI that uploads).
pub fn is_py_library(toml: &Value, has_manage_py: bool, published_by_ci: bool) -> bool {
    let Some(project) = toml.get("project").filter(|p| p.is_object()) else {
        return false;
    };
    if !project.get("name").is_some_and(Value::is_string) || toml.get("build-system").is_none() {
        return false;
    }
    if project.get("scripts").is_some() || project.get("gui-scripts").is_some() || has_manage_py {
        return false;
    }
    let classifiers: Vec<&str> = project
        .get("classifiers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if classifiers.iter().any(|c| c.starts_with("Private ::")) {
        return false;
    }
    published_by_ci || !classifiers.is_empty()
}

/// An npm library (`isNpmLibrary`): not private, named, exports an API, and is published.
pub fn is_npm_library(pkg: &Value, published_by_ci: bool) -> bool {
    if pkg.get("private") == Some(&Value::Bool(true))
        || !pkg.get("name").is_some_and(Value::is_string)
    {
        return false;
    }
    let has = |k: &str| pkg.get(k).is_some();
    let exports = has("exports")
        || ((has("main") || has("module")) && (has("types") || has("typings") || has("files")));
    exports && (published_by_ci || pkg.get("publishConfig").is_some_and(|v| !v.is_null()))
}

#[derive(Debug, Clone, Copy, Default)]
struct Published {
    pypi: bool,
    npm: bool,
}

/// Release set-ups that publish to PyPI or npm, found in CI workflows and Changesets.
fn publish_signals(repo: &Path, files: &[String]) -> Published {
    static CI: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\.github/workflows/[^/]+\.ya?ml$").expect("regex"));
    static PYPI: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"pypa/gh-action-pypi-publish|\b(uv|poetry|hatch|flit|pdm)\s+publish\b|\btwine\s+upload\b").expect("regex")
    });
    static NPM: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\b(npm|pnpm|bun)\s+publish\b|\byarn\s+(npm\s+)?publish\b|changesets/action|semantic-release|JS-DevTools/npm-publish").expect("regex")
    });
    let mut text = String::new();
    for f in files
        .iter()
        .filter(|f| CI.is_match(f) || *f == ".gitlab-ci.yml")
    {
        text.push_str(&std::fs::read_to_string(repo.join(f)).unwrap_or_default());
        text.push('\n');
    }
    Published {
        pypi: PYPI.is_match(&text),
        npm: files.iter().any(|f| f == ".changeset/config.json") || NPM.is_match(&text),
    }
}

/// The workspace packages of a monorepo, relative to the root (`workspaceDirs`):
/// pnpm-workspace.yaml `packages`, package.json `workspaces`, `[tool.uv.workspace] members`
/// and lerna.json, with their exclusions. A package is a directory with a package.json or
/// pyproject.toml.
pub fn workspace_dirs(repo: &Path, files: &[String]) -> Vec<String> {
    let mut include: Vec<String> = Vec::new();
    let mut exclude: Vec<String> = Vec::new();
    let clean = |g: &str| g.trim_start_matches("./").trim_end_matches('/').to_string();
    let mut add = |globs: Option<&Value>| {
        for g in globs
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            match g.strip_prefix('!') {
                Some(rest) => exclude.push(clean(rest)),
                None => include.push(clean(g)),
            }
        }
    };
    if let Ok(text) = std::fs::read_to_string(repo.join("pnpm-workspace.yaml"))
        && let Ok(p) = forge_packs::yaml::parse_strict(&text)
    {
        add(p.value.get("packages"));
    }
    if let Some(pkg) = read_json(&repo.join("package.json")) {
        match pkg.get("workspaces") {
            Some(w @ Value::Array(_)) => add(Some(w)),
            Some(w) => add(w.get("packages")),
            None => {}
        }
    }
    let uv = read_toml(&repo.join("pyproject.toml"))
        .and_then(|t| t.pointer("/tool/uv/workspace").cloned());
    if let Some(uv) = &uv {
        add(uv.get("members"));
    }
    if let Some(lerna) = read_json(&repo.join("lerna.json")) {
        match lerna.get("packages") {
            Some(p @ Value::Array(_)) => add(Some(p)),
            _ => add(Some(&Value::from(vec!["packages/*"]))),
        }
    }
    if let Some(ex) = uv
        .as_ref()
        .and_then(|u| u.get("exclude"))
        .and_then(Value::as_array)
    {
        exclude.extend(ex.iter().filter_map(Value::as_str).map(clean));
    }
    static MANIFEST: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(.+)/(?:package\.json|pyproject\.toml)$").expect("regex"));
    static NODE_MODULES: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(^|/)node_modules/").expect("regex"));
    static SAFE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[A-Za-z0-9._@+/-]+$").expect("regex"));
    let mut dirs = BTreeSet::new();
    for f in files {
        let Some(c) = MANIFEST.captures(f) else {
            continue;
        };
        let dir = &c[1];
        if NODE_MODULES.is_match(f) || !SAFE.is_match(dir) {
            continue;
        }
        let excluded = exclude
            .iter()
            .any(|g| glob_match(g, dir) || glob_match(g, &format!("{dir}/")));
        if include.iter().any(|g| glob_match(g, dir)) && !excluded {
            dirs.insert(dir.to_string());
        }
    }
    dirs.into_iter().collect()
}

/// Each file under the workspace packages holding it (nested packages: each of them),
/// relative to the package, and under "" the files outside every package.
fn files_by_package(files: &[String], dirs: &[String]) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    out.insert(String::new(), Vec::new());
    for d in dirs {
        out.insert(d.clone(), Vec::new());
    }
    let set: BTreeSet<&str> = dirs.iter().map(String::as_str).collect();
    for f in files {
        let mut in_package = false;
        for (i, _) in f.match_indices('/') {
            if i == 0 {
                continue;
            }
            let d = &f[..i];
            if set.contains(d) {
                out.get_mut(d).expect("dir").push(f[i + 1..].to_string());
                in_package = true;
            }
        }
        if !in_package {
            out.get_mut("").expect("root").push(f.clone());
        }
    }
    out
}

/// Directories that hold someone else's code, generated code or an environment.
const VENDORED_DIRS: &[&str] = &[
    "node_modules",
    ".venv",
    "venv",
    ".env",
    "env",
    "virtualenv",
    "site-packages",
    "dist-packages",
    "vendor",
    "vendored",
    "_vendor",
    "third_party",
    "third-party",
    "build",
    "dist",
    ".tox",
    ".nox",
    ".eggs",
    "__pycache__",
    ".mypy_cache",
    ".git",
];

/// A path inside a vendored, generated or environment directory, `.forge/` or `.claude/`, or
/// a generated module (protobuf's `*_pb2.py`).
pub fn is_vendored(path: &str) -> bool {
    static PB2: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"_pb2(_grpc)?\.pyi?$").expect("regex"));
    let mut parts: Vec<&str> = path.split('/').collect();
    let name = parts.pop().unwrap_or("");
    if PB2.is_match(name) {
        return true;
    }
    if parts
        .first()
        .is_some_and(|p| *p == ".forge" || *p == ".claude")
    {
        return true;
    }
    parts
        .iter()
        .any(|d| VENDORED_DIRS.contains(d) || d.ends_with(".egg-info"))
}

/// The top-level keys of a YAML text's first document, when it is a mapping (none when it
/// does not parse: a template that only renders to YAML is not YAML).
pub fn first_document_keys(text: &str) -> BTreeSet<String> {
    use saphyr_parser::{Event, Parser};
    let mut keys = BTreeSet::new();
    let mut depth = 0usize;
    let mut expect_key = false;
    let mut is_map = false;
    let mut docs = 0;
    for item in Parser::new_from_str(text) {
        let Ok((event, _)) = item else {
            return BTreeSet::new();
        };
        match event {
            Event::DocumentStart(_) => {
                docs += 1;
                if docs > 1 {
                    break;
                }
            }
            Event::DocumentEnd => break,
            Event::MappingStart(..) => {
                if depth == 0 {
                    is_map = true;
                    expect_key = true;
                } else if depth == 1 {
                    expect_key = !expect_key;
                }
                depth += 1;
            }
            Event::SequenceStart(..) => {
                if depth == 1 {
                    expect_key = !expect_key;
                }
                depth += 1;
            }
            Event::MappingEnd | Event::SequenceEnd => depth = depth.saturating_sub(1),
            Event::Scalar(v, ..) => {
                if depth == 1 && is_map {
                    if expect_key && !v.is_empty() {
                        keys.insert(v.to_string());
                    }
                    expect_key = !expect_key;
                }
            }
            Event::Alias(_) => {
                if depth == 1 {
                    expect_key = !expect_key;
                }
            }
            _ => {}
        }
    }
    if is_map { keys } else { BTreeSet::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_top_keys() {
        let k = first_document_keys(
            "# c\napiVersion: v1\nkind: Pod\nspec:\n  a: &x 1\n  b: *x\nlist:\n  - 1\n---\nother: 1\n",
        );
        assert_eq!(
            k.into_iter().collect::<Vec<_>>(),
            ["apiVersion", "kind", "list", "spec"]
        );
        assert!(first_document_keys("- a\n- b\n").is_empty());
        assert!(first_document_keys("a: [b\n").is_empty());
    }

    #[test]
    fn vendored() {
        assert!(is_vendored("node_modules/a.py"));
        assert!(is_vendored("a/b_pb2.py"));
        assert!(is_vendored(".forge/x.py"));
        assert!(!is_vendored("src/app.py"));
        assert!(is_vendored("pkg.egg-info/x.py"));
    }
}
