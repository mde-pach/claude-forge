//! Which files a rule is about: its scope (`scope.language`, `scope.files`), file roles (`@role`
//! entries), and the paths no rule ever looks at.
//!
//! Globs match as the generated scripts matched them (shell `case` patterns, `src/core/globs.ts`
//! `globToCase`): `*` also matches `/`, `**/` is zero or more directories, `{a,b}` alternatives,
//! `[0-9]` / `[!x]` one character. ast-grep's own `files` globs (globset) behave the same way.

use std::collections::BTreeMap;
use std::sync::Arc;

use forge_packs::model::{Language, Rule};
use regex::Regex;

/// Paths that are never part of a change or of `--all`'s scope: git's, dependencies, virtual
/// environments, nested worktrees (Claude Code's `.claude/worktrees/`), and `.forge/` (local
/// packs and rules hold the rules' own patterns).
pub fn is_excluded(path: &str) -> bool {
    const DIRS: &[&str] = &[".git", "node_modules", ".venv"];
    if path.starts_with(".claude/worktrees/")
        || path == ".claude/worktrees"
        || path.starts_with(".forge/")
    {
        return true;
    }
    path.split('/')
        .rev()
        .skip(1)
        .any(|seg| DIRS.contains(&seg))
        || DIRS.iter().any(|d| path == *d)
}

/// The glob as an anchored regular expression with shell `case` semantics.
pub fn glob_regex(glob: &str) -> Regex {
    Regex::new(&format!("^(?:{})$", glob_source(glob)))
        .unwrap_or_else(|_| Regex::new("[^\\s\\S]").expect("never-matching regex"))
}

fn glob_source(glob: &str) -> String {
    let g: Vec<char> = glob.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < g.len() {
        let c = g[i];
        match c {
            '*' => {
                // `**/`: zero or more directories; any other run of `*`: anything
                let mut j = i;
                while j < g.len() && g[j] == '*' {
                    j += 1;
                }
                if j - i >= 2 && g.get(j) == Some(&'/') {
                    out.push_str("(?:.*/)?");
                    i = j + 1;
                    continue;
                }
                out.push_str(".*");
                i = j;
                continue;
            }
            '?' => out.push('.'),
            '[' => {
                let close = g.iter().skip(i + 2).position(|&x| x == ']').map(|p| p + i + 2);
                match close {
                    Some(end) => {
                        let mut body: String = g[i + 1..end].iter().collect();
                        let neg = body.starts_with('!') || body.starts_with('^');
                        if neg {
                            body.remove(0);
                        }
                        let body: String = body
                            .chars()
                            .map(|ch| {
                                if matches!(ch, '\\' | ']' | '^' | '[' | '&' | '~') {
                                    format!("\\{ch}")
                                } else {
                                    ch.to_string()
                                }
                            })
                            .collect();
                        out.push_str(&format!("[{}{body}]", if neg { "^" } else { "" }));
                        i = end;
                    }
                    None => out.push_str("\\["),
                }
            }
            '{' => {
                let mut alts = Vec::new();
                let mut depth = 0;
                let mut start = i + 1;
                let mut end = None;
                let mut k = i + 1;
                while k < g.len() && end.is_none() {
                    match g[k] {
                        '{' => depth += 1,
                        '}' if depth > 0 => depth -= 1,
                        '}' => end = Some(k),
                        ',' if depth == 0 => {
                            alts.push(g[start..k].iter().collect::<String>());
                            start = k + 1;
                        }
                        _ => {}
                    }
                    k += 1;
                }
                match end {
                    None => out.push_str("\\{"),
                    Some(end) => {
                        alts.push(g[start..end].iter().collect());
                        let parts: Vec<String> = alts.iter().map(|a| glob_source(a)).collect();
                        out.push_str(&format!("(?:{})", parts.join("|")));
                        i = end;
                    }
                }
            }
            c => out.push_str(&regex::escape(&c.to_string())),
        }
        i += 1;
    }
    out
}

/// Every file role's effective patterns ("!" before an exclusion), by name.
#[derive(Debug, Clone, Default)]
pub struct Roles {
    map: BTreeMap<String, Vec<String>>,
}

