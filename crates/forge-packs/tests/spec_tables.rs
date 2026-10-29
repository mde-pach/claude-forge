//! §13 of the specification is generated from the built-in packs as loaded: one file per root
//! pack (`spec/13-packs-and-rules/<root>.md`) and the index in its README (between the
//! `<!-- generated:packs -->` markers). This test renders them and fails when the files differ.
//!
//! Regenerate: `FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use forge_packs::model::{
    CheckTarget, ContextWhen, DepSpec, Ecosystem, GuardTarget, Mode, OneOrMany, Pack, PackDetect,
    Rule, Target,
};
use forge_packs::{Level, Options, PackSet};
use serde_json::Value;

const BEGIN: &str = "<!-- generated:packs -->";
const END: &str = "<!-- /generated:packs -->";
const HEADER: &str = "<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->";

fn spec_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/13-packs-and-rules")
}

fn bless() -> bool {
    std::env::var_os("FORGE_BLESS").is_some_and(|v| !v.is_empty() && v != "0")
}

// ---- Markdown helpers

/// Inline code, safe inside a table cell.
fn code(s: &str) -> String {
    let s = s.replace('\n', " ");
    let fence = if s.contains('`') { "``" } else { "`" };
    let pad = if s.starts_with('`') || s.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{fence}{pad}{}{pad}{fence}", s.replace('|', "\\|"))
}

/// Plain text, safe inside a table cell.
fn cell(s: &str) -> String {
    s.trim().replace('\n', " ").replace('|', "\\|")
}

