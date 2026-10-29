//! The packs and rules: the file formats (`pack.yaml`, `rules/*.yaml`, `forge.yaml`) and what
//! loading adds. These types are the reference: the JSON Schemas (`schemas/*.schema.json`) are
//! generated from them ([`crate::schema`]), with the fields loading adds left out, so a file
//! holding one is refused. What a schema cannot say (one field's value depending on another's)
//! is checked when loading (`validate`).

use std::collections::BTreeMap;

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A relative path in an example: no leading `/`, no `..`, not a directory.
pub(crate) const EXAMPLE_PATH: &str = r"^(?!/)(?!(.*/)?\.\.(/|$)).*[^/]$";

/// The words packs and rules use (`src/core/vocabulary.ts`).
macro_rules! vocabulary {
    ($(#[$m:meta])* $name:ident { $($(#[$vm:meta])* $variant:ident = $text:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
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
vocabulary!(
    /// The guard's built-in checks, each reading the whole parsed command line.
    GuardCheck {
        /// A package launcher that would fetch what it runs from a registry.
        EphemeralExec = "ephemeral-exec",
        /// A command that adds a named package (read-only and lock-file installs pass).
        PackageAdd = "package-add",
        /// pip into the project's environment where the project's packages are uv's, or
        /// where it has no packaging yet.
        PythonPm = "python-pm",
    }
);
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum OneOrMany<T> {
    One(T),
    Many(#[schemars(length(min = 1))] Vec<T>),
}

impl<T> OneOrMany<T> {
    pub fn as_slice(&self) -> &[T] {
        match self {
            OneOrMany::One(t) => std::slice::from_ref(t),
            OneOrMany::Many(v) => v,
        }
    }
}

/// A dependency: by ecosystem and name, in a version range (npm semver, or PEP 440 for PyPI).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DepSpec {
    pub ecosystem: Ecosystem,
    #[schemars(length(min = 1))]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1))]
    pub version: Option<String>,
}

/// How a project-level pack is detected (§5.1): any one of these facts.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PackDetect {
    /// A file matches one of the globs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(inner(length(min = 1)))]
    pub files_any: Option<Vec<String>>,
    /// One of the dependencies is declared, in its version range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deps_any: Option<Vec<DepSpec>>,
    /// The project publishes a library to one of the ecosystems.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("uniqueItems" = true))]
    pub library_any: Option<Vec<Ecosystem>>,
    /// Active in every repository (only `true`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("const" = true))]
    pub always: Option<bool>,
    /// A source file matches one of the globs, outside vendored, generated and environment
    /// directories.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1), inner(length(min = 1)))]
    pub sources_any: Option<Vec<String>>,
    /// A line of a file matching the globs matches the pattern.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1))]
    pub content_any: Option<Vec<ContentMatch>>,
    /// A .yaml / .yml file's first document is a mapping with all these top-level keys.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1), extend("uniqueItems" = true), inner(length(min = 1)))]
    pub yaml_keys: Option<Vec<String>>,
    /// A named fact has one of the values (`python.pm: [poetry]`); any entry may hold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("minProperties" = 1))]
    pub facts_any: Option<BTreeMap<String, OneOrMany<String>>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContentMatch {
    #[schemars(length(min = 1), inner(length(min = 1)))]
    pub files: Vec<String>,
    /// A regular expression, searched in each line.
    #[schemars(length(min = 1))]
    pub pattern: String,
}

/// A tool diagnostic a pack switches off where it is active.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolOff {
    #[schemars(extend("enum" = ["biome"]))]
    pub tool: String,
    #[schemars(pattern(r"^lint/[a-zA-Z]+/[a-zA-Z]+$"))]
    pub expects: String,
    #[schemars(length(min = 1))]
    pub reason: String,
}

/// A piece of the text the agent reads while the pack is active: a norm, true wherever its
/// `when` holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContextItem {
    /// Items with the same id are rendered once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(pattern(r"^[a-z][a-z0-9-]*$"))]
    pub id: Option<String>,
    #[schemars(length(min = 1))]
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<ContextWhen>,
}

/// When a context item holds. Every condition given must hold, in one place the pack applies
/// to (the root or a workspace package); `packs` is about the whole repository.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("minProperties" = 1))]
pub struct ContextWhen {
    /// Every dependency is declared, in its version range.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deps: Vec<DepSpec>,
    /// A file matches one of the globs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(inner(length(min = 1)))]
    pub files: Vec<String>,
    /// Every named fact has the value (or one of the values): `python.pm: uv`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub facts: BTreeMap<String, OneOrMany<String>>,
    /// One of these packs is active.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(inner(pattern(r"^[a-z][a-z0-9-]*(/[a-z][a-z0-9-]*)*$")))]
    pub packs: Vec<String>,
}

/// A pack: `pack.yaml` plus what loading adds (its place in the tree, its rules).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "pack", extend("$id" = "forge:pack"))]
pub struct Pack {
    /// Path in the pack tree ("typescript/nextjs/react").
    #[schemars(pattern(r"^[a-z][a-z0-9-]*(/[a-z][a-z0-9-]*)*$"))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub description: String,
    pub level: Level,
    /// A user-level pack has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detect: Option<PackDetect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1))]
    pub tool_off: Option<Vec<ToolOff>>,
    #[serde(default)]
    #[schemars(extend("uniqueItems" = true), inner(pattern(r"^[a-z][a-z0-9-]*(/[a-z][a-z0-9-]*)*$")))]
    pub requires: Vec<String>,
    /// What the agent reads while the pack is active.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<ContextItem>,
    // ---- added by the loader (never in pack.yaml: the schema leaves them out)
    /// The pack's directory: below the built-in packs root, or relative to the repository.
    #[serde(default)]
    #[schemars(skip)]
    pub dir: String,
    #[schemars(skip)]
    pub parent: Option<String>,
    #[serde(default)]
    #[schemars(skip)]
    pub children: Vec<String>,
    /// The init/ directory (same base as `dir`), when there is one.
    #[serde(rename = "initDir")]
    #[schemars(skip)]
    pub init_dir: Option<String>,
    #[serde(default)]
    #[schemars(skip)]
    pub rules: Vec<Rule>,
    /// Built-in packs leave it out of the dump.
    #[serde(default, skip_serializing_if = "PackSource::is_builtin")]
    #[schemars(skip)]
    pub source: PackSource,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub language: Language,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1), inner(length(min = 1)))]
    pub files: Option<Vec<String>>,
}

