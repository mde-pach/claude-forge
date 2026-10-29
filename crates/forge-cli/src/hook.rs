//! `forge hook <event>` (what the settings' hook entries run) and the hidden `forge engine`
//! commands the SessionStart hook starts the types engine with.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Subcommand;
use forge_hooks::{Checker, Env, NoChecker, Runtime};

#[derive(Subcommand, Debug)]
pub enum EngineCommand {
    /// Runs the types engine daemon with a configuration file.
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
        /// Comma-separated: python, typescript.
        #[arg(long, default_value = "")]
        langs: String,
    },
}

/// The checker the hooks run.
fn checker() -> Box<dyn Checker> {
    Box::new(NoChecker)
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
    let checker = checker();
    let rt = Runtime {
        home,
        env: Env::from_process(),
        checker: checker.as_ref(),
        exe: std::env::current_exe().unwrap_or_else(|_| PathBuf::from("claude-forge")),
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
        EngineCommand::Serve { config } => match forge_hooks::engine::serve(&config) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("forge engine: {e}");
                ExitCode::from(1)
            }
        },
        EngineCommand::Start {
            root,
            session,
            langs,
        } => {
            let Ok(home) = forge_home::Home::from_env() else {
                return ExitCode::SUCCESS;
            };
            let langs: Vec<forge_engine::Lang> = langs
                .split(',')
                .filter_map(|l| serde_json::from_value(serde_json::json!(l.trim())).ok())
                .collect();
            let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("claude-forge"));
            let notes = forge_hooks::engine::start(&home, &exe, &root, &session, &langs);
            if let Ok(id) = forge_home::RepoId::for_root(&root) {
                forge_hooks::state::record_notes(&home.repo_state_dir(&id), &notes);
            }
            ExitCode::SUCCESS
        }
    }
}
