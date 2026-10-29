//! `forge hook <event>` (what the settings' hook entries run) and the hidden `forge engine`
//! commands the SessionStart hook starts the types engine with.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Subcommand;
use forge_check::Status;
use forge_core::{Note, NoteKind, Report};
use forge_hooks::{Checker, Env, Runtime, StopOutcome, StopStatus, TestBlock, TestEditRule};
use forge_packs::model::Target;

#[derive(Subcommand, Debug)]
pub enum EngineCommand {
    /// Runs the types engine daemon with the configuration file the checker wrote.
    Serve {
        #[arg(long)]
        config: PathBuf,
    },
    /// Makes sure the engine of a repository runs and registers a session (detached, from SessionStart).
    Start {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        session: String,
    },
}

/// The checker the hooks run: forge-check on the rules `forge apply` selected (cached in the
/// state directory, see [`crate::check::select_cached`]).
pub struct HookChecker {
    /// This executable (check commands call its builtins; the engine is started with it).
    pub exe: PathBuf,
}

fn forge_failure(what: String) -> Report {
    Report {
        findings: Vec::new(),
        notes: vec![Note::new(NoteKind::ForgeFailure, what)],
    }
}

impl Checker for HookChecker {
    fn post_edit(&self, root: &Path, files: &[String], snapshot: Option<&Path>) -> Report {
        let selected = match crate::check::select_cached(root) {
            Ok(s) => s,
            Err(e) => return forge_failure(format!("The checks after an edit did not run: {e}.")),
        };
        let mut checker = crate::check::checker_of(root, selected, false);
        let mut notes = crate::check::with_engine(&mut checker, &self.exe, false);
        let mut report = match checker.check_files(
            files,
            snapshot,
            &forge_check::Options::post_edit(self.exe.clone()),
        ) {
            Ok(o) => o.report,
            Err(e) => forge_failure(format!("The checks after an edit did not run: {e}.")),
        };
        report.notes.append(&mut notes);
        report
    }

    fn stop(&self, root: &Path, snapshot: Option<&Path>) -> StopOutcome {
        let selected = match crate::check::select_cached(root) {
            Ok(s) => s,
            Err(e) => {
                return StopOutcome {
                    report: forge_failure(format!("The Stop check did not run: {e}.")),
                    status: StopStatus::Pass,
                };
            }
        };
        let mut checker = crate::check::checker_of(root, selected, true);
        let mut notes = crate::check::with_engine(&mut checker, &self.exe, true);
        let mode = match snapshot {
            Some(s) => forge_check::Mode::Turn(s.to_path_buf()),
            None => forge_check::Mode::Changed,
        };
        let mut outcome = match checker.check(mode, &forge_check::Options::stop(self.exe.clone())) {
            Ok(o) => StopOutcome {
                report: o.report,
                status: match o.status {
                    Status::Pass => StopStatus::Pass,
                    Status::Fail => StopStatus::Fail,
                    Status::Unverified => StopStatus::Unverified,
                    Status::AdviceOnly => StopStatus::AdviceOnly,
                },
            },
            Err(e) => StopOutcome {
                report: forge_failure(format!("The Stop check did not run: {e}.")),
                status: StopStatus::Pass,
            },
        };
        outcome.report.notes.append(&mut notes);
        outcome
    }

    fn test_edit_rule(&self, root: &Path, path: &str) -> Option<TestEditRule> {
        let selected = crate::check::select_cached(root).ok()?;
        let roles = forge_check::scope::Roles::from_map(selected.roles);
        selected.rules.iter().find_map(|a| {
            a.rule.enforce.iter().find_map(|t| {
                let Target::Check(c) = t else { return None };
                if !c.commands().any(|cmd| cmd.contains("forge_test_edits")) {
                    return None;
                }
                let scope = forge_check::scope::RuleScope::new(
                    &a.rule,
                    &roles,
                    a.dirs.as_deref(),
                    c.config_files.as_deref(),
                );
                scope.matches(path).then(|| TestEditRule {
                    id: a.rule.id.clone(),
                    review: a.rule.review.clone(),
                })
            })
        })
    }

    fn test_blocks(&self, path: &str, source: &str) -> Vec<TestBlock> {
        forge_check::builtins::blocks(path, source)
            .into_iter()
            .map(|b| TestBlock {
                key: b.key,
                print: b.print,
            })
            .collect()
    }
}

/// Answers one hook call: the hook input on stdin, the answer on stdout (stderr and exit 2 for
/// the asyncRewake typecheck). Anything else exits 0 silently: Forge's own failures never block.
pub fn run(event: &str, launcher_notes: Vec<forge_launcher::Note>) -> ExitCode {
    // a panic message must never reach the agent
    std::panic::set_hook(Box::new(|_| {}));
    let Ok(home) = forge_home::Home::from_env() else {
        return ExitCode::SUCCESS;
    };
    let mut stdin = Vec::new();
    let _ = std::io::stdin().lock().read_to_end(&mut stdin);
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("claude-forge"));
    let checker = HookChecker { exe: exe.clone() };
    let rt = Runtime {
        home,
        env: Env::from_process(),
        checker: &checker,
        exe,
        launcher_notes,
    };
    let out = forge_hooks::run(&rt, event, &stdin);
    if !out.stdout.is_empty() {
        let _ = std::io::stdout().lock().write_all(out.stdout.as_bytes());
    }
    if !out.stderr.is_empty() {
        let _ = std::io::stderr().lock().write_all(out.stderr.as_bytes());
    }
    ExitCode::from(out.code)
}

pub fn engine(cmd: EngineCommand) -> ExitCode {
    match cmd {
        EngineCommand::Serve { config } => crate::check::engine_serve(&config),
        EngineCommand::Start { root, session } => {
            let Ok(home) = forge_home::Home::from_env() else {
                return ExitCode::SUCCESS;
            };
            let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("claude-forge"));
            let notes = crate::check::engine_start(&root, &session, &exe);
            if let Ok(id) = forge_home::RepoId::for_root(&root) {
                forge_hooks::state::record_notes(&home.repo_state_dir(&id), &notes);
            }
            ExitCode::SUCCESS
        }
    }
}
