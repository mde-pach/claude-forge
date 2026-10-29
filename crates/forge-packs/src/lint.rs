//! Content lints: what loads fine but is probably wrong. They never fail loading; they are
//! findings `forge packs lint` reports (and the built-in packs' tests require none).
//!
//! The agent-facing text of a pack or rule is what the agent reads: a pack's `context` items,
//! a rule's `description`, `review` and its guard targets' `reason`.

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::glob::glob_to_regex;
use crate::model::{Language, Pack, Rule, Target};
use crate::roles;
use crate::yaml::Scalar;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LintKind {
    /// An unquoted agent-facing string that a ` #` cut short: YAML read the rest as a comment.
    CutByComment,
    /// A grep target that reads every file, or whose scope reaches Markdown or docs.
    GrepScope,
    /// Agent-facing text naming Forge or its internals (its files, commands, variables).
    ForgeInternals,
    /// Context text claiming a tool enforces the norm ("X rejects", "is linted by"): false
    /// wherever the tool is not installed. Context states the norm.
    EnforcementClaim,
    /// Agent-facing text about what is not checked ("is not followed", "not recognised").
    EnforcementLimit,
    /// A description written with an "Advice" prefix: the prefix comes from the mode.
    AdvicePrefix,
}

impl LintKind {
    pub const ALL: [LintKind; 6] = [
        LintKind::CutByComment,
        LintKind::GrepScope,
        LintKind::ForgeInternals,
        LintKind::EnforcementClaim,
        LintKind::EnforcementLimit,
        LintKind::AdvicePrefix,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            LintKind::CutByComment => "cut-by-comment",
            LintKind::GrepScope => "grep-scope",
            LintKind::ForgeInternals => "forge-internals",
            LintKind::EnforcementClaim => "enforcement-claim",
            LintKind::EnforcementLimit => "enforcement-limit",
            LintKind::AdvicePrefix => "advice-prefix",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub lint: LintKind,
    /// The file (as packs and rules report it: `dir`, `file`).
    pub file: String,
    /// 1-based line in the file, when known.
    pub line: Option<usize>,
    /// The rule or pack id.
    pub subject: String,
    pub message: String,
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.line {
            Some(l) => write!(
                f,
                "{}:{l}: {} [{}] {}",
                self.file,
                self.subject,
                self.lint.as_str(),
                self.message
            ),
            None => write!(
                f,
                "{}: {} [{}] {}",
                self.file,
                self.subject,
                self.lint.as_str(),
                self.message
            ),
        }
    }
}

/// Whether a scalar's pointer is an agent-facing field of a rule file.
fn rule_text_field(pointer: &str) -> bool {
    pointer == "/description"
        || pointer == "/review"
        || pointer
            .strip_prefix("/enforce/")
            .and_then(|r| r.split_once('/'))
            .is_some_and(|(i, f)| i.parse::<usize>().is_ok() && f == "reason")
}

/// Whether a scalar's pointer is an agent-facing field of a pack.yaml.
fn pack_text_field(pointer: &str) -> bool {
    pointer
        .strip_prefix("/context/")
        .and_then(|r| r.split_once('/'))
        .is_some_and(|(i, f)| i.parse::<usize>().is_ok() && f == "text")
}

/// (a) agent-facing plain scalars followed by a comment on their last line.
fn cut_by_comment(
    file: &str,
    subject: &str,
    text: &str,
    scalars: &[Scalar],
    field: fn(&str) -> bool,
    out: &mut Vec<Finding>,
) {
    let lines: Vec<&str> = text.lines().collect();
    for s in scalars.iter().filter(|s| s.plain && field(&s.pointer)) {
        let Some(line) = lines.get(s.end_line.saturating_sub(1)) else {
            continue;
        };
        let rest: String = line.chars().skip(s.end_col).collect();
        if let Some(comment) = rest.trim_start().strip_prefix('#')
            && rest.starts_with([' ', '\t'])
        {
            out.push(Finding {
                lint: LintKind::CutByComment,
                file: file.to_string(),
                line: Some(s.end_line),
                subject: subject.to_string(),
                message: format!(
                    "{} is cut at \" #\": YAML reads {:?} and drops \"#{}\" as a comment; quote the text",
                    &s.pointer[1..],
                    s.value,
                    comment.trim_end()
                ),
            });
        }
    }
}

/// Paths a grep scope should not reach: Markdown and documentation.
const DOC_PROBES: &[&str] = &[
    "README.md",
    "CHANGELOG.md",
    "docs/index.md",
    "docs/guide.mdx",
    "docs/api.rst",
    "doc/notes.md",
    "src/notes.md",
    "packages/app/README.md",
    "NOTES.txt",
];

