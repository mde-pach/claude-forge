//! Where a tool is: the project's own install, the executable on PATH, or Forge's pinned
//! release. The question is forge-core's [`ToolResolver`]; [`LocalTools`] answers it from what
//! is on the machine, and asks a pinned-tools resolver (forge-tools' cache) first when it is
//! given one. Which tools a check names is forge-detect's ([`forge_detect::tools`]).

use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use forge_core::tools::{ToolCmd, ToolResolver};
pub use forge_detect::tools::{PROJECT_TOOLS, SYSTEM_TOOLS};

/// The tools project checks run from the project's Python environment (`PYTHON_TOOLS`).
pub const PYTHON_TOOLS: &[&str] = &["ruff", "alembic", "pytest", "python", "sqlfluff"];

/// Set (to `1`) for the builtins of a run that may download a missing pinned tool (Stop and
/// `forge check`); unset after an edit.
pub const ENV_DOWNLOAD: &str = "FORGE_TOOL_DOWNLOAD";

/// Pinned tools that uvx can run from PyPI: package name, executable.
const UVX_PACKAGES: &[(&str, &str, &str)] = &[
    ("ty", "ty", "ty"),
    ("ast-grep", "ast-grep-cli", "ast-grep"),
    ("zizmor", "zizmor", "zizmor"),
    ("hadolint", "hadolint-bin", "hadolint"),
    ("squawk", "squawk-cli", "squawk"),
];

/// What is on this machine: the project's installs, PATH, and for Forge's pinned tools, the
/// pinned-tools resolver it was given (forge-tools' cache), else uvx for the ones PyPI
/// distributes. Downloads nothing itself; its pinned resolver may, when `download` is set.
#[derive(Clone, Default)]
pub struct LocalTools {
    /// PATH, when not the process's.
    pub path: Option<String>,
    /// Forge's pinned tools (forge-tools' cache).
    pub pinned: Option<Arc<dyn ToolResolver>>,
    /// A missing pinned tool may be installed (Stop, `forge check`, `forge apply`).
    pub download: bool,
}

impl std::fmt::Debug for LocalTools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalTools")
            .field("path", &self.path)
            .field("pinned", &self.pinned.is_some())
            .field("download", &self.download)
            .finish()
    }
}

impl LocalTools {
    /// With Forge's pinned tools; `download`: a missing one may be installed.
    pub fn with_pinned(pinned: Arc<dyn ToolResolver>, download: bool) -> LocalTools {
        LocalTools {
            path: None,
            pinned: Some(pinned),
            download,
        }
    }

    fn path_var(&self) -> String {
        self.path
            .clone()
            .or_else(|| std::env::var("PATH").ok())
            .unwrap_or_default()
    }

    fn python_tool(&self, name: &str) -> Option<ToolCmd> {
        if !PYTHON_TOOLS.contains(&name) {
            return None;
        }
        if let Some(venv) = std::env::var_os("VIRTUAL_ENV").filter(|v| !v.is_empty()) {
            let exe = PathBuf::from(venv).join("bin").join(name);
            if is_exe(&exe) {
                return Some(ToolCmd::exe(exe));
            }
        }
        if name == "python" {
            return self
                .on_path("python3")
                .or_else(|| self.on_path("python"))
                .map(ToolCmd::exe);
        }
        self.on_path(name).map(ToolCmd::exe)
    }
}

