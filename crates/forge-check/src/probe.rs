//! What the machine's tools can do, for the person (forge-render's [`ToolProbe`]): the version
//! of each tool the checks drive, the tools they need that are missing, what the installed
//! releases cannot do, and the tool targets' audits (`src/audit.ts`): the project's own tool run
//! on the rule's examples, to see whether its config reports the rule's code as an error.
//!
//! Machine state: it goes to `forge apply` and `forge status`, never into the rules files.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use forge_core::Note;
use forge_packs::model::{AuditedTool, Example, Language, Rule, ToolTarget};
use forge_render::plan::Plan;
use forge_render::report::{AuditResult, AuditStatus, ToolProbe};
use serde_json::Value;

use crate::adapters::{Adapter, OpLookup, build_args, cmp_version, locate, picked};
use crate::scope::Roles;
use crate::tools::{self, ToolCmd, ToolResolver, check_tools, has_tool, tool_label};

/// The checker's answers about the machine's tools.
pub struct CheckerProbe {
    pub tools: Arc<dyn ToolResolver>,
}

impl CheckerProbe {
    pub fn new(tools: Arc<dyn ToolResolver>) -> CheckerProbe {
        CheckerProbe { tools }
    }
}

/// The adapter operation each audited tool runs on the examples (§11, §16).
fn audit_op(tool: AuditedTool) -> &'static str {
    match tool {
        AuditedTool::Ruff => "lint-json-stdin",
        AuditedTool::Biome | AuditedTool::Eslint => "lint-json",
        AuditedTool::Tsc => "typecheck",
        AuditedTool::Ty => "typecheck-gitlab",
    }
}

/// `forge_do <tool> <op>` uses in a command: (tool, op).
fn do_uses(command: &str) -> Vec<(String, String)> {
    static RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"\bforge_do\s+([A-Za-z0-9_.-]+)\s+([A-Za-z0-9_-]+)").expect("regex")
    });
    RE.captures_iter(command)
        .map(|c| (c[1].to_string(), c[2].to_string()))
        .collect()
}

fn major_minor(v: &str) -> String {
    v.split('.').take(2).collect::<Vec<_>>().join(".")
}

impl ToolProbe for CheckerProbe {
    fn version(&self, repo: &Path, tool: &str) -> Option<String> {
        let name = picked(tool, repo, self.tools.as_ref());
        let a = Adapter::load(&name)?;
        locate(&a, self.tools.as_ref(), repo, repo).ok().map(|(_, v)| v)
    }

    fn audit(&self, repo: &Path, rule: &Rule, target: &ToolTarget, dir: &str) -> Option<AuditResult> {
        let mut r = audit_target(self.tools.as_ref(), repo, rule, target, dir);
        if !dir.is_empty() {
            r.dir = Some(dir.to_string());
        }
        Some(r)
    }

    fn missing_tools(&self, plan: &Plan) -> Vec<String> {
        let root = &plan.base;
        let mut out: Vec<String> = Vec::new();
        let mut push = |s: String| {
            if !out.contains(&s) {
                out.push(s);
            }
        };
        for st in &plan.active {
            for c in &st.checks {
                let commands: Vec<&str> = c.target.commands().collect();
                for t in check_tools(&commands) {
                    match &c.dirs {
                        Some(dirs) => {
                            for d in dirs {
                                if !has_tool(self.tools.as_ref(), root, &root.join(d), &t) {
                                    push(format!("{} ({d})", tool_label(root, &t)));
                                }
                            }
                        }
                        None => {
                            if !has_tool(self.tools.as_ref(), root, root, &t) {
                                push(tool_label(root, &t));
                            }
                        }
                    }
                }
            }
        }
        let typed_python = plan.active.iter().any(|st| {
            st.rule.scope.language == Language::Python
                && st
                    .astgrep
                    .as_ref()
                    .is_some_and(|a| a.target.where_.is_some())
        });
        if typed_python
            && let Some(pin) = Adapter::load("ty").and_then(|a| a.pinned)
            && self.tools.pinned("ty", &pin).is_err()
        {
            push(format!("ty {pin} (types engine)"));
        }
        out
    }

