//! Guard rules: what a pack writes, and how it is matched.
//!
//! A guard target ([`GuardSpec`]) has a decision (`deny` or `ask`) and any of:
//!
//! - `pattern`: the current pack form, a POSIX ERE matched per simple command ([`crate::ere`]);
//! - `command`: the structural form, one [`CommandMatch`] or a list of them (any matches);
//! - `check`: a built-in check ([`Check`]) that reads the whole parsed command line.
//!
//! The target matches when every filter it has matches (`pattern` and `command` each on some
//! simple command); with a `check`, the check then decides, and its finding is the "Here: …"
//! detail of the reason. A target with only a `check` runs it on every command line.
//!
//! ```yaml
//! - decision: deny
//!   command: {name: git, sub: [push], flags_any: [--force, -f]}
//! - decision: ask
//!   command:
//!     - {name: [kubectl], sub: ['delete|drain|edit|patch|replace']}
//!     - {name: [kubectl], sub: [scale], flags_any: ['--replicas=0']}
//! - decision: ask
//!   check: package-add
//! ```

use std::collections::BTreeMap;
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::args::Args;
use crate::parse::{Parsed, SimpleCommand, basename};
use crate::text::Rendered;

/// What a guard rule decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Decision {
    Deny,
    Ask,
}

impl From<forge_packs::model::Decision> for Decision {
    fn from(d: forge_packs::model::Decision) -> Self {
        match d {
            forge_packs::model::Decision::Deny => Decision::Deny,
            forge_packs::model::Decision::Ask => Decision::Ask,
        }
    }
}

/// The built-in checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Check {
    /// A package launcher that would fetch what it runs from a registry (§9.2).
    EphemeralExec,
    /// A command that adds a named package (read-only and lock-file installs pass).
    PackageAdd,
    /// pip into the project's environment where the project's packages are uv's.
    PythonPm,
}

impl Check {
    pub const ALL: &'static [Check] = &[Check::EphemeralExec, Check::PackageAdd, Check::PythonPm];
}

/// A value or a list of them.
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

/// One guard target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuardSpec {
    pub decision: Decision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<OneOrMany<CommandMatch>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<Check>,
    /// The reason for this target, in place of the rule's description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// A simple command, described by its structure. Every field given must hold; lists hold
/// when any of their entries does, except `flags_all`.
///
/// Names and values are matched whole; `*` in them matches any run of characters
/// (`delete-*`, `python*`). A `sub` or `args_any` entry written `/regex/` is a regex
/// searched in the positional instead. The command is matched at each of its command positions: the
/// command itself, what a wrapper runs (`sudo`, `env`, `nice -n 5`, `timeout 5`…) and what a
/// runner runs (`npx`, `bunx`, `bun x`, `npm exec`, `pnpm exec|dlx`, `uv run`, `uvx`,
/// `poetry run`, `python <script>`, `python -m <module>`…): `name: prisma` matches
/// `bunx prisma migrate reset`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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
    /// Options that take the next word as their value, besides those Forge knows for the
    /// command (`git -C <dir>`, `kubectl -n <ns>`…).
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

/// A regex compiled on first use.
#[derive(Debug)]
pub(crate) struct LazyRegex {
    source: String,
    re: OnceLock<Option<Regex>>,
}

impl LazyRegex {
    pub fn new(source: &str) -> Result<Self, String> {
        let full = format!("(?s){source}");
        Ok(LazyRegex {
            source: full,
            re: OnceLock::new(),
        })
    }

    /// Compiles it now: an error names the regex.
    pub fn check(&self) -> Result<(), String> {
        match self.re.get_or_init(|| Regex::new(&self.source).ok()) {
            Some(_) => Ok(()),
            None => Err(format!(
                "regex {:?}: {}",
                &self.source[4..],
                Regex::new(&self.source)
                    .err()
                    .map(|e| e.to_string())
                    .unwrap_or_default()
            )),
        }
    }

    pub fn is_match(&self, s: &str) -> bool {
        self.re
            .get_or_init(|| Regex::new(&self.source).ok())
            .as_ref()
            .is_some_and(|r| r.is_match(s))
    }
}

/// A name or value with `*` wildcards.
#[derive(Debug, Clone)]
pub(crate) struct Glob(Vec<String>);

impl Glob {
    pub fn new(s: &str) -> Self {
        Glob(s.split('*').map(str::to_owned).collect())
    }

