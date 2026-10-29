//! The loaded packs and rules (`src/types.ts`). Field names, optionality and the JSON shape
//! are those of `dist/packs.json`: serializing a [`Pack`] to JSON gives the bundle's entry.
//! The JSON Schemas (`schemas/*.schema.json`) validate the files first; these types then
//! deny unknown fields, so a field added to a schema and not here fails loudly.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The words packs and rules use (`src/core/vocabulary.ts`).
macro_rules! vocabulary {
    ($(#[$m:meta])* $name:ident { $($(#[$vm:meta])* $variant:ident = $text:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub enum $name {
            $($(#[$vm])* #[serde(rename = $text)] $variant),+
        }
        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];
            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => $text),+ }
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

vocabulary!(Level { User = "user", Project = "project" });
vocabulary!(Language { Python = "python", Typescript = "typescript", Tsx = "tsx", Yaml = "yaml", Any = "any" });
vocabulary!(Decision { Deny = "deny", Ask = "ask" });
vocabulary!(Ecosystem { Pypi = "pypi", Npm = "npm" });
vocabulary!(
    /// check.sh's stages, in the order it runs them (§10.1).
    Stage { Format = "format", Lint = "lint", Codegen = "codegen", Typecheck = "typecheck", Migration = "migration", Test = "test" }
);
vocabulary!(
    /// How an ast-grep or grep match turns into a failure (§6.2).
    Mode { Forbid = "forbid", NoIncrease = "no-increase", NoDecrease = "no-decrease", Advise = "advise" }
);
vocabulary!(GuardCheck { EphemeralExec = "ephemeral-exec" });
vocabulary!(
    /// Tools a `tool` target audits (§11).
    AuditedTool { Biome = "biome", Ruff = "ruff", Ty = "ty", Tsc = "tsc", Eslint = "eslint" }
);
vocabulary!(
    /// Where a rule comes from: a pack, or the project's `.forge/rules`.
    #[derive(Default)]
    Origin { #[default] Pack = "pack", Local = "local" }
);
vocabulary!(
    /// Where a pack comes from: built into Forge, or the project's `.forge/packs`.
    #[derive(Default)]
    PackSource { #[default] Builtin = "builtin", Local = "local" }
);

impl PackSource {
    fn is_builtin(&self) -> bool {
        *self == PackSource::Builtin
    }
}

/// A value or a non-empty list of them, kept in the shape it was written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany<T> {
    One(T),
    Many(Vec<T>),
}

impl<T> OneOrMany<T> {
    pub fn as_slice(&self) -> &[T] {
        match self {
            OneOrMany::One(t) => std::slice::from_ref(t),
            OneOrMany::Many(v) => v,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DepSpec {
    pub ecosystem: Ecosystem,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// How a project-level pack is detected (§5.1): any one of these facts.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackDetect {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files_any: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deps_any: Option<Vec<DepSpec>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library_any: Option<Vec<Ecosystem>>,
    /// Active in every repository (the schema allows only `true`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub always: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources_any: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_any: Option<Vec<ContentMatch>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yaml_keys: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentMatch {
    pub files: Vec<String>,
    pub pattern: String,
}

/// A tool diagnostic a pack switches off where it is active.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolOff {
    pub tool: String,
    pub expects: String,
    pub reason: String,
}

/// A pack: `pack.yaml` plus what loading adds (its place in the tree, its rules, context).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pack {
    /// Path in the pack tree ("typescript/nextjs/react").
    pub id: String,
    pub description: String,
    pub level: Level,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detect: Option<PackDetect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_off: Option<Vec<ToolOff>>,
    #[serde(default)]
    pub requires: Vec<String>,
    // ---- added by the loader (never in pack.yaml: the schema refuses them)
    /// The pack's directory: below the built-in packs root, or relative to the repository.
    #[serde(default)]
    pub dir: String,
    pub parent: Option<String>,
    #[serde(default)]
    pub children: Vec<String>,
    /// context.md, verbatim.
    pub context: Option<String>,
    /// The init/ directory (same base as `dir`), when there is one.
    #[serde(rename = "initDir")]
    pub init_dir: Option<String>,
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// Built-in packs leave it out, so the dump is `dist/packs.json`'s shape.
    #[serde(default, skip_serializing_if = "PackSource::is_builtin")]
    pub source: PackSource,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub language: Language,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
}

/// A rule: its file as written plus what loading adds (file, hash, origin, pack).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    pub description: String,
    pub scope: Scope,
    pub enforce: Vec<Target>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub examples: Option<Examples>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<When>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<Deprecated>,
    // ---- added by the loader
    /// The rule file: below the built-in packs root, or relative to the repository.
    #[serde(default)]
    pub file: String,
    /// sha256 (hex) of the rule file's bytes.
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub origin: Origin,
    /// Pack id: the rule id without its last segment.
    #[serde(default)]
    pub pack: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct When {
    pub deps: Vec<DepSpec>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deprecated {
    pub replaced_by: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Examples {
    #[serde(default, rename = "match", skip_serializing_if = "Option::is_none")]
    pub match_: Option<Vec<Example>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_match: Option<Vec<Example>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deps: Option<Vec<DepSpec>>,
}

/// Files of an example, by path.
pub type Files = IndexMap<String, String>;

/// A rule example: code in the rule's scope, a fixture (files on top of `head`), or a
/// guard command run among `files`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Example {
    Code(String),
    Command(CommandExample),
    Fixture(FixtureExample),
}

/// Files committed at HEAD, then the files of the change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureExample {
    pub files: Files,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<Files>,
}

/// A guard example: a command run in a directory holding `files`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandExample {
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Files>,
}

/// An `enforce` entry (§6.2), by kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "target", rename_all = "kebab-case")]
pub enum Target {
    Tool(ToolTarget),
    AstGrep(AstGrepTarget),
    Permission(PermissionTarget),
    Guard(GuardTarget),
    Sandbox(SandboxTarget),
    Check(CheckTarget),
    Grep(GrepTarget),
}

impl Target {
    pub fn kind(&self) -> &'static str {
        match self {
            Target::Tool(_) => "tool",
            Target::AstGrep(_) => "ast-grep",
            Target::Permission(_) => "permission",
            Target::Guard(_) => "guard",
            Target::Sandbox(_) => "sandbox",
            Target::Check(_) => "check",
            Target::Grep(_) => "grep",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolTarget {
    pub tool: AuditedTool,
    pub expects: String,
    pub snippet: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AstGrepTarget {
    pub mode: Mode,
    /// ast-grep's own rule object, passed through.
    pub rule: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constraints: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utils: Option<Map<String, Value>>,
    /// Engine capabilities on metavariables (§15.1).
    #[serde(rename = "where", default, skip_serializing_if = "Option::is_none")]
    pub where_: Option<IndexMap<String, WhereCondition>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needs: Option<Vec<Need>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Need {
    pub ecosystem: Ecosystem,
    pub name: String,
}

/// A where condition (§15.1, §15.6, §15.7): written as `{type: …}`, `{declared: …}`,
/// `{deprecated: true}` or `{not: …}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WhereCondition {
    Type(OneOrMany<TypeSpec>),
    Declared(OneOrMany<TypeSpec>),
    Deprecated(bool),
    Not(Box<WhereCondition>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypeSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<OneOrMany<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<OneOrMany<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<OneOrMany<String>>,
    /// "match" or "no-match".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unknown: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionTarget {
    pub decision: Decision,
    pub rule: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuardTarget {
    pub decision: Decision,
    /// POSIX ERE on the JSON-escaped command; with `check`, a pre-filter.
    pub pattern: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<GuardCheck>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxTarget {
    pub path: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckTarget {
    pub stage: Stage,
    pub run: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed_run: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix_run: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix_lints: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format_stdin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rerun: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when_changed: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_files: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires_local_db: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fail_on_warnings: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u32>,
}

impl CheckTarget {
    /// The shell commands, in the order the fields are declared.
    pub fn commands(&self) -> impl Iterator<Item = &str> {
        [
            Some(&self.run),
            self.changed_run.as_ref(),
            self.fix_run.as_ref(),
            self.format_stdin.as_ref(),
            self.rerun.as_ref(),
        ]
        .into_iter()
        .flatten()
        .map(String::as_str)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrepTarget {
    pub mode: Mode,
    pub pattern: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignore_case: Option<bool>,
}

/// forge.yaml (`ProjectConfig`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    pub forge: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packs: Option<PackChanges>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disable: Option<Vec<Disable>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<IndexMap<String, RoleChange>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub python: Option<PythonConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accept: Option<Vec<Accept>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackChanges {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub add: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remove: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Disable {
    pub id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleChange {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub add: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PythonConfig {
    /// "any" or "source".
    #[serde(
        rename = "untyped-packages",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub untyped_packages: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Accept {
    pub rule: String,
    pub change: String,
    pub reason: String,
}