    fn tool_versions(&self, plan: &Plan) -> Vec<String> {
        let root = &plan.base;
        // tool → (op, rule) uses, in order of first use
        let mut uses: Vec<(String, Vec<(String, String)>)> = Vec::new();
        for st in &plan.active {
            for c in &st.checks {
                for cmd in c.target.commands() {
                    for (tool, op) in do_uses(cmd) {
                        let u = (op, st.rule.id.clone());
                        match uses.iter_mut().find(|(t, _)| *t == tool) {
                            Some((_, v)) => {
                                if !v.contains(&u) {
                                    v.push(u)
                                }
                            }
                            None => uses.push((tool, vec![u])),
                        }
                    }
                }
            }
        }
        let mut out = Vec::new();
        for (tool, ops) in uses {
            if !has_tool(self.tools.as_ref(), root, root, &tool) {
                continue;
            }
            let name = picked(&tool, root, self.tools.as_ref());
            let Some(a) = Adapter::load(&name) else {
                continue;
            };
            let version = match locate(&a, self.tools.as_ref(), root, root) {
                Ok((_, v)) => v,
                Err(_) => {
                    out.push(format!("{name}: its version could not be read"));
                    continue;
                }
            };
            let mut ids: Vec<&str> = ops.iter().map(|(_, r)| r.as_str()).collect();
            ids.dedup();
            if let Some((op, _)) = ops.first()
                && let OpLookup::NotCovered { first } = a.lookup(&version, op)
            {
                out.push(format!(
                    "{name} {version}: no Forge adapter covers it (Forge drives {name} {first} and later): {} not enforced",
                    ids.join(", ")
                ));
                continue;
            }
            if cmp_version(&major_minor(&a.verified), &major_minor(&version)).is_lt() {
                out.push(format!(
                    "{name} {version} is newer than the releases Forge's adapter was proven on (up to {v}): driven as {v}",
                    v = a.verified
                ));
            }
            for (op, rule) in &ops {
                if let OpLookup::Cannot { need } = a.lookup(&version, op) {
                    let need = need
                        .map(|n| format!(" (it needs {name} {n} or later)"))
                        .unwrap_or_default();
                    out.push(format!(
                        "{name} {version} cannot {op}{need}: {rule} not enforced"
                    ));
                }
            }
        }
        out
    }

    fn notes(&self, _plan: &Plan) -> Vec<Note> {
        Vec::new()
    }
}

/// Forge's pinned tools (adapters with a `pinned` version) the plan's checks run — by
/// `forge_do <tool>`, `forge_pinned <tool>`, a `forge_<tool>` builtin or by name — and ty when
/// a Python rule asks for types: what `forge apply` installs.
pub fn pinned_tools(plan: &Plan) -> Vec<String> {
    static WORDS: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"\bforge_(?:do|pinned)\s+([A-Za-z0-9_.-]+)|\bforge_([a-z_]+)|([A-Za-z0-9_.-]+)")
            .expect("regex")
    });
    let pinned: Vec<String> = Adapter::all()
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a.pinned.is_some())
        .map(|a| a.tool)
        .collect();
    let mut out: Vec<String> = Vec::new();
    let mut add = |t: &str| {
        if pinned.iter().any(|p| p == t) && !out.iter().any(|o| o == t) {
            out.push(t.to_string());
        }
    };
    for st in &plan.active {
        for c in &st.checks {
            for cmd in c.target.commands() {
                for cap in WORDS.captures_iter(cmd) {
                    if let Some(m) = cap.get(1).or(cap.get(3)) {
                        add(m.as_str());
                    } else if let Some(m) = cap.get(2) {
                        add(&m.as_str().replace('_', "-"));
                    }
                }
            }
        }
        if st.rule.scope.language == Language::Python
            && st
                .astgrep
                .as_ref()
                .is_some_and(|a| a.target.where_.is_some())
        {
            add("ty");
        }
    }
    out
}

// ------------------------------------------------------------------------------- audits

