//! File roles' effective patterns (§15.4, `src/roles.ts`): Forge's defaults, replaced by what
//! the tool's own declarative config says (data only, never code), then changed by forge.yaml
//! (`files: {<role>: {set | add}}`). The role names and defaults are
//! [`forge_packs::roles::ROLES`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::LazyLock;

use fancy_regex::Regex;
use forge_packs::model::{RoleChange, Rule, Target, TypeSpec, WhereCondition};
use forge_packs::roles::{ROLES, is_role, role_ref, roles_of_rule};
use serde::Serialize;
use serde_json::Value;

use crate::deps::{read_json, read_toml};

pub const FORGE_DEFAULT: &str = "forge default";
pub const FORGE_YAML: &str = "forge.yaml";

/// A pattern of a role and where it comes from: "forge default", the tool's config file, or "forge.yaml".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RolePattern {
    pub glob: String,
    pub exclude: bool,
    pub source: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct EffectiveRole {
    pub patterns: Vec<RolePattern>,
    /// What the tool's config said that Forge could not use.
    pub notes: Vec<String>,
}

impl EffectiveRole {
    /// The patterns as glob-list entries ("!" before an exclusion).
    pub fn globs(&self) -> Vec<String> {
        self.patterns
            .iter()
            .map(|p| {
                if p.exclude {
                    format!("!{}", p.glob)
                } else {
                    p.glob.clone()
                }
            })
            .collect()
    }
}

pub type Roles = BTreeMap<String, EffectiveRole>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleError(pub String);

impl std::fmt::Display for RoleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RoleError {}

struct Part {
    patterns: Vec<String>,
    source: String,
}

fn part(patterns: Vec<String>, source: &str) -> Part {
    Part {
        patterns,
        source: source.to_string(),
    }
}

/// What role configs read: the repository and its files.
pub struct RoleInputs<'a> {
    pub repo: &'a Path,
    pub files: &'a [String],
}

impl RoleInputs<'_> {
    fn text(&self, rel: &str) -> Option<String> {
        std::fs::read_to_string(self.repo.join(rel)).ok()
    }
    fn toml(&self, rel: &str) -> Option<Value> {
        read_toml(&self.repo.join(rel))
    }
    fn json(&self, rel: &str) -> Option<Value> {
        read_json(&self.repo.join(rel))
    }
}

fn under(dir: &str, glob: &str) -> String {
    if dir.is_empty() {
        glob.to_string()
    } else {
        format!("{dir}/{glob}")
    }
}

/// An INI file's sections: name → key → value (continuation lines joined with a space).
pub fn parse_ini(text: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    static HEAD: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"^\s*\[([^\]]+)\]\s*$").expect("regex"));
    static KV: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"^([^=:]+?)\s*[=:]\s*(.*)$").expect("regex"));
    let mut out: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut section: Option<String> = None;
    let mut last: Option<String> = None;
    for raw in text.lines() {
        let t = raw.trim_start();
        if t.is_empty() || t.starts_with('#') || t.starts_with(';') {
            continue;
        }
        if let Some(c) = HEAD.captures(raw) {
            let name = c[1].trim().to_string();
            out.entry(name.clone()).or_default();
            section = Some(name);
            last = None;
            continue;
        }
        let Some(s) = &section else { continue };
        let sec = out.get_mut(s).expect("section");
        if raw.starts_with(char::is_whitespace)
            && let Some(l) = &last
        {
            let v = format!("{} {}", sec.get(l).cloned().unwrap_or_default(), raw.trim());
            sec.insert(l.clone(), v.trim().to_string());
            continue;
        }
        if let Some(c) = KV.captures(raw) {
            let k = c[1].trim().to_string();
            sec.insert(k.clone(), c[2].trim().to_string());
            last = Some(k);
        }
    }
    out
}