impl Roles {
    /// Forge's default patterns (`src/roles.ts` ROLES).
    pub fn defaults() -> Roles {
        let map = forge_packs::roles::ROLES
            .iter()
            .map(|(n, _)| (n.to_string(), forge_packs::roles::default_patterns(n)))
            .collect();
        Roles { map }
    }

    /// The defaults, with forge.yaml's `files:` changes (`set` replaces, `add` appends).
    pub fn with_config(config: Option<&forge_packs::ProjectConfig>) -> Roles {
        let mut roles = Roles::defaults();
        if let Some(files) = config.and_then(|c| c.files.as_ref()) {
            for (name, change) in files {
                let entry = roles.map.entry(name.clone()).or_default();
                if let Some(set) = &change.set {
                    *entry = set.clone();
                }
                if let Some(add) = &change.add {
                    entry.extend(add.iter().cloned());
                }
            }
            // a role named in another role's patterns: expanded once more
            let names: Vec<String> = roles.map.keys().cloned().collect();
            for n in names {
                let globs = roles.map[&n].clone();
                let expanded = roles.expand(&globs);
                roles.map.insert(n, expanded);
            }
        }
        roles
    }

    pub fn from_map(map: BTreeMap<String, Vec<String>>) -> Roles {
        Roles { map }
    }

    pub fn map(&self) -> &BTreeMap<String, Vec<String>> {
        &self.map
    }

    /// A glob list with each `@role` / `!@role` entry replaced by the role's patterns.
    pub fn expand(&self, globs: &[String]) -> Vec<String> {
        let mut out = Vec::new();
        for g in globs {
            match forge_packs::roles::role_ref(g) {
                Some((neg, name)) => {
                    for p in self.map.get(&name).into_iter().flatten() {
                        let (pneg, pg) = match p.strip_prefix('!') {
                            Some(rest) => (true, rest),
                            None => (false, p.as_str()),
                        };
                        out.push(if neg != pneg {
                            format!("!{pg}")
                        } else {
                            pg.to_string()
                        });
                    }
                }
                None => out.push(g.clone()),
            }
        }
        out
    }

    /// Whether `path` is one of the role's files (exclusions first).
    pub fn matches(&self, role: &str, path: &str) -> bool {
        let name = role.trim_start_matches('@');
        let Some(globs) = self.map.get(name) else {
            return false;
        };
        GlobList::new(globs).matches(path)
    }
}

/// Include and exclude globs ("!" before an exclusion), compiled.
#[derive(Debug, Clone, Default)]
pub struct GlobList {
    include: Vec<Regex>,
    exclude: Vec<Regex>,
}

impl GlobList {
    pub fn new(globs: &[String]) -> GlobList {
        let mut l = GlobList::default();
        for g in globs {
            match g.strip_prefix('!') {
                Some(x) => l.exclude.push(glob_regex(x)),
                None => l.include.push(glob_regex(g)),
            }
        }
        l
    }

    pub fn is_empty(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty()
    }

    pub fn has_include(&self) -> bool {
        !self.include.is_empty()
    }

    /// Not excluded, and included (or there are no include globs).
    pub fn matches(&self, path: &str) -> bool {
        if self.exclude.iter().any(|r| r.is_match(path)) {
            return false;
        }
        self.include.is_empty() || self.include.iter().any(|r| r.is_match(path))
    }

    pub fn any(&self, path: &str) -> bool {
        self.include.iter().any(|r| r.is_match(path))
    }
}

/// `LANGUAGE_CASE` (`src/render/checks.ts`): the files of a scope language for grep and check
/// targets.
pub fn language_globs(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Python => &["*.py", "*.pyi"],
        Language::Typescript => &[
            "*.ts", "*.tsx", "*.mts", "*.cts", "*.js", "*.jsx", "*.mjs", "*.cjs",
        ],
        Language::Tsx => &["*.tsx", "*.jsx"],
        Language::Yaml => &["*.yaml", "*.yml"],
        Language::Any => &[],
    }
}

/// A rule's scope for grep and check targets (`scopeFunction`): a config file of the check is
/// in; an excluded file is out; an include glob must match; the language's files must match.
#[derive(Debug, Clone)]
pub struct RuleScope {
    config: GlobList,
    files: GlobList,
    lang: GlobList,
}