    pub fn matches(&self, s: &str) -> bool {
        let parts = &self.0;
        if parts.len() == 1 {
            return parts[0] == s;
        }
        let (first, last) = (&parts[0], &parts[parts.len() - 1]);
        if !s.starts_with(first.as_str()) || s.len() < first.len() + last.len() {
            return false;
        }
        let mut rest = &s[first.len()..];
        for mid in &parts[1..parts.len() - 1] {
            match rest.find(mid.as_str()) {
                Some(i) => rest = &rest[i + mid.len()..],
                None => return false,
            }
        }
        rest.ends_with(last.as_str())
    }
}

/// A positional pattern: globs, or a `/regex/`.
#[derive(Debug)]
pub(crate) enum ArgPat {
    Globs(Vec<Glob>),
    Regex(LazyRegex),
}

impl ArgPat {
    fn new(s: &str, split: bool) -> Result<Self, String> {
        if s.len() >= 2 && s.starts_with('/') && s.ends_with('/') {
            return LazyRegex::new(&s[1..s.len() - 1]).map(ArgPat::Regex);
        }
        Ok(ArgPat::Globs(if split {
            s.split('|').map(Glob::new).collect()
        } else {
            vec![Glob::new(s)]
        }))
    }

    fn check(&self) -> Result<(), String> {
        match self {
            ArgPat::Globs(_) => Ok(()),
            ArgPat::Regex(r) => r.check(),
        }
    }

    fn matches(&self, a: &str) -> bool {
        match self {
            ArgPat::Globs(gs) => gs.iter().any(|g| g.matches(a)),
            ArgPat::Regex(r) => r.is_match(a),
        }
    }
}

/// A [`CommandMatch`], compiled.
#[derive(Debug)]
pub(crate) struct Matcher {
    names: Vec<Glob>,
    sub: Vec<ArgPat>,
    args_any: Vec<ArgPat>,
    last_arg: Option<LazyRegex>,
    flags_any: Vec<String>,
    flags_all: Vec<String>,
    flags_none: Vec<String>,
    value_flags: Vec<String>,
    arg_regex: Option<LazyRegex>,
    stdin: bool,
    words: Option<LazyRegex>,
    option_values: Vec<(Vec<String>, LazyRegex)>,
    env_any: Vec<(String, Option<Glob>)>,
    wrapped_by: Vec<Glob>,
    pipe_to: Vec<Glob>,
    redirect_to: Option<LazyRegex>,
    /// The fields need the arguments read.
    needs_args: bool,
}

impl Matcher {
    pub fn new(m: &CommandMatch) -> Result<Self, String> {
        let lazy = |s: &Option<String>| s.as_deref().map(LazyRegex::new).transpose();
        let names: Vec<Glob> = m
            .name
            .as_ref()
            .map(|n| n.as_slice().iter().map(|s| Glob::new(s)).collect())
            .unwrap_or_default();
        Ok(Matcher {
            needs_args: !m.sub.is_empty()
                || !m.args_any.is_empty()
                || m.last_arg.is_some()
                || !m.flags_any.is_empty()
                || !m.flags_all.is_empty()
                || !m.flags_none.is_empty()
                || !m.option_values.is_empty(),
            names,
            sub: m
                .sub
                .iter()
                .map(|s| ArgPat::new(s, true))
                .collect::<Result<_, _>>()?,
            args_any: m
                .args_any
                .iter()
                .map(|s| ArgPat::new(s, false))
                .collect::<Result<_, _>>()?,
            last_arg: lazy(&m.last_arg)?,
            flags_any: m.flags_any.clone(),
            flags_all: m.flags_all.clone(),
            flags_none: m.flags_none.clone(),
            // an option whose value is matched takes one
            value_flags: m
                .value_flags
                .iter()
                .cloned()
                .chain(
                    m.option_values
                        .keys()
                        .flat_map(|k| k.split('|').map(str::to_owned)),
                )
                .collect(),
            arg_regex: lazy(&m.arg_regex)?,
            stdin: m.stdin,
            words: lazy(&m.words)?,
            option_values: m
                .option_values
                .iter()
                .map(|(k, v)| {
                    Ok((
                        k.split('|').map(str::to_owned).collect(),
                        LazyRegex::new(v)?,
                    ))
                })
                .collect::<Result<_, String>>()?,
            env_any: m
                .env_any
                .iter()
                .map(|e| match e.split_once('=') {
                    Some((n, v)) => (n.to_owned(), Some(Glob::new(v))),
                    None => (e.clone(), None),
                })
                .collect(),
            wrapped_by: m.wrapped_by.iter().map(|s| Glob::new(s)).collect(),
            pipe_to: m.pipe_to.iter().map(|s| Glob::new(s)).collect(),
            redirect_to: lazy(&m.redirect_to)?,
        })
    }