/// A rule: its file as written plus what loading adds (file, hash, origin, pack).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "rule", extend("$id" = "forge:rule"))]
pub struct Rule {
    #[schemars(pattern(r"^[a-z][a-z0-9-]*(/[a-z][a-z0-9-]*)+$"))]
    pub id: String,
    /// What the agent reads when the rule fires: the norm and its reason, one line.
    #[schemars(pattern(r"^[^\n\t]+$"))]
    pub description: String,
    pub scope: Scope,
    #[schemars(length(min = 1))]
    pub enforce: Vec<Target>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub examples: Option<Examples>,
    /// The question a reviewer answers for an ask nobody is there for (a guard `ask`, or the
    /// test-edit check).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(pattern(r"^[^\n\t]+$"))]
    pub review: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<When>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<Deprecated>,
    // ---- added by the loader
    /// The rule file: below the built-in packs root, or relative to the repository.
    #[serde(default)]
    #[schemars(skip)]
    pub file: String,
    /// sha256 (hex) of the rule file's bytes.
    #[serde(default)]
    #[schemars(skip)]
    pub hash: String,
    #[serde(default)]
    #[schemars(skip)]
    pub origin: Origin,
    /// Pack id: the rule id without its last segment.
    #[serde(default)]
    #[schemars(skip)]
    pub pack: String,
}

/// The rule applies where every dependency is declared in its range.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct When {
    #[schemars(length(min = 1))]
    pub deps: Vec<DepSpec>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Deprecated {
    #[schemars(pattern(r"^[a-z][a-z0-9-]*(/[a-z][a-z0-9-]*)+$"))]
    pub replaced_by: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Examples {
    #[serde(default, rename = "match", skip_serializing_if = "Option::is_none")]
    pub match_: Option<Vec<Example>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_match: Option<Vec<Example>>,
    /// The dependencies the examples' project declares (each with a version).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1))]
    pub deps: Option<Vec<DepSpec>>,
}

/// Files of an example, by path.
pub type Files = IndexMap<String, String>;

/// A rule example: code in the rule's scope, a fixture (files on top of `head`), or a
/// guard command run among `files`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Example {
    Code(String),
    Command(CommandExample),
    Fixture(FixtureExample),
}