/// A path from a config file, relative to the repository: "./x/", "%(here)s/x" → "x".
fn clean_path(p: &str, base: &str) -> Option<String> {
    let s = p.trim();
    let s = s
        .strip_prefix("%(here)s/")
        .or_else(|| s.strip_prefix("%(here)s\\"))
        .or_else(|| s.strip_prefix("%(here)s"))
        .unwrap_or(s)
        .replace('\\', "/");
    if s.is_empty()
        || s.starts_with('/')
        || (s.len() > 1 && s.as_bytes()[1] == b':' && s.as_bytes()[0].is_ascii_alphabetic())
        || s.contains("%(")
    {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    for seg in base.split('/').chain(s.split('/')) {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            x => parts.push(x),
        }
    }
    Some(parts.join("/"))
}

fn list(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        Some(Value::String(s)) => s.split_whitespace().map(str::to_owned).collect(),
        _ => Vec::new(),
    }
}

/// pytest's ini options, in pytest's own order of config files.
fn pytest_options(inputs: &RoleInputs) -> Option<(serde_json::Map<String, Value>, String)> {
    let ini_map = |m: &BTreeMap<String, String>| {
        m.iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect::<serde_json::Map<_, _>>()
    };
    if let Some(ini) = inputs.text("pytest.ini") {
        let opts = parse_ini(&ini)
            .get("pytest")
            .map(ini_map)
            .unwrap_or_default();
        return Some((opts, "pytest.ini".into()));
    }
    if let Some(py) = inputs
        .toml("pyproject.toml")
        .and_then(|t| t.pointer("/tool/pytest").cloned())
        && let Value::Object(py) = py
    {
        let mut opts = py
            .get("ini_options")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        for (k, v) in py.iter().filter(|(k, _)| *k != "ini_options") {
            opts.insert(k.clone(), v.clone());
        }
        return Some((opts, "pyproject.toml".into()));
    }
    for (file, section) in [("tox.ini", "pytest"), ("setup.cfg", "tool:pytest")] {
        if let Some(s) = parse_ini(&inputs.text(file).unwrap_or_default()).get(section) {
            return Some((ini_map(s), file.into()));
        }
    }
    None
}

fn python_tests(inputs: &RoleInputs, _notes: &mut Vec<String>) -> Option<Vec<Part>> {
    let (opts, source) = pytest_options(inputs)?;
    let testpaths: Vec<String> = list(opts.get("testpaths"))
        .iter()
        .filter_map(|p| clean_path(p, ""))
        .collect();
    let py_files = list(opts.get("python_files"));
    if testpaths.is_empty() && py_files.is_empty() {
        return None;
    }
    let names = if py_files.is_empty() {
        vec!["test_*.py".to_string(), "*_test.py".to_string()]
    } else {
        py_files
    };
    let dirs = if testpaths.is_empty() {
        vec![String::new()]
    } else {
        testpaths
    };
    let mut patterns: Vec<String> = dirs
        .iter()
        .flat_map(|d| names.iter().map(move |n| under(d, &format!("**/{n}"))))
        .collect();
    patterns.push("**/conftest.py".into());
    Some(vec![part(patterns, &source)])
}

type Getter = Box<dyn Fn(&str) -> Option<Value>>;

