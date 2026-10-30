//! GitHub Actions: zizmor, actionlint, and CI jobs that
//! disappear.

use std::sync::LazyLock;

use forge_core::{NoteKind, Report};
use regex::Regex;

use crate::cx::{self, Cx, path_arg};

macro_rules! re {
    ($name:ident, $src:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($src).expect("regex"));
    };
}

/// What the agent reads after a change the user may accept.
pub const ACCEPT_HINT: &str = "Restore it, or ask the user to accept this change.";

/// The fields of a GitHub annotation line (`::error file=…,line=…,title=…::message`).
pub fn annotation(l: &str) -> Option<(String, std::collections::BTreeMap<String, String>, String)> {
    re!(ANN, r"^::(error|warning|notice) ([^:]*)::(.*)$");
    let c = ANN.captures(l)?;
    let meta = c[2]
        .split(',')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    Some((c[1].to_string(), meta, c[3].to_string()))
}

/// `forge_zizmor <audit>...`: zizmor (offline audits, inline ignores and ignore rules not
/// honoured) on the in-scope files; the findings of the named audits.
pub fn zizmor(cx: &Cx<'_>, audits: &[String]) -> Report {
    re!(LOC, r"^[^:]*:[0-9]+: ");
    let mut r = Report::default();
    let files = cx.scope_files();
    if files.is_empty() {
        return r;
    }
    let bin = match cx.pinned("zizmor") {
        Ok(b) => b,
        Err(n) => {
            r.notes.push(n);
            return r;
        }
    };
    let mut argv = cx::with(
        &bin,
        vec![
            "--offline".into(),
            "--no-progress".into(),
            "--no-ignores".into(),
            "--format".into(),
            "github".into(),
        ],
    );
    argv.extend(files);
    let o = cx::run(&argv, cx.root(), &[]);
    // 0 clean, 11-14 findings; 1-3 an error, a usage error, no input it could read
    match o.code {
        Some(0) => return r,
        Some(c) if c >= 10 => {}
        _ => {
            let why = o.error.clone().unwrap_or_else(|| cx::last_line(&o.stderr));
            r.notes.push(cx.note(
                NoteKind::ForgeFailure,
                format!("zizmor could not run ({why})"),
            ));
            return r;
        }
    }
    for l in o.stdout.lines() {
        let Some((_, meta, msg)) = annotation(l) else {
            continue;
        };
        let title = meta.get("title").cloned().unwrap_or_default();
        if !audits.contains(&title) {
            continue;
        }
        let file = meta.get("file").cloned().unwrap_or_default();
        let line = meta.get("line").and_then(|l| l.parse().ok()).unwrap_or(0);
        r.findings
            .push(cx.at(&file, line, format!("{title}: {}", LOC.replace(&msg, ""))));
    }
    r
}

const ACTIONLINT_FORMAT: &str = r#"{{range $err := .}}{{$err.Filepath}}:{{$err.Line}}:{{$err.Column}}: {{$err.Kind}}: {{$err.Message}}\n{{end}}"#;

/// `forge_actionlint`: the pinned actionlint on the in-scope workflows. Its shellcheck and
/// pyflakes integrations are off (results do not depend on what is installed), and its config
/// is Forge's own, with only the self-hosted-runner and config-variables blocks of the
/// project's .github/actionlint.y*ml (the project's `paths:` ignores do not apply).
pub fn actionlint(cx: &Cx<'_>, _args: &[String]) -> Report {
    re!(TOP, r"^[^\s#]");
    re!(KEEP, r"^(self-hosted-runner|config-variables)\s*:");
    re!(LOC, r"^(\S+?):([0-9]+):([0-9]+): (.*)$");
    let mut r = Report::default();
    let files = cx.scope_files();
    if files.is_empty() {
        return r;
    }
    let bin = match cx.pinned("actionlint") {
        Ok(b) => b,
        Err(n) => {
            r.notes.push(n);
            return r;
        }
    };
    let mut config = String::new();
    let mut keep = false;
    for f in [".github/actionlint.yaml", ".github/actionlint.yml"] {
        for l in cx.now(f).unwrap_or_default().lines() {
            if TOP.is_match(l) {
                keep = KEEP.is_match(l);
            }
            if keep {
                config.push_str(l);
                config.push('\n');
            }
        }
    }
    let scratch = cx.scratch();
    let cfg = scratch
        .path()
        .join(format!("infra-actionlint.{}.yaml", std::process::id()));
    let _ = std::fs::write(&cfg, config);
    let mut argv = cx::with(
        &bin,
        vec![
            "-shellcheck=".into(),
            "-pyflakes=".into(),
            "-no-color".into(),
            "-format".into(),
            ACTIONLINT_FORMAT.into(),
            "-config-file".into(),
            path_arg(&cfg),
        ],
    );
    argv.extend(files);
    let o = cx::run(&argv, cx.root(), &[]);
    let _ = std::fs::remove_file(&cfg);
    match o.code {
        Some(0) => {}
        Some(1) => {
            for l in o.stdout.lines().filter(|l| !l.trim().is_empty()) {
                r.findings.push(match LOC.captures(l) {
                    Some(c) => cx.at(&c[1], c[2].parse().unwrap_or(0), c[4].to_string()),
                    None => cx.error(l),
                });
            }
        }
        _ => {
            let why = o.error.clone().unwrap_or_else(|| cx::last_line(&o.both()));
            r.notes.push(cx.note(
                NoteKind::ForgeFailure,
                format!("actionlint could not run ({why})"),
            ));
        }
    }
    r
}

