//! Python dead code (`forge_condor`, rule `python/no-dead-code`): Forge's pinned condor on the
//! tree as it is and on a copy of it as it was at the base, and only what the change left
//! unused: the findings that are new, wherever they are (a function whose last caller the
//! change removed is in a file it did not touch).
//!
//! - A finding is its file, kind and name (not its line, which edits above it move). Of the
//!   findings with the same identity, as many count as there are more of them now than in the
//!   base, those on added lines first. A finding whose file the change deleted or renamed is
//!   matched by kind and name alone, so moved code does not bring its dead code along.
//! - With `--all` (CI's full run) every finding counts.
//! - What fails is decided by kind and name, not by condor's confidence (0.2.0 and 0.3.0 give every
//!   function, class and variable 60): an unused import, unreachable code, an unused private
//!   name (a leading `_`: function, method, class, variable, attribute) and, in an
//!   application, an unused top-level function or class. The rest (public methods, which may
//!   override a framework's, classes nested in a class like `class Meta`, public variables,
//!   attributes and properties) is advice at Stop, given once per turn, and
//!   neither the commit-time check nor CI reports it. The project's `min_confidence` still
//!   decides what condor reports at all.
//! - Unused imports are left to ruff's F401 when the run also runs `python/lint` and ruff can
//!   lint (one finding, not two); otherwise condor's count.
//! - A Python library (the `python/library` facts: a published package) is used from outside:
//!   only its private names and unused imports count.
//! - The project's own `[tool.condor]` or `[tool.vulture]` applies: condor reads it from
//!   `pyproject.toml` (paths, whitelists given as paths, `ignore_names`, `min_confidence`,
//!   `no_report`). Forge's excludes (environments, `node_modules`, nested worktrees) are added
//!   to the project's `exclude`, since condor's `--exclude` replaces it; vendored and generated
//!   code is scanned for uses but its findings dropped, as are those outside the rule's scope
//!   (migrations, stubs, configuration a tool calls by name) and pytest's hooks (`pytest_*`
//!   functions).
//! - condor that cannot run, rejects its options, prints what is not its JSON, or meets a file
//!   it cannot parse (in the tree but not in the base, or the reverse) is a note for the
//!   person; the check then passes.

use std::collections::BTreeMap;
use std::path::Path;

use forge_check::builtins::{RunMode, ToolRun, not_reported};
use forge_core::{Finding, NoteKind, Report};
use forge_packs::model::Ecosystem;

use crate::Run;
use crate::util::{last_line, re};

/// The rule that lints with ruff, whose F401 reports unused imports.
pub const RUFF_LINT_RULE: &str = "python/lint";

/// What the agent reads after a finding of code left unused.
pub const UNUSED_HINT: &str = "remove it, or use it where it is needed. If it is used dynamically (a framework hook, an entry point), ask the user.";

/// The kinds of condor (0.2.0, 0.3.0) that are code; `whitelist_entry` (a whitelist line naming
/// nothing) is the project's own file.
const KINDS: &[&str] = &[
    "function",
    "class",
    "method",
    "property",
    "attribute",
    "variable",
    "import",
    "unreachable_code",
];

/// Directories condor does not scan at all: environments, dependencies, nested worktrees.
const EXCLUDED_DIRS: &[&str] = &[
    ".git",
    ".venv",
    "venv",
    ".tox",
    ".nox",
    "node_modules",
    "site-packages",
    ".claude/worktrees",
    ".forge",
];

/// One condor finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub path: String,
    pub line: u32,
    pub kind: String,
    pub name: String,
    pub message: String,
    pub confidence: u32,
    /// A class defined in a class's body (`class Meta`, `class Config`), read from the source.
    pub nested: bool,
}

impl Hit {
    /// A private name: a leading `_`, not a dunder.
    fn private(&self) -> bool {
        let last = self.name.rsplit('.').next().unwrap_or(&self.name);
        last.starts_with('_') && !(last.starts_with("__") && last.ends_with("__"))
    }