fn alembic_migrations(inputs: &RoleInputs, notes: &mut Vec<String>) -> Option<Vec<Part>> {
    let ini = parse_ini(&inputs.text("alembic.ini").unwrap_or_default());
    let from_ini = ini.get("alembic");
    let from_toml = inputs
        .toml("pyproject.toml")
        .and_then(|t| t.pointer("/tool/alembic").cloned());
    let (get, source): (Getter, &str) = if let Some(s) = from_ini {
        let s = s.clone();
        (
            Box::new(move |k: &str| s.get(k).map(|v| Value::String(v.clone()))),
            "alembic.ini",
        )
    } else if let Some(t) = from_toml {
        (Box::new(move |k: &str| t.get(k).cloned()), "pyproject.toml")
    } else {
        return None;
    };
    if let Some(versions) = get("version_locations") {
        let parts: Vec<String> = match &versions {
            Value::Array(a) => a
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
            v => {
                let s = v.as_str().unwrap_or("").to_string();
                let sep = if source == "alembic.ini" {
                    let i = from_ini.expect("ini");
                    i.get("path_separator")
                        .or_else(|| i.get("version_path_separator"))
                        .cloned()
                        .unwrap_or_else(|| "space".into())
                } else {
                    "space".into()
                };
                let split: Vec<&str> = match sep.as_str() {
                    "space" | "os" => s.split_whitespace().collect(),
                    "colon" => s.split(':').collect(),
                    "semicolon" => s.split(';').collect(),
                    "newline" => s.split('\n').collect(),
                    _ => s.split(',').collect(),
                };
                split.into_iter().map(str::to_owned).collect()
            }
        };
        let dirs: Vec<Option<String>> = parts.iter().map(|p| clean_path(p, "")).collect();
        if dirs.iter().all(Option::is_some) {
            return Some(vec![part(
                dirs.into_iter()
                    .map(|d| under(&d.expect("dir"), "**"))
                    .collect(),
                source,
            )]);
        }
        notes.push(format!(
            "{source}: version_locations {} is not a path in the repository",
            versions
        ));
        return None;
    }
    let loc = get("script_location")?;
    let loc = loc.as_str().filter(|s| !s.is_empty())?.to_string();
    let dir = if loc.contains(':') && !loc.starts_with("%(here)s") {
        None
    } else {
        clean_path(&loc, "")
    };
    match dir {
        None => {
            notes.push(format!("{source}: script_location {} is not a path in the repository (set files: alembic-migrations in forge.yaml)", Value::String(loc)));
            None
        }
        Some(d) => Some(vec![part(vec![under(&d, "versions/**")], source)]),
    }
}

struct PrismaSchema {
    path: String,
    dir: String,
    source: String,
}

fn dirname(p: &str) -> String {
    match p.rfind('/') {
        Some(i) => p[..i].to_string(),
        None => String::new(),
    }
}

fn prisma_schema(inputs: &RoleInputs) -> Option<PrismaSchema> {
    if let Some(set) = inputs.json("package.json").and_then(|p| {
        p.pointer("/prisma/schema")
            .and_then(Value::as_str)
            .map(str::to_owned)
    }) {
        let p = clean_path(&set, "")?;
        let is_file = p.ends_with(".prisma");
        return Some(PrismaSchema {
            dir: if is_file { dirname(&p) } else { p.clone() },
            path: p,
            source: "package.json".into(),
        });
    }
    for p in ["prisma/schema.prisma", "schema.prisma"] {
        if inputs.repo.join(p).exists() {
            return Some(PrismaSchema {
                path: p.into(),
                dir: dirname(p),
                source: p.into(),
            });
        }
    }
    None
}

fn prisma_migrations(inputs: &RoleInputs, _: &mut Vec<String>) -> Option<Vec<Part>> {
    let s = prisma_schema(inputs)?;
    (s.source == "package.json")
        .then(|| vec![part(vec![under(&s.dir, "migrations/**")], "package.json")])
}

