//! Releases (`release.sh`): hand-edited versions and release notes.

use std::collections::BTreeSet;

use forge_core::Report;

use crate::Run;
use crate::util::{dirname, join, re, toml_get};

/// `forge_json_version`: a package.json's top-level "version" (the first one at depth 1).
fn json_version(text: &str) -> Option<String> {
    let b: Vec<char> = text.chars().collect();
    let n = b.len();
    let mut d = 0i32;
    let mut i = 0;
    let skip_ws = |mut k: usize| {
        while k < n && matches!(b[k], ' ' | '\t' | '\r' | '\n') {
            k += 1;
        }
        k
    };
    while i < n {
        let c = b[i];
        if c == '"' {
            let mut j = i + 1;
            let mut s = String::new();
            while j < n {
                if b[j] == '\\' {
                    s.push(b[j]);
                    if let Some(x) = b.get(j + 1) {
                        s.push(*x);
                    }
                    j += 2;
                    continue;
                }
                if b[j] == '"' {
                    break;
                }
                s.push(b[j]);
                j += 1;
            }
            i = j + 1;
            let mut k = skip_ws(i);
            if d == 1 && k < n && b[k] == ':' && s == "version" {
                k = skip_ws(k + 1);
                if k < n && b[k] == '"' {
                    return Some(b[k + 1..].iter().take_while(|c| **c != '"').collect());
                }
                return None;
            }
            continue;
        }
        match c {
            '{' | '[' => d += 1,
            '}' | ']' => d -= 1,
            _ => {}
        }
        i += 1;
    }
    None
}

fn manifest_version(path: &str, text: &str) -> Option<String> {
    if path.ends_with("package.json") {
        json_version(text)
    } else {
        toml_get(text, "project", "version")
    }
}

/// `forge_version_changed`: each changed package.json / pyproject.toml present in the base
/// whose declared version differs from the base's: the release tooling sets versions.
pub fn version_changed(run: &Run<'_>, _args: &[String]) -> Report {
    let mut r = Report::default();
    for f in run.changed() {
        let manifest = ["package.json", "pyproject.toml"]
            .iter()
            .any(|m| f == m || f.ends_with(&format!("/{m}")));
        if !manifest || format!("/{f}").contains("/node_modules/") {
            continue;
        }
        let (Some(now), Some(was)) = (run.read(f), run.base(f)) else {
            continue;
        };
        let (was, now) = (manifest_version(f, &was), manifest_version(f, &now));
        if was == now {
            continue;
        }
        let show = |v: Option<String>| v.filter(|v| !v.is_empty()).unwrap_or_else(|| "none".into());
        r.findings.push(run.at(
            f,
            None,
            format!(
                "the version changed ({} -> {}); the release tooling sets versions, so revert it unless this change is the release itself",
                show(was),
                show(now)
            ),
        ));
    }
    r
}

/// How this project runs the changeset CLI: the runner of its package manager.
fn changeset_cmd(run: &Run<'_>) -> &'static str {
    if run.exists("bun.lock") || run.exists("bun.lockb") {
        "bunx changeset"
    } else if run.exists("pnpm-lock.yaml") {
        "pnpm changeset"
    } else if run.exists("yarn.lock") {
        "yarn changeset"
    } else {
        "npx changeset"
    }
}

/// `forge_release_notes <@role>`: Changesets (.changeset/config.json): a changed file of a
/// package whose package.json (the nearest one above it) is not "private": true, with no
/// .changeset/*.md added in the change. towncrier ([tool.towncrier] directory in towncrier.toml
/// or pyproject.toml): a changed .py file outside <@role> (tests) and that directory, with no
/// file added there.
pub fn release_notes(run: &Run<'_>, args: &[String]) -> Report {
    let role = args.first().map(String::as_str).unwrap_or("@python-tests");
    let added: BTreeSet<String> = run.added().into_iter().collect();
    let mut r = Report::default();
    if run.exists(".changeset/config.json")
        && !added.iter().any(|f| {
            re(r"^\.changeset/[^/]+\.md$").is_match(f) && f != ".changeset/README.md"
        })
    {
        let mut pkgs = BTreeSet::new();
        for f in run.changed() {
            if f.starts_with(".changeset/")
                || f.starts_with("node_modules/")
                || f.contains("/node_modules/")
            {
                continue;
            }
            let mut d = dirname(f);
            while d != "." && !run.exists(&join(&d, "package.json")) {
                d = dirname(&d);
            }
            let Some(pkg) = run.read(&join(&d, "package.json")) else {
                continue;
            };
            if re(r#""private"[ \t\r\n]*:[ \t\r\n]*true"#).is_match(&pkg) {
                continue;
            }
            pkgs.insert(d);
        }
        if !pkgs.is_empty() {
            r.findings.push(run.finding(format!(
                "The change touches published packages ({}) and adds no changeset; add one (.changeset/<name>.md, e.g. {}) describing it, or say why it needs no release note",
                pkgs.into_iter().collect::<Vec<_>>().join(", "),
                changeset_cmd(run)
            )));
        }
    }
    let mut dir = run
        .read("towncrier.toml")
        .and_then(|t| toml_get(&t, "tool.towncrier", "directory"))
        .unwrap_or_default();
    if dir.is_empty() {
        dir = run
            .read("pyproject.toml")
            .and_then(|t| toml_get(&t, "tool.towncrier", "directory"))
            .unwrap_or_default();
    }
    let dir = dir.trim_end_matches('/');
    let dir = dir.strip_prefix("./").unwrap_or(dir);
    if !dir.is_empty() {
        let prefix = format!("{dir}/");
        if added.iter().any(|f| f.starts_with(&prefix)) {
            return r;
        }
        if let Some(src) = run.changed().iter().find(|f| {
            !f.starts_with(&prefix) && f.ends_with(".py") && run.exists(f) && !run.role(role, f)
        }) {
            r.findings.push(run.finding(format!(
                "The change touches source code ({src}) and adds no news fragment; add one in {dir}/ (towncrier create), or say why it needs no release note"
            )));
        }
    }
    r
}