pub fn is_exe(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

impl ToolResolver for LocalTools {
    fn project(&self, root: &Path, cwd: &Path, name: &str) -> Option<ToolCmd> {
        let nm = cwd.join("node_modules/.bin").join(name);
        let venv = cwd.join(".venv/bin").join(name);
        let bun_repo = cwd.join("bun.lock").is_file() || cwd.join("bun.lockb").is_file();
        if bun_repo {
            if is_exe(&nm)
                && let Some(bun) = self.on_path("bun")
            {
                return Some(ToolCmd {
                    argv: vec![
                        bun.to_string_lossy().into_owned(),
                        "run".into(),
                        "--bun".into(),
                        name.into(),
                    ],
                    bin: nm,
                });
            }
        } else if cwd.join("uv.lock").is_file() {
            if is_exe(&venv) {
                return Some(ToolCmd::exe(venv));
            }
            return self.python_tool(name);
        } else if is_exe(&nm) {
            return Some(ToolCmd::exe(nm));
        } else if is_exe(&venv) {
            return Some(ToolCmd::exe(venv));
        }
        // in a workspace package: the root's install (hoisted by bun, npm and Yarn; uv's .venv)
        if cwd != root {
            return self.project(root, root, name);
        }
        if bun_repo {
            None
        } else {
            self.python_tool(name)
        }
    }

    fn on_path(&self, name: &str) -> Option<PathBuf> {
        if name.contains('/') {
            let p = PathBuf::from(name);
            return is_exe(&p).then_some(p);
        }
        self.path_var()
            .split(':')
            .filter(|d| !d.is_empty())
            .map(|d| Path::new(d).join(name))
            .find(|p| is_exe(p))
    }

    fn pinned(&self, name: &str, version: &str) -> Result<ToolCmd, String> {
        let mut why = None;
        if let Some(p) = &self.pinned {
            let r = if self.download {
                p.ensure(name, version)
            } else {
                p.pinned(name, version)
            };
            match r {
                Ok(c) => return Ok(c),
                Err(e) => why = Some(e),
            }
        }
        // without Forge's cache, the machine's uvx (which may download: never with the cache,
        // whose own downloads are allowed only where the person waits)
        if self.pinned.is_none()
            && let Some((_, pkg, exe)) = UVX_PACKAGES.iter().find(|(t, _, _)| *t == name)
            && let Some(uvx) = self.on_path("uvx")
        {
            return Ok(ToolCmd {
                argv: vec![
                    uvx.to_string_lossy().into_owned(),
                    "-q".into(),
                    "--from".into(),
                    format!("{pkg}=={version}"),
                    exe.to_string(),
                ],
                bin: uvx,
            });
        }
        match self.on_path(name) {
            Some(p) => Ok(ToolCmd::exe(p)),
            None => Err(why.unwrap_or_else(|| format!("{name} {version} is not installed"))),
        }
    }

    fn ensure(&self, name: &str, version: &str) -> Result<ToolCmd, String> {
        LocalTools {
            download: true,
            ..self.clone()
        }
        .pinned(name, version)
    }
}

/// Whether a check's tool is here (`forge_has_tool`): system tools on PATH (Yarn 2 or later
/// only; terraform as the project's Terraform CLI; kustomize, or kubectl's), the pinned ty
/// through its resolver, project tools as the project installs them.
pub fn has_tool(tools: &dyn ToolResolver, root: &Path, cwd: &Path, name: &str) -> bool {
    match name {
        "uv" | "uvx" | "bun" | "docker" | "tofu" | "helm" | "kubectl" | "npm" | "pnpm" => {
            tools.on_path(name).is_some()
        }
        "yarn" => tools.on_path("yarn").is_some_and(|y| {
            let v = crate::adapters::ask_version(&ToolCmd::exe(y), "--version");
            v.is_some_and(|v| !v.starts_with("0.") && !v.starts_with("1."))
        }),
        "terraform" => tools
            .on_path(crate::adapters::opentofu_pick(root))
            .is_some(),
        "kustomize" => tools.on_path("kustomize").is_some() || tools.on_path("kubectl").is_some(),
        "ty" => crate::adapters::Adapter::load("ty")
            .and_then(|a| a.pinned.clone())
            .is_some_and(|v| tools.pinned("ty", &v).is_ok()),
        _ => tools.project(root, cwd, name).is_some(),
    }
}

/// How a missing tool is named to the person (`forge_tool_label`).
pub fn tool_label(root: &Path, name: &str) -> String {
    match name {
        "terraform" => crate::adapters::opentofu_pick(root).to_string(),
        "kustomize" => "kustomize or kubectl".into(),
        "yarn" => "yarn 2 or later".into(),
        n => n.to_string(),
    }
}

/// The tools a check target runs (forge-detect's `check_tools`), less uvx: Forge runs its pinned
/// tools itself.
pub fn check_tools(commands: &[&str]) -> Vec<String> {
    forge_detect::tools::tools_of_commands(commands)
        .into_iter()
        .filter(|t| *t != "uvx")
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_of_commands() {
        assert_eq!(check_tools(&["forge_do ruff lint {files}"]), ["ruff"]);
        assert_eq!(
            check_tools(&["echo 'the pnpm field'; forge_do pnpm lock-check"]),
            ["pnpm"]
        );
        assert_eq!(check_tools(&["forge_sqlfluff {files}"]), ["sqlfluff"]);
        assert!(check_tools(&["forge_large_files 1048576"]).is_empty());
    }
}