/// A tool rule's examples: code in the rule's scope, or a one-file fixture (its path).
fn tool_examples(rule: &Rule, matching: bool) -> Vec<(String, Option<String>)> {
    let ex = rule.examples.as_ref().and_then(|e| {
        if matching {
            e.match_.as_ref()
        } else {
            e.no_match.as_ref()
        }
    });
    ex.into_iter()
        .flatten()
        .filter_map(|e| match e {
            Example::Code(c) => Some((c.clone(), None)),
            Example::Fixture(f) if f.head.is_none() && f.files.len() == 1 => {
                let (p, c) = f.files.iter().next()?;
                Some((c.clone(), Some(p.clone())))
            }
            _ => None,
        })
        .collect()
}

fn ext_of(lang: Language) -> &'static str {
    match lang {
        Language::Python => ".py",
        Language::Typescript => ".ts",
        Language::Tsx => ".tsx",
        Language::Yaml => ".yaml",
        Language::Any => ".txt",
    }
}

/// A concrete relative path inside the rule's scope for an example file (`examplePath`).
pub fn example_path(rule: &Rule, stem: &str) -> String {
    let files = rule.scope.files.clone().unwrap_or_default();
    let globs = Roles::defaults().expand(&files);
    let ext = ext_of(rule.scope.language);
    let Some(glob) = globs.iter().find(|g| !g.starts_with('!')) else {
        return format!("{stem}{ext}");
    };
    let re = |p: &str| regex::Regex::new(p).expect("regex");
    let mut p = re(r"\{([^,}]*)[^}]*\}").replace_all(glob, "$1").into_owned();
    p = re(r"\.\*$").replace(&p, ext).into_owned();
    p = re(r"(^|/)\*\*/").replace_all(&p, "$1").into_owned();
    p = p.replace("**", stem).replace('*', stem).replace('?', "x");
    p = re(r"\[(!?)(.)[^\]]*\]")
        .replace_all(&p, |c: &regex::Captures| {
            if &c[1] == "!" {
                "x".to_string()
            } else {
                c[2].to_string()
            }
        })
        .into_owned();
    if p.ends_with('/') {
        p.push_str(stem);
    }
    let base = p.rsplit('/').next().unwrap_or(&p);
    if !re(r"\.[A-Za-z0-9]+$").is_match(base) {
        p.push_str(ext);
    }
    p
}

fn stem_of(n: usize) -> String {
    format!("forge_audit_{}_{n}", std::process::id())
}

fn example_rel(rule: &Rule, path: Option<&str>, n: usize) -> String {
    let Some(path) = path else {
        return example_path(rule, &stem_of(n));
    };
    let (dir, name) = match path.rsplit_once('/') {
        Some((d, f)) => (Some(d), f),
        None => (None, path),
    };
    let rel = format!(
        "{}{}",
        stem_of(n),
        name.find('.').map(|i| &name[i..]).unwrap_or("")
    );
    match dir {
        Some(d) => format!("{d}/{rel}"),
        None => rel,
    }
}

/// Temporary files inside the project, removed (with the directories made for them) on drop.
struct TempFiles {
    created: Vec<PathBuf>,
}

impl TempFiles {
    fn write(dir: &Path, files: &[(&str, &str)]) -> Option<TempFiles> {
        let mut t = TempFiles {
            created: Vec::new(),
        };
        for (rel, text) in files {
            let abs = dir.join(rel);
            if abs.exists() {
                return None;
            }
            let mut missing = Vec::new();
            let mut d = abs.parent()?.to_path_buf();
            while !d.exists() {
                missing.push(d.clone());
                d = d.parent()?.to_path_buf();
            }
            for m in missing.into_iter().rev() {
                std::fs::create_dir(&m).ok()?;
                t.created.push(m);
            }
            std::fs::write(&abs, text).ok()?;
            t.created.push(abs);
        }
        Some(t)
    }
}

impl Drop for TempFiles {
    fn drop(&mut self) {
        for p in self.created.iter().rev() {
            let _ = std::fs::remove_dir_all(p).or_else(|_| std::fs::remove_file(p));
        }
    }
}

