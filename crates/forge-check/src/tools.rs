//! Where a tool is: the project's own install, the executable on PATH, or Forge's pinned
//! release. Provisioning (downloading pinned tools, uv) belongs to forge-tools; the checker asks
//! through [`ToolResolver`], and [`LocalTools`] answers from what is already on the machine.

use std::path::{Path, PathBuf};

/// How to run a tool: the words before its own arguments, and the executable they resolve to
/// (whose changes invalidate what was learnt about it, such as its version).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCmd {
    pub argv: Vec<String>,
    pub bin: PathBuf,
}

impl ToolCmd {
    pub fn exe(bin: PathBuf) -> ToolCmd {
        ToolCmd {
            argv: vec![bin.to_string_lossy().into_owned()],
            bin,
        }
    }
}

/// Answers "where is tool X" for the checker.
pub trait ToolResolver: Send + Sync {
    /// The project's own `name` (node_modules/.bin, .venv, its Python environment), for a
    /// command run in `cwd` (a workspace package falls back to the root's install).
    fn project(&self, root: &Path, cwd: &Path, name: &str) -> Option<ToolCmd>;
    /// The executable `name` on PATH.
    fn on_path(&self, name: &str) -> Option<PathBuf>;
    /// Forge's pinned `name` at `version`; `Err` says why it cannot be had (for the person).
    fn pinned(&self, name: &str, version: &str) -> Result<ToolCmd, String>;
}

/// The tools project checks run from the project's Python environment (`PYTHON_TOOLS`).
pub const PYTHON_TOOLS: &[&str] = &["ruff", "alembic", "pytest", "python", "sqlfluff"];

/// Project tools resolved through `tool <name>` (`PROJECT_TOOLS`).
pub const PROJECT_TOOLS: &[&str] = &[
    "biome",
    "ruff",
    "tsc",
    "alembic",
    "pytest",
    "prisma",
    "vitest",
    "playwright",
    "astro",
    "python",
    "sqlfluff",
    "vue-tsc",
    "ngc",
    "mikro-orm",
    "graphql-codegen",
    "wrangler",
    "better-auth",
    "prettier",
    "eslint",
    "oxlint",
];

/// System tools, found on PATH (`SYSTEM_TOOLS`).
pub const SYSTEM_TOOLS: &[&str] = &[
    "uv",
    "uvx",
    "bun",
    "npm",
    "pnpm",
    "yarn",
    "docker",
    "terraform",
    "tofu",
    "helm",
    "kustomize",
    "kubectl",
];

/// Pinned tools that uvx can run from PyPI: package name, executable.
const UVX_PACKAGES: &[(&str, &str, &str)] = &[
    ("ty", "ty", "ty"),
    ("ast-grep", "ast-grep-cli", "ast-grep"),
    ("zizmor", "zizmor", "zizmor"),
    ("hadolint", "hadolint-bin", "hadolint"),
    ("squawk", "squawk-cli", "squawk"),
];

/// What is already on this machine: the project's installs, PATH, and uvx for the pinned tools
/// PyPI distributes. Downloads nothing itself.
#[derive(Debug, Clone, Default)]
pub struct LocalTools {
    /// PATH, when not the process's.
    pub path: Option<String>,
}

impl LocalTools {
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
        if let Some((_, pkg, exe)) = UVX_PACKAGES.iter().find(|(t, _, _)| *t == name)
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
            None => Err(format!("{name} {version} is not installed")),
        }
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

/// The tools a check target runs (`checkTools`): named in its commands as commands (or by
/// `forge_do <tool>`), and those the builtins it calls run.
pub fn check_tools(commands: &[&str]) -> Vec<String> {
    use regex::Regex;
    use std::sync::LazyLock;
    static HELPER_TOOLS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
        vec![
            (
                Regex::new(r"\bforge_(trusted_deps|openapi_lint|lockfile_lint)\b").unwrap(),
                "bun",
            ),
            (Regex::new(r"\bforge_tf_validate\b").unwrap(), "terraform"),
            (Regex::new(r"\bforge_helm\b").unwrap(), "helm"),
            (Regex::new(r"\bforge_sqlfluff\b").unwrap(), "sqlfluff"),
        ]
    });
    static TOOL_RES: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
        PROJECT_TOOLS
            .iter()
            .chain(["ty"].iter())
            .chain(SYSTEM_TOOLS.iter())
            .map(|t| {
                let lead = if matches!(*t, "npm" | "pnpm" | "yarn") {
                    r"(?m)(^|[;&|(`]|\$\(|\b(?:then|do|else)\s|\bforge_do\s)[\t ]*"
                } else {
                    r"(?m)(^|[\s;&|(`]|\$\()"
                };
                let re = Regex::new(&format!(r"{lead}{}($|[\s;&|)`])", regex::escape(t)))
                    .expect("regex");
                (*t, re)
            })
            .collect()
    });
    let mut found: Vec<(usize, &str)> = Vec::new();
    for (tool, re) in TOOL_RES.iter() {
        let at = commands
            .iter()
            .enumerate()
            .filter_map(|(i, c)| re.find(c).map(|m| i * 1_000_000 + m.start()))
            .min();
        if let Some(at) = at {
            found.push((at, tool));
        }
    }
    found.sort();
    let mut out: Vec<String> = found.into_iter().map(|(_, t)| t.to_string()).collect();
    for (re, tool) in HELPER_TOOLS.iter() {
        if commands.iter().any(|c| re.is_match(c)) && !out.iter().any(|t| t == tool) {
            out.push(tool.to_string());
        }
    }
    // uvx is how pinned tools run today; Forge provisions them itself
    out.retain(|t| t != "uvx");
    out
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
