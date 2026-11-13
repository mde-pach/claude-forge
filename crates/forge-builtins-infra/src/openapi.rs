//! OpenAPI: linting, the linter-config ratchet, breaking changes,
//! generated clients kept in sync.

use std::sync::LazyLock;

use forge_core::{NoteKind, Report};
use regex::Regex;

use crate::cx::{self, Cx, is_excluded, path_arg};
use crate::github_actions::annotation;

macro_rules! re {
    ($name:ident, $src:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($src).expect("regex"));
    };
}

/// What the agent reads after a loosened setting.
const RATCHET_HINT: &str = "Restore the setting, or ask the user to accept this change.";

/// `forge_openapi_spec`: an OpenAPI or Swagger document (JSON or YAML with an openapi / swagger
/// version near its top).
fn is_spec(cx: &Cx<'_>, p: &str) -> bool {
    re!(SPEC, r#"["']?(openapi|swagger)["']?\s*:\s*["']?[0-9]"#);
    if !(p.ends_with(".json") || p.ends_with(".yaml") || p.ends_with(".yml")) {
        return false;
    }
    let Ok(bytes) = std::fs::read(cx.path(p)) else {
        return false;
    };
    let head = &bytes[..bytes.len().min(4096)];
    SPEC.is_match(&String::from_utf8_lossy(head))
}

/// The repository's OpenAPI documents.
fn specs(cx: &Cx<'_>) -> Vec<String> {
    cx.repo_files(&["*.yaml", "*.yml", "*.json"])
        .into_iter()
        .filter(|f| !is_excluded(f) && is_spec(cx, f))
        .collect()
}

/// `forge_openapi_lint`: each OpenAPI document of the repository linted by the project's
/// declared linter: the pinned Redocly CLI with redocly.yaml, the pinned Spectral CLI with
/// .spectral.y*ml / .json (both when both are configured), else Redocly's recommended rules.
pub fn openapi_lint(cx: &Cx<'_>, _args: &[String]) -> Report {
    re!(
        SPECTRAL_LINE,
        r"^(\S+):([0-9]+):([0-9]+) (error|warning|information|hint) (\S+) (.*)$"
    );
    let mut r = Report::default();
    let roots = specs(cx);
    if roots.is_empty() {
        return r;
    }
    let spectral = [".spectral.yaml", ".spectral.yml", ".spectral.json"]
        .into_iter()
        .find(|f| cx.exists(f));
    if cx.exists("redocly.yaml") || spectral.is_none() {
        match cx.pinned("redocly") {
            Err(n) => r.notes.push(n),
            Ok(bin) => {
                let mut argv =
                    cx::with(&bin, vec!["lint".into(), "--format=github-actions".into()]);
                argv.extend(roots.iter().cloned());
                let o = cx::run(&argv, cx.root(), &[]);
                let errors: Vec<&str> = o
                    .stdout
                    .lines()
                    .filter(|l| l.starts_with("::error "))
                    .collect();
                // 1: errors, or a document it cannot read; 2: a redocly.yaml it cannot load
                if o.code.is_none_or(|c| c > 1) || (o.code == Some(1) && errors.is_empty()) {
                    let why = o.error.clone().unwrap_or_else(|| cx::last_line(&o.stderr));
                    r.notes.push(cx.note(
                        NoteKind::ForgeFailure,
                        format!("redocly could not run ({why})"),
                    ));
                } else {
                    for l in errors {
                        let Some((_, meta, msg)) = annotation(l) else {
                            continue;
                        };
                        let msg = msg.split("%0A").next().unwrap_or("");
                        let g = |k: &str| meta.get(k).cloned().unwrap_or_default();
                        r.findings.push(cx.at(
                            &g("file"),
                            g("line").parse().unwrap_or(0),
                            format!("{}: {msg}", g("title")),
                        ));
                    }
                }
            }
        }
    }
    if let Some(ruleset) = spectral {
        match cx.pinned("spectral") {
            Err(n) => r.notes.push(n),
            Ok(bin) => {
                let mut argv = cx::with(
                    &bin,
                    vec![
                        "lint".into(),
                        "--format".into(),
                        "text".into(),
                        "--fail-severity".into(),
                        "error".into(),
                        "--ruleset".into(),
                        ruleset.into(),
                    ],
                );
                argv.extend(roots.iter().cloned());
                let o = cx::run(&argv, cx.root(), &[]);
                let out = o.both();
                let rows: Vec<regex::Captures<'_>> = out
                    .lines()
                    .filter_map(|l| SPECTRAL_LINE.captures(l))
                    .collect();
                if rows.is_empty() && o.code != Some(0) {
                    let why = o.error.clone().unwrap_or_else(|| cx::last_line(&out));
                    r.notes.push(cx.note(
                        NoteKind::ForgeFailure,
                        format!("spectral could not run ({why})"),
                    ));
                }
                for c in rows.iter().filter(|c| &c[4] == "error") {
                    let msg = c[6].trim();
                    let msg = msg
                        .strip_prefix('"')
                        .and_then(|m| m.strip_suffix('"'))
                        .unwrap_or(msg);
                    r.findings.push(cx.at(
                        &relative(cx, &c[1]),
                        c[2].parse().unwrap_or(0),
                        format!("{}: {msg}", &c[5]),
                    ));
                }
            }
        }
    }
    r
}

fn relative(cx: &Cx<'_>, p: &str) -> String {
    for root in [
        cx.root().to_path_buf(),
        std::fs::canonicalize(cx.root()).unwrap_or_default(),
    ] {
        if let Some(rest) = p.strip_prefix(&format!("{}/", root.display())) {
            return rest.to_string();
        }
    }
    p.to_string()
}

/// `forge_yaml_rules`: ("rule", level) for each entry of a Redocly / Spectral config's
/// top-level `rules:`, ("extends", entry) for each extends entry, ("overrides", n) per item.
fn yaml_rules(text: &str) -> Vec<(String, String)> {
    re!(SKIP, r"^\s*(#|$)");
    re!(VAL_HEAD, r"^[^:]*:\s*");
    re!(VAL_COMMENT, r"\s+#.*$");
    re!(KEY_TAIL, r"\s*:.*$");
    fn val(s: &str) -> String {
        let s = VAL_HEAD.replace(s, "");
        let s = VAL_COMMENT.replace(&s, "");
        let s: String = s.chars().filter(|c| !matches!(c, '"' | '\'')).collect();
        s.trim_end().to_string()
    }
    fn key(s: &str) -> String {
        let s = s.trim_start();
        let s = s.strip_prefix('-').unwrap_or(s).trim_start();
        let s = KEY_TAIL.replace(s, "");
        s.chars().filter(|c| !matches!(c, '"' | '\'')).collect()
    }
    let mut out = Vec::new();
    let (mut sect, mut ind, mut pend, mut no) = (String::new(), None::<usize>, String::new(), 0);
    for raw in text.lines() {
        let l = raw.strip_suffix('\r').unwrap_or(raw);
        if SKIP.is_match(l) {
            continue;
        }
        if !l.starts_with(char::is_whitespace) {
            sect = key(l);
            ind = None;
            if sect == "extends" {
                let v = val(l);
                if v.starts_with('[') {
                    let v: String = v.chars().filter(|c| !matches!(c, '[' | ']')).collect();
                    for a in v.split(',').map(str::trim).filter(|a| !a.is_empty()) {
                        out.push(("extends".into(), a.to_string()));
                    }
                } else if !v.is_empty() {
                    out.push(("extends".into(), v));
                }
            }
            continue;
        }
        let w = l.len() - l.trim_start().len();
        let i = *ind.get_or_insert(w);
        match sect.as_str() {
            "extends" if w == i => {
                let v = l.trim_start().trim_start_matches('-').trim_start();
                let v = v.strip_prefix('[').unwrap_or(v);
                let v: String = v
                    .chars()
                    .filter(|c| !matches!(c, '"' | '\'' | ']'))
                    .collect();
                let v = v.split(',').next().unwrap_or("").to_string();
                if !v.is_empty() {
                    out.push(("extends".into(), v));
                }
            }
            "overrides" if w == i && l.trim_start().starts_with('-') => {
                no += 1;
                out.push(("overrides".into(), no.to_string()));
            }
            "rules" if w == i => {
                let (k, v) = (key(l), val(l));
                if v.is_empty() {
                    pend = k;
                } else {
                    out.push((k, v));
                }
            }
            "rules" if w > i && !pend.is_empty() && key(l) == "severity" => {
                out.push((std::mem::take(&mut pend), val(l)));
            }
            _ => {}
        }
    }
    out
}

/// A Redocly / Spectral level as a number (higher is stricter).
fn level(l: &str) -> Option<u8> {
    Some(match l {
        "error" | "0" | "true" | "on" => 4,
        "warn" | "warning" | "1" => 3,
        "info" | "information" | "2" => 2,
        "hint" | "3" => 1,
        "off" | "false" | "-1" => 0,
        _ => return None,
    })
}

/// `forge_openapi_ratchet`: for each changed redocly.yaml / .redocly.yaml / .spectral.y*ml present
/// in the base, a rule lowered or removed, an extends entry removed, an overrides item added; for
/// a changed .redocly.lint-ignore.yaml, entries added. Unless the user accepted the line.
pub fn openapi_ratchet(cx: &Cx<'_>, _args: &[String]) -> Report {
    re!(ITEM, r"^\s*-");
    let mut found = Vec::new();
    for f in &cx.ctx.changed {
        let known = [
            "redocly.yaml",
            ".redocly.yaml",
            ".spectral.yaml",
            ".spectral.yml",
            ".redocly.lint-ignore.yaml",
        ];
        if !known.contains(&f.as_str()) {
            continue;
        }
        let Some(then) = cx
            .base_path(f)
            .and_then(|b| std::fs::read_to_string(b).ok())
        else {
            continue;
        };
        let now = cx.now(f).unwrap_or_default();
        if f == ".redocly.lint-ignore.yaml" {
            let n = |t: &str| t.lines().filter(|l| ITEM.is_match(l)).count();
            let (was, is) = (n(&then), n(&now));
            if is > was {
                found.push(cx.error(format!("{f}: ignored problems {was} -> {is}")));
            }
            continue;
        }
        let (then, now) = (yaml_rules(&then), yaml_rules(&now));
        for (k, v) in &then {
            match k.as_str() {
                "extends" => {
                    if !now.iter().any(|(a, b)| a == "extends" && b == v) {
                        found.push(cx.error(format!("{f}: extends no longer has {v}")));
                    }
                }
                "overrides" => {}
                _ => match now.iter().find(|(a, _)| a == k) {
                    None => found.push(cx.error(format!("{f}: rule {k} no longer set (was {v})"))),
                    Some((_, n)) => {
                        if let (Some(a), Some(b)) = (level(v), level(n))
                            && b < a
                        {
                            found.push(cx.error(format!("{f}: rule {k} {v} -> {n}")));
                        }
                    }
                },
            }
        }
        let is_rule = |k: &str| k != "extends" && k != "overrides";
        let mut seen = std::collections::HashSet::new();
        for (k, v) in &now {
            if is_rule(k)
                && !then.iter().any(|(a, _)| a == k)
                && seen.insert(k.clone())
                && matches!(v.as_str(), "off" | "false" | "-1")
            {
                found.push(cx.error(format!("{f}: rule {k} turned off")));
            }
        }
        let count = |v: &[(String, String)]| v.iter().filter(|(k, _)| k == "overrides").count();
        let (o, n) = (count(&then), count(&now));
        if n > o {
            let d = n - o;
            found.push(cx.error(format!(
                "{f}: overrides gains {d} entr{}",
                if d == 1 { "y" } else { "ies" }
            )));
        }
    }
    cx.unaccepted(found, RATCHET_HINT)
}

/// `forge_oasdiff`: each OpenAPI document of the repository that existed at the base, compared
/// by the pinned oasdiff with its base version: each breaking change (level ERR). The base side
/// is a copy of the repository's YAML and JSON files as they were, so multi-file $refs resolve.
/// Not with --all (no base to compare).
pub fn oasdiff(cx: &Cx<'_>, _args: &[String]) -> Report {
    let mut r = Report::default();
    if cx.all() {
        return r;
    }
    let all: Vec<String> = cx.repo_files(&["*.yaml", "*.yml", "*.json"]);
    let roots: Vec<String> = all.iter().filter(|f| is_spec(cx, f)).cloned().collect();
    if roots.is_empty() {
        return r;
    }
    let bin = match cx.pinned("oasdiff") {
        Ok(b) => b,
        Err(n) => {
            r.notes.push(n);
            return r;
        }
    };
    let scratch = cx.scratch();
    let base = scratch
        .path()
        .join(format!("infra-oas-base.{}", std::process::id()));
    let copy = |from: &std::path::Path, to: &str| {
        let dest = base.join(to);
        if let Some(p) = dest.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let _ = std::fs::copy(from, dest);
    };
    for f in &all {
        if cx.is_changed(f) {
            continue;
        }
        copy(&cx.path(f), f);
    }
    for f in &cx.ctx.changed {
        if let Some(b) = cx.base_path(f) {
            copy(&b, f);
        }
    }
    let prefixes: Vec<String> = [
        base.clone(),
        std::fs::canonicalize(&base).unwrap_or_default(),
    ]
    .iter()
    .map(|p| format!("{}/", p.display()))
    .collect();
    let mut lines = Vec::new();
    for f in &roots {
        let b = base.join(f);
        if !b.is_file() {
            continue;
        }
        let argv = cx::with(
            &bin,
            vec![
                "breaking".into(),
                path_arg(&b),
                f.clone(),
                "--fail-on".into(),
                "ERR".into(),
                "--format".into(),
                "singleline".into(),
            ],
        );
        let o = cx::run(&argv, cx.root(), &[]);
        // 0 no breaking change, 1 one (--fail-on ERR), 100-107 could not compare
        if !matches!(o.code, Some(0) | Some(1)) {
            let why = o.error.clone().unwrap_or_else(|| cx::last_line(&o.both()));
            r.notes.push(cx.note(
                NoteKind::ForgeFailure,
                format!("oasdiff could not compare {f} ({why})"),
            ));
            continue;
        }
        for l in o.both().lines().filter(|l| l.starts_with("error at ")) {
            let mut l = l.to_string();
            for p in &prefixes {
                l = l.replace(p.as_str(), "");
            }
            let l = l.trim_end().to_string();
            if !lines.contains(&l) {
                lines.push(l);
            }
        }
    }
    let _ = std::fs::remove_dir_all(&base);
    let found = lines.into_iter().map(|l| cx.error(l)).collect();
    let mut out = cx.unaccepted(
        found,
        "Restore what clients rely on, or ask the user to accept the break.",
    );
    out.notes = r.notes;
    out
}

re!(
    MARK,
    r"(?i)auto-generated by (openapi-typescript|@hey-api/openapi-ts)|generated by orval"
);
re!(
    GEN,
    r#""([^"]+)"\s*:\s*"[^"]*(openapi-typescript|orval|openapi-ts)"#
);

/// `forge_openapi_generated`: a file whose header says one of the generators wrote it.
fn is_generated(cx: &Cx<'_>, p: &str) -> bool {
    cx.now(p).is_some_and(|t| {
        let head: Vec<&str> = t.lines().take(15).collect();
        MARK.is_match(&head.join("\n"))
    })
}

/// `forge_openapi_pairs`: the "key": "value" pairs of a package.json that drive the generator.
fn pairs(text: &str) -> Vec<String> {
    re!(PAIR, r#""[^"]*"\s*:\s*"[^"]*""#);
    re!(WS, r"\s+");
    let t = text.lines().collect::<Vec<_>>().join(" ");
    PAIR.find_iter(&t)
        .map(|m| m.as_str())
        .filter(|p| {
            ["openapi-typescript", "orval", "openapi-ts"]
                .iter()
                .any(|g| p.contains(g))
        })
        .map(|p| WS.replace_all(p, " ").into_owned())
        .collect()
}

/// `forge_openapi_sync regenerate | hand-edits`: regenerate: a changed OpenAPI document needs a
/// changed generated client file. hand-edits: a changed generated file needs a changed document
/// or generator config (orval.config.*, openapi-ts.config.*, redocly.*, or a package.json line
/// naming the generator), when the repository tracks a document. Not with --all.
pub fn openapi_sync(cx: &Cx<'_>, args: &[String]) -> Report {
    re!(
        CONFIG,
        r"(^|/)(orval\.config\.[a-z]+|openapi-ts\.config\.[a-z]+|redocly\.[a-z]+)$"
    );
    let mut r = Report::default();
    if cx.all() {
        return r;
    }
    let pkgs = cx.git_files(&["ls-files", "--", "package.json", "*/package.json"]);
    let Some(script) = pkgs
        .iter()
        .filter_map(|p| cx.now(p))
        .find_map(|t| GEN.captures(&t).map(|c| c[1].to_string()))
    else {
        return r;
    };
    let existing = cx.existing();
    let specs: Vec<&String> = existing.iter().filter(|f| is_spec(cx, f)).collect();
    let generated: Vec<&String> = existing.iter().filter(|f| is_generated(cx, f)).collect();
    match args.first().map(String::as_str) {
        Some("regenerate") => {
            if specs.is_empty() || !generated.is_empty() {
                return r;
            }
            for f in specs {
                r.findings.push(cx.at(
                    f,
                    0,
                    "the OpenAPI document changed, but no generated client file did",
                ));
            }
            r.findings.push(cx.error(format!(
                "Regenerate the client (the \"{script}\" script) and keep both changes together."
            )));
        }
        Some("hand-edits") => {
            if generated.is_empty() || !specs.is_empty() {
                return r;
            }
            if existing.iter().any(|f| CONFIG.is_match(f)) {
                return r;
            }
            // a package.json counts when its generator lines (script, dependency) changed
            for p in existing
                .iter()
                .filter(|p| *p == "package.json" || p.ends_with("/package.json"))
            {
                let now = pairs(&cx.now(p).unwrap_or_default());
                let then = pairs(
                    &cx.base_path(p)
                        .and_then(|b| std::fs::read_to_string(b).ok())
                        .unwrap_or_default(),
                );
                if now != then {
                    return r;
                }
            }
            let tracked = cx
                .git_files(&["ls-files", "--", "*.json", "*.yaml", "*.yml"])
                .iter()
                .any(|f| is_spec(cx, f));
            if !tracked {
                return r;
            }
            for f in generated {
                r.findings.push(cx.at(
                    f,
                    0,
                    "generated from the OpenAPI document (its header says so), but neither the document nor the generator config changed",
                ));
            }
            r.findings.push(cx.error(format!(
                "Change the OpenAPI document (or the server it is generated from), then regenerate with the \"{script}\" script; never edit generated files by hand."
            )));
        }
        _ => {}
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_of_a_config() {
        let v = yaml_rules(
            "extends: [\"spectral:oas\", x]\nrules:\n  a: error\n  b:\n    severity: warn\noverrides:\n  - files: [x]\n",
        );
        let v: Vec<String> = v.iter().map(|(a, b)| format!("{a}={b}")).collect();
        assert_eq!(
            v,
            [
                "extends=spectral:oas",
                "extends=x",
                "a=error",
                "b=warn",
                "overrides=1"
            ]
        );
        let v = yaml_rules("extends:\n  - recommended\n");
        assert_eq!(v, [("extends".to_string(), "recommended".to_string())]);
    }

    #[test]
    fn generator_pairs() {
        assert_eq!(
            pairs("{\"scripts\": {\"gen\":  \"openapi-typescript a\"},\n \"x\": \"y\"}"),
            ["\"gen\": \"openapi-typescript a\""]
        );
    }
}