fn generated_clients(inputs: &RoleInputs, _: &mut Vec<String>) -> Option<Vec<Part>> {
    static BLOCK: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?m)^\s*generator\s+\w+\s*\{([^}]*)\}").expect("regex")
    });
    static OUTPUT: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r#"(?m)^\s*output\s*=\s*"([^"]+)""#).expect("regex"));
    let s = prisma_schema(inputs)?;
    let files: Vec<String> = if s.path.ends_with(".prisma") {
        vec![s.path.clone()]
    } else {
        let prefix = if s.dir.is_empty() {
            String::new()
        } else {
            format!("{}/", s.dir)
        };
        inputs
            .files
            .iter()
            .filter(|f| f.starts_with(&prefix) && f.ends_with(".prisma"))
            .cloned()
            .collect()
    };
    let mut out = Vec::new();
    for f in files {
        let text = inputs.text(&f).unwrap_or_default();
        let dir = dirname(&f);
        for block in BLOCK.captures_iter(&text) {
            if let Some(m) = OUTPUT.captures(&block[1])
                && let Some(p) = clean_path(&m[1], &dir)
            {
                out.push(part(vec![under(&p, "**")], &f));
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

fn django_settings(inputs: &RoleInputs, _: &mut Vec<String>) -> Option<Vec<Part>> {
    static MODULE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"^[A-Za-z_][\w.]*$").expect("regex"));
    let (opts, source) = pytest_options(inputs)?;
    let m = opts.get("DJANGO_SETTINGS_MODULE")?.as_str()?.to_string();
    if !MODULE.is_match(&m) {
        return None;
    }
    let p = m.replace('.', "/");
    Some(vec![part(
        vec![format!("{p}.py"), format!("{p}/*.py")],
        &source,
    )])
}

fn adr(inputs: &RoleInputs, _: &mut Vec<String>) -> Option<Vec<Part>> {
    let dir = clean_path(inputs.text(".adr-dir")?.trim(), "")?;
    (!dir.is_empty()).then(|| vec![part(vec![under(&dir, "**")], ".adr-dir")])
}

fn sql_migrations(inputs: &RoleInputs, notes: &mut Vec<String>) -> Option<Vec<Part>> {
    static FLYWAY: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?m)^\s*flyway\.locations\s*=\s*(.+)$").expect("regex")
    });
    static ATLAS: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r#"\bdir\s*=\s*"file://([^"?]+)"#).expect("regex"));
    let mut parts = Vec::new();
    let mut flyway: Option<Part> = None;
    let mut found_conf = false;
    for f in ["flyway.conf", "conf/flyway.conf"] {
        if let Some(text) = inputs.text(f)
            && let Some(m) = FLYWAY.captures(&text)
        {
            flyway = flyway_part(m[1].split(',').map(str::to_owned).collect(), f, notes);
            found_conf = true;
            break;
        }
    }
    if !found_conf
        && let Some(loc) = inputs
            .toml("flyway.toml")
            .and_then(|t| t.pointer("/flyway/locations").cloned())
    {
        flyway = flyway_part(list(Some(&loc)), "flyway.toml", notes);
    }
    parts.push(flyway.unwrap_or_else(|| part(vec!["db/migration/**/*.sql".into()], FORGE_DEFAULT)));
    parts.push(part(
        vec![
            "supabase/migrations/*.sql".into(),
            "db/migrations/*.sql".into(),
        ],
        FORGE_DEFAULT,
    ));
    if inputs.repo.join("sqitch.plan").exists() {
        let top = parse_ini(&inputs.text("sqitch.conf").unwrap_or_default())
            .get("core")
            .and_then(|c| c.get("top_dir").cloned())
            .and_then(|t| clean_path(&t, ""))
            .unwrap_or_default();
        parts.push(part(vec![under(&top, "deploy/**/*.sql")], "sqitch.plan"));
    }
    if let Some(atlas) = inputs.text("atlas.hcl") {
        let mut dirs: Vec<String> = Vec::new();
        for m in ATLAS.captures_iter(&atlas) {
            if let Some(d) = clean_path(&m[1], "")
                && !dirs.contains(&d)
            {
                dirs.push(d);
            }
        }
        if dirs.is_empty() {
            dirs.push("migrations".into());
        }
        parts.push(part(
            dirs.iter().map(|d| under(d, "*.sql")).collect(),
            "atlas.hcl",
        ));
    }
    Some(parts)
}

fn flyway_part(locations: Vec<String>, source: &str, notes: &mut Vec<String>) -> Option<Part> {
    let mut dirs = Vec::new();
    for raw in locations.iter().map(|l| l.trim()).filter(|l| !l.is_empty()) {
        match raw.strip_prefix("filesystem:").and_then(|d| clean_path(d, "")) {
            Some(d) => dirs.push(d),
            None => notes.push(format!(
                "{source}: location {} is not a directory of the repository (set files: sql-migrations in forge.yaml)",
                Value::String(raw.to_string())
            )),
        }
    }
    (!dirs.is_empty()).then(|| part(dirs.iter().map(|d| under(d, "**/*.sql")).collect(), source))
}