struct ToolRun {
    stdout: String,
    stderr: String,
}

fn run(cmd: &ToolCmd, args: &[String], cwd: &Path, input: Option<&str>) -> Option<ToolRun> {
    use std::io::Write;
    let mut c = Command::new(&cmd.argv[0]);
    c.args(&cmd.argv[1..])
        .args(args)
        .current_dir(cwd)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = c.spawn().ok()?;
    if let (Some(i), Some(mut sin)) = (input, child.stdin.take()) {
        let i = i.to_string();
        std::thread::spawn(move || {
            let _ = sin.write_all(i.as_bytes());
        });
    }
    let out = child.wait_with_output().ok()?;
    Some(ToolRun {
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Fails,
    Warning,
    Absent,
}

fn parse_json(text: &str) -> Result<Value, String> {
    let start = text
        .find(['[', '{'])
        .ok_or_else(|| "no JSON in the output".to_string())?;
    serde_json::from_str(&text[start..]).map_err(|e| e.to_string())
}

fn judge(expects: &str, diags: &[(String, String)]) -> Verdict {
    let mine: Vec<&(String, String)> = diags.iter().filter(|(id, _)| id == expects).collect();
    if mine.is_empty() {
        Verdict::Absent
    } else if mine.iter().any(|(_, s)| s == "error") {
        Verdict::Fails
    } else {
        Verdict::Warning
    }
}

/// Whether the run reported `expects`, and at which level.
fn verdict(tool: AuditedTool, expects: &str, r: &ToolRun) -> Result<Verdict, String> {
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    match tool {
        AuditedTool::Tsc => {
            let re = regex::Regex::new(&format!(r"\berror {}\b", regex::escape(expects)))
                .map_err(|e| e.to_string())?;
            Ok(if re.is_match(&format!("{}{}", r.stdout, r.stderr)) {
                Verdict::Fails
            } else {
                Verdict::Absent
            })
        }
        AuditedTool::Ruff => {
            let v = parse_json(&r.stdout)?;
            let any = v
                .as_array()
                .ok_or("not a list")?
                .iter()
                .any(|d| d.get("code").and_then(Value::as_str) == Some(expects));
            Ok(if any { Verdict::Fails } else { Verdict::Absent })
        }
        AuditedTool::Eslint => {
            let v = parse_json(&r.stdout)?;
            let mut diags = Vec::new();
            for f in v.as_array().ok_or("not a list")? {
                for m in f
                    .get("messages")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if m.get("fatal").and_then(Value::as_bool) == Some(true) {
                        return Err(format!(
                            "eslint could not parse the example: {}",
                            s(m, "message")
                        ));
                    }
                    let sev = match m.get("severity").and_then(Value::as_i64) {
                        Some(2) => "error",
                        Some(1) => "warning",
                        _ => "",
                    };
                    diags.push((s(m, "ruleId"), sev.to_string()));
                }
            }
            Ok(judge(expects, &diags))
        }
        AuditedTool::Biome => {
            let v = parse_json(&r.stdout)?;
            let diags: Vec<(String, String)> = v
                .get("diagnostics")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|d| (s(d, "category"), s(d, "severity")))
                .collect();
            Ok(judge(expects, &diags))
        }
        AuditedTool::Ty => {
            let v = parse_json(&r.stdout)?;
            let diags: Vec<(String, String)> = v
                .as_array()
                .ok_or("not a list")?
                .iter()
                .map(|d| {
                    let sev = match s(d, "severity").as_str() {
                        "major" | "critical" | "blocker" => "error".to_string(),
                        "minor" => "warning".to_string(),
                        o => o.to_string(),
                    };
                    (s(d, "check_name"), sev)
                })
                .collect();
            Ok(judge(expects, &diags))
        }
    }
}

fn first_line(s: &str) -> String {
    let l = s.lines().next().unwrap_or("");
    if l.chars().count() > 60 {
        format!("{}...", l.chars().take(57).collect::<String>())
    } else {
        l.to_string()
    }
}