/// Files committed at HEAD, then the files of the change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FixtureExample {
    #[schemars(extend("minProperties" = 1, "propertyNames" = {"pattern": EXAMPLE_PATH}))]
    pub files: Files,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("minProperties" = 1, "propertyNames" = {"pattern": EXAMPLE_PATH}))]
    pub head: Option<Files>,
}

/// A guard example: a command run in a directory holding `files`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommandExample {
    #[schemars(length(min = 1))]
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("minProperties" = 1, "propertyNames" = {"pattern": EXAMPLE_PATH}))]
    pub files: Option<Files>,
}

/// An `enforce` entry (§6.2), by kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolTarget {
    pub tool: AuditedTool,
    #[schemars(length(min = 1))]
    pub expects: String,
    #[schemars(length(min = 1))]
    pub snippet: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AstGrepTarget {
    pub mode: Mode,
    /// ast-grep's own rule object, passed through.
    #[schemars(extend("minProperties" = 1))]
    pub rule: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constraints: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utils: Option<Map<String, Value>>,
    /// Engine capabilities on metavariables (§15.1); `forbid` and `advise` only.
    #[serde(rename = "where", default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("minProperties" = 1, "propertyNames" = {"pattern": "^[A-Z][A-Z0-9_]*$"}))]
    pub where_: Option<IndexMap<String, WhereCondition>>,
    /// Packages the `where` answers need installed; requires `where`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1))]
    pub needs: Option<Vec<Need>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Need {
    pub ecosystem: Ecosystem,
    #[schemars(length(min = 1))]
    pub name: String,
}

/// A where condition (§15.1, §15.6, §15.7): written as `{type: …}`, `{declared: …}`,
/// `{deprecated: true}` or `{not: …}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum WhereCondition {
    Type(OneOrMany<TypeSpec>),
    Declared(OneOrMany<TypeSpec>),
    Deprecated(#[schemars(extend("const" = true))] bool),
    Not(Box<WhereCondition>),
}

/// A type, by its package or module (at least one), and its name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("anyOf" = [{"required": ["package"]}, {"required": ["module"]}]))]
pub struct TypeSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<OneOrMany<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<OneOrMany<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<OneOrMany<String>>,
    /// "match" or "no-match".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("enum" = ["match", "no-match"]))]
    pub unknown: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PermissionTarget {
    pub decision: Decision,
    /// A Claude Code permission rule: `Tool` or `Tool(specifier)`.
    #[schemars(pattern(r"^[A-Za-z]+(\(.+\))?$"))]
    pub rule: String,
}

/// A guard target: the Bash commands it decides on, by their structure (`command`), by a
/// built-in check (`check`), or both (the check then decides on what `command` matched).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GuardTarget {
    pub decision: Decision,
    /// One simple command shape, or a list of them (any matches).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<OneOrMany<CommandMatch>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<GuardCheck>,
    /// The reason for this target, in place of the rule's description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(pattern(r"^[^\n\t]+$"))]
    pub reason: Option<String>,
}