fn angular_tsconfig(inputs: &RoleInputs, _: &mut Vec<String>) -> Option<Vec<Part>> {
    let projects = inputs
        .json("angular.json")?
        .get("projects")?
        .as_object()?
        .clone();
    let mut files: Vec<String> = Vec::new();
    for p in projects.values() {
        let t = p
            .get("architect")
            .or_else(|| p.get("targets"))
            .and_then(|a| a.pointer("/build/options/tsConfig"))
            .and_then(Value::as_str)
            .and_then(|f| clean_path(f, ""));
        if let Some(f) = t.filter(|f| !f.is_empty())
            && !files.contains(&f)
        {
            files.push(f);
        }
    }
    (!files.is_empty()).then(|| vec![part(files, "angular.json")])
}

fn esm_modules(inputs: &RoleInputs, _: &mut Vec<String>) -> Option<Vec<Part>> {
    let mut manifests: Vec<&String> = inputs
        .files
        .iter()
        .filter(|f| {
            (*f == "package.json" || f.ends_with("/package.json"))
                && !f.starts_with("node_modules/")
                && !f.contains("/node_modules/")
        })
        .collect();
    manifests.sort();
    let esm: BTreeSet<String> = manifests
        .iter()
        .filter(|f| {
            inputs
                .json(f)
                .is_some_and(|p| p.get("type").and_then(Value::as_str) == Some("module"))
        })
        .map(|f| dirname(f))
        .collect();
    if esm.is_empty() {
        return None;
    }
    let ext = "*.{ts,tsx,js,jsx}";
    let mut patterns = vec!["**/*.mts".to_string(), "**/*.mjs".to_string()];
    for d in &esm {
        patterns.push(under(d, &format!("**/{ext}")));
    }
    for f in &manifests {
        let d = dirname(f);
        if esm.contains(&d) {
            continue;
        }
        if esm
            .iter()
            .any(|e| e.is_empty() || d.starts_with(&format!("{e}/")))
        {
            patterns.push(format!("!{}", under(&d, &format!("**/{ext}"))));
        }
    }
    Some(vec![part(patterns, "package.json")])
}

type ConfigReader = fn(&RoleInputs, &mut Vec<String>) -> Option<Vec<Part>>;

fn config_of(name: &str) -> Option<ConfigReader> {
    Some(match name {
        "adr" => adr,
        "alembic-migrations" => alembic_migrations,
        "angular-tsconfig" => angular_tsconfig,
        "django-settings" => django_settings,
        "esm-modules" => esm_modules,
        "generated-clients" => generated_clients,
        "prisma-migrations" => prisma_migrations,
        "python-tests" => python_tests,
        "sql-migrations" => sql_migrations,
        _ => return None,
    })
}

/// What a role is, for status and docs.
pub fn about(name: &str) -> &'static str {
    match name {
        "adr" => "architecture decision records",
        "alembic-migrations" => "Alembic revision files",
        "angular-tsconfig" => "the tsconfig each Angular application is built with",
        "better-auth-config" => "the Better Auth config (the auth CLI's search paths)",
        "build-artifacts" => "dependency, cache and build output that is never committed",
        "compose" => "Docker Compose files",
        "django-migrations" => "Django migration modules",
        "django-settings" => "Django settings modules",
        "django-templates" => "Django templates",
        "docs" => "documentation",
        "dockerfile" => "Dockerfiles and Containerfiles",
        "drizzle-migrations" => "Drizzle migrations",
        "e2e-tests" => "end-to-end tests",
        "env-files" => "dotenv files",
        "env-template" => "dotenv templates",
        "esm-modules" => "ES module files",
        "generated-clients" => "generated API and database clients",
        "gunicorn-config" => "Gunicorn config files",
        "lockfiles" => "dependency lock files",
        "prisma-migrations" => "Prisma migrations",
        "procfile" => "Procfiles",
        "python-tests" => "pytest test files",
        "sql-migrations" => "plain SQL migrations",
        "ts-tests" => "TypeScript and JavaScript test files",
        "typeorm-datasource" => "the TypeORM data source file",
        "wrangler-config" => "Wrangler configs",
        "wrangler-types" => "the `wrangler types` output",
        _ => "",
    }
}

