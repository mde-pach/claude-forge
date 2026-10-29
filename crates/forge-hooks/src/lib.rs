//! `forge hook <event>`: one process per Claude Code hook call. It reads the hook input on stdin
//! and answers on stdout (and, for the asyncRewake typecheck, stderr with exit 2), per the Claude
//! Code hooks protocol.
//!
//! | Event                      | Does                                                              |
//! |----------------------------|-------------------------------------------------------------------|
//! | `session-start`            | context lines (branch, changes, commits); uv on PATH; engine start |
//! | `user-prompt-submit`       | turn snapshot, Stop counter reset, engine told of changed files    |
//! | `pre-tool-use`             | internals refusal; Bash: guard, then Bash snapshot; test-edit ask  |
//! | `post-tool-use[-failure]`  | the edit-time checks on the files the tool changed                 |
//! | `post-tool-use-typecheck`  | asyncRewake: the engine's new diagnostics; exit 2 when any         |
//! | `stop`                     | the turn's checks; block with the findings                         |
//! | `permission-request`       | an independent reviewer when nobody can answer                     |
//! | `session-end`              | unregisters from the engine                                        |
//! | `user-pre-tool-use`        | the user-level guard                                               |
//! | `user-permission-request`  | the reviewer, user level                                           |
//! | `user-notification`        | a terminal notification                                            |
//!
//! What the agent reads is findings and refusal reasons only: never Forge's internals and never
//! notes. Notes go to the state directory for `forge status`. A failure of Forge itself never
//! blocks or denies anything: the hook then says nothing.

pub mod checker;
pub mod engine;
mod events;
mod guard;
pub mod input;
pub mod proc;
mod review;
pub mod rules;
pub mod snapshot;
pub mod state;
mod testedit;

use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::time::Duration;

use forge_home::{Home, RepoId};
use serde_json::{Value, json};

pub use checker::{Checker, NoChecker, StopOutcome, StopStatus, TestBlock, TestEditRule};
pub use input::HookInput;

/// The reason the agent reads when it reaches for Forge's own files.
pub const INTERNAL_REASON: &str = "Internal files of Forge, this project's quality tooling, used by Claude Code hooks; its checks run automatically on your edits; nothing here is needed for your task.";

/// The events `forge hook` answers.
pub const EVENTS: &[&str] = &[
    "session-start",
    "user-prompt-submit",
    "pre-tool-use",
    "post-tool-use",
    "post-tool-use-failure",
    "post-tool-use-typecheck",
    "stop",
    "permission-request",
    "session-end",
    "user-pre-tool-use",
    "user-permission-request",
    "user-notification",
];

/// What the hooks read from the environment (kept apart from `std::env` for tests).
#[derive(Debug, Clone, Default)]
pub struct Env {
    /// `CLAUDE_CODE_ENTRYPOINT`: `cli`, `sdk-cli` (claude -p), `sdk-ts`, `sdk-py`, …
    pub entrypoint: Option<String>,
    /// `FORGE_ASK_REVIEW`: `always`, `never`, else auto.
    pub ask_review: Option<String>,
    /// Set inside a review: never review again.
    pub in_review: bool,
    pub review_model: Option<String>,
    /// The `claude` executable the reviewer runs.
    pub claude: PathBuf,
    /// `CLAUDE_ENV_FILE` (SessionStart).
    pub env_file: Option<PathBuf>,
    pub term_program: Option<String>,
    /// `$HOME`.
    pub home_dir: Option<PathBuf>,
    /// `CLAUDE_PROJECT_DIR`.
    pub project_dir: Option<PathBuf>,
    /// `FORGE_EDIT_TYPECHECK`: `always`, `never`, else auto.
    pub edit_typecheck: Option<String>,
    /// The longest a Bash snapshot may take.
    pub snapshot_cap: Duration,
    /// Start the types engine at SessionStart (`FORGE_ENGINE=off`: never).
    pub engine: bool,
}

impl Env {
    pub fn from_process() -> Env {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        Env {
            entrypoint: var("CLAUDE_CODE_ENTRYPOINT"),
            ask_review: var("FORGE_ASK_REVIEW"),
            in_review: var("FORGE_REVIEWER").is_some(),
            review_model: var("FORGE_REVIEW_MODEL"),
            claude: PathBuf::from("claude"),
            env_file: var("CLAUDE_ENV_FILE").map(PathBuf::from),
            term_program: var("TERM_PROGRAM"),
            home_dir: var("HOME").map(PathBuf::from),
            project_dir: var("CLAUDE_PROJECT_DIR").map(PathBuf::from),
            edit_typecheck: var("FORGE_EDIT_TYPECHECK"),
            snapshot_cap: Duration::from_secs(
                var("FORGE_SNAPSHOT_TIMEOUT")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(3),
            ),
            engine: var("FORGE_ENGINE").as_deref() != Some("off"),
        }
    }
}

/// Everything a hook runs with.
pub struct Runtime<'a> {
    pub home: Home,
    pub env: Env,
    pub checker: &'a dyn Checker,
    /// This executable (the engine is started with it).
    pub exe: PathBuf,
    /// What the launcher noted before the hook ran (a pinned version not downloaded yet).
    pub launcher_notes: Vec<forge_launcher::Note>,
}