/// Runs the tool on one example.
fn run_example(
    tool: AuditedTool,
    cmd: &ToolCmd,
    words: &str,
    dir: &Path,
    rel: &str,
    code: &str,
    n: usize,
) -> Option<ToolRun> {
    match tool {
        AuditedTool::Ruff => run(cmd, &build_args(words, &[rel.to_string()]), dir, Some(code)),
        AuditedTool::Tsc => {
            let cfg = format!("tsconfig.{}.json", stem_of(n));
            let tsconfig = serde_json::json!({
                "extends": "./tsconfig.json",
                "compilerOptions": {"noEmit": true, "incremental": false, "composite": false},
                "include": [],
                "files": [rel],
            })
            .to_string();
            let _files = TempFiles::write(dir, &[(rel, code), (&cfg, &tsconfig)])?;
            run(cmd, &build_args(words, &[cfg.clone()]), dir, None)
        }
        _ => {
            let _files = TempFiles::write(dir, &[(rel, code)])?;
            run(cmd, &build_args(words, &[rel.to_string()]), dir, None)
        }
    }
}

/// §11: every match example fails the tool's run on the expected diagnostic; no no_match
/// example reports it. `dir`: the workspace package whose config is audited ("" the root).
pub fn audit_target(
    tools: &dyn ToolResolver,
    repo: &Path,
    rule: &Rule,
    t: &ToolTarget,
    dir: &str,
) -> AuditResult {
    let tool = t.tool;
    let name = tool.as_str();
    let cwd = if dir.is_empty() {
        repo.to_path_buf()
    } else {
        repo.join(dir)
    };
    let result = |status: AuditStatus, detail: Option<String>, snippet: Option<String>| {
        AuditResult {
            rule: rule.id.clone(),
            tool: name.to_string(),
            expects: t.expects.clone(),
            status,
            dir: None,
            detail,
            snippet,
        }
    };
    let Some(a) = Adapter::load(name) else {
        return result(
            AuditStatus::Unavailable,
            Some(format!("Forge has no adapter for {name}")),
            None,
        );
    };
    let (cmd, version) = match locate(&a, tools, repo, &cwd) {
        Ok(x) => x,
        Err(why) => {
            let detail = if tool == AuditedTool::Ty {
                format!("{why} (Forge runs its pinned ty)")
            } else if why.ends_with("is not installed") {
                let py = if tools::PYTHON_TOOLS.contains(&name) {
                    " or the project's Python environment"
                } else {
                    ""
                };
                format!(
                    "{name} is configured but not installed (node_modules/.bin, .venv/bin{py})"
                )
            } else {
                why
            };
            return result(AuditStatus::Skipped, Some(detail), None);
        }
    };
    let op = audit_op(tool);
    let words = match a.lookup(&version, op) {
        OpLookup::Args(w) => w,
        OpLookup::NotCovered { first } => {
            return result(
                AuditStatus::Unavailable,
                Some(format!(
                    "no Forge adapter covers {name} {version} (Forge drives {name} {first} and later)"
                )),
                None,
            );
        }
        OpLookup::Cannot { need } => {
            return result(
                AuditStatus::Unavailable,
                Some(format!(
                    "{name} {version} cannot {op} (it needs {name} {} or later)",
                    need.as_deref().unwrap_or("a newer release")
                )),
                None,
            );
        }
    };
    if a.pinned.is_none()
        && let Some(h) = a.codes.as_ref().and_then(|c| c.history.get(&t.expects))
        && cmp_version(&version, &h.since).is_lt()
    {
        return result(
            AuditStatus::Unavailable,
            Some(format!(
                "{} needs {name} ≥ {}; this project has {version}",
                t.expects, h.since
            )),
            None,
        );
    }
    let matching = tool_examples(rule, true);
    let not_matching = tool_examples(rule, false);
    if matching.is_empty() {
        return result(
            AuditStatus::Skipped,
            Some("the rule has no match examples".into()),
            None,
        );
    }
    let mut n = 0;
    let mut seen = BTreeSet::new();
    for (expect_match, (code, path)) in matching
        .iter()
        .map(|e| (true, e))
        .chain(not_matching.iter().map(|e| (false, e)))
    {
        let rel = example_rel(rule, path.as_deref(), n);
        n += 1;
        if !seen.insert(rel.clone()) {
            continue;
        }
        let shown = match path {
            Some(p) => format!("{p}: {code}"),
            None => code.clone(),
        };
        let Some(r) = run_example(tool, &cmd, &words, &cwd, &rel, code, n) else {
            return result(
                AuditStatus::Skipped,
                Some(format!("{name} could not run on the examples")),
                None,
            );
        };
        let v = match verdict(tool, &t.expects, &r) {
            Ok(v) => v,
            Err(e) => {
                let said = if e.starts_with("eslint could not parse") {
                    Some(e)
                } else {
                    format!("{}{}", r.stdout, r.stderr)
                        .lines()
                        .map(str::trim)
                        .find(|l| !l.is_empty() && !l.chars().all(|c| "━─-".contains(c)))
                        .map(str::to_string)
                };
                return result(
                    AuditStatus::Skipped,
                    Some(format!(
                        "could not read {name}'s output{}",
                        said.map(|s| format!(": {}", first_line(&s)))
                            .unwrap_or_default()
                    )),
                    None,
                );
            }
        };
        let snippet = Some(t.snippet.clone());
        if expect_match {
            match v {
                Verdict::Absent => {
                    return result(
                        AuditStatus::Unsatisfied,
                        Some(format!(
                            "{} not reported for: {}",
                            t.expects,
                            first_line(&shown)
                        )),
                        snippet,
                    );
                }
                Verdict::Warning => {
                    return result(
                        AuditStatus::Unsatisfied,
                        Some(format!(
                            "{} is only a warning in your {name} config, and Forge's checks do not block on warnings (e.g. {})",
                            t.expects,
                            first_line(&shown)
                        )),
                        snippet,
                    );
                }
                Verdict::Fails => {}
            }
        } else if v != Verdict::Absent {
            return result(
                AuditStatus::Unsatisfied,
                Some(format!(
                    "{} reported for a no_match example: {}",
                    t.expects,
                    first_line(&shown)
                )),
                snippet,
            );
        }
    }
    result(AuditStatus::Satisfied, None, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(files: &[&str], lang: &str) -> Rule {
        serde_json::from_value(serde_json::json!({
            "id": "t/r", "description": "Do not.",
            "scope": {"language": lang, "files": files},
            "enforce": [{"target": "tool", "tool": "ruff", "expects": "S307", "snippet": "x"}],
        }))
        .unwrap()
    }

    #[test]
    fn example_paths_are_in_scope() {
        assert_eq!(example_path(&rule(&["app/**/*.py"], "python"), "s"), "app/s.py");
        assert_eq!(example_path(&rule(&[], "python"), "s"), "s.py");
        assert_eq!(
            example_path(&rule(&["src/**/*.{ts,tsx}"], "typescript"), "s"),
            "src/s.ts"
        );
    }

    #[test]
    fn forge_do_uses() {
        assert_eq!(
            do_uses("forge_do ruff lint {files} && forge_do biome format-check"),
            [
                ("ruff".to_string(), "lint".to_string()),
                ("biome".to_string(), "format-check".to_string())
            ]
        );
    }

    #[test]
    fn verdicts() {
        let r = ToolRun {
            stdout: r#"[{"code": "S307"}]"#.into(),
            stderr: String::new(),
        };
        assert_eq!(verdict(AuditedTool::Ruff, "S307", &r), Ok(Verdict::Fails));
        assert_eq!(verdict(AuditedTool::Ruff, "S101", &r), Ok(Verdict::Absent));
        let r = ToolRun {
            stdout: r#"{"diagnostics": [{"category": "lint/x", "severity": "warning"}]}"#.into(),
            stderr: String::new(),
        };
        assert_eq!(verdict(AuditedTool::Biome, "lint/x", &r), Ok(Verdict::Warning));
    }
}