/// The rule's file globs with roles expanded to Forge's defaults.
fn expanded_scope(globs: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for g in globs {
        match roles::role_ref(g) {
            Some((neg, name)) => {
                for p in roles::default_patterns(&name) {
                    let excluded = p.starts_with('!');
                    let bare = p.trim_start_matches('!').to_string();
                    out.push(if excluded != neg {
                        format!("!{bare}")
                    } else {
                        bare
                    });
                }
            }
            None => out.push(g.clone()),
        }
    }
    out
}

/// The probes a glob list selects (some pattern matches, no exclusion does).
fn reached<'p>(globs: &[String], probes: &[&'p str]) -> Vec<&'p str> {
    let compile = |g: &str| glob_to_regex(g);
    let pos: Vec<_> = globs
        .iter()
        .filter(|g| !g.starts_with('!'))
        .map(|g| compile(g))
        .collect();
    let neg: Vec<_> = globs
        .iter()
        .filter_map(|g| g.strip_prefix('!'))
        .map(compile)
        .collect();
    let hit = |re: &fancy_regex::Regex, p: &str| re.is_match(p).unwrap_or(false);
    probes
        .iter()
        .copied()
        .filter(|p| pos.iter().any(|re| hit(re, p)) && !neg.iter().any(|re| hit(re, p)))
        .collect()
}

/// (b) grep targets that read prose: no scope at all, or a scope reaching Markdown or docs.
/// A language scope alone keeps a grep on that language's source files.
fn grep_scope(rule: &Rule, file: &str, out: &mut Vec<Finding>) {
    if !rule.enforce.iter().any(|t| matches!(t, Target::Grep(_))) {
        return;
    }
    // the engine scopes a rule by its files, then by its language's extensions
    let extensions: &[&str] = match rule.scope.language {
        Language::Python => &[".py", ".pyi"],
        Language::Typescript => &[".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs"],
        Language::Tsx => &[".tsx", ".jsx"],
        Language::Yaml => &[".yaml", ".yml"],
        Language::Any => &[],
    };
    let message = match &rule.scope.files {
        None if extensions.is_empty() => {
            "grep target with no scope.files: it reads every file, docs included".to_string()
        }
        None => return,
        Some(globs) => {
            let globs = expanded_scope(globs);
            let probes: Vec<&str> = DOC_PROBES
                .iter()
                .copied()
                .filter(|p| extensions.is_empty() || extensions.iter().any(|e| p.ends_with(e)))
                .collect();
            let reached = reached(&globs, &probes);
            if reached.is_empty() {
                return;
            }
            format!(
                "grep target whose scope.files reaches docs ({})",
                reached.join(", ")
            )
        }
    };
    out.push(Finding {
        lint: LintKind::GrepScope,
        file: file.to_string(),
        line: None,
        subject: rule.id.clone(),
        message,
    });
}

/// Agent-facing text and where it was written.
struct Text<'a> {
    file: &'a str,
    subject: &'a str,
    /// The field ("description", "context/2/text").
    what: &'a str,
    text: &'a str,
    /// The source and the lines (1-based, inclusive) the text spans in it.
    source: &'a str,
    lines: (usize, usize),
}

impl Text<'_> {
    /// The source line holding `term` (else the text's first line).
    fn line_of(&self, term: &str) -> usize {
        let (first, last) = self.lines;
        self.source
            .lines()
            .enumerate()
            .skip(first.saturating_sub(1))
            .take(last + 1 - first.max(1))
            .find(|(_, l)| l.contains(term))
            .map_or(first, |(i, _)| i + 1)
    }

    fn finding(&self, lint: LintKind, term: &str, message: String) -> Finding {
        Finding {
            lint,
            file: self.file.to_string(),
            line: Some(self.line_of(term)),
            subject: self.subject.to_string(),
            message,
        }
    }
}

/// "Forge" itself: the agent is never told about the tool behind the norms.
static FORGE_WORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?-u:\b)Forge(?-u:\b)").expect("regex"));

/// (c) the internals a text names, each once, with the source line it is on.
fn forge_internals(t: &Text, out: &mut Vec<Finding>) {
    let mut seen: Vec<&str> = Vec::new();
    let terms = forge_core::internals::find(t.text)
        .into_iter()
        .map(|(_, term)| term)
        .chain(FORGE_WORD.find(t.text).map(|m| m.as_str()));
    for term in terms {
        if seen.contains(&term) {
            continue;
        }
        out.push(t.finding(
            LintKind::ForgeInternals,
            term,
            format!("{} names Forge's internals: {term}", t.what),
        ));
        seen.push(term);
    }
}

/// A tool said to enforce the norm.
static ENFORCEMENT_CLAIM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:is|are) (?:rejected|linted|scanned|checked|flagged|blocked|caught|reported) (?:by|with)\b|\brejects?\b")
        .expect("regex")
});

/// What is not enforced.
static ENFORCEMENT_LIMIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:is|are) not (?:checked|followed|recogni[sz]ed|detected|seen|enforced)\b|\bnot checked by\b")
        .expect("regex")
});

