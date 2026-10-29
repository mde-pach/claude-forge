//! What loading checks beyond the JSON Schemas: each target kind's `validate`
//! (`src/targets/*.ts`) and the rule's file roles.

use std::sync::LazyLock;

use regex::Regex;

use crate::model::{Language, Rule, Target};
use crate::roles;

/// The tool adapters (§16) a check command must ask through `forge_do <tool> <operation>`,
/// in `src/adapters/index.ts` order, with the adapters that override how a direct run is
/// recognised (`direct`). Kept in step with the TypeScript adapters until they move here.
const ADAPTERS: &[(&str, Option<&str>)] = &[
    ("ruff", None),
    ("ty", None),
    ("alembic", None),
    ("astro", None),
    ("better-auth", None),
    ("biome", None),
    (
        "bun",
        Some(
            r"(^|[;&|`!{]|\$\(|\b(?:then|do|else|exec|time)\s)\s*bun\s+(?:install|i|add|a|remove|rm|update|upgrade|outdated|link|unlink|pm|test|ci|audit|publish|patch|patch-commit|why|info)(?=$|[\s;&|)`])",
        ),
    ),
    ("docker", None),
    ("eslint", None),
    ("graphql-codegen", None),
    ("helm", None),
    ("kubectl", None),
    ("kustomize", None),
    ("mikro-orm", None),
    ("ngc", None),
    (
        "npm",
        Some(r"(^|[;&|`!{]|\$\(|\b(?:then|do|else|exec|time)\s)\s*npm(?=$|[\s;&|)`])"),
    ),
    ("oxlint", None),
    ("playwright", None),
    (
        "pnpm",
        Some(r"(^|[;&|`!{]|\$\(|\b(?:then|do|else|exec|time)\s)\s*pnpm(?=$|[\s;&|)`])"),
    ),
    ("prettier", None),
    ("prisma", None),
    ("pytest", None),
    ("sqlfluff", None),
    ("terraform", None),
    ("tofu", None),
    ("tsc", None),
    ("typeorm", None),
    ("typeorm-ts-node-esm", None),
    ("typeorm-ts-node-commonjs", None),
    (
        "uv",
        Some(r"(^|[;&|`!{]|\$\(|\b(?:then|do|else|exec|time)\s)\s*uv(?=$|[\s;&|)`])"),
    ),
    ("vitest", None),
    ("vue-tsc", None),
    ("wrangler", None),
    (
        "yarn",
        Some(r"(^|[;&|`!{]|\$\(|\b(?:then|do|else|exec|time)\s)\s*yarn(?=$|[\s;&|)`])"),
    ),
    ("actionlint", None),
    ("ast-grep", None),
    ("editorconfig-checker", None),
    ("gitleaks", None),
    ("hadolint", None),
    ("kube-linter", None),
    ("kubeconform", None),
    ("lockfile-lint", None),
    ("oasdiff", None),
    ("osv-scanner", None),
    ("redocly", None),
    ("spectral", None),
    ("squawk", None),
    ("tflint", None),
    ("trivy", None),
    ("zizmor", None),
];

/// Project tools whose name has a "-" (`PROJECT_TOOLS` in `src/core/tools.ts`).
const DASHED_PROJECT_TOOLS: &[&str] = &["vue-tsc", "mikro-orm", "graphql-codegen", "better-auth"];

fn escape_tool(tool: &str) -> String {
    tool.replace('.', r"\.").replace('-', r"\-")
}

/// `directPattern`: a check command that runs the adapted tool directly.
/// A JavaScript pattern of the adapters' as the `regex` crate reads it: `\b` is ASCII, as in
/// JavaScript, and the trailing lookahead `(?=…)` (which `regex` lacks) becomes a group,
/// which changes what is matched but not whether something is.
fn js_pattern(src: &str) -> String {
    let src = src.replace(r"\b", r"(?-u:\b)");
    match src.rfind("(?=") {
        Some(i) if src.ends_with(')') => format!("{}(?:{}", &src[..i], &src[i + 3..]),
        _ => src,
    }
}

