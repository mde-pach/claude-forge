//! File roles (§15.4) as the loader needs them: which role names exist, Forge's default
//! patterns, and the roles a rule names. Mirrors `ROLES` and `rolesOfRule` in
//! `src/roles.ts` (the tool-config readers stay with the planner).

use std::collections::BTreeSet;
use std::sync::LazyLock;

use fancy_regex::Regex;

use crate::model::{Rule, Target, WhereCondition};

/// Every role and Forge's default patterns, sorted by name (`src/roles.ts` ROLES).
pub const ROLES: &[(&str, &[&str])] = &[
    ("adr", &["docs/adr/**", "doc/adr/**", "docs/decisions/**"]),
    (
        "alembic-migrations",
        &["alembic/versions/**", "migrations/versions/**"],
    ),
    ("angular-tsconfig", &["**/tsconfig.app.json"]),
    (
        "better-auth-config",
        &[
            "{,src/,app/}{,lib/server/,server/auth/,server/,auth/,lib/,utils/}{auth,auth/index}{.ts,.tsx,.js,.jsx,.server.js,.server.ts}",
        ],
    ),
    (
        "build-artifacts",
        &[
            "**/node_modules/**",
            "**/__pycache__/**",
            "**/*.pyc",
            "**/.next/**",
            "**/.nuxt/**",
            "**/.svelte-kit/**",
            "**/.turbo/**",
            "**/.coverage",
            "**/.DS_Store",
        ],
    ),
    (
        "compose",
        &[
            "**/compose*.yaml",
            "**/compose*.yml",
            "**/docker-compose*.yaml",
            "**/docker-compose*.yml",
        ],
    ),
    ("django-migrations", &["**/migrations/[0-9]*.py"]),
    ("django-settings", &["**/settings.py", "**/settings/*.py"]),
    (
        "django-templates",
        &["**/templates/**/*.html", "**/templates/**/*.txt"],
    ),
    (
        "docs",
        &["docs/**/*.{md,mdx,rst}", "doc/**/*.{md,mdx,rst}", "README*"],
    ),
    (
        "dockerfile",
        &[
            "**/Dockerfile",
            "**/Dockerfile.*",
            "**/*.Dockerfile",
            "**/Containerfile",
            "!**/*.dockerignore",
        ],
    ),
    ("drizzle-migrations", &["drizzle/**"]),
    ("e2e-tests", &["e2e/**", "tests/e2e/**"]),
    ("env-files", &["**/.env", "**/.env.*", "!@env-template"]),
    (
        "env-template",
        &[
            "**/.env.example",
            "**/.env.sample",
            "**/.env.template",
            "**/.env.dist",
        ],
    ),
    ("esm-modules", &["**/*.mts", "**/*.mjs"]),
    ("generated-clients", &[]),
    ("gunicorn-config", &["**/gunicorn.conf.py"]),
    (
        "lockfiles",
        &[
            "uv.lock",
            "bun.lock",
            "bun.lockb",
            "package-lock.json",
            "pnpm-lock.yaml",
            "yarn.lock",
        ],
    ),
    ("prisma-migrations", &["prisma/migrations/**"]),
    ("procfile", &["Procfile", "Procfile.*"]),
    (
        "python-tests",
        &["**/test_*.py", "**/*_test.py", "**/conftest.py"],
    ),
    (
        "sql-migrations",
        &[
            "db/migration/**/*.sql",
            "supabase/migrations/*.sql",
            "db/migrations/*.sql",
        ],
    ),
    (
        "ts-tests",
        &[
            "**/*.{test,spec}.{ts,tsx,mts,cts,js,jsx,mjs,cjs}",
            "**/__tests__/**",
        ],
    ),
    ("typeorm-datasource", &[]),
    (
        "wrangler-config",
        &["**/wrangler.toml", "**/wrangler.json", "**/wrangler.jsonc"],
    ),
    ("wrangler-types", &["worker-configuration.d.ts"]),
];