/// Every role's effective patterns: the defaults (or the tool's config), then forge.yaml.
/// Without inputs, Forge's defaults only.
pub fn resolve_roles(
    inputs: Option<&RoleInputs>,
    config: Option<&indexmap::IndexMap<String, RoleChange>>,
) -> Result<Roles, RoleError> {
    if let Some(c) = config {
        for name in c.keys() {
            if !is_role(name) {
                return Err(RoleError(format!(
                    "forge.yaml: files: unknown file role \"{name}\" (known: {})",
                    ROLES.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
                )));
            }
        }
    }
    let mut raw: BTreeMap<&str, Vec<(String, String)>> = BTreeMap::new();
    let mut notes: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (name, defaults) in ROLES {
        let mut n = Vec::new();
        let cfg = config.and_then(|c| c.get(*name));
        let mut parts = if let Some(set) = cfg.and_then(|c| c.set.clone()) {
            vec![part(set, FORGE_YAML)]
        } else {
            inputs
                .and_then(|i| config_of(name).and_then(|f| f(i, &mut n)))
                .unwrap_or_else(|| {
                    vec![part(
                        defaults.iter().map(|s| s.to_string()).collect(),
                        FORGE_DEFAULT,
                    )]
                })
        };
        if let Some(add) = cfg.and_then(|c| c.add.clone()) {
            parts.push(part(add, FORGE_YAML));
        }
        raw.insert(
            name,
            parts
                .into_iter()
                .flat_map(|p| {
                    let src = p.source;
                    p.patterns.into_iter().map(move |g| (g, src.clone()))
                })
                .collect(),
        );
        notes.insert(name, n);
    }
    let mut done = Roles::new();
    for (name, _) in ROLES {
        expand(name, &raw, &notes, &mut done, &mut Vec::new())?;
    }
    Ok(done)
}

fn expand(
    name: &str,
    raw: &BTreeMap<&str, Vec<(String, String)>>,
    notes: &BTreeMap<&str, Vec<String>>,
    done: &mut Roles,
    stack: &mut Vec<String>,
) -> Result<EffectiveRole, RoleError> {
    if let Some(r) = done.get(name) {
        return Ok(r.clone());
    }
    if stack.iter().any(|s| s == name) {
        stack.push(name.to_string());
        return Err(RoleError(format!(
            "file roles: @{} is a cycle",
            stack.join(" -> @")
        )));
    }
    let mut patterns = Vec::new();
    for (glob, source) in &raw[name] {
        match role_ref(glob) {
            None => patterns.push(match glob.strip_prefix('!') {
                Some(g) => RolePattern {
                    glob: g.to_string(),
                    exclude: true,
                    source: source.clone(),
                },
                None => RolePattern {
                    glob: glob.clone(),
                    exclude: false,
                    source: source.clone(),
                },
            }),
            Some((neg, other)) => {
                if !is_role(&other) {
                    return Err(RoleError(format!(
                        "file role {name}: unknown file role \"@{other}\""
                    )));
                }
                stack.push(name.to_string());
                let inner = expand(&other, raw, notes, done, stack)?;
                stack.pop();
                patterns.extend(negate(inner.patterns, neg, &format!("@{name}"))?);
            }
        }
    }
    let role = EffectiveRole {
        patterns: dedupe(patterns),
        notes: notes.get(name).cloned().unwrap_or_default(),
    };
    done.insert(name.to_string(), role.clone());
    Ok(role)
}

