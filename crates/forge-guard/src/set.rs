//! The active guard rules, and the decision on a command.

use crate::checks::{self, Hit};
use crate::ctx::GuardCtx;
use crate::ere::Pattern;
use crate::parse::{self, Dir, Parsed, Start};
use crate::rule::{Check, Decision, GuardSpec, Matcher};
use crate::text::Rendered;

/// A guard rule: its id (never shown to the agent), the description that is the reason, the
/// question a reviewer answers, its targets.
#[derive(Debug, Clone, PartialEq)]
pub struct GuardRule {
    pub id: String,
    pub description: String,
    pub review: Option<String>,
    pub targets: Vec<GuardSpec>,
}

impl GuardRule {
    /// A pack rule's guard targets, as the pack writes them.
    pub fn from_pack(rule: &forge_packs::Rule) -> Option<Self> {
        let targets: Vec<GuardSpec> = rule
            .enforce
            .iter()
            .filter_map(|t| match t {
                forge_packs::model::Target::Guard(g) => Some(GuardSpec {
                    decision: g.decision.into(),
                    pattern: Some(g.pattern.clone()),
                    command: None,
                    check: g.check.map(|c| match c {
                        forge_packs::model::GuardCheck::EphemeralExec => Check::EphemeralExec,
                    }),
                    reason: None,
                }),
                _ => None,
            })
            .collect();
        (!targets.is_empty()).then(|| GuardRule {
            id: rule.id.clone(),
            description: rule.description.clone(),
            review: rule.review.clone(),
            targets,
        })
    }
}

/// The guard's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing to say: the command goes on to the permission system.
    None,
    Ask(Verdict),
    Deny(Verdict),
}

impl Outcome {
    pub fn verdict(&self) -> Option<&Verdict> {
        match self {
            Outcome::None => None,
            Outcome::Ask(v) | Outcome::Deny(v) => Some(v),
        }
    }

    /// `deny`, `ask` or `` (none).
    pub fn decision(&self) -> &'static str {
        match self {
            Outcome::None => "",
            Outcome::Ask(_) => "ask",
            Outcome::Deny(_) => "deny",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// For the person and the logs; never part of the reason.
    pub rule: String,
    /// For the agent: the rule's description, then "Here: …" when a check found something.
    pub reason: String,
    /// The rule's review question, for a reviewer answering an ask nobody is there for.
    pub review: Option<String>,
}

struct Target {
    decision: Decision,
    pattern: Option<Pattern>,
    command: Vec<Matcher>,
    check: Option<Check>,
    reason: Option<String>,
}

struct Compiled {
    rule: GuardRule,
    targets: Vec<Target>,
}

/// Rules ready to evaluate commands. Regexes compile on first use, so a command that names
/// none of a rule's commands costs that rule no regex.
pub struct GuardSet {
    rules: Vec<Compiled>,
    /// (rule, target), deny targets first, each in rule order.
    order: Vec<(usize, usize)>,
}

impl GuardSet {
    /// The rules, every regex compiled now: an invalid one is an error.
    pub fn new(rules: Vec<GuardRule>) -> Result<Self, String> {
        let set = GuardSet::new_lazy(rules)?;
        for r in &set.rules {
            for t in &r.targets {
                if let Some(p) = &t.pattern {
                    p.check().map_err(|e| format!("{}: {e}", r.rule.id))?;
                }
                for m in &t.command {
                    m.check().map_err(|e| format!("{}: {e}", r.rule.id))?;
                }
            }
        }
        Ok(set)
    }

    /// The rules, regexes compiled when a command first needs them: for a hook process, the
    /// rules having been checked when they were loaded. A regex that does not compile then
    /// matches nothing.
    pub fn new_lazy(rules: Vec<GuardRule>) -> Result<Self, String> {
        let mut compiled = Vec::new();
        for r in rules {
            let mut targets = Vec::new();
            for t in &r.targets {
                let command = match &t.command {
                    Some(c) => c
                        .as_slice()
                        .iter()
                        .map(Matcher::new)
                        .collect::<Result<_, _>>()
                        .map_err(|e| format!("{}: {e}", r.id))?,
                    None => vec![],
                };
                targets.push(Target {
                    decision: t.decision,
                    pattern: t
                        .pattern
                        .as_deref()
                        .map(Pattern::new)
                        .transpose()
                        .map_err(|e| format!("{}: {e}", r.id))?,
                    command,
                    check: t.check,
                    reason: t.reason.clone(),
                });
            }
            compiled.push(Compiled { rule: r, targets });
        }
        let mut order = Vec::new();
        for want in [Decision::Deny, Decision::Ask] {
            for (i, r) in compiled.iter().enumerate() {
                for (j, t) in r.targets.iter().enumerate() {
                    if t.decision == want {
                        order.push((i, j));
                    }
                }
            }
        }
        Ok(GuardSet {
            rules: compiled,
            order,
        })
    }

