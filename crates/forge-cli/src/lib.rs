//! The `forge` command line (also installed as `claude-forge`). Stage 1: `--version`,
//! `packs dump`, `packs lint`; `hook` is reserved for the hook runtime.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

pub mod project;

#[derive(Parser)]
#[command(
    name = "forge",
    version,
    about = "Generates a standalone Claude Code setup from rule packs."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(flatten)]
    Project(project::ProjectCommand),
    /// The rule packs Forge loads.
    Packs {
        #[command(subcommand)]
        command: PacksCommand,
    },
    /// Runs a Claude Code hook (not implemented yet).
    #[command(hide = true)]
    Hook {
        /// The hook event (PreToolUse, PostToolUse, Stop, ...).
        event: Option<String>,
    },
}

#[derive(Args)]
struct LoadArgs {
    /// Also load this repository's local packs (.forge/packs) and rules (.forge/rules).
    #[arg(long)]
    repo: Option<PathBuf>,
    /// Cache parsed packs in this directory.
    #[arg(long)]
    cache_dir: Option<PathBuf>,
}

#[derive(Subcommand)]
enum PacksCommand {
    /// Prints the loaded packs (the shape of dist/packs.json).
    Dump {
        /// Print JSON (the only format so far; accepted for scripts).
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        load: LoadArgs,
    },
    /// Reports the content lints' findings; exits 1 when there are any.
    Lint {
        #[command(flatten)]
        load: LoadArgs,
    },
}

fn load(args: &LoadArgs) -> Result<forge_packs::PackSet, String> {
    let opts = forge_packs::Options {
        cache_dir: args.cache_dir.as_deref(),
    };
    forge_packs::load(args.repo.as_deref(), opts).map_err(|e| e.to_string())
}

pub fn main() -> ExitCode {
    // Runs the version pinned in forge.lock (execs it and never returns) when it is not this one.
    // Its notes are recorded for `forge status`; downloading comes with provisioning.
    let _notes =
        forge_launcher::launch_from_env(env!("CARGO_PKG_VERSION"), &forge_launcher::NoFetcher);
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Packs {
            command:
                PacksCommand::Dump {
                    json: _,
                    load: args,
                },
        } => load(&args).map(|set| {
            let source = if args.repo.is_some() {
                None
            } else {
                Some(forge_packs::PackSource::Builtin)
            };
            let mut out = std::io::stdout().lock();
            let _ = serde_json::to_writer(&mut out, &set.bundle(source));
            let _ = writeln!(out);
            ExitCode::SUCCESS
        }),
        Command::Packs {
            command: PacksCommand::Lint { load: args },
        } => load(&args).map(|set| {
            print!("{}", forge_packs::lint::render(&set.findings));
            if set.findings.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }),
        Command::Hook { .. } => Err("forge hook: not implemented yet".into()),
        Command::Project(c) => Ok(project::run(c)),
    };
    result.unwrap_or_else(|e| {
        eprintln!("forge: {e}");
        ExitCode::from(2)
    })
}