fn codes<S: AsRef<str>>(items: impl IntoIterator<Item = S>) -> String {
    items
        .into_iter()
        .map(|s| code(s.as_ref()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn or_list(items: &[String]) -> String {
    match items.len() {
        0 => String::new(),
        1 => items[0].clone(),
        _ => format!(
            "{} or {}",
            items[..items.len() - 1].join(", "),
            items[items.len() - 1]
        ),
    }
}

fn ecosystem(e: Ecosystem) -> &'static str {
    match e {
        Ecosystem::Pypi => "PyPI",
        Ecosystem::Npm => "npm",
    }
}

fn dep(d: &DepSpec) -> String {
    let name = match &d.version {
        Some(v) => code(&format!("{} {v}", d.name)),
        None => code(&d.name),
    };
    format!("{name} ({})", ecosystem(d.ecosystem))
}

fn values(v: &OneOrMany<String>) -> String {
    or_list(&v.as_slice().iter().map(|s| code(s)).collect::<Vec<_>>())
}

/// A YAML flow rendering of a value (guard command shapes).
fn flow(v: &Value) -> String {
    match v {
        Value::String(s) => {
            let plain = !s.is_empty()
                && !s.contains([',', ':', '{', '}', '[', ']', '\'', '"', '#'])
                && !s.starts_with([' ', '*', '&', '!', '|', '>', '%', '@', '`', '-'])
                && !s.ends_with(' ');
            if plain || (s.starts_with('-') && !s.contains([',', ':', '{', '}', '[', ']', ' '])) {
                s.clone()
            } else {
                format!("'{}'", s.replace('\'', "''"))
            }
        }
        Value::Array(a) => format!("[{}]", a.iter().map(flow).collect::<Vec<_>>().join(", ")),
        Value::Object(o) => format!(
            "{{{}}}",
            o.iter()
                .map(|(k, v)| format!("{}: {}", flow(&Value::String(k.clone())), flow(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        other => other.to_string(),
    }
}

// ---- What each part of a pack says

fn detected_by(p: &Pack) -> String {
    if p.level == Level::User {
        return "User level; not detected: `forge apply --user` installs it.".into();
    }
    let Some(d) = &p.detect else {
        return "Not detected: active only when added in `forge.yaml` or required by another pack."
            .into();
    };
    let PackDetect {
        files_any,
        deps_any,
        library_any,
        always,
        sources_any,
        content_any,
        yaml_keys,
        facts_any,
    } = d;
    let mut any: Vec<String> = Vec::new();
    if always == &Some(true) {
        any.push("every repository".into());
    }
    if let Some(f) = facts_any {
        for (k, v) in f {
            any.push(format!("the fact {} is {}", code(k), values(v)));
        }
    }
    if let Some(deps) = deps_any {
        let ds: Vec<String> = deps.iter().map(dep).collect();
        any.push(if ds.len() == 1 {
            format!("a dependency on {}", ds[0])
        } else {
            format!("a dependency on one of {}", ds.join(", "))
        });
    }
    if let Some(f) = files_any {
        any.push(format!("a file matching {}", codes(f)));
    }
    if let Some(s) = sources_any {
        any.push(format!(
            "a source file of the project's own matching {}",
            codes(s)
        ));
    }
    if let Some(c) = content_any {
        for m in c {
            any.push(format!(
                "a line matching {} in a file matching {}",
                code(&m.pattern),
                codes(&m.files)
            ));
        }
    }
    if let Some(k) = yaml_keys {
        any.push(format!(
            "a YAML file whose first document has the top-level keys {}",
            codes(k)
        ));
    }
    if let Some(l) = library_any {
        let ls: Vec<String> = l.iter().map(|e| ecosystem(*e).to_string()).collect();
        any.push(format!(
            "the project publishes a library to {}",
            or_list(&ls)
        ));
    }
    if any.is_empty() {
        return "Not detected: active only when added in `forge.yaml` or required by another pack."
            .into();
    }
    format!("Detected by: {}.", any.join("; or "))
}

fn when_text(w: &ContextWhen) -> String {
    let mut all = Vec::new();
    if !w.deps.is_empty() {
        all.push(format!(
            "{} declared",
            w.deps.iter().map(dep).collect::<Vec<_>>().join(" and ")
        ));
    }
    if !w.files.is_empty() {
        all.push(format!("a file matches {}", codes(&w.files)));
    }
    for (k, v) in &w.facts {
        all.push(format!("{} is {}", code(k), values(v)));
    }
    if !w.packs.is_empty() {
        let ps: Vec<String> = w.packs.iter().map(|p| code(p)).collect();
        all.push(format!("pack {} active", or_list(&ps)));
    }
    all.join(", and ")
}

/// Builtins and adapter operations a check command calls.
fn calls(run: &str) -> Vec<String> {
    let re_do = regex::Regex::new(r"\bforge_do\s+([A-Za-z0-9_.-]+)\s+([A-Za-z0-9_-]+)").unwrap();
    let re_fn = regex::Regex::new(r"\bforge_[a-z0-9_]+").unwrap();
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: String| {
        if !out.contains(&s) {
            out.push(s);
        }
    };
    let mut spans = Vec::new();
    for c in re_do.captures_iter(run) {
        let m = c.get(0).unwrap();
        spans.push(m.range());
        push(format!("forge_do {} {}", &c[1], &c[2]));
    }
    for m in re_fn.find_iter(run) {
        if spans.iter().any(|r| r.contains(&m.start())) {
            continue;
        }
        push(m.as_str().to_string());
    }
    out
}

fn check_text(c: &CheckTarget) -> String {
    let mut s = format!("check {}: ", c.stage);
    let run = c.run.trim();
    if !run.contains('\n') && run.chars().count() <= 100 {
        s.push_str(&code(run));
    } else {
        let cs = calls(run);
        if cs.is_empty() {
            let _ = write!(s, "a shell script of {} lines", run.lines().count());
        } else {
            let _ = write!(s, "a shell script calling {}", codes(&cs));
        }
    }
    let mut extra = Vec::new();
    if let Some(w) = &c.when_changed {
        extra.push(format!(
            "when {} changes",
            or_list(&w.iter().map(|x| code(x)).collect::<Vec<_>>())
        ));
    }
    if c.fix_run.is_some() {
        extra.push("safe fixes after an edit".into());
    }
    if c.rerun.is_some() {
        extra.push("reruns failing tests".into());
    }
    if c.requires_local_db == Some(true) {
        extra.push("needs a local database".into());
    }
    if c.fail_on_warnings == Some(true) {
        extra.push("fails on warnings".into());
    }
    if let Some(t) = c.timeout {
        extra.push(format!("timeout {t} s"));
    }
    if !extra.is_empty() {
        let _ = write!(s, " ({})", extra.join("; "));
    }
    s
}

fn guard_text(g: &GuardTarget) -> String {
    let mut s = format!("guard {}", g.decision);
    let mut parts = Vec::new();
    if let Some(c) = &g.command {
        let shapes: Vec<String> = c
            .as_slice()
            .iter()
            .map(|m| code(&flow(&serde_json::to_value(m).unwrap())))
            .collect();
        parts.push(format!("command {}", shapes.join(" or ")));
    }
    if let Some(ch) = g.check {
        parts.push(format!("check {}", code(ch.as_str())));
    }
    if !parts.is_empty() {
        let _ = write!(s, ": {}", parts.join(", decided by "));
    }
    s
}

fn target_text(t: &Target) -> String {
    match t {
        Target::Tool(t) => format!("tool {} {}", t.tool, code(&t.expects)),
        Target::AstGrep(a) => {
            let mut s = format!("ast-grep {}", a.mode);
            if let Some(w) = &a.where_ {
                let _ = write!(
                    s,
                    " (where {}: types engine, §15)",
                    codes(w.keys().map(|k| format!("${k}")))
                );
            }
            s
        }
        Target::Permission(p) => format!("permission {} {}", p.decision, code(&p.rule)),
        Target::Guard(g) => guard_text(g),
        Target::Sandbox(s) => format!("sandbox {} = {}", code(&s.path), code(&s.value.to_string())),
        Target::Check(c) => check_text(c),
        Target::Grep(g) => {
            let mut s = format!("grep {} {}", g.mode, code(&g.pattern));
            if g.ignore_case == Some(true) {
                s.push_str(" (ignoring case)");
            }
            s
        }
    }
}

fn advisory(r: &Rule) -> bool {
    r.enforce.iter().all(|t| match t {
        Target::AstGrep(a) => a.mode == Mode::Advise,
        Target::Grep(g) => g.mode == Mode::Advise,
        Target::Check(c) => c.commands().any(|x| x.contains("forge_advise")),
        _ => false,
    })
}

fn scope_text(r: &Rule) -> String {
    let mut s = r.scope.language.to_string();
    if let Some(f) = &r.scope.files {
        let _ = write!(s, ": {}", codes(f));
    }
    s
}

fn rule_row(r: &Rule) -> String {
    let mut how: Vec<String> = r.enforce.iter().map(target_text).collect();
    if let Some(w) = &r.when {
        how.push(format!(
            "applies where {} declared",
            w.deps.iter().map(dep).collect::<Vec<_>>().join(" and ")
        ));
    }
    if advisory(r) {
        how.push("advice, never blocks".into());
    }
    if let Some(d) = &r.deprecated {
        how.push(format!("deprecated: replaced by {}", code(&d.replaced_by)));
    }
    format!(
        "| {} | {} | {} | {} |",
        code(&r.id),
        cell(&r.description),
        scope_text(r),
        how.join("<br>")
    )
}

fn render_pack(out: &mut String, set: &PackSet, p: &Pack) {
    let _ = writeln!(out, "### {}\n", p.id);
    let _ = writeln!(out, "{}\n", p.description.trim());
    let _ = writeln!(out, "{}", detected_by(p));
    if let Some(parent) = &p.parent {
        let _ = writeln!(out, "Child of {}: active only where it is.", code(parent));
    }
    if !p.requires.is_empty() {
        let _ = writeln!(out, "Requires {}.", codes(&p.requires));
    }
    if let Some(off) = &p.tool_off {
        for t in off {
            let _ = writeln!(
                out,
                "Switches off {} {} where active: {}.",
                t.tool,
                code(&t.expects),
                t.reason.trim().trim_end_matches('.')
            );
        }
    }
    out.push('\n');
    if !p.context.is_empty() {
        let _ = writeln!(out, "Context (in `.claude/rules/forge-*.md`, §7):\n");
        for c in &p.context {
            let mut line = String::from("- ");
            if let Some(id) = &c.id {
                let _ = write!(line, "{} ", code(id));
            }
            if let Some(w) = &c.when {
                let _ = write!(line, "(when {}) ", when_text(w));
            }
            line.push_str(&c.text.trim().replace('\n', " "));
            let _ = writeln!(out, "{line}");
        }
        out.push('\n');
    }
    if p.rules.is_empty() {
        let _ = writeln!(out, "No rules of its own.\n");
    } else {
        let _ = writeln!(out, "| Rule | Description | Scope | Enforced by |");
        let _ = writeln!(out, "|---|---|---|---|");
        for r in &p.rules {
            let _ = writeln!(out, "{}", rule_row(r));
        }
        out.push('\n');
    }
    let mut children: Vec<&Pack> = p.children.iter().filter_map(|c| set.packs.get(c)).collect();
    children.sort_by(|a, b| a.id.cmp(&b.id));
    for c in children {
        render_pack(out, set, c);
    }
}

fn subtree<'a>(set: &'a PackSet, p: &'a Pack, out: &mut Vec<&'a Pack>) {
    out.push(p);
    for c in &p.children {
        if let Some(c) = set.packs.get(c) {
            subtree(set, c, out);
        }
    }
}

fn roots(set: &PackSet) -> Vec<&Pack> {
    let mut r: Vec<&Pack> = set.packs.values().filter(|p| p.parent.is_none()).collect();
    r.sort_by(|a, b| a.id.cmp(&b.id));
    r
}

/// Every generated file: name → text.
fn render(set: &PackSet) -> (BTreeMap<String, String>, String) {
    let mut files = BTreeMap::new();
    let mut index = String::new();
    let _ = writeln!(index, "| Root pack | Level | Packs | Rules |");
    let _ = writeln!(index, "|---|---|---|---|");
    for root in roots(set) {
        let mut all = Vec::new();
        subtree(set, root, &mut all);
        let rules: usize = all.iter().map(|p| p.rules.len()).sum();
        let mut text = format!("{HEADER}\n\n## 13. Packs and rules: `{}`\n\n", root.id);
        render_pack(&mut text, set, root);
        let text = format!("{}\n", text.trim_end());
        let name = format!("{}.md", root.id);
        let _ = writeln!(
            index,
            "| [{}]({name}) | {} | {} | {rules} |",
            root.id,
            root.level,
            all.len()
        );
        files.insert(name, text);
    }
    let total_rules: usize = set.packs.values().map(|p| p.rules.len()).sum();
    let _ = writeln!(index, "\n{} packs, {} rules.", set.packs.len(), total_rules);
    (files, index)
}

fn splice(readme: &str, index: &str) -> Option<String> {
    let b = readme.find(BEGIN)?;
    let e = readme.find(END)?;
    (b < e).then(|| format!("{}{BEGIN}\n{index}{}", &readme[..b], &readme[e..]))
}

#[test]
fn spec_13_is_generated_from_the_packs() {
    let set = forge_packs::load(None, Options::default()).expect("built-in packs load");
    let (files, index) = render(&set);
    let dir = spec_dir();
    let mut stale = Vec::new();

    let readme_path = dir.join("README.md");
    let readme = std::fs::read_to_string(&readme_path).expect("spec/13 README.md");
    let want_readme = splice(&readme, &index)
        .unwrap_or_else(|| panic!("README.md needs the {BEGIN} ... {END} markers"));
    if readme != want_readme {
        if bless() {
            std::fs::write(&readme_path, &want_readme).unwrap();
        } else {
            stale.push("README.md".to_string());
        }
    }
    for (name, text) in &files {
        let path = dir.join(name);
        if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
            if bless() {
                std::fs::write(&path, text).unwrap();
            } else {
                stale.push(name.clone());
            }
        }
    }
    // a file for a root pack that no longer exists
    for e in std::fs::read_dir(&dir).unwrap() {
        let name = e.unwrap().file_name().to_string_lossy().into_owned();
        if name.ends_with(".md") && name != "README.md" && !files.contains_key(&name) {
            if bless() {
                std::fs::remove_file(dir.join(&name)).unwrap();
            } else {
                stale.push(format!("{name} (no such root pack)"));
            }
        }
    }
    assert!(
        stale.is_empty(),
        "spec/13 is out of date with the packs: {}. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables",
        stale.join(", ")
    );
}

/// Every pack the loader knows is in exactly one generated file, under its own heading, and
/// every rule is a row under its own pack's heading (no rule sits under another pack).
#[test]
fn every_rule_sits_under_its_own_pack() {
    let set = forge_packs::load(None, Options::default()).unwrap();
    let (files, _) = render(&set);
    for p in set.packs.values() {
        let heading = format!("\n### {}\n", p.id);
        let holders: Vec<&String> = files
            .iter()
            .filter(|(_, t)| t.contains(&heading))
            .map(|(n, _)| n)
            .collect();
        assert_eq!(holders.len(), 1, "{}: in {holders:?}", p.id);
        let text = &files[holders[0]];
        let start = text.find(&heading).unwrap() + heading.len();
        let section = match text[start..].find("\n### ") {
            Some(e) => &text[start..start + e],
            None => &text[start..],
        };
        for r in &p.rules {
            assert!(
                section.contains(&format!("| {} |", code(&r.id))),
                "{} is not under {}",
                r.id,
                p.id
            );
        }
        assert!(
            section.contains("Detected by")
                || section.contains("User level")
                || section.contains("Not detected"),
            "{}: no detection line",
            p.id
        );
    }
}

/// §6.1: every rule has examples. A rule with an ast-grep, grep, guard, permission or tool
/// target has both `match` and `no_match` examples. A rule of check targets only may go
/// without when it needs a whole project: its tests (stage `test`), its migrations (stage
/// `migration`), `astro check`, the Angular compiler, a lock-file check, or the file-size
/// check (whose files do not fit in an example).
#[test]
fn every_built_in_rule_has_examples() {
    use forge_packs::model::Stage;
    let set = forge_packs::load(None, Options::default()).unwrap();
    let mut bad = Vec::new();
    for r in set.packs.values().flat_map(|p| &p.rules) {
        let ex = r.examples.as_ref();
        let m = ex
            .and_then(|e| e.match_.as_ref())
            .is_some_and(|v| !v.is_empty());
        let n = ex
            .and_then(|e| e.no_match.as_ref())
            .is_some_and(|v| !v.is_empty());
        let checks_only = r.enforce.iter().all(|t| matches!(t, Target::Check(_)));
        if r.enforce.iter().all(|t| matches!(t, Target::Sandbox(_))) {
            // a settings value: nothing to run an example against
            continue;
        }
        if !checks_only {
            if !(m && n) {
                bad.push(format!("{}: needs match and no_match examples", r.id));
            }
            continue;
        }
        if m || n {
            continue;
        }
        let exempt = r.enforce.iter().all(|t| {
            let Target::Check(c) = t else { return false };
            matches!(c.stage, Stage::Test | Stage::Migration)
                || c.commands().any(|x| {
                    [
                        "lock-check",
                        "forge_do astro check",
                        "forge_do ngc",
                        "forge_large_files",
                    ]
                    .iter()
                    .any(|w| x.contains(w))
                })
        });
        if !exempt {
            bad.push(format!("{}: a check rule without examples", r.id));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
