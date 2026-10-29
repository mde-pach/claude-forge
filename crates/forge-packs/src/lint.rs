//! Content lints: what loads fine but is probably wrong. They never fail loading; they are
//! findings `forge packs lint` reports.

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
    /// A grep target with no `files` scope, or whose scope reaches Markdown or docs.
    GrepScope,
    /// Agent-facing text naming Forge's internals (its files, commands, variables).
    ForgeInternals,
}

impl LintKind {
    pub fn as_str(self) -> &'static str {
        match self {
            LintKind::CutByComment => "cut-by-comment",
            LintKind::GrepScope => "grep-scope",
            LintKind::ForgeInternals => "forge-internals",
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

/// The agent-facing fields of rule files and pack.yaml.
const RULE_TEXT: &[&str] = &["/description", "/review"];
const PACK_TEXT: &[&str] = &["/description"];

/// (a) agent-facing plain scalars followed by a comment on their last line.
fn cut_by_comment(
    file: &str,
    subject: &str,
    text: &str,
    scalars: &[Scalar],
    fields: &[&str],
    out: &mut Vec<Finding>,
) {
    let lines: Vec<&str> = text.lines().collect();
    for s in scalars
        .iter()
        .filter(|s| s.plain && fields.contains(&s.pointer.as_str()))
    {
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

/// (b) grep targets without a files scope, or whose scope reaches Markdown or docs.
fn grep_scope(rule: &Rule, file: &str, out: &mut Vec<Finding>) {
    if !rule.enforce.iter().any(|t| matches!(t, Target::Grep(_))) {
        return;
    }
    // the engine scopes a rule by its files, then by its language's extensions (LANGUAGE_CASE)
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
        None => format!(
            "grep target with no scope.files: only its language ({}) scopes it",
            extensions.join(" ")
        ),
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
    /// The field ("description") or file ("context.md").
    what: &'a str,
    text: &'a str,
    /// The source and the lines (1-based, inclusive) the text spans in it.
    source: &'a str,
    lines: (usize, usize),
}

/// (c) the internals a text names, each once, with the source line it is on.
fn forge_internals(t: &Text, out: &mut Vec<Finding>) {
    let mut seen: Vec<&str> = Vec::new();
    for (_, term) in forge_core::internals::find(t.text) {
        if seen.contains(&term) {
            continue;
        }
        let (first, last) = t.lines;
        let line = t
            .source
            .lines()
            .enumerate()
            .skip(first.saturating_sub(1))
            .take(last + 1 - first.max(1))
            .find(|(_, l)| l.contains(term))
            .map_or(first, |(i, _)| i + 1);
        out.push(Finding {
            lint: LintKind::ForgeInternals,
            file: t.file.to_string(),
            line: Some(line),
            subject: t.subject.to_string(),
            message: format!("{} names Forge's internals: {term}", t.what),
        });
        seen.push(term);
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
    cut_by_comment(file, &rule.id, source, scalars, RULE_TEXT, &mut out);
    grep_scope(rule, file, &mut out);
    let fields = [
        ("description", Some(&rule.description)),
        ("review", rule.review.as_ref()),
    ];
    for (what, text) in fields {
        if let Some(text) = text {
            let lines = field_lines(scalars, &format!("/{what}"));
            forge_internals(
                &Text {
                    file,
                    subject: &rule.id,
                    what,
                    text,
                    source,
                    lines,
                },
                &mut out,
            );
        }
    }
    out
}

/// The findings of a pack.yaml and its context.md.
pub fn lint_pack(
    pack: &Pack,
    pack_file: &str,
    source: &str,
    scalars: &[Scalar],
    context_file: &str,
) -> Vec<Finding> {
    let mut out = Vec::new();
    cut_by_comment(pack_file, &pack.id, source, scalars, PACK_TEXT, &mut out);
    let lines = field_lines(scalars, "/description");
    forge_internals(
        &Text {
            file: pack_file,
            subject: &pack.id,
            what: "description",
            text: &pack.description,
            source,
            lines,
        },
        &mut out,
    );
    if let Some(context) = &pack.context {
        let lines = (1, context.lines().count().max(1));
        forge_internals(
            &Text {
                file: context_file,
                subject: &pack.id,
                what: "context.md",
                text: context,
                source: context,
                lines,
            },
            &mut out,
        );
    }
    out
}

/// One finding per line, sorted, then a count per lint.
pub fn render(findings: &[Finding]) -> String {
    let mut lines: Vec<String> = findings.iter().map(ToString::to_string).collect();
    lines.sort();
    let mut out = lines.join("\n");
    for kind in [
        LintKind::CutByComment,
        LintKind::GrepScope,
        LintKind::ForgeInternals,
    ] {
        let n = findings.iter().filter(|f| f.lint == kind).count();
        out.push_str(&format!("\n{}: {n}", kind.as_str()));
    }
    out.push('\n');
    out
}