pub fn is_role(name: &str) -> bool {
    ROLES.iter().any(|(n, _)| *n == name)
}

/// A role's default patterns, other roles expanded ("!" before an exclusion).
pub fn default_patterns(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    expand(name, false, &mut out, 0);
    out
}

fn expand(name: &str, negate: bool, out: &mut Vec<String>, depth: usize) {
    let Some((_, globs)) = ROLES.iter().find(|(n, _)| *n == name) else {
        return;
    };
    if depth > ROLES.len() {
        return;
    }
    for g in *globs {
        let (neg, glob) = match g.strip_prefix('!') {
            Some(rest) => (!negate, rest),
            None => (negate, *g),
        };
        if let Some(role) = glob.strip_prefix('@') {
            expand(role, neg, out, depth + 1);
        } else {
            out.push(if neg {
                format!("!{glob}")
            } else {
                glob.to_string()
            });
        }
    }
}

/// `@role` or `!@role` as an entry of a glob list.
static REF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("^(!?)@([a-z][a-z0-9-]*)$").expect("regex"));
/// `@role` as an unquoted word of a check command.
static COMMAND_REF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\s)@([a-z][a-z0-9-]*)(?=$|[\s;|&)])").expect("regex"));
static QUOTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"'[^']*'|"(?:\\.|[^"\\])*""#).expect("regex"));

/// The role a glob-list entry names, if it is a role reference.
pub fn role_ref(glob: &str) -> Option<(bool, String)> {
    let c = REF.captures(glob).ok().flatten()?;
    Some((&c[1] == "!", c[2].to_string()))
}

/// The roles a rule names: in scope.files, when_changed, config_files, check commands and
/// where modules (`rolesOfRule`), sorted.
pub fn roles_of_rule(rule: &Rule) -> Vec<String> {
    let mut out = BTreeSet::new();
    let from_list = |l: Option<&Vec<String>>, out: &mut BTreeSet<String>| {
        for g in l.into_iter().flatten() {
            if let Some((_, name)) = role_ref(g) {
                out.insert(name);
            }
        }
    };
    from_list(rule.scope.files.as_ref(), &mut out);
    for t in &rule.enforce {
        match t {
            Target::Check(c) => {
                from_list(c.when_changed.as_ref(), &mut out);
                from_list(c.config_files.as_ref(), &mut out);
                for cmd in c.commands() {
                    let blank = QUOTED.replace_all(cmd, |caps: &fancy_regex::Captures<str>| {
                        " ".repeat(caps[0].chars().count())
                    });
                    for m in COMMAND_REF.captures_iter(blank.as_ref()).flatten() {
                        out.insert(m[1].to_string());
                    }
                }
            }
            Target::AstGrep(a) => {
                for c in a.where_.iter().flat_map(|w| w.values()) {
                    for m in where_modules(c) {
                        if let Some(name) = m.strip_prefix('@')
                            && REF.is_match(&m).unwrap_or(false)
                        {
                            out.insert(name.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out.into_iter().collect()
}

/// The roles a rule names that Forge does not know.
pub fn unknown_roles(rule: &Rule) -> Vec<String> {
    roles_of_rule(rule)
        .into_iter()
        .filter(|r| !is_role(r))
        .collect()
}

fn where_modules(c: &WhereCondition) -> Vec<String> {
    match c {
        WhereCondition::Not(inner) => where_modules(inner),
        WhereCondition::Deprecated(_) => Vec::new(),
        WhereCondition::Type(specs) | WhereCondition::Declared(specs) => specs
            .as_slice()
            .iter()
            .flat_map(|s| {
                s.module
                    .as_ref()
                    .map(|m| m.as_slice().to_vec())
                    .unwrap_or_default()
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_defaults_expand_references() {
        assert_eq!(
            default_patterns("env-files"),
            [
                "**/.env",
                "**/.env.*",
                "!**/.env.example",
                "!**/.env.sample",
                "!**/.env.template",
                "!**/.env.dist"
            ]
        );
    }
}