/// `forge_workflow_jobs`: whether the workflow runs on push or pull_request, and the ids of its
/// jobs (the keys under the top-level `jobs:`).
fn workflow_jobs(text: &str) -> (bool, Vec<String>) {
    re!(TRIG, r"(^|[^A-Za-z_])(push|pull_request)([^A-Za-z_]|$)");
    re!(TOP, r"^[^\s#]");
    re!(SKIP, r"^\s*(#|$)");
    re!(ITEM, r"^\s+-");
    fn key(s: &str) -> String {
        let s = s.trim_start();
        let s = match s.find(':') {
            Some(i) => &s[..i],
            None => s,
        };
        s.trim_end()
            .chars()
            .filter(|c| !matches!(c, '"' | '\''))
            .collect()
    }
    let uncomment = |s: &str| s.split('#').next().unwrap_or("").to_string();
    let (mut on, mut jobs) = (false, Vec::new());
    let mut sect = String::new();
    let mut ind: Option<usize> = None;
    for raw in text.lines() {
        let l = raw.strip_suffix('\r').unwrap_or(raw);
        if TOP.is_match(l) {
            let top = key(l);
            sect = top.clone();
            ind = None;
            if top == "on" || top == "true" {
                let v = l.split_once(':').map(|x| x.1).unwrap_or("");
                on |= TRIG.is_match(&uncomment(v));
                sect = "on".into();
            }
            continue;
        }
        if SKIP.is_match(l) {
            continue;
        }
        if sect == "on" {
            let l = uncomment(l);
            if ITEM.is_match(&l) {
                on |= TRIG.is_match(&l);
            } else {
                let k = key(&l);
                on |= k == "push" || k == "pull_request";
            }
            continue;
        }
        if sect == "jobs" {
            let w = l.len() - l.trim_start().len();
            let i = *ind.get_or_insert(w);
            if w == i {
                jobs.push(key(l));
            }
        }
    }
    (on, jobs)
}

/// `forge_ci_jobs`: for each workflow of the change that existed in the base and ran on push or
/// pull_request there, each job of the base that is gone now (the whole file deleted included),
/// unless the user accepted that line.
pub fn ci_jobs(cx: &Cx<'_>, _args: &[String]) -> Report {
    let mut found = Vec::new();
    for f in &cx.ctx.changed {
        let wf =
            f.starts_with(".github/workflows/") && (f.ends_with(".yml") || f.ends_with(".yaml"));
        if !wf {
            continue;
        }
        let Some(then) = cx
            .base_path(f)
            .and_then(|b| std::fs::read_to_string(b).ok())
        else {
            continue;
        };
        let (on, was) = workflow_jobs(&then);
        if !on {
            continue;
        }
        let now = cx.now(f).map(|t| workflow_jobs(&t).1).unwrap_or_default();
        for j in was.iter().filter(|j| !now.contains(j)) {
            found.push(cx.at(
                f,
                0,
                format!(
                    "job {j} was removed from a workflow that runs on push or pull_request; restore it rather than switching a check off"
                ),
            ));
        }
    }
    cx.unaccepted(found, ACCEPT_HINT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jobs_and_triggers() {
        let (on, jobs) = workflow_jobs(
            "name: ci\non:\n  pull_request:\n  push:\n    branches: [main]\njobs:\n  lint:\n    runs-on: x\n    steps:\n      - run: a\n  'test-unit':\n    runs-on: x\n",
        );
        assert!(on);
        assert_eq!(jobs, ["lint", "test-unit"]);
        assert!(workflow_jobs("on: [push, pull_request]\n").0);
        assert!(!workflow_jobs("on:\n  workflow_dispatch:\n").0);
    }

    #[test]
    fn annotations() {
        let (lvl, m, msg) =
            annotation("::error file=a.yml,line=3,title=unpinned-uses::a.yml:3: x: y").unwrap();
        assert_eq!(lvl, "error");
        assert_eq!(m["line"], "3");
        assert_eq!(msg, "a.yml:3: x: y");
    }
}
