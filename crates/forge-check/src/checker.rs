//! The checker: runs the active rules on a change (`.forge/check.sh`, `post-edit.sh`).
//!
//! At Stop and for `forge check`, in order, stopping after the first failing step:
//! 1. tests and counts: deleted test files; `no-increase` / `no-decrease` counts against the base;
//! 2. forbid rules (ast-grep, grep) on the added lines (every line with `--all`);
//! 3. the `check` stages: format, lint, codegen, typecheck, migration, test.
//!
//! Advice never stops the steps after it. After an edit: the edited files are formatted
//! (silently), the lint checks' safe fixes applied, the files linted, and the forbid and advise
//! rules run on their added lines.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ast_grep_language::SupportLang;
use forge_core::builtin::{
    ENV_ACCEPTED, ENV_BASE, ENV_CHANGED, ENV_NOTES, ENV_ROOT, ENV_RULE, ENV_SCOPE, Scope,
};
use forge_core::{Finding, Note, NoteKind, Report, Severity};
use forge_packs::model::{Accept, CheckTarget, Mode as RuleMode, Rule, Stage, Target};

use crate::astgrep::{self, AstMatch, Compiled, TypeOracle};
use crate::builtins::{self, CheckRule};
use crate::change::{Added, ChangeSet, Mode, added_lines};
use crate::diagnostics::{self, Attribution, Level, Target as DiagTarget};
use crate::grep::{self, GrepRule};
use crate::scope::{GlobList, Roles, RuleScope, is_prose};
use crate::shell::{self, Exit, Spec};
use crate::tools::{self, ToolResolver};

/// A rule the project selected, and the workspace packages its pack was detected in.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ActiveRule {
    pub rule: Rule,
    /// Where it applies: workspace package directories, or `None` for the whole repository.
    pub dirs: Option<Vec<String>>,
    /// Where each check target runs, by its index in `rule.enforce`: in workspace packages, or
    /// at the root (`None`). A check missing here runs in `dirs`.
    pub check_dirs: BTreeMap<usize, Option<Vec<String>>>,
}

impl ActiveRule {
    pub fn new(rule: Rule) -> ActiveRule {
        ActiveRule {
            rule,
            dirs: None,
            check_dirs: BTreeMap::new(),
        }
    }
}

/// When the checker runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// After an edit (PostToolUse): format, safe fixes, lint, forbid and advise on added lines.
    PostEdit,
    /// When the agent stops, and `forge check`: every step.
    Stop,
}

/// How a run went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Pass,
    Fail,
    /// The only failures are tests that also fail at the start of the turn.
    Unverified,
    /// Advice was the only failure.
    AdviceOnly,
}

impl Status {
    /// `check.sh`'s exit status: 0, 1, 3, 4.
    pub fn exit_code(self) -> i32 {
        match self {
            Status::Pass => 0,
            Status::Fail => 1,
            Status::Unverified => 3,
            Status::AdviceOnly => 4,
        }
    }
}

/// Where the time went.
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct Timings {
    /// Forge's own work (git, ast-grep, grep, bookkeeping).
    pub forge: Duration,
    /// The project's checks and tools.
    pub tools: Duration,
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub report: Report,
    pub status: Status,
    /// For the Stop hook (§10.6): `test<TAB><rule> <test id>`, `env<TAB><phrase>`.
    pub unverified: Vec<String>,
    pub timings: Timings,
}

/// What a run may spend and with which executable builtins run.
#[derive(Debug, Clone)]
pub struct Options {
    pub phase: Phase,
    /// The Forge executable the builtins' shell functions call.
    pub exe: PathBuf,
    /// The cap on the project's checks (their own time, not Forge's).
    pub budget: Duration,
}

impl Options {
    /// Stop and `forge check`: project checks capped at 5 minutes.
    pub fn stop(exe: PathBuf) -> Options {
        Options {
            phase: Phase::Stop,
            exe,
            budget: Duration::from_secs(300),
        }
    }

    /// After an edit: the tools get 45 seconds in all.
    pub fn post_edit(exe: PathBuf) -> Options {
        Options {
            phase: Phase::PostEdit,
            exe,
            budget: Duration::from_secs(45),
        }
    }
}

/// The checker for one repository's active rules.
pub struct Checker {
    pub root: PathBuf,
    pub rules: Vec<ActiveRule>,
    pub roles: Roles,
    /// forge.yaml `accept:` entries.
    pub accept: Vec<Accept>,
    pub tools: Arc<dyn ToolResolver>,
    pub types: Option<Arc<dyn TypeOracle>>,
}

/// A check target of an active rule.
struct Check<'a> {
    active: &'a ActiveRule,
    target: &'a CheckTarget,
    scope: RuleScope,
    tools: Vec<String>,
    /// The workspace packages it runs in; `None`: the root.
    run_dirs: Option<Vec<String>>,
    /// The workspace packages its files are in; `None`: the whole repository.
    scope_dirs: Option<Vec<String>>,
}

impl Check<'_> {
    fn id(&self) -> &str {
        &self.active.rule.id
    }
}

/// One check's result.
enum CheckResult {
    Pass,
    Fail(Vec<Finding>),
    Unverified(Vec<Finding>, Vec<String>),
    Advice(Vec<Finding>),
}

/// The per-run files builtins read.
struct RunFiles {
    dir: tempfile::TempDir,
}

impl RunFiles {
    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
}

struct Run<'a> {
    change: &'a mut ChangeSet,
    opts: &'a Options,
    files: RunFiles,
    deadline: Instant,
    tool_time: Duration,
    report: Report,
    unverified: Vec<String>,
    advice: Vec<Finding>,
}

const DELETED_TESTS: &[&str] = &[
    "test_*.py",
    "*/test_*.py",
    "*_test.py",
    "*/conftest.py",
    "conftest.py",
    "tests/*",
    "*.test.*",
    "*.spec.*",
    "*/__tests__/*",
    "e2e/*",
];

fn stage_header(stage: Stage) -> &'static str {
    match stage {
        Stage::Format => "The formatting check failed:",
        Stage::Lint => "The lint check failed:",
        Stage::Codegen => "The generated code is not up to date:",
        Stage::Typecheck => "The type check failed:",
        Stage::Migration => "The migration check failed:",
        Stage::Test => "Tests failed:",
    }
}

fn note(kind: NoteKind, rule: &str, text: impl Into<String>) -> Note {
    Note::new(kind, text).for_rule(rule.to_string())
}

impl Checker {
    pub fn new(root: PathBuf, rules: Vec<ActiveRule>) -> Checker {
        Checker {
            root,
            rules,
            roles: Roles::defaults(),
            accept: Vec::new(),
            tools: Arc::new(tools::LocalTools::default()),
            types: None,
        }
    }