impl RuleScope {
    /// `dirs`: the workspace packages the rule's pack was detected in (globs then relative to
    /// them).
    pub fn new(
        rule: &Rule,
        roles: &Roles,
        dirs: Option<&[String]>,
        config_files: Option<&[String]>,
    ) -> RuleScope {
        let files = roles.expand(rule.scope.files.as_deref().unwrap_or(&[]));
        let files = in_dirs(&files, dirs);
        let config = config_files
            .map(|c| in_dirs(&roles.expand(c), dirs))
            .unwrap_or_default();
        let lang: Vec<String> = language_globs(rule.scope.language)
            .iter()
            .map(|s| s.to_string())
            .collect();
        RuleScope {
            config: GlobList::new(&config),
            files: GlobList::new(&files),
            lang: GlobList::new(&lang),
        }
    }

    pub fn matches(&self, path: &str) -> bool {
        if self.config.any(path) {
            return true;
        }
        if !self.files.matches(path) {
            return false;
        }
        self.lang.is_empty() || self.lang.any(path)
    }

    /// The file globs only (no language): ast-grep rules' `files` / `ignores`.
    pub fn files_match(&self, path: &str) -> bool {
        self.files.matches(path)
    }
}

/// `scopeInDirs`: a glob list relative to each workspace package (the whole package when the
/// list has no include glob); without dirs, the list as it is.
pub fn in_dirs(files: &[String], dirs: Option<&[String]>) -> Vec<String> {
    let Some(dirs) = dirs else {
        return files.to_vec();
    };
    let include: Vec<&String> = files.iter().filter(|g| !g.starts_with('!')).collect();
    let all = "**".to_string();
    let include: Vec<&String> = if include.is_empty() {
        vec![&all]
    } else {
        include
    };
    let mut out: Vec<String> = include
        .iter()
        .flat_map(|g| dirs.iter().map(move |d| format!("{d}/{g}")))
        .collect();
    for g in files.iter().filter_map(|g| g.strip_prefix('!')) {
        for d in dirs {
            out.push(format!("!{d}/{g}"));
        }
    }
    out
}

/// Documentation and prose files: grep rules never look at them (a line explaining what not
/// to write is not that code).
pub fn is_prose(path: &str) -> bool {
    static PROSE: std::sync::LazyLock<GlobList> = std::sync::LazyLock::new(|| {
        GlobList::new(
            &[
                "*.md", "*.mdx", "*.rst", "*.markdown", "*.adoc", "docs/**", "doc/**",
                "README*", "CHANGELOG*", "**/README*", "**/CHANGELOG*",
            ]
            .map(String::from),
        )
    });
    PROSE.any(path)
}

/// Shared, cheap to clone.
pub type SharedRoles = Arc<Roles>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_semantics() {
        for (glob, path, want) in [
            ("*.py", "a/b.py", true),
            ("**/*.md", "README.md", true),
            ("**/migrations/[0-9]*.py", "app/migrations/0001_x.py", true),
            ("**/migrations/[0-9]*.py", "app/migrations/x.py", false),
            ("docs/**", "docs/a/b", true),
            ("{,src/}a.ts", "src/a.ts", true),
            ("{,src/}a.ts", "a.ts", true),
            ("Dockerfile", "x/Dockerfile", false),
            ("**/Dockerfile*", "Dockerfile.dev", true),
        ] {
            assert_eq!(glob_regex(glob).is_match(path), want, "{glob} ~ {path}");
        }
    }

    #[test]
    fn excluded_dirs() {
        assert!(is_excluded("node_modules/x/y.js"));
        assert!(is_excluded("a/node_modules/x.js"));
        assert!(is_excluded(".claude/worktrees/agent/a.py"));
        assert!(is_excluded(".venv/lib/a.py"));
        assert!(!is_excluded("src/venv.py"));
        assert!(!is_excluded(".claude/settings.json"));
    }

    #[test]
    fn roles_expand() {
        let r = Roles::defaults();
        assert!(r.matches("@compose", "docker-compose.yml"));
        assert!(r.matches("dockerfile", "Dockerfile"));
        let e = r.expand(&["@compose".into(), "!x".into()]);
        assert!(e.contains(&"**/compose*.yaml".to_string()));
    }
}
