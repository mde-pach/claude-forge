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