/// A hook's answer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
    pub code: u8,
}

impl Output {
    pub fn silent() -> Output {
        Output::default()
    }

    pub fn json(v: Value) -> Output {
        Output {
            stdout: format!("{v}\n"),
            ..Output::default()
        }
    }
}

/// The repository a project-level hook works on.
pub(crate) struct Project {
    pub root: PathBuf,
    pub id: RepoId,
    /// `<state>/<repo-id>`.
    pub state: PathBuf,
    pub session: String,
    /// `<state>/<repo-id>/sessions/<session>`.
    pub session_dir: PathBuf,
}

impl Project {
    pub fn turn_dir(&self) -> PathBuf {
        self.session_dir.join("turn")
    }

    /// The turn snapshot, when the turn has one.
    pub fn snapshot(&self) -> Option<PathBuf> {
        let t = self.turn_dir();
        t.join("head").is_file().then_some(t)
    }

    pub fn source(&self, home: &Home) -> rules::Source<'_> {
        rules::Source {
            lock: self.root.join(forge_home::LOCK_FILE),
            repo: Some(&self.root),
            packs_cache: Some(home.packs_dir()),
        }
    }

    pub fn rules(&self, home: &Home) -> Option<rules::HookCache> {
        rules::load(&self.state, &self.source(home))
    }

    pub fn note(&self, notes: &[forge_core::Note]) {
        state::record_notes(&self.state, notes);
    }

    /// `path` (absolute, or relative to `cwd`) relative to the root; None outside it.
    pub fn rel(&self, path: &str, cwd: &Path) -> Option<String> {
        let p = Path::new(path);
        let abs = if p.is_absolute() {
            p.to_path_buf()
        } else {
            cwd.join(p)
        };
        let (dir, name) = (abs.parent()?, abs.file_name()?);
        let dir = dir.canonicalize().ok()?;
        let rel = dir.strip_prefix(&self.root).ok()?;
        let rel = rel.join(name);
        let s = rel.to_str()?.to_string();
        (!s.is_empty()).then_some(s)
    }
}

impl Runtime<'_> {
    /// Where the hook runs: the input's cwd, else the project directory.
    pub(crate) fn cwd(&self, input: &HookInput) -> PathBuf {
        input
            .cwd
            .as_deref()
            .filter(|c| !c.is_empty())
            .map(PathBuf::from)
            .or_else(|| self.env.project_dir.clone())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("/"))
    }

    /// The Forge project of the hook's cwd (a repository with a lock or a config).
    pub(crate) fn project(&self, input: &HookInput) -> Option<Project> {
        let root = forge_home::find_repo_root(&self.cwd(input)).ok()??;
        let id = root.id();
        let session = input.session_key();
        let state = self.home.repo_state_dir(&id);
        let sid = forge_home::SessionId::parse(&session).ok()?;
        let session_dir = self.home.session_dir(&id, &sid);
        Some(Project {
            root: root.path().to_path_buf(),
            id,
            state,
            session,
            session_dir,
        })
    }

    /// The state directory of the user-level hooks.
    pub(crate) fn user_state(&self) -> PathBuf {
        self.home.state_root().join("user")
    }

    /// The user-level setup's lock (`~/.claude/forge-user.lock`).
    pub(crate) fn user_lock(&self) -> Option<PathBuf> {
        self.env
            .home_dir
            .as_ref()
            .map(|h| h.join(forge_render::lock::USER_LOCK_FILE))
    }
}

/// Answers one hook call. Never panics: a failure of Forge itself is a silent answer.
pub fn run(rt: &Runtime, event: &str, stdin: &[u8]) -> Output {
    let input = HookInput::parse(stdin);
    std::panic::catch_unwind(AssertUnwindSafe(|| dispatch(rt, event, &input)))
        .unwrap_or_else(|_| Output::silent())
}

fn dispatch(rt: &Runtime, event: &str, input: &HookInput) -> Output {
    match event {
        "session-start" => events::session_start(rt, input),
        "user-prompt-submit" => events::prompt(rt, input),
        "pre-tool-use" => guard::pre_tool_use(rt, input, false),
        "user-pre-tool-use" => guard::pre_tool_use(rt, input, true),
        "post-tool-use" | "post-tool-use-failure" => events::post_tool_use(rt, input),
        "post-tool-use-typecheck" => events::typecheck(rt, input),
        "stop" => events::stop(rt, input),
        "permission-request" => review::permission_request(rt, input, false),
        "user-permission-request" => review::permission_request(rt, input, true),
        "user-notification" => events::notify(rt, input),
        "session-end" => events::session_end(rt, input),
        _ => Output::silent(),
    }
}

/// A PreToolUse decision.
pub(crate) fn pre_decision(decision: &str, reason: &str) -> Output {
    Output::json(json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": decision,
            "permissionDecisionReason": reason,
        }
    }))
}

/// Where a typecheck or a reviewer spends its time on a hook's path: never past its hook's timeout.
pub(crate) const REVIEW_CAP: Duration = Duration::from_secs(170);
