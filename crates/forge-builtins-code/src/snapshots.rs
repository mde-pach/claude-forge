//! Snapshots left stale by a change of output-producing code (the original `tests.sh`
//! `forge_snapshot_stale`).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use ast_grep_core::tree_sitter::LanguageExt;
use ast_grep_language::Tsx;
use forge_core::builtin::{BuiltinCtx, Scope};
use forge_core::{Finding, Report};
use regex::Regex;

use crate::CodeBuiltin;
use crate::ctx::{Extra, read};
use crate::util::git_lines;

macro_rules! re {
    ($name:ident, $src:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($src).expect("regex"));
    };
}

re!(
    SNAPSHOT_FILE,
    r"\.snap$|(^|/)(__snapshots__|testdata|golden|__golden__)/|\.golden(\.|$)"
);
// a test compares with snapshot or golden files: a snapshot matcher, or a string naming a path
// under testdata/ or golden/, or a .golden file (never the word "golden" in prose)
re!(
    SNAPSHOT_USE,
    r#"toMatch(File|Inline)?Snapshot\(|["'`](?:[^"'`\n]*/)?(?:testdata|golden|__golden__)/|["'`][^"'`\n]*\.golden(?:[."'`])"#
);
re!(
    FILE_SNAPSHOT_USE,
    r#"toMatch(File)?Snapshot\(|["'`](?:[^"'`\n]*/)?(?:testdata|golden|__golden__)/|["'`][^"'`\n]*\.golden(?:[."'`])"#
);
re!(
    IMPORT,
    r#"(?:from|require\(|import\()\s*["'](\.[^"']*)["']"#
);
re!(STEM, r"\.(d\.)?[cm]?[jt]sx?$");

/// What the agent reads after the files whose snapshots are stale.
pub const HINT: &str = "Output-producing code changed but its snapshots did not: run the snapshot tests with inputs that exercise the new output and update them.";

/// `forge_snapshot_stale`: at Stop only. A changed source file whose added lines hold a string
/// literal in an expression or an object literal key, imported directly (relative import) by a
/// test that compares output with snapshots or golden files, while no snapshot or golden file
/// changed in the turn. A test that uses only inline snapshots counts as updated when it
/// changed. Run as advice.
pub struct SnapshotStale;

impl CodeBuiltin for SnapshotStale {
    fn name(&self) -> &'static str {
        "snapshot_stale"
    }

    fn run_with(&self, ctx: &BuiltinCtx, x: &Extra, _args: &[String]) -> Report {
        let mut r = Report::default();
        if ctx.scope != Scope::Turn || ctx.changed.iter().any(|f| SNAPSHOT_FILE.is_match(f)) {
            return r;
        }
        let sources: BTreeSet<String> = x
            .files
            .iter()
            .filter(|f| x.has_added(f))
            .filter(|f| {
                read(&ctx.root, f).is_some_and(|t| output_values_added(&t, |n| x.on_added(f, n)))
            })
            .cloned()
            .collect();
        if sources.is_empty() {
            return r;
        }
        let by_stem: BTreeMap<String, &String> = sources.iter().map(|s| (stem(s), s)).collect();
        let mut out = BTreeSet::new();
        for t in git_lines(&ctx.root, &["ls-files", "-co", "--exclude-standard"]) {
            if !(t.contains(".test.") || t.contains(".spec.") || t.contains("__tests__/")) {
                continue;
            }
            let Some(text) = read(&ctx.root, &t) else {
                continue;
            };
            if !SNAPSHOT_USE.is_match(&text) {
                continue;
            }
            if ctx.changed.contains(&t) && !FILE_SNAPSHOT_USE.is_match(&text) {
                continue;
            }
            let dir = t.rsplit_once('/').map_or(".", |(d, _)| d);
            for c in IMPORT.captures_iter(&text) {
                let s = stem(&normalize(&format!("{dir}/{}", &c[1])));
                let hit = by_stem
                    .get(&s)
                    .or_else(|| by_stem.get(&format!("{s}/index")));
                if let Some(f) = hit {
                    out.insert(format!(
                        "{f}: imported by {t}, which compares output with snapshots or golden files"
                    ));
                }
            }
        }
        if out.is_empty() {
            return r;
        }
        for l in out.into_iter().take(10) {
            r.findings.push(Finding::error(&ctx.rule, l));
        }
        r.findings.push(Finding::error(&ctx.rule, HINT));
        r
    }
}

fn stem(p: &str) -> String {
    STEM.replace(p, "").into_owned()
}

/// `a/./b/../c` → `a/c`.
fn normalize(p: &str) -> String {
    let mut out: Vec<&str> = vec![];
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." if !out.is_empty() => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    out.join("/")
}

/// Whether an added line (1-based, `added`) holds an output value: a string literal in an
/// expression (not in an import, a re-export, a type annotation, a literal type, an interface,
/// a type alias or type arguments), or an object literal key.
pub fn output_values_added(text: &str, added: impl Fn(u32) -> bool) -> bool {
    const TYPE_CONTEXT: &[&str] = &[
        "import_statement",
        "type_annotation",
        "literal_type",
        "interface_declaration",
        "type_alias_declaration",
        "type_arguments",
    ];
    let root = Tsx.ast_grep(text);
    let node = root.root();
    for n in node.dfs() {
        let kind = n.kind();
        let (a, b) = match kind.as_ref() {
            "string" | "template_string" => {
                let in_type = n.ancestors().any(|p| {
                    let k = p.kind();
                    TYPE_CONTEXT.contains(&k.as_ref())
                        || (k == "export_statement"
                            && p.field("source").is_some_and(|s| s.kind() == "string"))
                });
                if in_type {
                    continue;
                }
                (n.start_pos().line(), n.end_pos().line())
            }
            "pair" | "shorthand_property_identifier"
                if n.parent().is_some_and(|p| p.kind() == "object") =>
            {
                (n.start_pos().line(), n.start_pos().line())
            }
            _ => continue,
        };
        if (a..=b).any(|l| added(l as u32 + 1)) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_values() {
        let all = |_| true;
        assert!(output_values_added("return { name, kind: \"llm\" };", all));
        assert!(!output_values_added(
            "import type { Kind } from \"./kind\";\nexport interface S { mode: \"a\" | \"b\" }\n",
            all
        ));
        assert!(!output_values_added("return n + 2;", all));
        assert!(!SNAPSHOT_USE.is_match("// the golden path of the app\nexpect(x).toBe(1)"));
        assert!(SNAPSHOT_USE.is_match("readFileSync(\"src/testdata/a.json\")"));
        assert_eq!(normalize("src/./a/../b"), "src/b");
    }
}
