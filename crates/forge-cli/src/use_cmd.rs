//! `claude-forge use <name>` (§20.6): prints an active, available capability's text on stdout and
//! nothing else. The one Forge command the agent reads and runs. Read-only and stateless: it finds
//! the project from the working directory, loads its packs without the parsed cache, writes
//! nothing and never downloads, so it runs inside Claude Code's sandbox. Loads are logged by the
//! PreToolUse hook, not here.

use std::io::Write;
use std::process::ExitCode;

use forge_capabilities::render::resolve;

/// What the agent reads when the name is not an active, available capability here.
fn not_here(name: &str) -> ExitCode {
    eprintln!("No capability named {name:?} is available here.");
    ExitCode::from(1)
}

pub fn run(name: &str) -> ExitCode {
    let Ok(cwd) = std::env::current_dir() else {
        return not_here(name);
    };
    let Ok(Some(root)) = forge_home::find_repo_root(&cwd) else {
        return not_here(name);
    };
    let root = root.path().to_path_buf();
    let Ok(set) = forge_packs::load(Some(&root), forge_packs::Options::default()) else {
        return not_here(name);
    };
    let Ok(config) = forge_packs::load_project_config(&root) else {
        return not_here(name);
    };
    let Ok(sel) = forge_check::select::project(&root, &set, config.as_ref()) else {
        return not_here(name);
    };
    let active = &sel.plan.capabilities;
    let Some(cap) = resolve(name, active) else {
        return not_here(name);
    };
    let provisioner = forge_home::Home::from_env()
        .ok()
        .map(forge_tools::Provisioner::new);
    if !forge_capabilities::available(cap, provisioner.as_ref()).is_available() {
        return not_here(name);
    }
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(cap.text.as_bytes());
    if !cap.text.ends_with('\n') {
        let _ = writeln!(out);
    }
    ExitCode::SUCCESS
}
