//! A fixture repository and the context the checker builds for a check in it.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use forge_builtins_code::Extra;
use forge_builtins_code::ctx::{Added, CheckRule};
use forge_builtins_code::scope::{Roles, RuleScope};
use forge_core::builtin::{BuiltinCtx, Scope};
use forge_packs::model::Rule;

pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn write(dir: &Path, files: &BTreeMap<String, String>) {
    for (p, text) in files {
        let path = dir.join(p);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

/// A git repository with `head` committed and `files` written over it, and the change set as
/// the checker makes it: the changed files, their base copies, the added lines.
pub struct Repo {
    pub dir: tempfile::TempDir,
    pub root: PathBuf,
    pub base: PathBuf,
    pub changed: Vec<String>,
    pub added: BTreeMap<String, Added>,
}

impl Repo {
    pub fn new(head: &BTreeMap<String, String>, files: &BTreeMap<String, String>) -> Repo {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        write(&root, head);
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-qm", "head", "--allow-empty"]);
        write(&root, files);
        // git-ignored files are not part of a change
        let listed: BTreeSet<String> = git(&root, &["ls-files", "-co", "--exclude-standard"])
            .lines()
            .map(str::to_owned)
            .collect();
        let changed: Vec<String> = files
            .iter()
            .filter(|(p, t)| head.get(*p) != Some(*t) && listed.contains(*p))
            .map(|(p, _)| p.clone())
            .collect();
        let base = dir.path().join("base");
        std::fs::create_dir_all(&base).unwrap();
        let mut added = BTreeMap::new();
        for p in &changed {
            match head.get(p) {
                Some(t) => {
                    let b = base.join(p);
                    std::fs::create_dir_all(b.parent().unwrap()).unwrap();
                    std::fs::write(&b, t).unwrap();
                    let diff = git(&root, &["diff", "-U0", "--no-color", "HEAD", "--", p]);
                    let mut lines = BTreeSet::new();
                    for l in diff.lines().filter(|l| l.starts_with("@@")) {
                        let plus = l.split_whitespace().nth(2).unwrap_or("+0");
                        let mut it = plus.trim_start_matches('+').split(',');
                        let start: u32 = it.next().unwrap_or("0").parse().unwrap_or(0);
                        let n: u32 = it.next().map_or(1, |c| c.parse().unwrap_or(0));
                        lines.extend(start..start + n);
                    }
                    if !lines.is_empty() {
                        added.insert(p.clone(), Added::Lines(lines));
                    }
                }
                None => {
                    added.insert(p.clone(), Added::Whole);
                }
            }
        }
        Repo {
            dir,
            root,
            base,
            changed,
            added,
        }
    }

    /// The context of `rule`'s check in a turn.
    pub fn ctx(&self, rule: &str) -> BuiltinCtx {
        BuiltinCtx {
            root: self.root.clone(),
            base: Some(self.base.clone()),
            changed: self.changed.clone(),
            rule: rule.into(),
            scope: Scope::Turn,
            accepted: vec![],
            notes_file: None,
        }
    }

    /// The checker's extra context for `rule`: its in-scope changed files (none when only a
    /// `when_changed` file changed), the added lines, the default roles.
    pub fn extra(&self, rule: &Rule) -> Extra {
        let roles = Roles::defaults();
        let scope = RuleScope::of_check(rule, &roles, None);
        let files = self
            .changed
            .iter()
            .filter(|p| self.root.join(p).is_file() && scope.matches(p))
            .cloned()
            .collect();
        Extra {
            files,
            added: self.added.clone(),
            roles,
            check: Some(CheckRule {
                rule: rule.clone(),
                dirs: None,
            }),
            checks: vec![],
            snapshot: None,
        }
    }
}

/// The findings of a lint-stage check as the checker attributes them: a line naming a location
/// counts on an added line (`path:0`: in a changed file); when some name locations and none
/// counts, the check passes; when none names one, all count.
pub fn attribute(lines: &[String], repo: &Repo) -> Vec<String> {
    let loc = regex::Regex::new(r"^([^\s:(]+):([0-9]+)").unwrap();
    let mut named = false;
    let mut hit = vec![];
    for l in lines {
        let Some(c) = loc.captures(l) else { continue };
        named = true;
        let p = &c[1];
        let n: u32 = c[2].parse().unwrap();
        let whole = matches!(repo.added.get(p), Some(Added::Whole));
        let counts = whole
            || if n == 0 {
                repo.changed.iter().any(|x| x == p)
            } else {
                repo.added.get(p).is_some_and(|a| a.contains(n))
            };
        if counts {
            hit.push(l.clone());
        }
    }
    if named { hit } else { lines.to_vec() }
}

/// The words of a shell command (quotes removed), as far as `;`, `|`, `&` or a newline.
pub fn words(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut any = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                any = true;
                for d in chars.by_ref() {
                    if d == '\'' {
                        break;
                    }
                    cur.push(d);
                }
            }
            '"' => {
                any = true;
                while let Some(d) = chars.next() {
                    match d {
                        '"' => break,
                        '\\' => cur.extend(chars.next()),
                        d => cur.push(d),
                    }
                }
            }
            ';' | '|' | '&' | '\n' => break,
            c if c.is_whitespace() => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The tools on PATH, then Forge's cache (installing when `FORGE_TEST_INSTALL_TOOLS=1`).
pub struct TestTools(forge_builtins_code::Provisioned<forge_tools::Provisioner>);

impl forge_builtins_code::PinnedTools for TestTools {
    fn pinned(&self, name: &str) -> Result<PathBuf, forge_core::Note> {
        forge_builtins_code::OnPath::default()
            .pinned(name)
            .or_else(|_| self.0.pinned(name))
    }
}

pub fn options() -> forge_builtins_code::Options {
    let home = forge_home::Home::from_env().expect("home");
    forge_builtins_code::Options {
        tools: std::sync::Arc::new(TestTools(forge_builtins_code::Provisioned {
            resolver: forge_tools::Provisioner::new(home),
            install: std::env::var("FORGE_TEST_INSTALL_TOOLS").is_ok_and(|v| v == "1"),
        })),
        ..forge_builtins_code::Options::default()
    }
}

/// One builtin call on one fixture.
#[derive(Debug, Clone)]
pub struct Case {
    pub label: String,
    pub rule: Rule,
    pub name: String,
    pub args: Vec<String>,
    /// Run under `forge_advise`: no attribution to added lines.
    pub advised: bool,
    /// Some(true): must find something; Some(false): must find nothing.
    pub want: Option<bool>,
    pub head: BTreeMap<String, String>,
    pub files: BTreeMap<String, String>,
}

impl Case {
    pub fn repo(&self) -> Repo {
        Repo::new(&self.head, &self.files)
    }
}

/// The builtin call of a rule's check: name, arguments, whether advice wraps it.
pub fn call_of(rule: &Rule, names: &[&str]) -> Option<(String, Vec<String>, bool)> {
    let call = regex::Regex::new(r"forge_([a-z_]+)").unwrap();
    rule.enforce.iter().find_map(|t| {
        let forge_packs::model::Target::Check(c) = t else {
            return None;
        };
        c.commands().find_map(|cmd| {
            call.find_iter(cmd).find_map(|m| {
                let n = &cmd[m.start() + 6..m.end()];
                names.contains(&n).then(|| {
                    (
                        n.to_string(),
                        words(&cmd[m.end()..]),
                        cmd[..m.start()].contains("forge_advise"),
                    )
                })
            })
        })
    })
}

/// The rules calling one of `names`, and a case per fixture example of theirs.
pub fn pack_cases(names: &[&str]) -> (Vec<Rule>, Vec<Case>) {
    let set = forge_packs::load_builtin(forge_packs::Options::default()).expect("packs");
    let mut rules = vec![];
    let mut out = vec![];
    for rule in set.packs.values().flat_map(|p| p.rules.iter()) {
        let Some((name, args, advised)) = call_of(rule, names) else {
            continue;
        };
        rules.push(rule.clone());
        let Some(ex) = &rule.examples else { continue };
        for (want, list) in [(true, &ex.match_), (false, &ex.no_match)] {
            for (i, e) in list.iter().flatten().enumerate() {
                let forge_packs::model::Example::Fixture(f) = e else {
                    continue;
                };
                out.push(Case {
                    label: format!(
                        "{} {}[{i}]",
                        rule.id,
                        if want { "match" } else { "no_match" }
                    ),
                    rule: rule.clone(),
                    name: name.clone(),
                    args: args.clone(),
                    advised,
                    want: Some(want),
                    head: f.head.clone().unwrap_or_default().into_iter().collect(),
                    files: f.files.clone().into_iter().collect(),
                });
            }
        }
    }
    (rules, out)
}

/// What the Rust builtin reports on a case: the lines that count, the notes.
pub fn run_port(
    opts: &forge_builtins_code::Options,
    case: &Case,
    repo: &Repo,
) -> (Vec<String>, Vec<forge_core::Note>) {
    let ctx = repo.ctx(&case.rule.id);
    let x = repo.extra(&case.rule);
    let report = forge_builtins_code::run(opts, &case.name, &ctx, &x, &case.args).expect("builtin");
    let lines: Vec<String> = report.findings.iter().map(|f| f.agent_text()).collect();
    let counted = if case.advised {
        lines
    } else {
        attribute(&lines, repo)
    };
    (counted, report.notes)
}

/// Whether a note says the builtin could not do its work (a tool it lacks, a tool that failed).
pub fn not_run(notes: &[forge_core::Note]) -> Option<&forge_core::Note> {
    use forge_core::NoteKind;
    notes
        .iter()
        .find(|n| matches!(n.kind, NoteKind::MissingTool | NoteKind::ForgeFailure))
}