    /// Compiles every regex now.
    pub fn check(&self) -> Result<(), String> {
        for p in self.sub.iter().chain(&self.args_any) {
            p.check()?;
        }
        for r in [
            &self.last_arg,
            &self.arg_regex,
            &self.words,
            &self.redirect_to,
        ]
        .into_iter()
        .flatten()
        {
            r.check()?;
        }
        for (_, r) in &self.option_values {
            r.check()?;
        }
        Ok(())
    }

    /// Whether the command matches at one of its command positions.
    pub fn matches(&self, cmd: &SimpleCommand, parsed: &Parsed) -> bool {
        if !self.env_any.is_empty() && !self.assigns_any(cmd) {
            return false;
        }
        if let Some(re) = &self.redirect_to
            && !cmd
                .redirects
                .iter()
                .any(|r| r.writes_file() && re.is_match(&r.target.text))
        {
            return false;
        }
        if !self.pipe_to.is_empty() {
            let Some(next) = cmd.pipe.and_then(|p| parsed.next_stage(p)) else {
                return false;
            };
            let star = self
                .pipe_to
                .iter()
                .any(|g| g.0.len() == 2 && g.0.iter().all(String::is_empty));
            if !star
                && !next
                    .starts
                    .iter()
                    .any(|&i| self.pipe_to.iter().any(|g| g.matches(next.name_at(i))))
            {
                return false;
            }
        }
        if self.names.is_empty() {
            return match cmd.effective() {
                Some(e) => self.matches_at(cmd, e),
                None => !self.needs_args && self.arg_regex.is_none() && self.words.is_none(),
            };
        }
        cmd.starts.iter().enumerate().any(|(k, &p)| {
            let name = cmd.name_at(p);
            self.names.iter().any(|g| g.matches(name))
                && (self.wrapped_by.is_empty()
                    || cmd.starts[..k]
                        .iter()
                        .any(|&w| self.wrapped_by.iter().any(|g| g.matches(cmd.name_at(w)))))
                && self.matches_at(cmd, p)
        })
    }

    fn assigns_any(&self, cmd: &SimpleCommand) -> bool {
        let named = |n: &str, v: &str| {
            self.env_any
                .iter()
                .any(|(e, want)| e == n && want.as_ref().is_none_or(|g| g.matches(v)))
        };
        if cmd.assigns.iter().any(|(n, v)| named(n, v)) {
            return true;
        }
        let Some(e) = cmd.effective() else {
            return false;
        };
        matches!(
            cmd.name_at(e),
            "export" | "declare" | "typeset" | "readonly" | "local"
        ) && cmd.words[e + 1..]
            .iter()
            .any(|w| w.text.split_once('=').is_some_and(|(n, v)| named(n, v)))
    }

    fn matches_at(&self, cmd: &SimpleCommand, p: usize) -> bool {
        let rest = &cmd.words[p + 1..];
        if self.needs_args {
            let args = Args::read(basename(&cmd.words[p].text), rest, &self.value_flags);
            for (i, alts) in self.sub.iter().enumerate() {
                match args.positionals.get(i) {
                    Some(a) if alts.matches(a) => {}
                    _ => return false,
                }
            }
            if !self.args_any.is_empty()
                && !args.positionals[self.sub.len().min(args.positionals.len())..]
                    .iter()
                    .any(|a| self.args_any.iter().any(|g| g.matches(a)))
            {
                return false;
            }
            if let Some(re) = &self.last_arg
                && !args.positionals.last().is_some_and(|a| re.is_match(a))
            {
                return false;
            }
            if !self.flags_any.is_empty() && !self.flags_any.iter().any(|f| args.has(f)) {
                return false;
            }
            if !self
                .flags_all
                .iter()
                .all(|f| f.split('|').any(|alt| args.has(alt)))
            {
                return false;
            }
            if !self.option_values.is_empty()
                && !self.option_values.iter().any(|(names, re)| {
                    args.options.iter().any(|(o, v)| {
                        names.iter().any(|n| n == o) && v.is_some_and(|v| re.is_match(v))
                    })
                })
            {
                return false;
            }
            if self.flags_none.iter().any(|f| args.has(f)) {
                return false;
            }
        }
        if let Some(re) = &self.arg_regex
            && !rest.iter().any(|w| re.is_match(&w.text))
            && !(self.stdin && cmd.input.as_deref().is_some_and(|i| re.is_match(i)))
        {
            return false;
        }
        if let Some(re) = &self.words {
            let view = Rendered::of(&[], &cmd.words[p..], &[], None, &[]);
            if !re.is_match(&view.text) {
                return false;
            }
        }
        true
    }
}
