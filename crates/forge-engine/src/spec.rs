//! What a rule's `where` asks (spec §15.1, §15.6, §15.7) and how an answer is judged.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A string or a list of strings, as the YAML gives it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Unknown {
    Match,
    #[default]
    NoMatch,
}

/// A type or declaration spec: `{package, module, name, unknown}`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Spec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<OneOrMany<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<OneOrMany<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<OneOrMany<String>>,
    #[serde(default)]
    pub unknown: Unknown,
}

/// The condition on one metavariable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Condition {
    Type(OneOrMany<Spec>),
    Declared(OneOrMany<Spec>),
    Deprecated(bool),
    Not(Box<Condition>),
}

/// What a condition asks of its metavariable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ask {
    Type,
    Declared,
    Deprecated,
}

impl Condition {
    pub fn asks(&self) -> Ask {
        match self {
            Condition::Not(c) => c.asks(),
            Condition::Type(_) => Ask::Type,
            Condition::Declared(_) => Ask::Declared,
            Condition::Deprecated(_) => Ask::Deprecated,
        }
    }
}

/// The code a metavariable is bound to: byte offsets in the file as it is on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// One ast-grep match whose rule has a `where`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Match {
    /// Repository-relative path.
    pub file: String,
    pub rule: String,
    /// 1-based line of the match.
    pub line: u32,
    /// The single metavariables the match bound.
    pub bindings: BTreeMap<String, Binding>,
    #[serde(rename = "where")]
    pub conditions: BTreeMap<String, Condition>,
}

/// The engine's answer for one match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "verdict")]
pub enum Verdict {
    /// Every condition holds: the match stands.
    Holds,
    /// A condition does not hold: the match is dropped.
    Fails,
    /// A metavariable is bound to code whose type cannot be asked (spec §15.1 rule 9).
    Unchecked { why: String },
}

/// Where a type or a name is declared: `(package, module, name)` (spec §15.1 rule 5).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Decl {
    pub package: Option<String>,
    pub module: Option<String>,
    pub name: Option<String>,
}

/// What the engine found for one metavariable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    Decls { decls: Vec<Decl>, unknown: bool },
    Deprecated(bool),
    Unchecked,
}

/// Whether a spec holds: one declaration has the package, module (or a prefix by whole segments,
/// joined by `sep`) and name it gives; an unknown type holds only with `unknown: match`.
pub fn satisfies(found: &Found, spec: &Spec, sep: char) -> bool {
    let (decls, unknown) = match found {
        Found::Decls { decls, unknown } => (decls, *unknown),
        _ => return false,
    };
    if unknown {
        return spec.unknown == Unknown::Match;
    }
    decls.iter().any(|d| {
        spec.package
            .as_ref()
            .is_none_or(|p| d.package.as_ref().is_some_and(|x| p.as_slice().contains(x)))
            && spec.module.as_ref().is_none_or(|ms| {
                let m = d.module.as_deref().unwrap_or("");
                ms.as_slice().iter().any(|want| {
                    m == want
                        || (m.len() > want.len()
                            && m.starts_with(want.as_str())
                            && m[want.len()..].starts_with(sep))
                })
            })
            && spec
                .name
                .as_ref()
                .is_none_or(|n| d.name.as_ref().is_some_and(|x| n.as_slice().contains(x)))
    })
}

pub fn holds(found: &Found, cond: &Condition, sep: char) -> bool {
    match cond {
        Condition::Not(c) => !holds(found, c, sep),
        Condition::Deprecated(want) => matches!(found, Found::Deprecated(d) if d == want),
        Condition::Type(specs) | Condition::Declared(specs) => {
            specs.as_slice().iter().any(|s| satisfies(found, s, sep))
        }
    }
}

/// The text after "$X is bound to …" for a match whose metavariable cannot be asked.
pub fn unchecked_why(m: &Match, var: &str) -> String {
    let text = m
        .bindings
        .get(var)
        .map(|b| {
            b.text
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(40)
                .collect::<String>()
        })
        .unwrap_or_default();
    format!(
        "{}:{}: ${var} is bound to `{text}`, which is not an expression: its type cannot be asked",
        m.file, m.line
    )
}

/// The verdicts of `matches` from what was found per (match index, variable).
pub fn verdicts(
    matches: &[Match],
    found: &std::collections::HashMap<(usize, String), Found>,
    sep: char,
) -> Vec<Verdict> {
    matches
        .iter()
        .enumerate()
        .map(|(n, m)| {
            if let Some(var) = m
                .conditions
                .keys()
                .find(|v| matches!(found.get(&(n, (*v).clone())), Some(Found::Unchecked)))
            {
                return Verdict::Unchecked {
                    why: unchecked_why(m, var),
                };
            }
            let ok = m
                .conditions
                .iter()
                .all(|(v, c)| found.get(&(n, v.clone())).is_none_or(|f| holds(f, c, sep)));
            if ok { Verdict::Holds } else { Verdict::Fails }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(p: Option<&str>, m: &str, n: &str) -> Decl {
        Decl {
            package: p.map(str::to_owned),
            module: Some(m.to_owned()),
            name: Some(n.to_owned()),
        }
    }

    #[test]
    fn where_yaml_parses() {
        let c: Condition = serde_json::from_str(
            r#"{"type": {"package": "sqlalchemy", "name": ["Session", "scoped_session"]}}"#,
        )
        .unwrap();
        let f = Found::Decls {
            decls: vec![d(Some("sqlalchemy"), "sqlalchemy.orm.session", "Session")],
            unknown: false,
        };
        assert!(holds(&f, &c, '.'));
        let not: Condition =
            serde_json::from_str(r#"{"not": {"type": {"module": "sqlalchemy.orm"}}}"#).unwrap();
        assert!(!holds(&f, &not, '.'));
        let prefix: Condition =
            serde_json::from_str(r#"{"type": {"module": "sqlalchemy.or"}}"#).unwrap();
        assert!(!holds(&f, &prefix, '.'));
        let unknown = Found::Decls {
            decls: vec![],
            unknown: true,
        };
        let m: Condition =
            serde_json::from_str(r#"{"type": {"package": "x", "unknown": "match"}}"#).unwrap();
        assert!(holds(&unknown, &m, '.'));
        assert!(!holds(&unknown, &c, '.'));
    }
}