/// A simple command, described by its structure. Every field given must hold; lists hold
/// when any of their entries does, except `flags_all`.
///
/// Names and values are matched whole; `*` in them matches any run of characters
/// (`delete-*`, `python*`). A `sub` or `args_any` entry written `/regex/` is a regex
/// searched in the positional instead. The command is matched at each of its command
/// positions: the command itself, what a wrapper runs (`sudo`, `env`, `nice -n 5`,
/// `timeout 5`…) and what a runner runs (`npx`, `bunx`, `bun x`, `npm exec`, `pnpm exec|dlx`,
/// `uv run`, `uvx`, `poetry run`, `python <script>`, `python -m <module>`…): `name: prisma`
/// matches `bunx prisma migrate reset`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommandMatch {
    /// The command name (its basename: `./manage.py` is `manage.py`). Without a name, the
    /// command's own name position is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<OneOrMany<String>>,
    /// The first positionals, in order; each entry is alternatives separated by `|`
    /// (`[stash, drop]`, `['up|refresh']`), or a `/regex/`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sub: Vec<String>,
    /// Some positional after `sub` is one of these.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args_any: Vec<String>,
    /// The last positional matches this regex (searched).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_arg: Option<String>,
    /// Some option is one of these: `--force` (and `--force=x`, not `--force-with-lease`),
    /// `-f` (and the cluster `-uf`), `--require-approval=never` (or as two words).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags_any: Vec<String>,
    /// Every one of these options is given; an entry may be alternatives separated by `|`
    /// (`['-d|--delete', '-f|--force']`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags_all: Vec<String>,
    /// None of these options is given.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags_none: Vec<String>,
    /// Options that take the next word as their value, besides those known for the command
    /// (`git -C <dir>`, `kubectl -n <ns>`…).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub value_flags: Vec<String>,
    /// Some argument after the name matches this regex (searched).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arg_regex: Option<String>,
    /// `arg_regex` also searches the heredoc or here-string fed to the command (a program
    /// it reads: `psql <<EOF`, `python3 - <<EOF`). Otherwise that text is data.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stdin: bool,
    /// The escape hatch: a regex searched in the command's words from its name, joined by
    /// single spaces (a word holding whitespace is shown between single quotes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub words: Option<String>,
    /// Some option (a key: alternatives separated by `|`) has a value matching the regex
    /// (searched): `{'-v|--volume': '^/:'}`. Options with a value in two words need to be
    /// known to take one (`value_flags`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub option_values: BTreeMap<String, String>,
    /// The command assigns one of these variables (`SKIP=x git commit`, `export HUSKY=0`);
    /// `NAME=value` requires that value (`*` wildcards allowed).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_any: Vec<String>,
    /// The command position is reached through one of these wrappers (`sudo pip install`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wrapped_by: Vec<String>,
    /// The command's output is piped into one of these commands (`*`: any).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pipe_to: Vec<String>,
    /// The command writes, through a redirection, a file whose path matches this regex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redirect_to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SandboxTarget {
    #[schemars(pattern(r"^[A-Za-z][A-Za-z0-9]*(\.[A-Za-z][A-Za-z0-9]*)*$"))]
    pub path: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckTarget {
    pub stage: Stage,
    #[schemars(length(min = 1))]
    pub run: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1))]
    pub changed_run: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1), pattern(r"\{files\}"))]
    pub fix_run: Option<String>,
    /// Requires `fix_run` and `changed_run`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("const" = true))]
    pub fix_lints: Option<bool>,
    /// `format` and `lint` stages only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1), pattern(r"\{file\}"))]
    pub format_stdin: Option<String>,
    /// `test` stage only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1), pattern(r"\{files\}"))]
    pub rerun: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1), inner(length(min = 1)))]
    pub when_changed: Option<Vec<String>>,
    /// `lint` stage only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1), inner(length(min = 1)))]
    pub config_files: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires_local_db: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("const" = true))]
    pub fail_on_warnings: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 600))]
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

/// A POSIX extended regular expression matched on the lines of config files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GrepTarget {
    pub mode: Mode,
    #[schemars(pattern(r"^[^\t\n]+$"))]
    pub pattern: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignore_case: Option<bool>,
}

/// forge.yaml (`ProjectConfig`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "project", extend("$id" = "forge:project"))]
pub struct ProjectConfig {
    #[schemars(pattern(r"^\d+\.\d+$"))]
    pub forge: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packs: Option<PackChanges>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disable: Option<Vec<Disable>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("propertyNames" = {"pattern": "^[a-z][a-z0-9-]*$"}))]
    pub files: Option<IndexMap<String, RoleChange>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub python: Option<PythonConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accept: Option<Vec<Accept>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PackChanges {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(inner(pattern(r"^[a-z][a-z0-9-]*(/[a-z][a-z0-9-]*)*$")))]
    pub add: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(inner(pattern(r"^[a-z][a-z0-9-]*(/[a-z][a-z0-9-]*)*$")))]
    pub remove: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Disable {
    #[schemars(pattern(r"^[a-z][a-z0-9-]*(/[a-z][a-z0-9-]*)+$"))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("minProperties" = 1))]
pub struct RoleChange {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1), inner(length(min = 1)))]
    pub add: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1), inner(length(min = 1)))]
    pub set: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PythonConfig {
    /// "any" or "source".
    #[serde(
        rename = "untyped-packages",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(extend("enum" = ["any", "source"]))]
    pub untyped_packages: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Accept {
    #[schemars(pattern(r"^[a-z][a-z0-9-]*(/[a-z][a-z0-9-]*)+$"))]
    pub rule: String,
    #[schemars(pattern(r"^[^\n]+$"))]
    pub change: String,
    #[schemars(length(min = 1))]
    pub reason: String,
}
