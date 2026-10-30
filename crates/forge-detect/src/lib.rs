//! What Forge sees in a repository, from its content only (never from machine state):
//!
//! - [`facts`]: the root and each workspace package — declared dependencies with their
//!   resolved versions ([`deps`], [`locks`]), the files of each, whether it publishes a
//!   library, and named facts ([`pm`]: `python.pm`, `js.pm`) for `when:` conditions;
//! - [`activate`]: the active packs and why;
//! - [`place`]: where tools are configured, where packs and rules apply, where checks run;
//! - [`roles`]: file roles' effective patterns (tool configs, forge.yaml).

pub mod activate;
pub mod deps;
pub mod facts;
pub mod jsonc;
pub mod locks;
pub mod place;
pub mod pm;
pub mod roles;
pub mod tools;
pub mod versions;

use std::path::{Path, PathBuf};
use std::process::Command;

pub use activate::{Activation, PackStatus, active_packs, detect_anywhere, detect_pack};
pub use facts::{
    DepFacts, DetectError, FACT_JS_PM, FACT_PYTHON_PM, ProjectFacts, read_project_facts,
};
pub use place::{CheckPlace, Site};

/// The repository's files: tracked and untracked, not ignored (`git ls-files`), that exist.
pub fn list_repo_files(repo: &Path) -> Vec<String> {
    let out = Command::new("git")
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .current_dir(repo)
        .output();
    let Some(out) = out.ok().filter(|o| o.status.success()) else {
        return Vec::new();
    };
    let mut seen = std::collections::BTreeSet::new();
    String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|f| !f.is_empty() && repo.join(f).exists())
        .filter(|f| seen.insert(f.to_string()))
        .map(str::to_owned)
        .collect()
}

/// The top of the git working tree holding `cwd`.
pub fn git_toplevel(cwd: &Path) -> Option<PathBuf> {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then(|| PathBuf::from(s))
}