fn negate(patterns: Vec<RolePattern>, neg: bool, at: &str) -> Result<Vec<RolePattern>, RoleError> {
    if !neg {
        return Ok(patterns);
    }
    if patterns.iter().any(|p| p.exclude) {
        return Err(RoleError(format!(
            "{at}: a role with exclusions cannot be negated"
        )));
    }
    Ok(patterns
        .into_iter()
        .map(|p| RolePattern { exclude: true, ..p })
        .collect())
}

fn dedupe(patterns: Vec<RolePattern>) -> Vec<RolePattern> {
    let mut seen = BTreeSet::new();
    patterns
        .into_iter()
        .filter(|p| seen.insert(format!("{}{}", if p.exclude { "!" } else { "" }, p.glob)))
        .collect()
}

/// A glob list with its roles expanded; `empty` names the roles that have no file pattern.
pub fn expand_globs(
    globs: &[String],
    roles: &Roles,
) -> Result<(Vec<String>, Vec<String>), RoleError> {
    let mut out: Vec<String> = Vec::new();
    let mut empty = Vec::new();
    for g in globs {
        match role_ref(g) {
            None => {
                if !out.contains(g) {
                    out.push(g.clone());
                }
            }
            Some((neg, name)) => {
                let role = roles
                    .get(&name)
                    .ok_or_else(|| RoleError(format!("unknown file role \"@{name}\"")))?;
                if !role.patterns.iter().any(|p| !p.exclude) {
                    empty.push(name.clone());
                }
                let r = EffectiveRole {
                    patterns: negate(role.patterns.clone(), neg, g)?,
                    notes: Vec::new(),
                };
                for x in r.globs() {
                    if !out.contains(&x) {
                        out.push(x);
                    }
                }
            }
        }
    }
    Ok((out, empty))
}

static MODULE_REF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("^@([a-z][a-z0-9-]*)$").expect("regex"));

fn role_modules(name: &str, roles: &Roles, python: bool) -> Result<Vec<String>, RoleError> {
    static PY_EXT: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"\.pyi?$").expect("regex"));
    static TS_EXT: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"(\.d)?\.[cm]?[jt]sx?$").expect("regex"));
    let role = roles.get(name).cloned().unwrap_or_default();
    let mut out = Vec::new();
    for p in role.patterns.iter().filter(|p| !p.exclude) {
        if p.glob.contains(['*', '?', '[', '{']) {
            return Err(RoleError(format!(
                "file role @{name} is a where module: its patterns must be files, not globs ({})",
                p.glob
            )));
        }
        out.push(if python {
            let s = PY_EXT.replace(&p.glob, "").into_owned();
            let s = s.strip_suffix("/__init__").unwrap_or(&s);
            let s = s.strip_prefix("src/").unwrap_or(s);
            s.replace('/', ".")
        } else {
            TS_EXT.replace(&p.glob, "").into_owned()
        });
    }
    Ok(out)
}

fn map_where(c: &WhereCondition, roles: &Roles, python: bool) -> Result<WhereCondition, RoleError> {
    use forge_packs::model::OneOrMany;
    let map = |t: &TypeSpec| -> Result<TypeSpec, RoleError> {
        let Some(m) = &t.module else {
            return Ok(t.clone());
        };
        let mut mods = Vec::new();
        for x in m.as_slice() {
            match MODULE_REF
                .captures(x.as_str())
                .ok()
                .flatten()
                .and_then(|c| c.get(1))
            {
                Some(c) => mods.extend(role_modules(c.as_str(), roles, python)?),
                None => mods.push(x.clone()),
            }
        }
        Ok(TypeSpec {
            module: Some(OneOrMany::Many(mods)),
            ..t.clone()
        })
    };
    let specs = |v: &OneOrMany<TypeSpec>| -> Result<OneOrMany<TypeSpec>, RoleError> {
        Ok(match v {
            OneOrMany::One(t) => OneOrMany::One(map(t)?),
            OneOrMany::Many(ts) => OneOrMany::Many(ts.iter().map(map).collect::<Result<_, _>>()?),
        })
    };
    Ok(match c {
        WhereCondition::Not(inner) => {
            WhereCondition::Not(Box::new(map_where(inner, roles, python)?))
        }
        WhereCondition::Deprecated(b) => WhereCondition::Deprecated(*b),
        WhereCondition::Type(v) => WhereCondition::Type(specs(v)?),
        WhereCondition::Declared(v) => WhereCondition::Declared(specs(v)?),
    })
}