    /// The pack rules' guard targets, as written.
    pub fn from_pack_rules<'a>(
        rules: impl IntoIterator<Item = &'a forge_packs::Rule>,
    ) -> Result<Self, String> {
        GuardSet::new(rules.into_iter().filter_map(GuardRule::from_pack).collect())
    }

    pub fn rules(&self) -> impl Iterator<Item = &GuardRule> {
        self.rules.iter().map(|c| &c.rule)
    }

    /// The decision on `command`, run from `ctx.cwd`: the first deny target that matches,
    /// else the first ask target.
    pub fn evaluate(&self, command: &str, ctx: &GuardCtx) -> Outcome {
        let parsed = parse::parse(
            command,
            &Start {
                cwd: Dir::Known(parse::normalize(&ctx.cwd)),
                home: ctx.home.clone(),
            },
        );
        self.evaluate_parsed(&parsed, command, ctx)
    }

    pub fn evaluate_parsed(&self, parsed: &Parsed, command: &str, ctx: &GuardCtx) -> Outcome {
        let mut pipelines: Option<Vec<Rendered>> = None;
        let mut hits: [Option<Option<Hit>>; 3] = [None, None, None];
        for &(i, j) in &self.order {
            let rule = &self.rules[i];
            let t = &rule.targets[j];
            if let Some(p) = &t.pattern {
                let ok = if p.pipeline {
                    pipelines
                        .get_or_insert_with(|| pipeline_texts(parsed))
                        .iter()
                        .any(|r| p.matches(r))
                } else {
                    parsed.commands.iter().any(|c| p.matches(&c.rendered))
                };
                if !ok {
                    continue;
                }
            }
            if !t.command.is_empty()
                && !parsed
                    .commands
                    .iter()
                    .any(|c| t.command.iter().any(|m| m.matches(c, parsed)))
            {
                continue;
            }
            let hit = match t.check {
                None => None,
                Some(check) => {
                    let slot = &mut hits[Check::ALL.iter().position(|c| *c == check).unwrap_or(0)];
                    match slot.get_or_insert_with(|| checks::run(check, parsed, command, ctx)) {
                        Some(h) => Some(h.clone()),
                        None => continue,
                    }
                }
            };
            let base = hit
                .as_ref()
                .and_then(|h| h.reason.clone())
                .or_else(|| t.reason.clone())
                .unwrap_or_else(|| rule.rule.description.clone());
            let reason = match &hit {
                Some(h) if !h.detail.is_empty() => {
                    format!(
                        "{} Here: {}.",
                        base.trim_end(),
                        h.detail.trim_end_matches('.')
                    )
                }
                _ => base,
            };
            let v = Verdict {
                rule: rule.rule.id.clone(),
                reason,
                review: rule.rule.review.clone(),
            };
            return match t.decision {
                Decision::Deny => Outcome::Deny(v),
                Decision::Ask => Outcome::Ask(v),
            };
        }
        Outcome::None
    }
}

/// Each pipeline's stages joined by ` | `; a command in no pipeline is its own.
fn pipeline_texts(parsed: &Parsed) -> Vec<Rendered> {
    let mut out = Vec::new();
    let mut seen = Vec::new();
    for c in &parsed.commands {
        match c.pipe {
            None => out.push(c.rendered.clone()),
            Some((id, _)) => {
                if seen.contains(&id) {
                    continue;
                }
                seen.push(id);
                let mut stages: Vec<_> = parsed
                    .commands
                    .iter()
                    .filter(|x| x.pipe.is_some_and(|(p, _)| p == id))
                    .collect();
                stages.sort_by_key(|x| x.pipe.map(|(_, s)| s));
                out.push(Rendered::join(stages.iter().map(|x| &x.rendered)));
            }
        }
    }
    out
}
