//! Guard rules: what a pack writes, and how it is matched.
//!
//! A guard target ([`GuardSpec`], forge-packs' `GuardTarget`) has a decision (`deny` or `ask`)
//! and one or both of:
//!
//! - `command`: the structural form, one [`CommandMatch`] or a list of them (any matches);
//! - `check`: a built-in check ([`Check`]) that reads the whole parsed command line.
//!
//! The target matches when its `command` matches some simple command; with a `check`, the
//! check then decides, and its finding is the "Here: …" detail of the reason. A target with
//! only a `check` runs it on every command line.
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

use std::sync::OnceLock;

use regex::Regex;

use crate::args::Args;
use crate::parse::{Parsed, SimpleCommand, basename};
use crate::text::Rendered;

pub use forge_packs::model::{CommandMatch, Decision, GuardCheck as Check, OneOrMany};

/// One guard target, as the pack writes it.
pub type GuardSpec = forge_packs::model::GuardTarget;

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
