//! Downloads wired into the CLI: the launcher's fetcher, the background child that a session
//! start leaves running to download the pinned Forge version, and the tool resolver every
//! command asks (the project's tools, PATH, Forge's pinned tools from its cache).

use std::process::ExitCode;
use std::sync::Arc;

use forge_check::tools::{LocalTools, ToolResolver};
use forge_core::{Note, NoteKind};
use forge_tools::forge_release::{BACKGROUND_ARG, ReleaseFetcher, run_background_fetch};
use forge_tools::{PinnedTools, ProvisionReport, Provisioner};

/// Set to a non-empty value, Forge downloads nothing: no tool, no pinned Forge version (an
/// offline machine, the tests).
pub const ENV_NO_DOWNLOAD: &str = forge_launcher::NO_DOWNLOAD_ENV;

/// When this process is the background download child (`claude-forge __forge-fetch`), runs
/// it and returns its exit code. Checked before the launcher, so the child never execs
/// another version that would not know the command.
pub fn intercept() -> Option<ExitCode> {
    std::env::args_os()
        .nth(1)
        .is_some_and(|a| a == BACKGROUND_ARG)
        .then(run_background_fetch)
}

/// The fetcher the launcher downloads pinned versions with (GitHub releases, sha256-checked).
pub fn fetcher() -> ReleaseFetcher {
    ReleaseFetcher::new()
}

/// Whether downloads are turned off ([`ENV_NO_DOWNLOAD`]).
/// Puts Forge's PATH directory (uv and uvx only) first on this process's PATH, as the
/// SessionStart hook does for Claude's Bash commands: the checks, their builtins and the commands
/// they run then use the uv Claude uses. Otherwise a machine's older uv can read the project
/// differently (a lock it calls stale, settings it cannot parse) and Claude is asked to fix
/// what its own uv sees as fine.
pub fn use_forge_path() {
    let Ok(home) = forge_home::Home::from_env() else {
        return;
    };
    let dir = home.path_dir();
    if !dir.join("uv").is_file() {
        return;
    }
    let path = std::env::var("PATH").unwrap_or_default();
    let d = dir.to_string_lossy();
    if path.split(':').next() == Some(d.as_ref()) {
        return;
    }
    let joined = if path.is_empty() {
        d.into_owned()
    } else {
        format!("{d}:{path}")
    };
    // SAFETY: called first thing in main, before any thread is started.
    unsafe { std::env::set_var("PATH", joined) };
}

pub fn downloads_off() -> bool {
    forge_launcher::downloads_off()
}

fn provisioner() -> Option<Provisioner> {
    forge_home::Home::from_env().ok().map(Provisioner::new)
}

/// Where tools are, for a command: the project's installs and PATH, then Forge's pinned tools
/// from its cache. `download`: a missing pinned tool may be installed (`forge apply`, `forge
/// check`, Stop; never after an edit or before a command).
pub fn tools(download: bool) -> Arc<dyn ToolResolver> {
    let download = download && !downloads_off();
    match provisioner() {
        Some(p) => Arc::new(LocalTools::with_pinned(
            Arc::new(PinnedTools::new(p, download)),
            download,
        )),
        None => Arc::new(LocalTools::default()),
    }
}

/// The tools of a builtin process (`forge internal`): it may download when the run that
/// started it may.
pub fn builtin_tools() -> Arc<dyn ToolResolver> {
    let allowed = std::env::var_os(forge_check::tools::ENV_DOWNLOAD).is_some_and(|v| v == "1");
    tools(allowed)
}

/// `forge apply`: installs uv (and puts it in Forge's PATH directory) and the pinned tools the
/// active checks use. Each failure is a note for the person.
pub fn ensure_tools(names: &[String]) -> ProvisionReport {
    if downloads_off() {
        let mut r = ProvisionReport::default();
        r.notes.push(Note::new(
            NoteKind::Environment,
            format!(
                "Forge's tools were not installed: downloads are turned off ({ENV_NO_DOWNLOAD})"
            ),
        ));
        return r;
    }
    let Some(p) = provisioner() else {
        let mut r = ProvisionReport::default();
        r.notes.push(Note::new(
            NoteKind::ForgeFailure,
            "Forge's cache directory could not be found: set HOME or FORGE_HOME",
        ));
        return r;
    };
    p.ensure_all(names.iter().map(String::as_str))
}

#[cfg(test)]
mod tests {
    use forge_check::adapters::Adapter;
    use forge_tools::Catalog;

    /// The adapters' `pinned` versions (what the checker asks for) are the cache's pins (what
    /// apply installs and the lock records): one table, checked here.
    #[test]
    fn adapter_pins_are_the_catalog_pins() {
        let catalog = Catalog::builtin();
        for a in Adapter::all().unwrap() {
            let Some(pin) = &a.pinned else { continue };
            let spec = catalog
                .get(&a.tool)
                .unwrap_or_else(|| panic!("{} is pinned but not in Forge's tool table", a.tool));
            assert_eq!(spec.version, pin, "{}", a.tool);
        }
    }
}
