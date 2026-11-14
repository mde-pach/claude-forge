//! The project's own whole-project tools, run on the change where the project set them up
//! (knip, dependency-cruiser, import-linter, jscpd): Forge brings none of them and writes no
//! config of theirs; the project's install and config decide what they report. Each reports on
//! the whole project, so its builtin keeps what the change touched or caused
//! ([`forge_check::builtins::whole`]): a finding in or about a file the change touched counts;
//! for knip and dependency-cruiser, whose findings can land in a file the change did not touch
//! (an export it left unused, a module it left an orphan), the tool runs a second time on the
//! tree as it was at the base, and only what that run did not report counts.
//!
//! A tool the project does not have, a release its adapter does not cover, or a run that fails
//! is a note for the person; the check then passes.

use std::path::{Path, PathBuf};

use forge_check::base_tree::BaseTree;
use forge_check::builtins::whole::{Located, attributed_report, counted, touches};
use forge_check::builtins::{RunMode, ToolOutput, ToolRun, skipped};
use forge_core::builtin::{BuiltinCtx, Scope};
use forge_core::{Finding, Note, NoteKind, Report};

use crate::Options;
use crate::ctx::Extra;
use crate::util::{git_lines, note};

/// One run of a builtin of this module.
pub struct Run<'a> {
    pub ctx: &'a BuiltinCtx,
    pub x: &'a Extra,
    pub opts: &'a Options,
}

impl Run<'_> {
    pub fn root(&self) -> &Path {
        &self.ctx.root
    }

    /// `--all` (CI's full run): every finding counts.
    pub fn all(&self) -> bool {
        self.x.mode == RunMode::All || self.ctx.scope == Scope::All
    }

    pub fn changed(&self, path: &str) -> bool {
        self.ctx.changed.iter().any(|c| c == path)
    }

    pub fn note(&self, kind: NoteKind, text: impl Into<String>) -> Note {
        note(self.ctx, kind, text)
    }

    /// The base copy of a changed file.
    pub fn base_text(&self, path: &str) -> Option<String> {
        let p = self.ctx.base.as_ref()?.join(path);
        std::fs::read_to_string(p).ok()
    }

    /// `op` of the project's `tool` in `dir`, as its adapter says; a note when it cannot run.
    pub fn tool(
        &self,
        dir: &Path,
        tool: &str,
        op: &str,
        args: &[String],
        env: &[(String, String)],
    ) -> Result<ToolOutput, Note> {
        match self.opts.tools.run_env(dir, dir, tool, op, args, env) {
            ToolRun::Ran(o) => Ok(o),
            ToolRun::Missing(why) => Err(self.note(NoteKind::MissingTool, skipped(&why))),
            ToolRun::Unsupported(why) => Err(self.note(NoteKind::NotEnforced, why)),
        }
    }

    /// The note of a run that failed.
    pub fn failed(&self, tool: &str, out: &ToolOutput) -> Note {
        self.note(
            NoteKind::ForgeFailure,
            format!(
                "not checked: {tool} could not run (exit {}: {})",
                out.status,
                last_line(&out.text())
            ),
        )
    }

    /// The tree as it was at the base, with the project's installed dependencies.
    pub(crate) fn base_tree(&self) -> Result<BaseTree, String> {
        let inbase: Vec<String> = self
            .ctx
            .changed
            .iter()
            .filter(|p| self.ctx.base.as_ref().is_some_and(|b| b.join(p).is_file()))
            .cloned()
            .collect();
        let base = self.ctx.base.clone();
        forge_check::base_tree::build_at(self.root(), &|f| self.changed(f), &inbase, &|f| {
            let p: PathBuf = base.as_ref()?.join(f);
            p.is_file().then_some(p)
        })
    }

    /// The findings that count of a tool whose findings can land outside the files the change
    /// touched: `scan` on the tree now, and when one is in no touched file, on the base tree.
    /// A base tree that cannot be made leaves the touched files' findings, with a note.
    pub fn with_base<T: Located + Clone>(
        &self,
        scan: &dyn Fn(&Path) -> Result<Vec<T>, Note>,
    ) -> Result<(Vec<T>, Vec<Note>), Note> {
        let now = scan(self.root())?;
        let changed = |p: &str| self.changed(p);
        if self.all() || now.iter().all(|f| touches(f, &changed)) {
            return Ok((now, Vec::new()));
        }
        let only_touched = |why: String| {
            let kept = now
                .iter()
                .filter(|f| touches(*f, &changed))
                .cloned()
                .collect();
            let n = self.note(
                NoteKind::Environment,
                format!(
                    "its findings in other files could not be compared with the base: {why}; only those in the changed files count"
                ),
            );
            Ok((kept, vec![n]))
        };
        let tree = match self.base_tree() {
            Ok(t) => t,
            Err(why) => return only_touched(why),
        };
        let base = match scan(tree.path()) {
            Ok(b) => b,
            Err(n) => return only_touched(n.text),
        };
        let gone = |p: &str| !self.root().join(p).exists();
        Ok((counted(&now, &base, &changed, &gone), Vec::new()))
    }

    /// The findings that count of a tool whose findings always name what the change touched
    /// (an import chain holds the import the change added; a duplicate, the copy it made).
    pub fn touched<T: Located + Clone>(&self, now: Vec<T>) -> Vec<T> {
        if self.all() {
            return now;
        }
        let changed = |p: &str| self.changed(p);
        now.into_iter().filter(|f| touches(f, &changed)).collect()
    }

    /// The report: `errors` fail the check, `advice` is given at Stop once per turn.
    pub fn report(&self, errors: Vec<Finding>, advice: Vec<Finding>, notes: Vec<Note>) -> Report {
        let mut r = attributed_report(self.ctx, self.x, errors, advice);
        r.notes.extend(notes);
        r
    }

    pub fn only_note(&self, n: Note) -> Report {
        Report {
            findings: Vec::new(),
            notes: vec![n],
        }
    }

    /// The top-level directories and the root's files of these extensions that git sees
    /// (tracked, or untracked and not ignored), hidden ones left out: what a tool that takes
    /// paths scans of the project, never `.git` or an ignored `node_modules`.
    pub fn top_level(&self, exts: &[&str]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for f in git_lines(self.root(), &["ls-files", "-co", "--exclude-standard"]) {
            let entry = match f.split_once('/') {
                Some((dir, _)) => dir.to_owned(),
                None if exts.iter().any(|e| f.ends_with(e)) => f.clone(),
                None => continue,
            };
            if !entry.starts_with('.') && !out.contains(&entry) {
                out.push(entry);
            }
        }
        out.sort();
        out
    }
}