static DIRECT: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    ADAPTERS
        .iter()
        .map(|(tool, direct)| {
            let src = direct.map_or_else(
                || format!(r"(^|[\s;&|(`!]|\$\(){}(?=$|[\s;&|)`])", escape_tool(tool)),
                str::to_string,
            );
            (
                *tool,
                Regex::new(&format!("(?m){}", js_pattern(&src))).expect("adapter regex"),
            )
        })
        .collect()
});

static DASHED: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    DASHED_PROJECT_TOOLS
        .iter()
        .map(|tool| {
            (
                *tool,
                Regex::new(&js_pattern(&format!(
                    r"(^|[\s;&|(`]|\$\()({})(?=$|[\s;&|)`])",
                    escape_tool(tool)
                )))
                .expect("tool regex"),
            )
        })
        .collect()
});
static SINGLE_QUOTED: LazyLock<Regex> = LazyLock::new(|| Regex::new("'[^']*'").expect("regex"));
static FORGE_DO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?-u:\b)forge_do\s+[a-z][a-z0-9-]*").expect("regex"));
static WRAPPED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?-u:\b)forge_(exec_tool|do)\s+$").expect("regex"));

/// `directDashedTools`: dashed project tools a command runs directly, not through `forge_exec_tool`.
fn direct_dashed_tools(cmd: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    for (tool, re) in DASHED.iter() {
        let mut at = 0;
        // every occurrence (the lookahead's stand-in consumes a character the next may start with)
        while let Some(c) = re.captures_at(cmd, at) {
            let (lead, name) = (
                c.get(1).map_or(0, |g| g.end()),
                c.get(2).expect("tool group"),
            );
            if !WRAPPED.is_match(&cmd[..lead]) && !out.contains(tool) {
                out.push(*tool);
            }
            at = name.end();
        }
    }
    out
}

/// Checks a rule's targets and roles (beyond the schema); the error is the TS message.
pub fn validate_rule(rule: &Rule) -> Result<(), String> {
    for t in &rule.enforce {
        match t {
            Target::Check(c) => {
                for command in c.commands() {
                    // single-quoted text is data, not a command of this shell
                    let code = SINGLE_QUOTED.replace_all(command, "''");
                    let code = FORGE_DO.replace_all(&code, "forge_do _");
                    for (tool, re) in DIRECT.iter() {
                        if re.is_match(&code) {
                            return Err(format!(
                                "invalid rule: run {tool} as \"forge_do {tool} <operation>\" (§16: its adapter knows how each release does it)"
                            ));
                        }
                    }
                    if let Some(tool) = direct_dashed_tools(command).first() {
                        return Err(format!(
                            "invalid rule: run {tool} as \"forge_exec_tool {tool}\" (a name with \"-\" has no wrapper function, so it would not be the project's own)"
                        ));
                    }
                }
            }
            Target::AstGrep(a) => {
                // deprecated uses are answered by the TypeScript engine (§15.7)
                let where_json = serde_json::to_string(&a.where_.clone().unwrap_or_default())
                    .unwrap_or_default();
                if rule.scope.language == Language::Python && where_json.contains("\"deprecated\"")
                {
                    return Err(
                        "invalid rule: where deprecated is answered for TypeScript only (§15.7); Python's deprecations are ty's deprecated rule".into()
                    );
                }
            }
            _ => {}
        }
    }
    let unknown = roles::unknown_roles(rule);
    if !unknown.is_empty() {
        let names: Vec<String> = unknown.iter().map(|r| format!("@{r}")).collect();
        return Err(format!(
            "invalid rule: unknown file role {} (§15.4)",
            names.join(", ")
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::direct_dashed_tools;

    #[test]
    fn dashed_tools() {
        assert_eq!(direct_dashed_tools("vue-tsc --noEmit"), ["vue-tsc"]);
        assert!(direct_dashed_tools("forge_exec_tool vue-tsc --noEmit").is_empty());
        assert!(direct_dashed_tools("echo x-vue-tsc").is_empty());
    }
}
