//! Test integrity (§10.5) and the untested-change advice (§10.7).

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;
use std::sync::LazyLock;

use ast_grep_config::RuleConfig;
use ast_grep_language::SupportLang;
use forge_core::NoteKind;
use regex::Regex;

use super::{Invocation, Registry};
use crate::astgrep::{AstMatch, compile_yaml, file_language, scan_configs};
use crate::change::diff_lines;

pub(super) fn register(r: &mut Registry) {
    r.register_raw("test_edits", test_edits);
    r.register_raw("untested_change", untested_change);
}

/// Test blocks: pytest test functions (with their decorators) and classes; Vitest/Playwright
/// `test` / `it` calls and the `describe` calls around them; test support code (top-level
/// functions and classes not named test* / Test*, top-level declarations, `vi.mock` calls).
const TEST_BLOCKS: &str = r#"id: forge-test-support
language: Python
rule:
  any:
    - all:
        - any: [{kind: function_definition}, {kind: class_definition}]
        - inside: {kind: module, stopBy: neighbor}
        - not: {has: {field: name, regex: "^(test|Test)"}}
    - all:
        - kind: decorated_definition
        - inside: {kind: module, stopBy: neighbor}
        - not: {has: {field: definition, has: {field: name, regex: "^(test|Test)"}}}
---
id: forge-test-support
language: Tsx
rule:
  any:
    - all:
        - any: [{kind: function_declaration}, {kind: class_declaration}, {kind: lexical_declaration}, {kind: variable_declaration}]
        - any: [{inside: {kind: program, stopBy: neighbor}}, {inside: {kind: export_statement, stopBy: neighbor, inside: {kind: program, stopBy: neighbor}}}]
    - {kind: expression_statement, regex: "^vi[.](do)?[mM]ock[(]", inside: {kind: program, stopBy: neighbor}}
---
id: forge-test
language: Python
rule:
  any:
    - kind: decorated_definition
      has: {field: definition, kind: function_definition, has: {field: name, regex: "^test"}}
    - all:
        - kind: function_definition
        - has: {field: name, regex: "^test"}
        - not: {inside: {kind: decorated_definition, stopBy: neighbor}}
---
id: forge-test-scope
language: Python
rule: {kind: class_definition}
---
id: forge-test
language: Tsx
rule: {pattern: "$F($NAME, $$$REST)"}
constraints:
  F: {all: [{regex: "^(test|it)([.(]|$)"}, {not: {regex: "^test[.](describe|step|use|extend|info|beforeEach|afterEach|beforeAll|afterAll)"}}]}
  NAME: {any: [{kind: string}, {kind: template_string}]}
---
id: forge-test-scope
language: Tsx
rule: {pattern: "$F($NAME, $$$REST)"}
constraints:
  F: {regex: "^(describe|suite|test[.]describe)([.(]|$)"}
  NAME: {any: [{kind: string}, {kind: template_string}]}
"#;

static BLOCKS: LazyLock<Vec<RuleConfig<SupportLang>>> =
    LazyLock::new(|| compile_yaml(TEST_BLOCKS).expect("test block rules compile"));

/// A test or a piece of test support code, keyed by file and key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub key: String,
    /// Its whole text without whitespace, semicolons and trailing commas, `'` read as `"`.
    pub print: String,
}

fn fingerprint(text: &str) -> String {
    let t: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    t.replace(';', "")
        .replace('\'', "\"")
        .replace(",)", ")")
        .replace(",]", "]")
        .replace(",}", "}")
}

static DEF_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(def|class)[ ]+([A-Za-z_][A-Za-z0-9_]*)").expect("regex"));
static VI_MOCK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^vi[.](do)?[mM]ock[(][^,)]*").expect("regex"));
static DECL_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(def|class|function\*?|const|let|var)[ ]+([A-Za-z_$][A-Za-z0-9_$]*)")
        .expect("regex")
});

fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && (s.starts_with(['"', '\'', '`'])) {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// The tests and test support code of one file (`forge_test_blocks`).
pub fn blocks(path: &str, source: &str) -> Vec<Block> {
    let Some(lang) = file_language(path) else {
        return Vec::new();
    };
    let rules: Vec<&RuleConfig<SupportLang>> =
        BLOCKS.iter().filter(|r| r.language == lang).collect();
    if rules.is_empty() {
        return Vec::new();
    }
    let ms: Vec<AstMatch> = scan_configs(&rules, lang, path, source);
    let name_of = |m: &AstMatch| -> String {
        if let Some((n, _)) = m.vars.get("NAME") {
            return unquote(n);
        }
        DEF_NAME
            .captures(&m.text)
            .map(|c| c[2].to_string())
            .unwrap_or_default()
    };
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut out = Vec::new();
    let mut numbered = |key: String| -> String {
        let n = seen.entry(key.clone()).or_default();
        *n += 1;
        if *n > 1 { format!("{key} #{n}") } else { key }
    };
    for m in ms.iter().filter(|m| m.rule == "forge-test-support") {
        let name = if let Some(c) = VI_MOCK.find(&m.text) {
            format!("{})", c.as_str())
        } else if let Some(c) = DECL_NAME.captures(&m.text) {
            c[2].to_string()
        } else {
            m.text.chars().take(40).collect()
        };
        let key = numbered(format!("support:{name}"));
        out.push(Block {
            key,
            print: fingerprint(&m.text),
        });
    }
    let scopes: Vec<&AstMatch> = ms.iter().filter(|m| m.rule == "forge-test-scope").collect();
    for m in ms.iter().filter(|m| m.rule == "forge-test") {
        let mut around: Vec<&&AstMatch> = scopes
            .iter()
            .filter(|s| s.byte_start <= m.byte_start && m.byte_end <= s.byte_end)
            .collect();
        around.sort_by_key(|s| s.byte_start);
        let mut key = String::new();
        for s in around {
            key.push_str(&name_of(s));
            key.push_str(" > ");
        }
        key.push_str(&name_of(m));
        let key = numbered(key);
        out.push(Block {
            key,
            print: fingerprint(&m.text),
        });
    }
    out
}

fn read(p: &Path) -> Option<String> {
    std::fs::read(p)
        .ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// `forge_test_edits`: Stop only. Each test of the check's files that existed at the start of
/// the turn must keep its text (formatting aside) and still be present, unless an Edit or Write
/// changing it was approved during the turn (`<snapshot>/tests-approved`: `<path><TAB><key>`
/// lines). Test support code changed or removed is advice, reported once per turn.
fn test_edits(inv: &Invocation<'_>, out: &mut dyn Write, _err: &mut dyn Write) -> i32 {
    if !inv.turn() {
        return 0;
    }
    let snap = inv.snapshot.clone().unwrap_or_default();
    let approved: BTreeSet<String> = read(&snap.join("tests-approved"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect();
    let mut tests = Vec::new();
    let mut support = Vec::new();
    for f in &inv.files {
        let Some(base) = inv.base_copy(f) else {
            continue;
        };
        let then = blocks(f, &read(&base).unwrap_or_default());
        let now = blocks(f, &read(&inv.root().join(f)).unwrap_or_default());
        let now_map: BTreeMap<&str, &str> = now
            .iter()
            .map(|b| (b.key.as_str(), b.print.as_str()))
            .collect();
        for b in &then {
            let how = match now_map.get(b.key.as_str()) {
                None => "removed",
                Some(p) if *p != b.print => "changed",
                _ => continue,
            };
            if let Some(name) = b.key.strip_prefix("support:") {
                support.push(format!("{f}: test support `{name}` was {how}"));
            } else if !approved.contains(&format!("{f}\t{}", b.key)) {
                tests.push(format!("{f}: test \"{}\" was {how}", b.key));
            }
        }
    }
    tests.sort();
    support.sort();
    let mut rc = 0;
    if !tests.is_empty() {
        for t in &tests {
            let _ = writeln!(out, "{t}");
        }
        let _ = writeln!(
            out,
            "These tests existed before this turn and were changed without approval. Restore them. If the task requires the change (it changes the behaviour they assert), make it with Edit or Write, which asks for approval."
        );
        rc = 1;
    }
    if !support.is_empty() {
        let seen_file = snap.join("reported");
        let seen = read(&seen_file).unwrap_or_default();
        let rule = inv.rule();
        let new: Vec<&String> = support
            .iter()
            .filter(|l| !seen.lines().any(|s| *s == format!("{rule}\t{l}")))
            .collect();
        if !new.is_empty() {
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&seen_file)
            {
                for l in &new {
                    let _ = writeln!(f, "{rule}\t{l}");
                }
            }
            for l in &new {
                let _ = writeln!(out, "{l}");
            }
            let _ = writeln!(
                out,
                "Test doubles, fixtures and helpers encode the interface the code's callers rely on. Keep that interface in production (e.g. a property alias for a moved attribute) unless the task removes it; if it does, carry on: this is reported once."
            );
            if rc == 0 {
                inv.mark_advised();
            }
            rc = 1;
        }
    }
    if tests.is_empty()
        && support.is_empty()
        && inv.files.iter().any(|f| file_language(f).is_none())
    {
        inv.note(
            NoteKind::NotEnforced,
            "test files of a language ast-grep does not parse are not compared",
        );
    }
    rc
}

/// Test files of the packs' conventions (pytest, Vitest, Playwright).
pub const TEST_PATH_ERE: &str = r"(^|/)(tests?|__tests__|e2e)/|(^|/)(test_[^/]*|[^/]*_test|conftest)\.py$|\.(test|spec)\.[^/]*$";

/// Whether a removed or added line of the change to `path` is code: not blank, not only a comment.
fn code_changed(inv: &Invocation<'_>, path: &str) -> bool {
    let before = inv
        .base_copy(path)
        .and_then(|b| read(&b))
        .unwrap_or_default();
    let after = read(&inv.root().join(path)).unwrap_or_default();
    let py = path.ends_with(".py") || path.ends_with(".pyi");
    diff_lines(&before, &after).iter().any(|l| {
        let t = l[1..].trim();
        if t.is_empty() {
            return false;
        }
        if py {
            !t.starts_with('#')
        } else {
            !(t.starts_with("//") || t.starts_with("/*") || t.starts_with('*'))
        }
    })
}

/// `forge_untested_change [<test-path-ERE>]`: the turn changed code in the check's files and
/// added or changed no test file.
fn untested_change(inv: &Invocation<'_>, out: &mut dyn Write, _err: &mut dyn Write) -> i32 {
    let ere = inv
        .args
        .first()
        .map(String::as_str)
        .unwrap_or(TEST_PATH_ERE);
    let Ok(re) = Regex::new(&crate::grep::ere_to_regex(ere)) else {
        return 0;
    };
    if inv.existing().iter().any(|f| re.is_match(f)) {
        return 0;
    }
    let changed: Vec<&String> = inv.files.iter().filter(|f| code_changed(inv, f)).collect();
    if changed.is_empty() {
        return 0;
    }
    let mut list = changed
        .iter()
        .take(3)
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    if changed.len() > 3 {
        list.push_str(&format!(" and {} more", changed.len() - 3));
    }
    let _ = writeln!(
        out,
        "You changed behaviour in {list} without adding or updating a test; add a test that fails without your change, or say why none is needed."
    );
    // a finding of forge_once / forge_advise: its status is theirs
    0
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn python_and_vitest_blocks() {
        let py = "import pytest\n\nclass TestA:\n    def test_x(self):\n        assert f(1) == 2\n\n@pytest.fixture\ndef db():\n    return 1\n\ndef test_y():\n    assert True\n";
        let b = blocks("tests/test_a.py", py);
        let keys: Vec<&str> = b.iter().map(|b| b.key.as_str()).collect();
        assert!(keys.contains(&"TestA > test_x"), "{keys:?}");
        assert!(keys.contains(&"test_y"));
        assert!(keys.contains(&"support:db"), "{keys:?}");
        let ts = "describe('math', () => {\n  it('adds', () => { expect(1 + 1).toBe(2); });\n  it('adds', () => {});\n});\n";
        let b = blocks("a.test.ts", ts);
        let keys: Vec<&str> = b.iter().map(|b| b.key.as_str()).collect();
        assert_eq!(keys, ["math > adds", "math > adds #2"]);
        // formatting is not a change
        let ts2 = "describe(\"math\", () => {\n  it(\"adds\", () => {\n    expect(1 + 1).toBe(2)\n  })\n  it('adds', () => {});\n});\n";
        assert_eq!(
            blocks("a.test.ts", ts2)[0].print,
            blocks("a.test.ts", ts)[0].print
        );
    }
}