    fn checks(&self) -> Vec<Check<'_>> {
        let mut out = Vec::new();
        for a in &self.rules {
            for (i, t) in a.rule.enforce.iter().enumerate() {
                if let Target::Check(c) = t {
                    let run_dirs = a.check_dirs.get(&i).cloned().unwrap_or(a.dirs.clone());
                    let scope_dirs = run_dirs.clone().or(a.dirs.clone());
                    let scope = RuleScope::new(
                        &a.rule,
                        &self.roles,
                        scope_dirs.as_deref(),
                        c.config_files.as_deref(),
                    );
                    out.push(Check {
                        active: a,
                        target: c,
                        scope,
                        tools: tools::check_tools(&c.commands().collect::<Vec<_>>()),
                        run_dirs,
                        scope_dirs,
                    });
                }
            }
        }
        out
    }

    /// The ast-grep targets of the given languages, compiled; a rule that does not compile is a
    /// note.
    fn ast_rules(
        &self,
        langs: &BTreeSet<&'static str>,
        only: Option<&[String]>,
        notes: &mut Vec<Note>,
    ) -> Vec<Compiled> {
        let candidates: Vec<&ActiveRule> = self
            .rules
            .iter()
            .filter(|a| {
                astgrep::target_of(&a.rule).is_some()
                    && astgrep::language(a.rule.scope.language)
                        .is_some_and(|l| langs.contains(lang_key(l)))
            })
            .collect();
        // compiling a rule parses its patterns: in parallel, a rule's scope first
        let workers = std::thread::available_parallelism()
            .map_or(4, |n| n.get())
            .min(8);
        let chunk = candidates.len().div_ceil(workers).max(1);
        let results: Vec<Option<Result<Compiled, (String, String)>>> = std::thread::scope(|s| {
            let handles: Vec<_> = candidates
                .chunks(chunk)
                .map(|part| {
                    s.spawn(move || {
                        part.iter()
                            .map(|a| {
                                let lang = astgrep::language(a.rule.scope.language)?;
                                if let Some(files) = only {
                                    let scope = RuleScope::new(
                                        &a.rule,
                                        &self.roles,
                                        a.dirs.as_deref(),
                                        None,
                                    );
                                    if !files.iter().any(|f| {
                                        astgrep::file_language(f) == Some(lang)
                                            && scope.files_match(f)
                                    }) {
                                        return None;
                                    }
                                }
                                let t = astgrep::target_of(&a.rule)?;
                                Some(
                                    astgrep::compile(&a.rule, t, &self.roles, a.dirs.as_deref())
                                        .map_err(|e| (a.rule.id.clone(), e)),
                                )
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|h| h.join().unwrap_or_default())
                .collect()
        });
        let mut out = Vec::new();
        for r in results.into_iter().flatten() {
            match r {
                Ok(c) => out.push(c),
                Err((id, e)) => notes.push(note(
                    NoteKind::ForgeFailure,
                    &id,
                    format!("not enforced: the rule does not compile ({e})"),
                )),
            }
        }
        out
    }

    fn grep_rules(&self, notes: &mut Vec<Note>) -> Vec<GrepRule> {
        let mut out = Vec::new();
        for a in &self.rules {
            let Some(t) = grep::target_of(&a.rule) else {
                continue;
            };
            match grep::compile(&a.rule, t, &self.roles, a.dirs.as_deref()) {
                Ok(g) => out.push(g),
                Err(e) => notes.push(note(
                    NoteKind::ForgeFailure,
                    &a.rule.id,
                    format!("not enforced: {e}"),
                )),
            }
        }
        out
    }

    /// Computes the change for `mode` and checks it.
    pub fn check(&self, mode: Mode, opts: &Options) -> Result<Outcome, crate::change::ChangeError> {
        let started = Instant::now();
        let mut change = ChangeSet::compute(&self.root, mode)?;
        let mut o = self.check_change(&mut change, opts);
        o.timings.forge = started.elapsed().saturating_sub(o.timings.tools);
        Ok(o)
    }

    /// Checks the change made to `files` after an edit (against the turn snapshot when there
    /// is one, else HEAD).
    pub fn check_files(
        &self,
        files: &[String],
        snapshot: Option<&Path>,
        opts: &Options,
    ) -> Result<Outcome, crate::change::ChangeError> {
        let started = Instant::now();
        let mut change = ChangeSet::for_files(&self.root, files, snapshot)?;
        let mut o = self.check_change(&mut change, opts);
        o.timings.forge = started.elapsed().saturating_sub(o.timings.tools);
        Ok(o)
    }

    /// Checks a computed change.
    pub fn check_change(&self, change: &mut ChangeSet, opts: &Options) -> Outcome {
        let files = RunFiles {
            dir: tempfile::Builder::new()
                .prefix("forge-check.")
                .tempdir()
                .expect("temporary directory"),
        };
        let mut run = Run {
            change,
            opts,
            files,
            deadline: Instant::now() + opts.budget,
            tool_time: Duration::ZERO,
            report: Report::default(),
            unverified: Vec::new(),
            advice: Vec::new(),
        };
        // shared files the builtins read
        let roles: BTreeMap<String, Vec<String>> = self.roles.map().clone();
        let _ = std::fs::write(
            run.files.path("roles.json"),
            serde_json::to_string(&roles).unwrap_or_default(),
        );
        let ids: Vec<&str> = self
            .checks()
            .iter()
            .map(|c| c.active.rule.id.as_str())
            .collect::<Vec<_>>();
        let _ = std::fs::write(run.files.path("checks"), ids.join("\n"));
        let _ = std::fs::write(run.files.path("notes"), "");
        let status = match opts.phase {
            Phase::Stop => self.stop_steps(&mut run),
            Phase::PostEdit => self.edit_steps(&mut run),
        };
        let mut report = std::mem::take(&mut run.report);
        report.findings.append(&mut run.advice);
        dedupe_notes(&mut report.notes);
        Outcome {
            report,
            status,
            unverified: run.unverified,
            timings: Timings {
                forge: Duration::ZERO,
                tools: run.tool_time,
            },
        }
    }

    fn stop_steps(&self, run: &mut Run<'_>) -> Status {
        self.format_added_lines(run);
        self.write_change_files(run);
        let mut failed = false;
        failed |= self.step_counts(run);
        if !failed {
            failed |= self.step_forbid(run, false);
        }
        let mut unverified_only = false;
        if !failed {
            for stage in Stage::ALL {
                let (f, u) = self.run_stage(run, *stage);
                if f {
                    failed = true;
                    break;
                }
                if u {
                    unverified_only = true;
                }
            }
        }
        if failed {
            Status::Fail
        } else if unverified_only {
            Status::Unverified
        } else if !run.advice.is_empty() {
            Status::AdviceOnly
        } else {
            Status::Pass
        }
    }

    fn edit_steps(&self, run: &mut Run<'_>) -> Status {
        self.write_change_files(run);
        let checks = self.checks();
        let existing: Vec<String> = run.change.existing.clone();
        // format (silently): the {files} form of each format check
        for c in checks.iter().filter(|c| c.target.stage == Stage::Format) {
            let files: Vec<String> = existing
                .iter()
                .filter(|f| c.scope.matches(f))
                .cloned()
                .collect();
            if files.is_empty()
                || c.target
                    .changed_run
                    .as_deref()
                    .is_none_or(|r| !r.contains("{files}"))
            {
                continue;
            }
            if self.missing_tool(c).is_some() {
                continue;
            }
            let cmd = c.target.changed_run.clone().unwrap_or_default();
            let _ = self.exec(run, c, &cmd, &files, true, None);
        }
        // safe fixes, then lint; copies first, to tell what the fixes changed
        let pre = tempfile::tempdir().ok();
        if let Some(pre) = &pre {
            for f in &existing {
                let to = pre.path().join(f);
                if let Some(d) = to.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                let _ = std::fs::copy(self.root.join(f), to);
            }
        }
        let mut fix_linted: BTreeMap<String, (Exit, String, Vec<String>)> = BTreeMap::new();
        let mut fixed_codes: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for c in checks
            .iter()
            .filter(|c| c.target.stage == Stage::Lint && c.target.fix_run.is_some())
        {
            let files: Vec<String> = existing
                .iter()
                .filter(|f| c.scope.matches(f))
                .cloned()
                .collect();
            if files.is_empty() || self.missing_tool(c).is_some() {
                continue;
            }
            let cmd = c.target.fix_run.clone().unwrap_or_default();
            let (exit, out) = self.exec(run, c, &cmd, &files, true, None);
            for (f, code) in ruff_fixes(&out) {
                fixed_codes.entry(f).or_default().insert(code);
            }
            if c.target.fix_lints == Some(true) {
                fix_linted.insert(c.id().to_string(), (exit, out, files));
            }
        }
        let mut noted = BTreeSet::new();
        if let Some(pre) = &pre {
            for f in &existing {
                let before = std::fs::read(pre.path().join(f)).unwrap_or_default();
                let now = std::fs::read(self.root.join(f)).unwrap_or_default();
                if before != now && fix_linted_or_fixed(&checks, f) {
                    let codes = fixed_codes
                        .get(f)
                        .map(|c| c.iter().cloned().collect::<Vec<_>>().join(", "));
                    run.advice.push(auto_fixed(f, codes.as_deref()));
                    noted.insert(f.clone());
                }
            }
        }
        // lint: the {files} form on the edited files
        let mut lint_runs: Vec<(String, Stage, bool, Exit, String)> = Vec::new();
        let mut lint_ran = false;
        for c in checks.iter().filter(|c| c.target.stage == Stage::Lint) {
            if let Some((exit, out, _)) = fix_linted.remove(c.id()) {
                lint_runs.push((
                    c.id().to_string(),
                    c.target.stage,
                    c.target.fail_on_warnings == Some(true),
                    exit,
                    out,
                ));
                continue;
            }
            let Some(cmd) = c
                .target
                .changed_run
                .clone()
                .filter(|r| r.contains("{files}"))
            else {
                continue;
            };
            let files: Vec<String> = existing
                .iter()
                .filter(|f| c.scope.matches(f))
                .cloned()
                .collect();
            if files.is_empty() || self.missing_tool(c).is_some() {
                continue;
            }
            lint_ran = true;
            let (exit, out) = self.exec(run, c, &cmd, &files, true, None);
            lint_runs.push((
                c.id().to_string(),
                c.target.stage,
                c.target.fail_on_warnings == Some(true),
                exit,
                out,
            ));
        }
        // a lint run that writes (Biome's check --write) applies fixes too
        if lint_ran && let Some(pre) = &pre {
            for f in &existing {
                if noted.contains(f) {
                    continue;
                }
                let before = std::fs::read(pre.path().join(f)).unwrap_or_default();
                let now = std::fs::read(self.root.join(f)).unwrap_or_default();
                if before == now {
                    continue;
                }
                if let Some(formatted) = self.format_copy(run, f, &before)
                    && formatted == now
                {
                    continue;
                }
                run.advice.push(auto_fixed(f, None));
            }
        }
        // the added lines, on the files as the checks left them
        run.change.refresh_added();
        self.format_added_lines(run);
        self.write_change_files(run);
        for (id, stage, warn, exit, out) in lint_runs {
            if exit.ok() {
                continue;
            }
            let level = diagnostics::level_of(stage, false);
            let target = DiagTarget::of(run.change);
            let (what, lines) = match level {
                Some(l) => diagnostics::attribute(&out, &self.root, &target, l, warn),
                None => (Attribution::None, Vec::new()),
            };
            match what {
                Attribution::Other | Attribution::Warnings | Attribution::Forge => {}
                Attribution::Changed => run.report.findings.extend(cap(
                    diagnostics::findings_of(&lines.join("\n"), &id, &self.root),
                    15,
                    &id,
                )),
                Attribution::None => run
                    .report
                    .findings
                    .extend(self.failure_findings(&id, stage, &out, 15)),
            }
        }
        // forbid and advise rules on the added lines
        self.step_forbid(run, true);
        if run.report.fails() {
            Status::Fail
        } else if !run.advice.is_empty() {
            Status::AdviceOnly
        } else {
            Status::Pass
        }
    }

    /// The change's files for builtins: changed paths and added lines.
    /// The change's files, as the engine laid them out in `$FORGE_TMP` (which multi-line check
    /// commands still read): `changed`, `existing`, `deleted`, `inbase`, `added`, `base/`.
    fn write_change_files(&self, run: &mut Run<'_>) {
        let lines = |v: &[String]| {
            let mut s = v.join("\n");
            if !s.is_empty() {
                s.push('\n');
            }
            s
        };
        let _ = std::fs::write(run.files.path("changed"), lines(&run.change.changed));
        let _ = std::fs::write(run.files.path("existing"), lines(&run.change.existing));
        let _ = std::fs::write(run.files.path("deleted"), lines(&run.change.deleted));
        let _ = std::fs::write(run.files.path("inbase"), lines(&run.change.inbase));
        let _ = std::fs::write(
            run.files.path("added"),
            builtins::render_added(&run.change.added),
        );
        let base = run.files.path("base");
        let _ = std::fs::remove_file(&base);
        let _ = std::os::unix::fs::symlink(run.change.base_dir(), &base);
    }

    // ---------------------------------------------------------------- step 1: tests and counts

    fn step_counts(&self, run: &mut Run<'_>) -> bool {
        let change = &*run.change;
        let mut failed = false;
        let tests = GlobList::new(
            &DELETED_TESTS
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
        );
        for f in &change.deleted {
            if tests.any(f) {
                run.report.findings.push(
                    Finding::error("forge/deleted-test", "This test file was deleted: restore it (removing tests is the user's decision: ask them).").at(f.clone(), None),
                );
                failed = true;
            }
        }
        if change.existing.is_empty() {
            return failed;
        }
        let counted = |m: RuleMode| m == RuleMode::NoIncrease || m == RuleMode::NoDecrease;
        let mut notes = Vec::new();
        let langs: BTreeSet<&'static str> = change
            .existing
            .iter()
            .filter_map(|f| astgrep::file_language(f))
            .map(lang_key)
            .collect();
        let ast: Vec<Compiled> = self
            .ast_rules(&langs, Some(&change.existing), &mut notes)
            .into_iter()
            .filter(|c| counted(c.mode()))
            .collect();
        let greps: Vec<GrepRule> = self
            .grep_rules(&mut notes)
            .into_iter()
            .filter(|g| counted(g.target.mode))
            .collect();
        run.report.notes.extend(notes);
        let athead: Vec<String> = change
            .inbase
            .iter()
            .filter(|p| !change.deleted.contains(p))
            .cloned()
            .collect();
        let count = |dir: &Path, files: &[String]| -> BTreeMap<(String, String), usize> {
            let mut out = BTreeMap::new();
            let refs: Vec<&Compiled> = ast.iter().collect();
            for m in astgrep::scan_files(&refs, dir, files) {
                *out.entry((m.rule, m.path)).or_insert(0) += 1;
            }
            for g in &greps {
                for f in files {
                    if !g.scope.matches(f) || is_prose(f) {
                        continue;
                    }
                    let Ok(text) = std::fs::read(dir.join(f)) else {
                        continue;
                    };
                    let n = g.count(&String::from_utf8_lossy(&text));
                    out.insert((g.rule.id.clone(), f.clone()), n);
                }
            }
            out
        };
        let now = count(&self.root, &change.existing);
        let then = count(change.base_dir(), &athead);
        let keys: BTreeSet<&(String, String)> = now.keys().chain(then.keys()).collect();
        for k in keys {
            let (t, w) = (
                then.get(k).copied().unwrap_or(0),
                now.get(k).copied().unwrap_or(0),
            );
            let Some(a) = self.rules.iter().find(|a| a.rule.id == k.0) else {
                continue;
            };
            let mode = a.rule.enforce.iter().find_map(|t| match t {
                Target::AstGrep(x) => Some(x.mode),
                Target::Grep(x) => Some(x.mode),
                _ => None,
            });
            let bad = match mode {
                Some(RuleMode::NoIncrease) => w > t,
                Some(RuleMode::NoDecrease) => w < t,
                _ => false,
            };
            if bad {
                run.report.findings.push(
                    Finding::error(&k.0, format!("{} ({t} -> {w})", a.rule.description))
                        .at(k.1.clone(), None),
                );
                failed = true;
            }
        }
        failed
    }

    // ---------------------------------------------------------------- step 2: forbid

    /// Forbid rules (and, after an edit, advise rules) on the added lines.
    fn step_forbid(&self, run: &mut Run<'_>, edit: bool) -> bool {
        let change = &*run.change;
        let scope: Vec<String> = change.scope_files().to_vec();
        if scope.is_empty() {
            return false;
        }
        let mut notes = Vec::new();
        let wanted = |m: RuleMode| m == RuleMode::Forbid || (edit && m == RuleMode::Advise);
        let mut findings = Vec::new();
        for g in self
            .grep_rules(&mut notes)
            .into_iter()
            .filter(|g| wanted(g.target.mode))
        {
            for f in scope.iter().filter(|f| g.scope.matches(f) && !is_prose(f)) {
                let Ok(text) = std::fs::read(self.root.join(f)) else {
                    continue;
                };
                for n in g.matching_lines(&String::from_utf8_lossy(&text)) {
                    if change.on_added(f, n, n) {
                        findings.push(rule_finding(&g.rule, g.target.mode, f, n));
                    }
                }
            }
        }
        let langs: BTreeSet<&'static str> = scope
            .iter()
            .filter_map(|f| astgrep::file_language(f))
            .map(lang_key)
            .collect();
        let ast: Vec<Compiled> = self
            .ast_rules(&langs, Some(&scope), &mut notes)
            .into_iter()
            .filter(|c| wanted(c.mode()))
            .collect();
        let refs: Vec<&Compiled> = ast.iter().collect();
        let matches: Vec<AstMatch> = astgrep::scan_files(&refs, &self.root, &scope)
            .into_iter()
            .filter(|m| change.on_added(&m.path, m.line, m.end_line))
            .collect();
        let mut by_rule: BTreeMap<String, Vec<AstMatch>> = BTreeMap::new();
        for m in matches {
            by_rule.entry(m.rule.clone()).or_default().push(m);
        }
        for (id, ms) in by_rule {
            let Some(c) = ast.iter().find(|c| c.id() == id) else {
                continue;
            };
            let keep: Vec<AstMatch> = if c.typed() {
                match &self.types {
                    Some(t) => {
                        let (k, n) = t.confirm(&self.root, c, &ms);
                        notes.extend(n);
                        ms.into_iter()
                            .zip(k)
                            .filter(|(_, k)| *k)
                            .map(|(m, _)| m)
                            .collect()
                    }
                    None => {
                        notes.push(note(NoteKind::NotEnforced, &id, "not enforced: its matches need the types engine, which is not available"));
                        Vec::new()
                    }
                }
            } else {
                ms
            };
            for m in keep {
                findings.push(rule_finding(&c.rule, c.mode(), &m.path, m.line));
            }
        }
        run.report.notes.extend(notes);
        let failed = findings.iter().any(|f| f.severity == Severity::Error);
        for f in findings {
            if f.severity == Severity::Advice {
                run.advice.push(f);
            } else {
                run.report.findings.push(f);
            }
        }
        failed
    }

    // ---------------------------------------------------------------- step 3: check stages

    /// Runs the checks of a stage; (failed, only unverified tests).
    fn run_stage(&self, run: &mut Run<'_>, stage: Stage) -> (bool, bool) {
        let mut failed = false;
        let mut unverified = false;
        for c in self.checks().iter().filter(|c| c.target.stage == stage) {
            match self.run_check(run, c) {
                CheckResult::Pass => {}
                CheckResult::Fail(f) => {
                    run.report.findings.extend(f);
                    failed = true;
                }
                CheckResult::Unverified(f, u) => {
                    run.report.findings.extend(f);
                    run.unverified.extend(u);
                    unverified = true;
                }
                CheckResult::Advice(f) => run.advice.extend(f),
            }
        }
        (failed, unverified && !failed)
    }

    fn missing_tool(&self, c: &Check<'_>) -> Option<String> {
        for t in &c.tools {
            match c.run_dirs.as_deref() {
                Some(dirs) => {
                    for d in dirs {
                        if !tools::has_tool(self.tools.as_ref(), &self.root, &self.root.join(d), t)
                        {
                            return Some(format!("{} ({d})", tools::tool_label(&self.root, t)));
                        }
                    }
                }
                None => {
                    if !tools::has_tool(self.tools.as_ref(), &self.root, &self.root, t) {
                        return Some(tools::tool_label(&self.root, t));
                    }
                }
            }
        }
        None
    }

    /// One check under Stop or `forge check` (`forge_run_check_one`).
    fn run_check(&self, run: &mut Run<'_>, c: &Check<'_>) -> CheckResult {
        let change = &*run.change;
        let all = change.mode == Mode::All;
        let mode = change.mode.clone();
        let mut files = Vec::new();
        if !all {
            let when = c.target.when_changed.as_ref().map(|w| {
                GlobList::new(
                    &self
                        .roles
                        .expand(&crate::scope::in_dirs(w, c.scope_dirs.as_deref())),
                )
            });
            if let Some(w) = &when
                && !change.changed.iter().any(|f| w.any(f))
            {
                return CheckResult::Pass;
            }
            files = change
                .existing
                .iter()
                .filter(|f| c.scope.matches(f))
                .cloned()
                .collect();
            if when.is_none() && files.is_empty() {
                return CheckResult::Pass;
            }
        }
        if let Some(t) = self.missing_tool(c) {
            run.report.notes.push(note(
                NoteKind::MissingTool,
                c.id(),
                format!("skipped: {t} not installed"),
            ));
            return CheckResult::Pass;
        }
        if c.target.requires_local_db == Some(true) && !builtins::is_local_db(&self.root) {
            run.report.notes.push(note(
                NoteKind::Environment,
                c.id(),
                "skipped: no local database",
            ));
            return CheckResult::Pass;
        }
        let mut cmd = c.target.run.clone();
        let mut whole = true;
        if matches!(mode, Mode::Changed | Mode::Turn(_))
            && let Some(cr) = &c.target.changed_run
            && !files.is_empty()
        {
            cmd = cr.clone();
            whole = false;
        }
        let uses_files = !whole && cmd.contains("{files}");
        let (exit, out) = self.exec(run, c, &cmd, &files, uses_files, None);
        let advised = run.files.path(&advised_name(c.id())).exists();
        match exit {
            Exit::Status(0) => return CheckResult::Pass,
            Exit::Budget(s) => {
                run.report.notes.push(note(NoteKind::ForgeFailure, c.id(), format!("not checked: the run's time budget ran out after {s}s, before this check finished")));
                return CheckResult::Pass;
            }
            Exit::TimedOut(s) => {
                let mut f = Finding::error(
                    c.id(),
                    format!(
                        "timed out after {s}s and was stopped (a hung test, or a service that never answers?); its output until then:\n{}",
                        diagnostics::excerpt(&out, 15)
                    ),
                );
                f.message = f.message.trim_end().to_string();
                return CheckResult::Fail(vec![f]);
            }
            Exit::Failed(why) => {
                run.report.notes.push(note(
                    NoteKind::ForgeFailure,
                    c.id(),
                    format!("not checked: the check could not start ({why})"),
                ));
                return CheckResult::Pass;
            }
            Exit::Status(_) => {}
        }
        if advised {
            let mut f =
                diagnostics::findings_of(&diagnostics::excerpt(&out, 15), c.id(), &self.root);
            for x in &mut f {
                x.severity = Severity::Advice;
            }
            return CheckResult::Advice(f);
        }
        let level = diagnostics::level_of(c.target.stage, whole);
        let warn = c.target.fail_on_warnings == Some(true);
        if let Some(level) = level {
            let target = DiagTarget::of(&*run.change);
            let (what, lines) = diagnostics::attribute(&out, &self.root, &target, level, warn);
            if what == Attribution::Forge {
                run.report.notes.push(note(
                    NoteKind::Setup,
                    c.id(),
                    "ok: its findings are only in Forge's own files",
                ));
                return CheckResult::Pass;
            }
            if !all {
                match what {
                    Attribution::Other => {
                        let text = if level == Level::Line {
                            "ok for the added lines (findings remain on lines this change did not add)"
                        } else {
                            "ok for the changed files (errors remain in unchanged files)"
                        };
                        run.report
                            .notes
                            .push(note(NoteKind::Environment, c.id(), text));
                        return CheckResult::Pass;
                    }
                    Attribution::Warnings => {
                        run.report.notes.push(note(
                            NoteKind::Environment,
                            c.id(),
                            "ok: only warnings",
                        ));
                        return CheckResult::Pass;
                    }
                    Attribution::Changed => {
                        if c.target.stage == Stage::Typecheck {
                            return self
                                .typecheck_compare(run, c, &cmd, &files, uses_files, &out, &lines);
                        }
                        return CheckResult::Fail(cap(
                            diagnostics::findings_of(&lines.join("\n"), c.id(), &self.root),
                            15,
                            c.id(),
                        ));
                    }
                    _ => {}
                }
            }
        }
        if c.target.stage == Stage::Test
            && matches!(mode, Mode::Turn(_))
            && c.target.rerun.is_some()
        {
            return self.test_compare(run, c, &out);
        }
        let n = if c.target.stage == Stage::Test {
            25
        } else {
            15
        };
        CheckResult::Fail(self.failure_findings(c.id(), c.target.stage, &out, n))
    }

    /// An unattributed failure: its output as findings when it names locations, else the stage's
    /// header and an excerpt.
    fn failure_findings(&self, id: &str, stage: Stage, out: &str, n: usize) -> Vec<Finding> {
        let text = diagnostics::excerpt(out, n);
        let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
        if diagnostics::parse_location(first).is_some()
            || text.lines().filter(|l| !l.trim().is_empty()).count() <= 3 && !text.trim().is_empty()
        {
            return diagnostics::findings_of(&text, id, &self.root);
        }
        let body = if text.trim().is_empty() {
            "(it printed nothing)".to_string()
        } else {
            text
        };
        vec![Finding::error(
            id,
            format!("{}\n{body}", stage_header(stage)),
        )]
    }

    /// `forge_typecheck_compare`: errors in the changed files that the base already has are not
    /// this change's.
    #[allow(clippy::too_many_arguments)]
    fn typecheck_compare(
        &self,
        run: &mut Run<'_>,
        c: &Check<'_>,
        cmd: &str,
        files: &[String],
        uses_files: bool,
        out: &str,
        now_lines: &[String],
    ) -> CheckResult {
        let (now, all_now, base_name) = {
            let target = DiagTarget::of(&*run.change);
            (
                diagnostics::diag_keys(&now_lines.join("\n"), &self.root, &target),
                diagnostics::diag_keys(out, &self.root, &target),
                run.change.base_name(),
            )
        };
        let why;
        match crate::base_tree::build(&*run.change) {
            Err(w) => why = w,
            Ok(tree) => {
                let (exit, base_out) = self.exec(run, c, cmd, files, uses_files, Some(&tree));
                let target = DiagTarget::of(&*run.change);
                match exit {
                    Exit::Status(0) => return self.typecheck_result(c, &now, &all_now, &[]),
                    Exit::TimedOut(s) | Exit::Budget(s) => {
                        why = format!("the run there was stopped after {s}s")
                    }
                    Exit::Failed(e) => why = e,
                    Exit::Status(_) => {
                        let (what, _) = diagnostics::attribute(
                            &base_out,
                            tree.path(),
                            &target,
                            Level::File,
                            false,
                        );
                        if what == Attribution::None {
                            why = "the run there failed naming no file".into();
                        } else {
                            let base = diagnostics::diag_keys(&base_out, tree.path(), &target);
                            return self.typecheck_result(c, &now, &all_now, &base);
                        }
                    }
                }
            }
        }
        run.report.notes.push(note(NoteKind::Environment, c.id(), format!("its errors in the changed files could not be compared with {base_name}: {why}; all of them count")));
        CheckResult::Fail(cap(
            diagnostics::findings_of(&now_lines.join("\n"), c.id(), &self.root),
            15,
            c.id(),
        ))
    }

    fn typecheck_result(
        &self,
        c: &Check<'_>,
        now: &[diagnostics::DiagKey],
        all: &[diagnostics::DiagKey],
        base: &[diagnostics::DiagKey],
    ) -> CheckResult {
        let (new, old) = diagnostics::compare_with_base(now, all, base);
        if new.is_empty() {
            return CheckResult::Pass;
        }
        let mut f = cap(
            diagnostics::findings_of(&new.join("\n"), c.id(), &self.root),
            15,
            c.id(),
        );
        if old > 0 {
            f.push(Finding::error(c.id(), format!("({old} other error(s) in the changed files were already there before this change)")));
        }
        CheckResult::Fail(f)
    }

    /// `forge_test_compare` (§10.6).
    fn test_compare(&self, run: &mut Run<'_>, c: &Check<'_>, out: &str) -> CheckResult {
        let failing = failed_tests(out);
        if failing.is_empty() {
            return CheckResult::Fail(self.failure_findings(c.id(), Stage::Test, out, 25));
        }
        let mut why = None;
        let mut base_failing = BTreeSet::new();
        match crate::base_tree::build(run.change) {
            Err(w) => why = Some(w),
            Ok(tree) => {
                let files: Vec<String> = test_files(&failing);
                let rerun = c.target.rerun.clone().unwrap_or_default();
                let (exit, base_out) = self.exec(run, c, &rerun, &files, true, Some(&tree));
                base_failing = failed_tests(&base_out).into_iter().collect();
                match exit {
                    Exit::TimedOut(s) | Exit::Budget(s) => {
                        why = Some(format!("the re-run there was stopped after {s}s"))
                    }
                    Exit::Failed(e) => why = Some(e),
                    Exit::Status(st) if st != 0 && base_failing.is_empty() => {
                        why = Some("the re-run there failed without naming a failing test".into())
                    }
                    _ => {}
                }
            }
        }
        let list = |v: &[&String]| {
            v.iter()
                .take(8)
                .map(|t| format!("  {t}"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        if let Some(why) = why {
            run.report.notes.push(note(
                NoteKind::Environment,
                c.id(),
                format!("comparison with the start of the turn was not possible ({why})"),
            ));
            let all: Vec<&String> = failing.iter().collect();
            return CheckResult::Fail(vec![Finding::error(
                c.id(),
                format!(
                    "{} failing test(s):\n{}\n{}",
                    failing.len(),
                    list(&all),
                    diagnostics::excerpt(out, 20)
                ),
            )]);
        }
        let new: Vec<&String> = failing
            .iter()
            .filter(|t| !base_failing.contains(*t))
            .collect();
        let old: Vec<&String> = failing
            .iter()
            .filter(|t| base_failing.contains(*t))
            .collect();
        if !new.is_empty() {
            let mut msg = format!(
                "{} failing test(s) passed at the start of the turn:\n{}",
                new.len(),
                list(&new)
            );
            if !old.is_empty() {
                msg.push_str(&format!("\n({} other failing test(s) also fail at the start of the turn: not caused by this change)", old.len()));
            }
            msg.push('\n');
            msg.push_str(&diagnostics::excerpt(out, 20));
            return CheckResult::Fail(vec![Finding::error(c.id(), msg)]);
        }
        let env = env_errors(out);
        let mut msg = format!(
            "{} failing test(s) also fail at the start of the turn (re-run there), so this change did not cause them, but it is not verified by them either:\n{}\n",
            old.len(),
            list(&old)
        );
        let compose = [
            "compose.yaml",
            "compose.yml",
            "docker-compose.yml",
            "docker-compose.yaml",
        ]
        .into_iter()
        .find(|f| self.root.join(f).is_file());
        if !env.is_empty() {
            msg.push_str("Their errors look environmental:\n");
            for e in &env {
                msg.push_str(&format!("  {e}\n"));
            }
            match compose {
                Some(f) => msg.push_str(&format!("Set up what they need (e.g. docker compose up -d with {f}) and run them again. ")),
                None => msg.push_str("Set up what they need (the services, the network) and run them again. "),
            }
        } else {
            msg.push_str(
                "Check whether they relate to the task: fix them if so, and run them again. ",
            );
        }
        msg.push_str("If that is not possible, stop again without changing files: the stop is then allowed and the user is told these tests could not be verified.\n");
        msg.push_str(&diagnostics::excerpt(out, 12));
        let mut unverified: Vec<String> = old
            .iter()
            .map(|t| format!("test\t{} {t}", c.id()))
            .collect();
        unverified.extend(env.iter().map(|e| format!("env\t{e}")));
        CheckResult::Unverified(vec![Finding::error(c.id(), msg)], unverified)
    }

    /// Runs one of a check's commands, in each workspace package when it has some; in the base
    /// tree when given. Returns how it ended and its output.
    fn exec(
        &self,
        run: &mut Run<'_>,
        c: &Check<'_>,
        cmd: &str,
        files: &[String],
        uses_files: bool,
        tree: Option<&crate::base_tree::BaseTree>,
    ) -> (Exit, String) {
        let root: PathBuf = tree
            .map(|t| t.path().to_path_buf())
            .unwrap_or_else(|| self.root.clone());
        // a function body, as check.sh rendered each command (`return` works in it)
        let script = format!(
            "{}\nforge_check_cmd() {{\n{}\n}}\nforge_check_cmd",
            shell::preamble(c.id(), &c.tools),
            shell::fill(cmd)
        );
        let mut env = self.env_for(run, c);
        if let Some(t) = tree
            && !t.pythonpath.is_empty()
        {
            let mut pp: Vec<PathBuf> = t.pythonpath.clone();
            if let Some(old) = std::env::var_os("PYTHONPATH") {
                pp.extend(std::env::split_paths(&old));
            }
            env.insert(
                "PYTHONPATH".into(),
                std::env::join_paths(pp).unwrap_or_default(),
            );
            env.insert(ENV_ROOT.into(), root.as_os_str().to_owned());
        }
        let timeout = c.target.timeout.map(|s| Duration::from_secs(s as u64));
        let dirs: Vec<Option<&String>> = match c.run_dirs.as_deref() {
            Some(d) => d.iter().map(Some).collect(),
            None => vec![None],
        };
        let mut status = Exit::Status(0);
        let mut output = String::new();
        for d in dirs {
            let (cwd, fs, mut env) = match d {
                Some(d) => {
                    let prefix = format!("{d}/");
                    let fs: Vec<String> = files
                        .iter()
                        .filter_map(|f| f.strip_prefix(&prefix).map(str::to_string))
                        .collect();
                    if uses_files && fs.is_empty() {
                        continue;
                    }
                    let mut env = env.clone();
                    env.insert(builtins::ENV_WS_ROOT.into(), root.as_os_str().to_owned());
                    (root.join(d), fs, env)
                }
                None => (root.clone(), files.to_vec(), env.clone()),
            };
            env.insert(builtins::ENV_FILES.into(), OsString::from(fs.join("\n")));
            let budget = run.deadline.saturating_duration_since(Instant::now());
            let r = shell::run(&Spec {
                script: script.clone(),
                cwd,
                env,
                stdin: None,
                timeout,
                budget: Some(budget),
            });
            run.tool_time += r.took;
            let text = match d {
                Some(d) => diagnostics::prefix_paths(&r.output, d),
                None => r.output,
            };
            output.push_str(&text);
            match r.exit {
                Exit::Status(0) => {}
                e @ Exit::Status(_) => {
                    if status == Exit::Status(0) {
                        status = e;
                    }
                }
                e => {
                    status = e;
                    break;
                }
            }
        }
        // notes the builtins wrote, and the old protocol's "forge:<id> …" lines
        run.report
            .notes
            .extend(take_notes(&run.files.path("notes")));
        let mut kept = Vec::new();
        for l in output.lines() {
            let plain = crate::adapters::strip_ansi(l);
            if let Some(rest) = plain.strip_prefix("forge:") {
                let (rule, text) = rest.split_once(' ').unwrap_or((rest, ""));
                let text = text.trim();
                if !text.is_empty() && !text.starts_with("timed out") {
                    run.report
                        .notes
                        .push(note(NoteKind::NotEnforced, rule, text.to_string()));
                }
                continue;
            }
            kept.push(l);
        }
        let mut out = kept.join("\n");
        if !out.is_empty() {
            out.push('\n');
        }
        (status, out)
    }

    /// The environment of a check's commands (forge-core's protocol and the checker's).
    fn env_for(&self, run: &Run<'_>, c: &Check<'_>) -> BTreeMap<String, OsString> {
        let change = &*run.change;
        let mut env = shell::base_env(&run.opts.exe);
        let s = |p: &Path| p.as_os_str().to_owned();
        env.insert(ENV_ROOT.into(), s(&self.root));
        env.insert(ENV_RULE.into(), c.id().into());
        let scope = match change.mode.scope() {
            Scope::All => "all",
            Scope::Changed => "changed",
            Scope::Turn => "turn",
        };
        env.insert(ENV_SCOPE.into(), scope.into());
        env.insert(
            builtins::ENV_MODE.into(),
            match &change.mode {
                Mode::All => "all",
                Mode::Changed => "changed",
                Mode::Since(_) => "since",
                Mode::Turn(_) => "turn",
            }
            .into(),
        );
        if change.mode != Mode::All {
            env.insert(ENV_BASE.into(), s(change.base_dir()));
            env.insert(ENV_CHANGED.into(), s(&run.files.path("changed")));
        }
        env.insert(ENV_NOTES.into(), s(&run.files.path("notes")));
        env.insert(builtins::ENV_TMP.into(), s(run.files.dir.path()));
        // a missing pinned tool may be installed at Stop and `forge check`, never after an edit
        if run.opts.phase == Phase::Stop {
            env.insert(tools::ENV_DOWNLOAD.into(), "1".into());
        }
        let accepted: Vec<&str> = self
            .accept
            .iter()
            .filter(|a| a.rule == c.id())
            .map(|a| a.change.as_str())
            .collect();
        let acc = run.files.path(&format!("accepted.{}", safe(c.id())));
        let _ = std::fs::write(&acc, accepted.join("\n"));
        env.insert(ENV_ACCEPTED.into(), s(&acc));
        if let Some(snap) = &change.snapshot {
            env.insert(builtins::ENV_SNAPSHOT.into(), s(snap));
        }
        env.insert(
            builtins::ENV_ADVISED.into(),
            s(&run.files.path(&advised_name(c.id()))),
        );
        env.insert(builtins::ENV_ROLES.into(), s(&run.files.path("roles.json")));
        env.insert(builtins::ENV_ADDED.into(), s(&run.files.path("added")));
        env.insert(builtins::ENV_CHECKS.into(), s(&run.files.path("checks")));
        let rule_file = run.files.path(&format!("rule.{}.json", safe(c.id())));
        if !rule_file.exists() {
            let _ = std::fs::write(
                &rule_file,
                serde_json::to_string(&CheckRule {
                    rule: c.active.rule.clone(),
                    dirs: c.scope_dirs.clone(),
                })
                .unwrap_or_default(),
            );
        }
        env.insert(builtins::ENV_CHECK_RULE.into(), s(&rule_file));
        env
    }

    // ---------------------------------------------------------------- added lines and formatting

    /// `forge_format_copy`: `content` formatted as `path` would be, by the `format_stdin` filter
    /// of each check covering it, in check order; None when no filter applies or one fails.
    fn format_copy(&self, run: &mut Run<'_>, path: &str, content: &[u8]) -> Option<Vec<u8>> {
        let checks = self.checks();
        let mut cur = content.to_vec();
        let mut any = false;
        for c in checks
            .iter()
            .filter(|c| c.target.format_stdin.is_some() && c.scope.matches(path))
        {
            if self.missing_tool(c).is_some() {
                return None;
            }
            let cmd = c.target.format_stdin.clone().unwrap_or_default();
            let script = format!(
                "{}\n{}",
                shell::preamble(c.id(), &c.tools),
                shell::fill(&cmd)
            );
            let mut env = self.env_for(run, c);
            env.insert("FORGE_FILE".into(), path.into());
            let r = shell::run(&Spec {
                script,
                cwd: self.root.clone(),
                env,
                stdin: Some(cur.clone()),
                timeout: Some(Duration::from_secs(30)),
                budget: Some(run.deadline.saturating_duration_since(Instant::now())),
            });
            run.tool_time += r.took;
            let _ = take_notes(&run.files.path("notes"));
            if !r.exit.ok() || (r.output.is_empty() && !cur.is_empty()) {
                return None;
            }
            cur = r.output.into_bytes();
            any = true;
        }
        any.then_some(cur)
    }

    /// `forge_added_lines`: a line counts when it is added against the base copy and against the
    /// base copy formatted as the file would be, so a whole-file format does not make untouched
    /// lines the change's. Formatted base copies are kept in the turn snapshot.
    fn format_added_lines(&self, run: &mut Run<'_>) {
        if run.change.mode == Mode::All
            || !self
                .checks()
                .iter()
                .any(|c| c.target.format_stdin.is_some())
        {
            return;
        }
        let todo: Vec<(String, BTreeSet<u32>)> = run
            .change
            .added
            .iter()
            .filter_map(|(p, a)| match a {
                Added::Lines(l) if !l.is_empty() => Some((p.clone(), l.clone())),
                _ => None,
            })
            .collect();
        if let Some(s) = &run.change.snapshot {
            invalidate_format_cache(&self.root, s);
        }
        for (path, raw) in todo {
            let Some(base) = run.change.base_copy(&path) else {
                continue;
            };
            let base_bytes = std::fs::read(&base).unwrap_or_default();
            let cache = run.change.snapshot.clone();
            let formatted = match cache.as_ref().map(|s| cached_format(s, &path)) {
                Some(Some(Cached::Same)) => continue,
                Some(Some(Cached::Unformattable)) => continue,
                Some(Some(Cached::Formatted(b))) => b,
                _ => {
                    let f = self.format_copy(run, &path, &base_bytes);
                    if let Some(s) = &cache {
                        store_format(s, &path, f.as_deref(), &base_bytes);
                    }
                    match f {
                        Some(f) if f != base_bytes => f,
                        _ => continue,
                    }
                }
            };
            let now = std::fs::read(self.root.join(&path)).unwrap_or_default();
            let now_text = String::from_utf8_lossy(&now).into_owned();
            let fmt = added_lines(&String::from_utf8_lossy(&formatted), &now_text);
            let text: Vec<&str> = now_text.lines().collect();
            let line = |n: u32| text.get(n as usize - 1).copied().unwrap_or("");
            let mut left: BTreeMap<&str, i64> = BTreeMap::new();
            for l in &fmt {
                *left.entry(line(*l)).or_default() += 1;
            }
            let mut keep = BTreeSet::new();
            for n in &raw {
                if fmt.contains(n) {
                    keep.insert(*n);
                    *left.entry(line(*n)).or_default() -= 1;
                }
            }
            for n in &raw {
                if !keep.contains(n)
                    && let Some(k) = left.get_mut(line(*n))
                    && *k > 0
                {
                    *k -= 1;
                    keep.insert(*n);
                }
            }
            run.change.set_added(&path, Added::Lines(keep));
        }
    }
}

enum Cached {
    Same,
    Unformattable,
    Formatted(Vec<u8>),
}

const FORMATTER_CONFIGS: &[&str] = &[
    "pyproject.toml",
    "ruff.toml",
    ".ruff.toml",
    "biome.json",
    "biome.jsonc",
    ".prettierrc",
    ".prettierrc.json",
    ".prettierrc.yaml",
    ".prettierrc.yml",
    ".prettierrc.toml",
    "prettier.config.js",
    "prettier.config.mjs",
    ".editorconfig",
    "package.json",
    "dprint.json",
];

fn cached_format(snap: &Path, path: &str) -> Option<Cached> {
    if !snap.join("format.stamp").is_file() {
        return None;
    }
    if snap.join("same").join(path).is_file() {
        return Some(Cached::Same);
    }
    if snap.join("unformattable").join(path).is_file() {
        return Some(Cached::Unformattable);
    }
    std::fs::read(snap.join("formatted").join(path))
        .ok()
        .map(Cached::Formatted)
}

fn store_format(snap: &Path, path: &str, formatted: Option<&[u8]>, base: &[u8]) {
    let _ = std::fs::write(snap.join("format.stamp"), b"");
    let (dir, bytes): (&str, &[u8]) = match formatted {
        None => ("unformattable", b""),
        Some(f) if f == base => ("same", b""),
        Some(f) => ("formatted", f),
    };
    let to = snap.join(dir).join(path);
    if let Some(d) = to.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(to, bytes);
}

/// Drops the formatted base copies when a formatter's config at the root is newer than them.
pub fn invalidate_format_cache(root: &Path, snap: &Path) {
    let Ok(t) = std::fs::metadata(snap.join("format.stamp")).and_then(|m| m.modified()) else {
        return;
    };
    let newer = FORMATTER_CONFIGS.iter().any(|f| {
        std::fs::metadata(root.join(f))
            .and_then(|m| m.modified())
            .is_ok_and(|m| m >= t)
    });
    if newer {
        for d in ["formatted", "unformattable", "same"] {
            let _ = std::fs::remove_dir_all(snap.join(d));
        }
        let _ = std::fs::remove_file(snap.join("format.stamp"));
    }
}

fn lang_key(l: SupportLang) -> &'static str {
    match l {
        SupportLang::Python => "python",
        SupportLang::Yaml => "yaml",
        _ => "tsx",
    }
}

fn safe(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn advised_name(id: &str) -> String {
    format!("advised.{}", safe(id))
}

fn rule_finding(rule: &Rule, mode: RuleMode, path: &str, line: u32) -> Finding {
    let f = if mode == RuleMode::Advise {
        Finding::advice(&rule.id, rule.description.clone())
    } else {
        Finding::error(&rule.id, rule.description.clone())
    };
    f.at(path.to_string(), Some(line))
}

fn auto_fixed(path: &str, codes: Option<&str>) -> Finding {
    let what = match codes {
        Some(c) if !c.is_empty() => format!("safe fixes were applied ({c})"),
        _ => "safe lint fixes were applied".to_string(),
    };
    Finding::advice(
        "forge/auto-fix",
        format!("{what}; re-read the file before editing it again"),
    )
    .at(path.to_string(), None)
}

fn fix_linted_or_fixed(checks: &[Check<'_>], f: &str) -> bool {
    checks
        .iter()
        .any(|c| c.target.stage == Stage::Lint && c.target.fix_run.is_some() && c.scope.matches(f))
}

/// ruff's `--show-fixes` summary: "Fixed N errors:", "- <path>:", "    1 × I001 (unsorted-imports)".
fn ruff_fixes(out: &str) -> Vec<(String, String)> {
    let code = regex::Regex::new(r"([A-Z]+[0-9]+) \(").expect("regex");
    let mut on = false;
    let mut file = String::new();
    let mut res = Vec::new();
    for l in out.lines() {
        if l.starts_with("Fixed ") && l.contains(" error") {
            on = true;
            continue;
        }
        if on && l.starts_with("- ") && l.ends_with(':') {
            file = l[2..l.len() - 1].to_string();
            continue;
        }
        if on
            && l.starts_with(' ')
            && let Some(c) = code.captures(l)
        {
            res.push((file.clone(), c[1].to_string()));
        }
        if l.is_empty() {
            on = false;
        }
    }
    res
}

fn cap(mut f: Vec<Finding>, n: usize, rule: &str) -> Vec<Finding> {
    if f.len() > n {
        let more = f.len() - n;
        f.truncate(n);
        f.push(Finding::error(rule, format!("... and {more} more")));
    }
    f
}

fn take_notes(file: &Path) -> Vec<Note> {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    if !text.is_empty() {
        let _ = std::fs::write(file, "");
    }
    text.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn dedupe_notes(notes: &mut Vec<Note>) {
    let mut seen = BTreeSet::new();
    notes.retain(|n| seen.insert(format!("{n}")));
}

/// `forge_failed_tests`: the failing tests a run reports (Vitest `FAIL` lines, pytest's short
/// summary `FAILED` / `ERROR`), sorted.
pub fn failed_tests(out: &str) -> Vec<String> {
    let vitest = regex::Regex::new(r"^ *FAIL +(.*)$").expect("regex");
    let vitest_tail = regex::Regex::new(r" \[ .* \]$").expect("regex");
    let pytest = regex::Regex::new(r"^(FAILED|ERROR) ([^ ].*)$").expect("regex");
    let mut out_v = BTreeSet::new();
    for l in out.lines() {
        let l = crate::adapters::strip_ansi(l);
        let t = if let Some(c) = vitest.captures(&l) {
            vitest_tail.replace(&c[1], "").into_owned()
        } else if let Some(c) = pytest.captures(&l) {
            c[2].split(" - ").next().unwrap_or("").to_string()
        } else {
            continue;
        };
        let t = t.trim_end().to_string();
        if !t.is_empty() {
            out_v.insert(t);
        }
    }
    out_v.into_iter().collect()
}

/// `forge_test_files`: the test file of each failing test id.
pub fn test_files(ids: &[String]) -> Vec<String> {
    let proj = regex::Regex::new(r"^\|[^|]*\| ").expect("regex");
    let mut out: BTreeSet<String> = BTreeSet::new();
    for id in ids {
        let t = proj.replace(id, "");
        let t = t.split(" > ").next().unwrap_or("");
        let t = t.split("::").next().unwrap_or("");
        out.insert(t.to_string());
    }
    out.into_iter().collect()
}

/// `forge_env_errors`: the errors in a run's output that point at the environment, as short
/// phrases, at most 5.
pub fn env_errors(out: &str) -> Vec<String> {
    let svc = |port: &str| match port {
        "5432" => "Postgres",
        "3306" => "MySQL",
        "6379" => "Redis",
        "27017" => "MongoDB",
        "1433" => "SQL Server",
        "9200" => "Elasticsearch",
        "5672" => "RabbitMQ",
        "11211" => "Memcached",
        _ => "a service",
    };
    let refused = regex::Regex::new(r"ECONNREFUSED ([^ ,;)]+:([0-9]+))").expect("regex");
    let port = regex::Regex::new(r"port ([0-9]+) failed: Connection refused").expect("regex");
    let dns = regex::Regex::new(r#"(EAI_AGAIN|ENOTFOUND) ([^ ,;)'"]+)"#).expect("regex");
    let other = regex::Regex::new(r"[Cc]onnection refused|ECONNRESET|ETIMEDOUT|EHOSTUNREACH|ENETUNREACH|getaddrinfo|[Cc]ould not connect|[Tt]imed out|[Tt]imeout of [0-9]+ ?ms|Name or service not known|Temporary failure in name resolution").expect("regex");
    let mut seen = Vec::new();
    for l in out.lines() {
        if l.starts_with("forge: timed out") || l.starts_with("FAILED ") || l.starts_with("ERROR ")
        {
            continue;
        }
        let p = if let Some(c) = refused.captures(l) {
            format!(
                "{} refused the connection at {} (ECONNREFUSED)",
                svc(&c[2]),
                &c[1]
            )
        } else if let Some(c) = port.captures(l) {
            format!("{} refused the connection on port {}", svc(&c[1]), &c[1])
        } else if let Some(c) = dns.captures(l) {
            format!("DNS lookup for {} failed ({}): no network?", &c[2], &c[1])
        } else if other.is_match(l) {
            let t = l.trim();
            if t.chars().count() > 160 {
                format!("{}...", t.chars().take(157).collect::<String>())
            } else {
                t.to_string()
            }
        } else {
            continue;
        };
        if !seen.contains(&p) {
            seen.push(p);
        }
    }
    seen.truncate(5);
    seen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failing_test_ids() {
        let out = " FAIL  src/a.test.ts > math > adds\nFAILED tests/test_a.py::test_x - assert 1 == 2\nERROR tests/test_b.py\n";
        let f = failed_tests(out);
        assert_eq!(
            f,
            [
                "src/a.test.ts > math > adds",
                "tests/test_a.py::test_x",
                "tests/test_b.py"
            ]
        );
        assert_eq!(
            test_files(&f),
            ["src/a.test.ts", "tests/test_a.py", "tests/test_b.py"]
        );
    }

    #[test]
    fn environmental_errors() {
        let e = env_errors(
            "Error: connect ECONNREFUSED 127.0.0.1:5432\ngetaddrinfo EAI_AGAIN db.local\n",
        );
        assert_eq!(
            e[0],
            "Postgres refused the connection at 127.0.0.1:5432 (ECONNREFUSED)"
        );
        assert_eq!(
            e[1],
            "DNS lookup for db.local failed (EAI_AGAIN): no network?"
        );
    }

    #[test]
    fn ruff_fix_summary() {
        let out = "Fixed 2 errors:\n- app/a.py:\n    1 × I001 (unsorted-imports)\n    1 × UP006 (non-pep585-annotation)\n\nFound 2 errors (2 fixed, 0 remaining).\n";
        assert_eq!(
            ruff_fixes(out),
            [
                ("app/a.py".to_string(), "I001".to_string()),
                ("app/a.py".to_string(), "UP006".to_string())
            ]
        );
    }
}
