//! The tools a check names (`src/core/tools.ts`), as placement needs them: which audited tools
//! it depends on and whether it may run inside a workspace package.

use std::sync::LazyLock;

use fancy_regex::Regex;
use forge_packs::model::{AuditedTool, CheckTarget};

/// Project tools resolved through the project (audited tools first).
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
/// Forge's own pinned tools used in check commands.
pub const FORGE_TOOLS: &[&str] = &["ty"];
/// System tools, found on PATH.
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

/// Tools that builtins run, which their name in a command does not show.
const HELPER_TOOLS: &[(&str, &str)] = &[
    (r"\bforge_(zizmor|hadolint)\b", "uvx"),
    (
        r"\bforge_(trusted_deps|openapi_lint|lockfile_lint)\b",
        "bun",
    ),
    (r"\bforge_tf_validate\b", "terraform"),
    (r"\bforge_helm\b", "helm"),
    (r"\bforge_sqlfluff\b", "sqlfluff"),
];

fn escape(t: &str) -> String {
    fancy_regex::escape(t).into_owned()
}

static TOOL_RES: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    PROJECT_TOOLS
        .iter()
        .chain(FORGE_TOOLS)
        .chain(SYSTEM_TOOLS)
        .map(|&tool| {
            let lead = if ["npm", "pnpm", "yarn"].contains(&tool) {
                r"(?:^|[;&|(`]|\$\(|\b(?:then|do|else)\s|\bforge_do\s)[\t ]*"
            } else {
                r"(?:^|[\s;&|(`]|\$\()"
            };
            let re = Regex::new(&format!("(?m){lead}{}(?=$|[\\s;&|)`])", escape(tool)))
                .expect("tool regex");
            (tool, re)
        })
        .collect()
});

/// The runtime word a pack writes after a helper (`forge_helper python new_deps.py`): the
/// helpers are builtins of Forge, so it names no tool the check needs.
static HELPER_RUNTIME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(forge_(?:helper|ratchet))[\t ]+(?:python3?|bun|node)(?=$|[\s;&|)`])")
        .expect("helper runtime regex")
});

/// Tools invoked as commands in a shell string, in order of first use.
pub fn tools_in_command(cmd: &str) -> Vec<&'static str> {
    let cmd = HELPER_RUNTIME.replace_all(cmd, "$1");
    let cmd = cmd.as_ref();
    let mut found: Vec<(usize, &'static str)> = TOOL_RES
        .iter()
        .filter_map(|(tool, re)| re.find(cmd).ok().flatten().map(|m| (m.start(), *tool)))
        .collect();
    found.sort_by_key(|(at, _)| *at);
    found.into_iter().map(|(_, t)| t).collect()
}

/// The tools a check target runs: named in its commands, or run by the builtins it calls.
pub fn check_tools(t: &CheckTarget) -> Vec<&'static str> {
    let commands: Vec<&str> = t.commands().collect();
    tools_of_commands(&commands)
}

/// The tools shell commands run: named as commands, in order of first use, then those the
/// builtins they call run. The one list of the checker, the planner and the person's report.
pub fn tools_of_commands(commands: &[&str]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for c in commands {
        for tool in tools_in_command(c) {
            if !out.contains(&tool) {
                out.push(tool);
            }
        }
    }
    static HELPERS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
        HELPER_TOOLS
            .iter()
            .map(|(re, tool)| (Regex::new(re).expect("helper regex"), *tool))
            .collect()
    });
    for (re, tool) in HELPERS.iter() {
        if commands.iter().any(|c| re.is_match(c).unwrap_or(false)) && !out.contains(tool) {
            out.push(tool);
        }
    }
    out
}

/// The audited tools a check depends on.
pub fn audited_tools(t: &CheckTarget) -> Vec<AuditedTool> {
    check_tools(t)
        .into_iter()
        .filter_map(|n| AuditedTool::ALL.iter().copied().find(|a| a.as_str() == n))
        .collect()
}

/// Builtins that work the same from any directory.
const CWD_AGNOSTIC_HELPERS: &[&str] = &["forge_do", "forge_exec_tool", "forge_state_file"];

/// Whether a check may run inside workspace packages: it runs a project tool and no builtin
/// other than the directory-agnostic ones.
pub fn per_package(t: &CheckTarget) -> bool {
    static HELPER: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"\bforge_[a-z_]+").expect("regex"));
    let text: Vec<&str> = t.commands().collect();
    let text = text.join("\n");
    HELPER
        .find_iter(&text)
        .all(|m| CWD_AGNOSTIC_HELPERS.contains(&m.as_str()))
        && check_tools(t).iter().any(|x| PROJECT_TOOLS.contains(x))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_in_commands() {
        assert_eq!(
            tools_in_command("forge_do ruff lint {files} && pytest -q"),
            ["ruff", "pytest"]
        );
        assert!(tools_in_command("echo the pnpm field").is_empty());
        assert_eq!(tools_in_command("cd x && pnpm install"), ["pnpm"]);
    }

    /// The helpers are builtins: the runtime word after them is an argument, not a tool.
    #[test]
    fn a_helpers_runtime_is_not_a_tool() {
        assert!(
            tools_in_command("forge_helper python new_deps.py $FORGE_COOLDOWN_SECONDS").is_empty()
        );
        assert!(
            tools_in_command("forge_advise forge_helper bun pm_policy.mjs pnpm-catalog").is_empty()
        );
        assert!(tools_in_command("forge_ratchet bun ratchet.mjs").is_empty());
        assert!(tools_in_command("forge_ratchet python ratchet.py").is_empty());
        assert_eq!(
            tools_in_command("forge_helper python fk_index.py && python -m x"),
            ["python"]
        );
        assert!(tools_of_commands(&["forge_helper bun new_deps.mjs 0"]).is_empty());
    }
}
