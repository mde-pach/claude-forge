//! What `forge apply`, `status` and `explain` tell the person about enforcement:
//! whether each active rule is enforced here and why not, from the
//! plan and from what the installed tools can do. Asking the tools (their versions, whether
//! their config reports a code) is the checker's: [`ToolProbe`] is that question, and
//! [`NoProbe`] answers "not known" until the checker implements it.

use std::collections::BTreeSet;
use std::path::Path;

use forge_core::Note;
use forge_packs::model::{Rule, Target, ToolTarget};
use serde::Serialize;

use crate::plan::Plan;
use crate::rules::RuleStatus;

/// The outcome of auditing a tool target against the project's own config (§11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuditStatus {
    Satisfied,
    Unsatisfied,
    Skipped,
    /// The installed release does not have the code (never a snippet).
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuditResult {
    pub rule: String,
    pub tool: String,
    pub expects: String,
    pub status: AuditStatus,
    /// The workspace package audited; `None`: the root.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// The config to add, when unsatisfied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

/// What Forge asks of the machine's tools. Implemented by the checker; machine state, so it
/// goes to the person (apply, status), never into the rules files.
pub trait ToolProbe: Sync {
    /// The installed version of a tool here, if it can tell.
    fn version(&self, repo: &Path, tool: &str) -> Option<String>;
    /// Audits a tool target in a directory (relative to the repository, "" the root);
    /// `None` when it cannot audit.
    fn audit(
        &self,
        repo: &Path,
        rule: &Rule,
        target: &ToolTarget,
        dir: &str,
    ) -> Option<AuditResult>;
    /// The tools the plan's checks need that are not installed.
    fn missing_tools(&self, plan: &Plan) -> Vec<String>;
    /// What the installed tool releases cannot do (one line each).
    fn tool_versions(&self, plan: &Plan) -> Vec<String>;
    /// Anything else the person should know (environment, Forge's own state).
    fn notes(&self, plan: &Plan) -> Vec<Note>;
}

/// Knows nothing about the machine: no audit, no missing tool.
pub struct NoProbe;

impl ToolProbe for NoProbe {
    fn version(&self, _: &Path, _: &str) -> Option<String> {
        None
    }
    fn audit(&self, _: &Path, _: &Rule, _: &ToolTarget, _: &str) -> Option<AuditResult> {
        None
    }
    fn missing_tools(&self, _: &Plan) -> Vec<String> {
        Vec::new()
    }
    fn tool_versions(&self, _: &Plan) -> Vec<String> {
        Vec::new()
    }
    fn notes(&self, _: &Plan) -> Vec<Note> {
        Vec::new()
    }
}

/// Audits every audited tool target of the active rules, where each tool runs. Each audit
/// runs the tool on the rule's examples: several at a time, the results in plan order.
pub fn audit_rules(plan: &Plan, probe: &dyn ToolProbe) -> Vec<AuditResult> {
    let mut jobs = Vec::new();
    for st in &plan.active {
        for t in &st.audits {
            for dir in plan.tool_dirs.get(&t.tool).into_iter().flatten() {
                jobs.push((&st.rule, t, dir.as_str()));
            }
        }
    }
    let workers = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(8);
    let chunk = jobs.len().div_ceil(workers).max(1);
    std::thread::scope(|s| {
        let handles: Vec<_> = jobs
            .chunks(chunk)
            .map(|part| {
                s.spawn(move || {
                    part.iter()
                        .filter_map(|(rule, t, dir)| probe.audit(&plan.base, rule, t, dir))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    })
}

/// Whether an active rule is enforced here, and why not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Enforcement {
    pub id: String,
    pub enforced: bool,
    pub by: Vec<String>,
    pub not_enforced: Vec<String>,
}

fn unsatisfied_reason(tool: &str) -> String {
    match tool {
        "ruff" => "ruff codes not selected".into(),
        "biome" => "Biome rules not at error".into(),
        "ty" => "ty rules not at error".into(),
        "tsc" => "tsc options not set".into(),
        "eslint" => "ESLint rules not at error".into(),
        t => format!("{t} not configured"),
    }
}

fn push_unique(v: &mut Vec<String>, s: String) {
    if !v.contains(&s) {
        v.push(s);
    }
}

/// Each active rule's enforcement here.
pub fn enforcement(plan: &Plan, audits: &[AuditResult], missing: &[String]) -> Vec<Enforcement> {
    let absent: BTreeSet<String> = missing
        .iter()
        .map(|m| m.split(" (").next().unwrap_or(m).to_string())
        .collect();
    plan.active
        .iter()
        .map(|st| rule_enforcement(st, audits, &absent))
        .collect()
}

fn rule_enforcement(
    st: &RuleStatus,
    audits: &[AuditResult],
    absent: &BTreeSet<String>,
) -> Enforcement {
    let mut by = Vec::new();
    let mut not = Vec::new();
    let mine: Vec<&AuditResult> = audits.iter().filter(|a| a.rule == st.rule.id).collect();
    let mut tools: Vec<&str> = Vec::new();
    for a in &mine {
        if !tools.contains(&a.tool.as_str()) {
            tools.push(&a.tool);
        }
    }
    for tool in tools {
        let each: Vec<&&AuditResult> = mine.iter().filter(|a| a.tool == tool).collect();
        let avail: Vec<&&&AuditResult> = each
            .iter()
            .filter(|a| a.status != AuditStatus::Unavailable)
            .collect();
        let bad = avail.iter().find(|a| a.status != AuditStatus::Satisfied);
        match bad {
            None if !avail.is_empty() => {
                push_unique(&mut by, format!("{tool} {}", avail[0].expects))
            }
            Some(b) if b.status == AuditStatus::Unsatisfied => {
                push_unique(&mut not, unsatisfied_reason(tool))
            }
            Some(b) => push_unique(
                &mut not,
                if b.detail
                    .as_deref()
                    .is_some_and(|d| d.contains("not installed"))
                {
                    format!("{tool} not installed")
                } else {
                    format!("{tool} audit skipped")
                },
            ),
            None => {}
        }
        for u in each.iter().filter(|a| a.status == AuditStatus::Unavailable) {
            push_unique(
                &mut not,
                u.detail
                    .clone()
                    .unwrap_or_else(|| format!("{tool} {} unavailable", u.expects)),
            );
        }
    }
    for n in &st.not_enforced {
        push_unique(
            &mut not,
            n.strip_prefix("not enforced here: ")
                .unwrap_or(n)
                .to_string(),
        );
    }
    let has_check = st.enforced.iter().any(|e| e.starts_with("check "));
    for t in &st.rule.enforce {
        match t {
            Target::Permission(p) => push_unique(&mut by, format!("permission {}", p.decision)),
            Target::Guard(g) => push_unique(&mut by, format!("guard {}", g.decision)),
            Target::Sandbox(_) if st.enforced.iter().any(|e| e.starts_with("sandbox")) => {
                push_unique(&mut by, "sandbox".into())
            }
            Target::Grep(_) if !st.greps.is_empty() => push_unique(&mut by, "grep".into()),
            Target::AstGrep(_) => {
                if let Some(a) = &st.astgrep {
                    push_unique(&mut by, format!("ast-grep {}", a.target.mode));
                }
            }
            Target::Check(c) if has_check => {
                let gone: Vec<&str> = forge_detect::tools::check_tools(c)
                    .into_iter()
                    .filter(|x| absent.contains(*x))
                    .collect();
                if gone.is_empty() {
                    push_unique(&mut by, format!("check {}", c.stage));
                } else {
                    push_unique(&mut not, format!("{} not installed", gone.join(", ")));
                }
            }
            _ => {}
        }
    }
    Enforcement {
        id: st.rule.id.clone(),
        enforced: !by.is_empty(),
        by,
        not_enforced: not,
    }
}

/// `<n> active, <m> enforced, <k> not enforced (<count> <reason>, …)`.
pub fn enforcement_summary(rules: &[Enforcement]) -> String {
    let off: Vec<&Enforcement> = rules.iter().filter(|r| !r.enforced).collect();
    let mut reasons: Vec<(String, usize)> = Vec::new();
    for r in &off {
        let why = r
            .not_enforced
            .first()
            .cloned()
            .unwrap_or_else(|| "nothing applies here".into());
        match reasons.iter_mut().find(|(w, _)| *w == why) {
            Some((_, n)) => *n += 1,
            None => reasons.push((why, 1)),
        }
    }
    reasons.sort_by(|a, b| b.1.cmp(&a.1));
    let detail = reasons
        .iter()
        .map(|(w, n)| format!("{n} {w}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{} active, {} enforced, {} not enforced{}",
        rules.len(),
        rules.len() - off.len(),
        off.len(),
        if off.is_empty() {
            String::new()
        } else {
            format!(" ({detail})")
        }
    )
}

/// A suggestion block's tool and directory.
type BlockKey = (String, Option<String>);

/// Audits as apply, status and explain print them: one line each (as [`format_audit`], without
/// its snippet), then, per tool and directory, one block holding the config the unsatisfied
/// ones lack, merged: TOML snippets by table (arrays of one key joined), others once each.
pub fn format_audits<'a>(audits: impl IntoIterator<Item = &'a AuditResult>) -> String {
    let mut lines = Vec::new();
    let mut blocks: Vec<(BlockKey, Vec<&str>)> = Vec::new();
    for a in audits {
        let mut one = a.clone();
        one.snippet = None;
        lines.push(format_audit(&one));
        if a.status != AuditStatus::Unsatisfied {
            continue;
        }
        let Some(sn) = a.snippet.as_deref() else {
            continue;
        };
        let key = (a.tool.clone(), a.dir.clone());
        match blocks.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => v.push(sn),
            None => blocks.push((key, vec![sn])),
        }
    }
    for ((tool, dir), snippets) in blocks {
        let dir = dir.map(|d| format!(" (in {d})")).unwrap_or_default();
        lines.push(format!("forge: add to the {tool} config{dir}:"));
        for l in merge_snippets(&snippets) {
            lines.push(format!("  {l}"));
        }
    }
    lines.join("\n")
}

/// Config snippets as one: TOML (`[table] key = value`, or a `[table]` line then `key = value`
/// lines) grouped by table, a key's arrays joined; anything else once each, as it is.
fn merge_snippets(snippets: &[&str]) -> Vec<String> {
    // (table, [(key, value)]) in first-seen order
    let mut tables: Vec<(String, Vec<(String, String)>)> = Vec::new();
    let mut other: Vec<String> = Vec::new();
    let header = |l: &str| -> Option<(String, String)> {
        let rest = l.strip_prefix('[')?;
        let end = rest.find(']')?;
        let name = &rest[..end];
        (!name.is_empty() && !name.contains(['"', '{', ' ']))
            .then(|| (name.to_string(), rest[end + 1..].trim().to_string()))
    };
    for sn in snippets {
        let is_toml = sn
            .lines()
            .find(|l| !l.trim().is_empty())
            .is_some_and(|l| header(l.trim()).is_some());
        if !is_toml {
            if !other.iter().any(|o| o == sn.trim()) {
                other.push(sn.trim().to_string());
            }
            continue;
        }
        let mut table = String::new();
        for raw in sn.lines() {
            let l = raw.trim();
            if l.is_empty() {
                continue;
            }
            let entry = match header(l) {
                Some((t, rest)) => {
                    table = t;
                    if rest.is_empty() {
                        if !tables.iter().any(|(n, _)| *n == table) {
                            tables.push((table.clone(), Vec::new()));
                        }
                        continue;
                    }
                    rest
                }
                None => l.to_string(),
            };
            let (k, v) = match entry.split_once('=') {
                Some((k, v)) => (k.trim().to_string(), v.trim().to_string()),
                None => (entry.clone(), String::new()),
            };
            let pos = match tables.iter().position(|(n, _)| *n == table) {
                Some(i) => i,
                None => {
                    tables.push((table.clone(), Vec::new()));
                    tables.len() - 1
                }
            };
            let entries = &mut tables[pos].1;
            match entries.iter_mut().find(|(ek, _)| *ek == k) {
                Some((_, ev)) => {
                    if let (Some(a), Some(b)) = (array_items(ev), array_items(&v)) {
                        let mut items = a;
                        for i in b {
                            if !items.contains(&i) {
                                items.push(i);
                            }
                        }
                        *ev = format!("[{}]", items.join(", "));
                    } else if *ev != v {
                        entries.push((k, v));
                    }
                }
                None => entries.push((k, v)),
            }
        }
    }
    let mut out = Vec::new();
    for (name, entries) in tables {
        if !name.is_empty() {
            out.push(format!("[{name}]"));
        }
        for (k, v) in entries {
            out.push(if v.is_empty() {
                k
            } else {
                format!("{k} = {v}")
            });
        }
    }
    out.extend(other);
    out
}

/// The items of a one-line TOML array of plain values (`["A", "B"]`).
fn array_items(v: &str) -> Option<Vec<String>> {
    let inner = v.strip_prefix('[')?.strip_suffix(']')?;
    if inner.contains(['[', '{']) {
        return None;
    }
    Some(
        inner
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect(),
    )
}

/// One audit, as apply and status print it.
pub fn format_audit(a: &AuditResult) -> String {
    let status = match a.status {
        AuditStatus::Satisfied => "satisfied",
        AuditStatus::Unsatisfied => "unsatisfied",
        AuditStatus::Skipped => "skipped",
        AuditStatus::Unavailable => "unavailable",
    };
    let mut s = format!(
        "forge: {} {} {}{}: {status}",
        a.rule,
        a.tool,
        a.expects,
        a.dir
            .as_ref()
            .map(|d| format!(" (in {d})"))
            .unwrap_or_default()
    );
    if a.status == AuditStatus::Satisfied {
        return s;
    }
    if let Some(d) = &a.detail {
        s.push_str(&format!(" ({d})"));
    }
    if let Some(sn) = &a.snippet {
        s.push_str(&format!("\n  add to the {} config: {sn}", a.tool));
    }
    s
}