/// (d) a context text claiming enforcement.
fn enforcement_claim(t: &Text, out: &mut Vec<Finding>) {
    if let Some(m) = ENFORCEMENT_CLAIM.find(t.text) {
        out.push(t.finding(
            LintKind::EnforcementClaim,
            m.as_str(),
            format!(
                "{} claims enforcement ({:?}); state the norm and its reason",
                t.what,
                m.as_str()
            ),
        ));
    }
}

/// (e) text about what is not checked.
fn enforcement_limit(t: &Text, out: &mut Vec<Finding>) {
    if let Some(m) = ENFORCEMENT_LIMIT.find(t.text) {
        out.push(t.finding(
            LintKind::EnforcementLimit,
            m.as_str(),
            format!(
                "{} describes what is not enforced ({:?}); keep the norm and its reason",
                t.what,
                m.as_str()
            ),
        ));
    }
}

/// The lines a field's scalar spans.
fn field_lines(scalars: &[Scalar], pointer: &str) -> (usize, usize) {
    scalars
        .iter()
        .find(|s| s.pointer == pointer)
        .map_or((1, 1), |s| (s.line, s.end_line))
}

/// The findings of one rule file (`file` as findings name it).
pub fn lint_rule(rule: &Rule, file: &str, source: &str, scalars: &[Scalar]) -> Vec<Finding> {
    let mut out = Vec::new();
    cut_by_comment(file, &rule.id, source, scalars, rule_text_field, &mut out);
    grep_scope(rule, file, &mut out);
    let reasons = rule.enforce.iter().enumerate().filter_map(|(i, t)| match t {
        Target::Guard(g) => g.reason.as_ref().map(|r| (format!("enforce/{i}/reason"), r)),
        _ => None,
    });
    let fields: Vec<(String, &String)> = [
        Some(("description".to_string(), &rule.description)),
        rule.review.as_ref().map(|r| ("review".to_string(), r)),
    ]
    .into_iter()
    .flatten()
    .chain(reasons)
    .collect();
    for (what, text) in &fields {
        let t = Text {
            file,
            subject: &rule.id,
            what,
            text,
            source,
            lines: field_lines(scalars, &format!("/{what}")),
        };
        forge_internals(&t, &mut out);
        enforcement_limit(&t, &mut out);
    }
    if rule.description.trim_start().starts_with("Advice") {
        out.push(Finding {
            lint: LintKind::AdvicePrefix,
            file: file.to_string(),
            line: Some(field_lines(scalars, "/description").0),
            subject: rule.id.clone(),
            message: "description starts with \"Advice\": the prefix is written from the mode; start with the norm".into(),
        });
    }
    out
}

/// The findings of a pack.yaml.
pub fn lint_pack(pack: &Pack, pack_file: &str, source: &str, scalars: &[Scalar]) -> Vec<Finding> {
    let mut out = Vec::new();
    cut_by_comment(pack_file, &pack.id, source, scalars, pack_text_field, &mut out);
    for (i, item) in pack.context.iter().enumerate() {
        let what = format!("context/{i}/text");
        let t = Text {
            file: pack_file,
            subject: &pack.id,
            what: &what,
            text: &item.text,
            source,
            lines: field_lines(scalars, &format!("/{what}")),
        };
        forge_internals(&t, &mut out);
        enforcement_claim(&t, &mut out);
        enforcement_limit(&t, &mut out);
    }
    out
}

/// One finding per line, sorted, then a count per lint.
pub fn render(findings: &[Finding]) -> String {
    let mut lines: Vec<String> = findings.iter().map(ToString::to_string).collect();
    lines.sort();
    let mut out = lines.join("\n");
    for kind in LintKind::ALL {
        let n = findings.iter().filter(|f| f.lint == kind).count();
        out.push_str(&format!("\n{}: {n}", kind.as_str()));
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforcement_phrases() {
        for claim in [
            "zizmor's check rejects tags",
            "Changed manifests are linted by kube-linter",
            "Added lines are scanned with gitleaks",
        ] {
            assert!(ENFORCEMENT_CLAIM.is_match(claim), "{claim}");
        }
        for norm in [
            "Actions are pinned to a full commit SHA",
            "`ignoreDeprecations` in a tsconfig is rejected",
            "the rejection of a promise",
        ] {
            assert!(!ENFORCEMENT_CLAIM.is_match(norm), "{norm}");
        }
        assert!(ENFORCEMENT_LIMIT.is_match("$VARIABLE images are not checked"));
        assert!(ENFORCEMENT_LIMIT.is_match("check who is calling (not checked by Forge)"));
        assert!(!ENFORCEMENT_LIMIT.is_match("a session is not safe for concurrent use"));
    }

    #[test]
    fn agent_fields() {
        assert!(rule_text_field("/description"));
        assert!(rule_text_field("/enforce/2/reason"));
        assert!(!rule_text_field("/enforce/2/run"));
        assert!(pack_text_field("/context/0/text"));
        assert!(!pack_text_field("/description"));
    }
}