    /// Whether the finding fails the check (module docs); `library`: the file is in one.
    pub fn blocks(&self, library: bool) -> bool {
        match self.kind.as_str() {
            "import" | "unreachable_code" => true,
            "function" => !library || self.private(),
            // a framework calls the methods its base classes declare (`get_queryset`, `setUp`)
            // and reads the classes nested in a class (`class Meta`): condor cannot see it
            "class" => (!library && !self.nested) || self.private(),
            "method" | "variable" | "attribute" => self.private(),
            _ => false,
        }
    }

    fn key(&self) -> (&str, &str, &str) {
        (&self.path, &self.kind, &self.name)
    }

    /// What the agent reads (without the location).
    pub fn text(&self) -> String {
        if self.kind == "unreachable_code" {
            // "unreachable code after 'return'", "unsatisfiable 'while' condition"
            let msg = re(r"'([^']*)'").replace_all(&self.message, "`$1`");
            return format!("{msg}: remove it.");
        }
        format!(
            "{} `{}` is unused: {UNUSED_HINT}",
            self.kind.replace('_', " "),
            self.name
        )
    }
}

/// One scan: the findings, and the files condor could not parse.
#[derive(Debug, Default)]
pub struct Scan {
    pub hits: Vec<Hit>,
    pub unparsed: Vec<String>,
}

/// The project's condor config that Forge must know: whether it names the paths to scan, and
/// its excludes (which `--exclude` replaces). `[tool.condor]` first, then `[tool.vulture]`.
#[derive(Debug, Default)]
struct Config {
    has_paths: bool,
    exclude: Vec<String>,
}

impl Config {
    fn read(root: &Path) -> Config {
        let text = std::fs::read_to_string(root.join("pyproject.toml")).unwrap_or_default();
        let Ok(doc) = text.parse::<toml::Table>() else {
            return Config::default();
        };
        let tool = doc.get("tool").and_then(|t| t.as_table());
        let tables: Vec<&toml::Table> = ["condor", "vulture"]
            .iter()
            .filter_map(|n| tool.and_then(|t| t.get(*n)).and_then(|t| t.as_table()))
            .collect();
        let key = |k: &str| tables.iter().find_map(|t| t.get(k));
        let list = |v: Option<&toml::Value>| -> Vec<String> {
            match v {
                Some(toml::Value::Array(a)) => a
                    .iter()
                    .filter_map(|x| x.as_str().map(str::to_owned))
                    .collect(),
                Some(toml::Value::String(s)) => vec![s.clone()],
                _ => Vec::new(),
            }
        };
        Config {
            has_paths: !list(key("paths")).is_empty(),
            exclude: list(key("exclude")),
        }
    }

    /// condor's arguments for the tree at `root` (its absolute path).
    fn args(&self, root: &Path) -> Vec<String> {
        let r = root.display().to_string();
        let mut exclude = self.exclude.clone();
        for d in EXCLUDED_DIRS {
            // patterns are matched against absolute paths: anchored at the tree, so a
            // repository that lives under a directory named like one is still scanned
            exclude.push(format!("{r}/{d}/*"));
            exclude.push(format!("{r}/*/{d}/*"));
        }
        let mut a = vec!["--exclude".to_owned(), exclude.join(",")];
        if !self.has_paths {
            a.push(".".into());
        }
        a
    }
}

/// condor's JSON (an array of findings) as hits, paths relative to `root`.
pub fn parse_json(text: &str, root: &Path) -> Result<Vec<Hit>, String> {
    let v: serde_json::Value = serde_json::from_str(text.trim())
        .map_err(|e| format!("its output is not the JSON it prints ({e})"))?;
    let arr = v.as_array().ok_or("its output is not a list of findings")?;
    let roots = [
        root.display().to_string(),
        std::fs::canonicalize(root)
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
    ];
    let mut out = Vec::new();
    for f in arr {
        let s = |k: &str| f.get(k).and_then(|x| x.as_str()).unwrap_or("").to_owned();
        let n = |k: &str| f.get(k).and_then(|x| x.as_u64()).unwrap_or(0) as u32;
        let mut path = s("path");
        for r in roots.iter().filter(|r| !r.is_empty()) {
            if let Some(rest) = path.strip_prefix(&format!("{r}/")) {
                path = rest.to_owned();
            }
        }
        while let Some(rest) = path.strip_prefix("./") {
            path = rest.to_owned();
        }
        out.push(Hit {
            path,
            line: n("line"),
            kind: s("kind"),
            name: s("name"),
            message: s("message"),
            confidence: n("confidence"),
            nested: false,
        });
    }
    Ok(out)
}