/// A path a tool printed, relative to `root` (it may print it absolute, or with `./`).
pub fn relative(path: &str, root: &Path) -> String {
    let mut p = path.to_owned();
    let roots = [
        root.display().to_string(),
        std::fs::canonicalize(root)
            .map(|r| r.display().to_string())
            .unwrap_or_default(),
    ];
    for r in roots.iter().filter(|r| !r.is_empty()) {
        if let Some(rest) = p.strip_prefix(&format!("{r}/")) {
            p = rest.to_owned();
        }
    }
    while let Some(rest) = p.strip_prefix("./") {
        p = rest.to_owned();
    }
    p
}

/// The last non-blank line of a tool's output, without colours.
pub fn last_line(text: &str) -> String {
    let ansi = regex::Regex::new(r"\x1b\[[0-9;]*[A-Za-z]").expect("regex");
    ansi.replace_all(text, "")
        .lines()
        .rfind(|l| !l.trim().is_empty())
        .unwrap_or("no output")
        .trim()
        .to_owned()
}

/// The 1-based line of `path` (under `root`) that first holds `needle` in quotes: where an
/// import of that specifier is.
pub fn line_quoting(root: &Path, path: &str, needle: &str) -> Option<u32> {
    let text = std::fs::read_to_string(root.join(path)).ok()?;
    let quoted = [
        format!("\"{needle}\""),
        format!("'{needle}'"),
        format!("`{needle}`"),
    ];
    text.lines()
        .position(|l| quoted.iter().any(|q| l.contains(q.as_str())))
        .map(|i| i as u32 + 1)
}
