//! Starting the types engine for a repository (the CLI's hidden `engine start` / `engine serve`),
//! and the hooks' quick questions to it.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use forge_core::{Note, NoteKind};
use forge_engine::{Client, EngineConfig, Lang, PythonConfig, TypeScriptConfig};
use forge_home::{Home, RepoId};
use forge_tools::ToolResolver;

/// The client of the repository's engine when one is listening; never starts it.
pub fn running(home: &Home, id: &RepoId, timeout: Duration) -> Option<Client> {
    let socket = home.engine_socket(id);
    forge_engine::socket_addr(&socket)
        .exists()
        .then(|| Client::new(&socket).with_timeout(timeout))
}

/// The engine's configuration for `langs`; the notes say what cannot start.
pub fn config(
    home: &Home,
    root: &Path,
    langs: &[Lang],
    tools: &dyn ToolResolver,
) -> (EngineConfig, Vec<Note>) {
    let id = RepoId::from_physical_path(root);
    let mut cfg = EngineConfig::new(root, home.engine_socket(&id));
    cfg.log = Some(home.repo_state_dir(&id).join("engine.log"));
    let mut notes = Vec::new();
    for lang in langs {
        match lang {
            Lang::Python => match tools.ensure("ty") {
                Ok(ty) => {
                    cfg.python = Some(PythonConfig {
                        ty: vec![ty.display().to_string()],
                        ..PythonConfig::default()
                    })
                }
                Err(n) => notes.push(n),
            },
            Lang::Typescript => {
                cfg.typescript = Some(TypeScriptConfig {
                    js_runtime: None,
                    preload: true,
                })
            }
        }
    }
    cfg.want = format!(
        "{} {}",
        env!("CARGO_PKG_VERSION"),
        serde_json::to_string(&(&cfg.python, &cfg.typescript)).unwrap_or_default()
    );
    (cfg, notes)
}

/// Makes sure the engine of `root` runs for `langs` and registers `session`: what the detached
/// `engine start` does after SessionStart. `exe` is Forge's executable.
pub fn start(home: &Home, exe: &Path, root: &Path, session: &str, langs: &[Lang]) -> Vec<Note> {
    let provisioner = forge_tools::Provisioner::new(home.clone());
    let (cfg, mut notes) = config(home, root, langs, &provisioner);
    if cfg.python.is_none() && cfg.typescript.is_none() {
        return notes;
    }
    let id = RepoId::from_physical_path(root);
    let cfg_file: PathBuf = home.repo_state_dir(&id).join("engine.json");
    if crate::state::ensure_dir(home.engine_dir().as_path()) {
        if let Ok(b) = serde_json::to_vec(&cfg) {
            crate::state::write_atomic(&cfg_file, &b);
        }
    }
    let r = forge_engine::ensure(&cfg, Some(session), Duration::from_secs(60), || {
        forge_engine::spawn_detached(
            Command::new(exe)
                .args(["engine", "serve", "--config"])
                .arg(&cfg_file)
                .current_dir(root),
        )
    });
    if let Err(e) = r {
        notes.push(Note::new(
            NoteKind::ForgeFailure,
            format!("The types engine did not start ({e}); type-aware rules are not checked after edits until it does."),
        ));
    }
    notes
}

/// Runs the daemon with the configuration in `file`.
pub fn serve(file: &Path) -> Result<(), String> {
    let bytes = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let cfg: EngineConfig = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    forge_engine::serve(cfg)
}