/// The files a run says it could not parse (`path:line: message` on stderr).
pub fn unparsed(stderr: &str, root: &Path) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in stderr.lines() {
        if let Some(c) = re(r"^(.+?\.pyi?):[0-9]+: ").captures(l) {
            let p = c[1].to_owned();
            let p = p
                .strip_prefix(&format!("{}/", root.display()))
                .map(str::to_owned)
                .unwrap_or(p);
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// condor on the tree at `root`; `Err`: why it could not tell (for the person).
fn scan(run: &Run<'_>, root: &Path) -> Result<Scan, (NoteKind, String)> {
    let args = Config::read(root).args(root);
    match run.opts.tools.run(root, root, "condor", "dead-code", &args) {
        ToolRun::Missing(why) => Err((NoteKind::MissingTool, format!("skipped: {why}"))),
        ToolRun::Unsupported(why) => Err((NoteKind::NotEnforced, why)),
        ToolRun::Ran(o) => {
            let unparsed = unparsed(&o.stderr, root);
            // 0 nothing, 3 findings; 1 with JSON: only files it could not parse
            let json_ok = matches!(o.status, 0 | 3) || (o.status == 1 && !unparsed.is_empty());
            if !json_ok {
                let why = if o.stderr.trim().is_empty() {
                    last_line(&o.stdout)
                } else {
                    last_line(&o.stderr)
                };
                return Err((
                    NoteKind::ForgeFailure,
                    format!(
                        "not checked: condor could not run (exit {}: {why})",
                        o.status
                    ),
                ));
            }
            let hits = parse_json(&o.stdout, root).map_err(|why| {
                (
                    NoteKind::ForgeFailure,
                    format!("not checked: condor's output could not be read: {why}"),
                )
            })?;
            Ok(Scan { hits, unparsed })
        }
    }
}

/// The findings of `now` the base does not have (module docs).
pub fn new_hits(
    now: &[Hit],
    base: &[Hit],
    on_added: &dyn Fn(&Hit) -> bool,
    gone: &dyn Fn(&str) -> bool,
) -> Vec<Hit> {
    let mut base_n: BTreeMap<(&str, &str, &str), usize> = BTreeMap::new();
    for b in base {
        *base_n.entry(b.key()).or_default() += 1;
    }
    let mut groups: BTreeMap<(&str, &str, &str), Vec<&Hit>> = BTreeMap::new();
    for h in now {
        groups.entry(h.key()).or_default().push(h);
    }
    let mut fresh: Vec<&Hit> = Vec::new();
    for (key, mut hs) in groups {
        let had = base_n.get(&key).copied().unwrap_or(0);
        if hs.len() <= had {
            if let Some(n) = base_n.get_mut(&key) {
                *n -= hs.len();
            }
            continue;
        }
        base_n.remove(&key);
        // the ones on added lines are the new ones
        hs.sort_by_key(|h| (!on_added(h), h.line));
        let n = hs.len() - had;
        fresh.extend(hs.into_iter().take(n));
    }
    // moved code: what is left of the base in files the change deleted or renamed
    let mut moved: BTreeMap<(String, String), usize> = BTreeMap::new();
    for ((path, kind, name), n) in &base_n {
        if *n > 0 && gone(path) {
            *moved
                .entry(((*kind).to_owned(), (*name).to_owned()))
                .or_default() += n;
        }
    }
    let mut out = Vec::new();
    for h in fresh {
        if let Some(n) = moved
            .get_mut(&(h.kind.clone(), h.name.clone()))
            .filter(|n| **n > 0)
        {
            *n -= 1;
            continue;
        }
        out.push(h.clone());
    }
    out.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
    out
}

/// Whether the definition at `line` (1-based) of `source` is in a class's body: the first
/// block above it with less indentation is a `class` (a `def` in between: it is not).
pub fn in_class_body(source: &str, line: u32) -> bool {
    let lines: Vec<&str> = source.lines().collect();
    let Some(at) = lines.get((line as usize).wrapping_sub(1)) else {
        return false;
    };
    let indent = |l: &str| l.len() - l.trim_start().len();
    let mut level = indent(at);
    for l in lines[..line as usize - 1].iter().rev() {
        let t = l.trim_start();
        if level == 0 {
            return false;
        }
        if t.is_empty() || t.starts_with('#') || indent(l) >= level {
            continue;
        }
        if t.starts_with("class ") || t.starts_with("class\t") {
            return true;
        }
        if t.starts_with("def ") || t.starts_with("async def ") {
            return false;
        }
        // `if`, `try`, `with`…: look further up
        level = indent(l);
    }
    false
}

/// The workspace directories ("" for the root) that are Python libraries.
fn library_dirs(run: &Run<'_>) -> Vec<String> {
    let files = run.ls_files(&["-co", "--exclude-standard"]);
    let Ok(facts) = forge_detect::facts::read_project_facts(run.root(), &files) else {
        return Vec::new();
    };
    std::iter::once(&facts.root)
        .chain(&facts.workspaces)
        .filter(|f| f.library.contains(&Ecosystem::Pypi))
        .map(|f| f.dir.clone())
        .collect()
}

fn in_dir(path: &str, dir: &str) -> bool {
    dir.is_empty() || path.starts_with(&format!("{}/", dir.trim_end_matches('/')))
}

/// `forge_condor`.
pub fn condor(run: &Run<'_>, _args: &[String]) -> Report {
    let mut r = Report::default();
    let root = run.root().to_path_buf();
    let note = |r: &mut Report, (kind, text): (NoteKind, String)| {
        r.notes.push(run.note(kind, text));
    };
    let now = match scan(run, &root) {
        Ok(s) => s,
        Err(n) => {
            note(&mut r, n);
            return r;
        }
    };
    let hits = if run.all() {
        if !now.unparsed.is_empty() {
            note(
                &mut r,
                (
                    NoteKind::Environment,
                    format!(
                        "condor could not parse {}: what it uses was not counted",
                        now.unparsed.join(", ")
                    ),
                ),
            );
        }
        now.hits
    } else {
        let copy = match tempfile::Builder::new().prefix("forge-condor.").tempdir() {
            Ok(d) => d,
            Err(e) => {
                note(
                    &mut r,
                    (
                        NoteKind::ForgeFailure,
                        format!("not checked: the copy of the base could not be made ({e})"),
                    ),
                );
                return r;
            }
        };
        let dest = std::fs::canonicalize(copy.path()).unwrap_or_else(|_| copy.path().into());
        let changed = run.changed();
        let source = |f: &str| f.ends_with(".py") || f.ends_with(".pyi") || f == "pyproject.toml";
        if let Err(why) = forge_check::base_tree::copy_sources(
            &root,
            &dest,
            &|f| changed.iter().any(|c| c == f),
            &run.inbase(),
            &|f| run.base_path(f),
            &source,
        ) {
            note(
                &mut r,
                (
                    NoteKind::ForgeFailure,
                    format!("not checked: the copy of the base could not be made ({why})"),
                ),
            );
            return r;
        }
        let base = match scan(run, &dest) {
            Ok(s) => s,
            Err((kind, why)) => {
                note(
                    &mut r,
                    (kind, format!("{why} (on the tree as it was at the base)")),
                );
                return r;
            }
        };
        if now.unparsed != base.unparsed {
            let mut files: Vec<String> = now
                .unparsed
                .iter()
                .chain(&base.unparsed)
                .filter(|f| !(now.unparsed.contains(f) && base.unparsed.contains(f)))
                .cloned()
                .collect();
            files.dedup();
            note(
                &mut r,
                (
                    NoteKind::NotEnforced,
                    format!(
                        "not checked: condor could not parse {} in one of the tree and its base, so their findings cannot be compared",
                        files.join(", ")
                    ),
                ),
            );
            return r;
        }
        let on_added = |h: &Hit| run.check.on_added(&h.path, h.line);
        let gone = |p: &str| !run.exists(p);
        new_hits(&now.hits, &base.hits, &on_added, &gone)
    };
    let mut hits = hits;
    for h in hits.iter_mut().filter(|h| h.kind == "class") {
        let source = std::fs::read_to_string(root.join(&h.path)).unwrap_or_default();
        h.nested = in_class_body(&source, h.line);
    }
    // ruff's F401 reports unused imports when the run lints with it: not twice
    let ruff_imports = run.check.checks.iter().any(|c| c == RUFF_LINT_RULE)
        && run.opts.tools.available(&root, &root, "ruff", "lint");
    hits.retain(|h| !(ruff_imports && h.kind == "import"));
    let libraries = library_dirs(run);
    let in_library = |h: &Hit| libraries.iter().any(|d| in_dir(&h.path, d));
    let (blocking, advice): (Vec<Hit>, Vec<Hit>) = hits
        .into_iter()
        .filter(|h| KINDS.contains(&h.kind.as_str()))
        .filter(|h| run.check.in_scope(&h.path) && !forge_detect::facts::is_vendored(&h.path))
        // pytest calls its hooks by name (conftest.py, plugins)
        .filter(|h| !(h.kind == "function" && h.name.starts_with("pytest_")))
        // a library's public names are used from outside
        .filter(|h| !in_library(h) || h.blocks(true))
        .partition(|h| h.blocks(in_library(h)));
    let finding = |h: &Hit| run.at(&h.path, Some(h.line), h.text());
    let blocking: Vec<Finding> = blocking.iter().map(finding).collect();
    if !blocking.is_empty() {
        run.check.mark_attributed();
        r.findings = blocking;
        return r;
    }
    // advice: at Stop only, once per turn
    let Some(snap) = run
        .check
        .snapshot
        .as_ref()
        .filter(|_| run.check.mode == RunMode::Turn)
    else {
        return r;
    };
    let advice: Vec<Finding> = advice
        .iter()
        .map(|h| {
            let mut f = finding(h);
            f.severity = forge_core::Severity::Advice;
            f
        })
        .collect();
    let texts: Vec<String> = advice.iter().map(Finding::agent_text).collect();
    let fresh = not_reported(snap, &run.ctx.rule, &texts);
    r.findings = advice
        .into_iter()
        .filter(|f| fresh.contains(&f.agent_text()))
        .collect();
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(path: &str, line: u32, kind: &str, name: &str) -> Hit {
        Hit {
            path: path.into(),
            line,
            kind: kind.into(),
            name: name.into(),
            message: format!("unused {kind} '{name}'"),
            confidence: 60,
            nested: false,
        }
    }

    #[test]
    fn identity_is_file_kind_and_name_not_line() {
        let base = [hit("a.py", 3, "function", "f")];
        let now = [
            hit("a.py", 9, "function", "f"),
            hit("a.py", 1, "import", "os"),
        ];
        let got = new_hits(&now, &base, &|_| false, &|_| false);
        assert_eq!(got, vec![hit("a.py", 1, "import", "os")]);
    }

    #[test]
    fn more_of_one_identity_counts_the_added_ones() {
        let base = [hit("a.py", 5, "unreachable_code", "return")];
        let now = [
            hit("a.py", 5, "unreachable_code", "return"),
            hit("a.py", 20, "unreachable_code", "return"),
        ];
        let got = new_hits(&now, &base, &|h| h.line == 5, &|_| false);
        assert_eq!(got, vec![hit("a.py", 5, "unreachable_code", "return")]);
    }

    #[test]
    fn moved_code_brings_no_new_finding() {
        let base = [hit("old.py", 3, "function", "f")];
        let now = [hit("new.py", 3, "function", "f")];
        assert!(new_hits(&now, &base, &|_| true, &|p| p == "old.py").is_empty());
        // the old file still there: a second, new dead function
        assert_eq!(new_hits(&now, &base, &|_| true, &|_| false).len(), 1);
    }

    #[test]
    fn kind_and_name_decide_what_fails() {
        let b = |kind: &str, name: &str, lib: bool| hit("a.py", 1, kind, name).blocks(lib);
        for lib in [false, true] {
            assert!(b("import", "os", lib));
            assert!(b("unreachable_code", "return", lib));
            assert!(b("function", "_helper", lib));
            assert!(b("method", "_parse", lib));
            assert!(b("class", "_Impl", lib));
            assert!(b("variable", "_cache", lib));
            assert!(b("attribute", "_seen", lib));
            assert!(!b("variable", "TIMEOUT", lib));
            assert!(!b("attribute", "seen", lib));
            assert!(!b("property", "size", lib));
        }
        // an application's own top-level functions and classes
        assert!(b("function", "helper", false));
        assert!(b("class", "Impl", false));
        // a public method may override a framework's; a nested class is read by one
        assert!(!b("method", "get_queryset", false));
        let mut meta = hit("a.py", 1, "class", "Meta");
        meta.nested = true;
        assert!(!meta.blocks(false));
        meta.name = "_Meta".into();
        assert!(meta.blocks(false));
        // a library's public ones are used from outside
        assert!(!b("function", "helper", true));
        assert!(!b("class", "Impl", true));
        // a dunder is not private
        assert!(!b("method", "__repr__", true));
    }

    #[test]
    fn nested_is_read_from_the_enclosing_block() {
        let src = "class Book(Model):\n    title = 1\n\n    class Meta:\n        ordering = []\n\n\nclass Top:\n    pass\n\n\ndef f():\n    class Local:\n        pass\n\n\nclass Outer:\n    if True:\n        # a comment\n        class Inner:\n            pass\n";
        assert!(in_class_body(src, 4)); // Meta
        assert!(!in_class_body(src, 8)); // Top
        assert!(!in_class_body(src, 13)); // Local, in a function
        assert!(in_class_body(src, 20)); // Inner, under an if in a class
        assert!(!in_class_body(src, 99));
        assert!(!in_class_body(src, 0));
    }

    #[test]
    fn texts_name_the_code_and_what_to_do() {
        assert_eq!(
            hit("a.py", 1, "function", "f").text(),
            format!("function `f` is unused: {UNUSED_HINT}")
        );
        let mut u = hit("a.py", 1, "unreachable_code", "return");
        u.message = "unreachable code after 'return'".into();
        assert_eq!(u.text(), "unreachable code after `return`: remove it.");
        assert!(forge_core::internals::find(&u.text()).is_empty());
        assert!(forge_core::internals::find(UNUSED_HINT).is_empty());
    }

    #[test]
    fn json_and_parse_errors() {
        let root = Path::new("/r");
        let hits = parse_json(
            r#"[{"code":"V104","confidence":90,"end_line":1,"kind":"import","line":1,"message":"unused import 'os'","name":"os","path":"/r/pkg/a.py","size":1}]"#,
            root,
        )
        .unwrap();
        assert_eq!(hits[0].path, "pkg/a.py");
        assert_eq!(hits[0].confidence, 90);
        assert!(parse_json("unused import 'os'", root).is_err());
        assert_eq!(
            unparsed(
                "bad/c.py:1: Expected a parameter or the end of the parameter list\n",
                root
            ),
            ["bad/c.py"]
        );
    }

    #[test]
    fn excludes_are_the_projects_and_forges_anchored_at_the_tree() {
        let c = Config {
            has_paths: false,
            exclude: vec!["legacy".into()],
        };
        let a = c.args(Path::new("/r"));
        assert_eq!(a[0], "--exclude");
        assert!(
            a[1].starts_with("legacy,/r/.git/*,/r/*/.git/*,"),
            "{}",
            a[1]
        );
        assert_eq!(a.last().unwrap(), ".");
        let c = Config {
            has_paths: true,
            exclude: vec![],
        };
        assert_eq!(c.args(Path::new("/r")).len(), 2);
    }
}