/// The rule with its roles replaced by their effective patterns (scope.files, when_changed,
/// config_files, where modules); `empty` lists the roles that leave it without files.
pub fn apply_roles(rule: &Rule, roles: &Roles) -> Result<(Rule, Vec<String>), RoleError> {
    if roles_of_rule(rule).is_empty() {
        return Ok((rule.clone(), Vec::new()));
    }
    let mut empty = BTreeSet::new();
    let mut list = |globs: &[String]| -> Result<Vec<String>, RoleError> {
        let (out, e) = expand_globs(globs, roles)?;
        if globs.iter().any(|g| !g.starts_with('!')) && !out.iter().any(|g| !g.starts_with('!')) {
            empty.extend(e);
        }
        Ok(out)
    };
    let mut r = rule.clone();
    if let Some(files) = &rule.scope.files {
        r.scope.files = Some(list(files)?);
    }
    let python = rule.scope.language == forge_packs::model::Language::Python;
    for t in r.enforce.iter_mut() {
        match t {
            Target::Check(c) => {
                if let Some(w) = &c.when_changed {
                    c.when_changed = Some(list(w)?);
                }
                if let Some(cf) = &c.config_files {
                    c.config_files = Some(expand_globs(cf, roles)?.0);
                }
            }
            Target::AstGrep(a) => {
                if let Some(w) = &a.where_ {
                    let mut m = indexmap::IndexMap::new();
                    for (k, c) in w {
                        m.insert(k.clone(), map_where(c, roles, python)?);
                    }
                    a.where_ = Some(m);
                }
            }
            _ => {}
        }
    }
    Ok((r, empty.into_iter().collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini_and_paths() {
        let ini = parse_ini(
            "[alembic]\nscript_location = %(here)s/migrations\nversion_locations = a\n  b\n",
        );
        assert_eq!(ini["alembic"]["script_location"], "%(here)s/migrations");
        assert_eq!(ini["alembic"]["version_locations"], "a b");
        assert_eq!(clean_path("./x/", "").as_deref(), Some("x"));
        assert_eq!(
            clean_path("%(here)s/migrations", "").as_deref(),
            Some("migrations")
        );
        assert_eq!(clean_path("../x", ""), None);
        assert_eq!(clean_path("../gen", "prisma").as_deref(), Some("gen"));
    }

    #[test]
    fn defaults_and_config() {
        let roles = resolve_roles(None, None).unwrap();
        assert_eq!(
            roles["env-files"].globs(),
            [
                "**/.env",
                "**/.env.*",
                "!**/.env.example",
                "!**/.env.sample",
                "!**/.env.template",
                "!**/.env.dist"
            ]
        );
        let t = tempfile::tempdir().unwrap();
        std::fs::write(
            t.path().join("alembic.ini"),
            "[alembic]\nscript_location = db\n",
        )
        .unwrap();
        let files: Vec<String> = vec!["alembic.ini".into()];
        let roles = resolve_roles(
            Some(&RoleInputs {
                repo: t.path(),
                files: &files,
            }),
            None,
        )
        .unwrap();
        assert_eq!(roles["alembic-migrations"].globs(), ["db/versions/**"]);
        assert_eq!(
            roles["alembic-migrations"].patterns[0].source,
            "alembic.ini"
        );
    }
}
