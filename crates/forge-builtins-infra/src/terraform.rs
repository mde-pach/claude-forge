//! Terraform and OpenTofu (`templates/engine/terraform.sh`): validate, tflint, trivy.

use std::collections::{BTreeSet, HashSet};
use std::path::Path;
use std::sync::LazyLock;

use forge_core::{NoteKind, Report};
use regex::Regex;

use crate::cx::{self, Cx, dir_of, join, path_arg};

macro_rules! re {
    ($name:ident, $src:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($src).expect("regex"));
    };
}

/// tflint's provider rulesets Forge pins (their versions are forge-tools').
const RULESETS: &[&str] = &["aws", "google", "azurerm"];

/// `forge_tf_dirs`: the directories of the in-scope .tf / .tofu files.
fn tf_dirs(cx: &Cx<'_>) -> Vec<String> {
    let d: BTreeSet<String> = cx
        .scope_files()
        .iter()
        .filter(|f| f.ends_with(".tf") || f.ends_with(".tofu"))
        .map(|f| dir_of(f))
        .collect();
    d.into_iter().collect()
}

/// `forge_tf_pick`: tofu when the project uses OpenTofu (a *.tofu file, or a lock file naming
/// registry.opentofu.org providers), else terraform.
fn tf_bin(cx: &Cx<'_>) -> &'static str {
    for f in cx.repo_files(&["*.tofu", "*.terraform.lock.hcl"]) {
        if f.ends_with(".tofu")
            || cx
                .now(&f)
                .is_some_and(|t| t.contains("registry.opentofu.org/"))
        {
            return "tofu";
        }
    }
    "terraform"
}

/// `forge_tf_validate`: `validate` in each directory of a changed .tf / .tofu file (every one
/// with --all), with the project's Terraform CLI. A directory validate cannot judge before
/// `init` (providers or modules not installed) is a note: init downloads providers and
/// configures the backend, which a check does not do.
pub fn tf_validate(cx: &Cx<'_>, _args: &[String]) -> Report {
    re!(INIT, r"(terraform|tofu) init");
    re!(ON, r"^  on (\S+) line ([0-9]+)");
    let mut r = Report::default();
    let dirs = tf_dirs(cx);
    if dirs.is_empty() {
        return r;
    }
    let bin = tf_bin(cx);
    let cmd = match cx.project(bin, "validate") {
        Ok(c) => c,
        Err(n) => {
            r.notes.push(n);
            return r;
        }
    };
    for d in dirs {
        let mut argv = cmd.clone();
        argv.extend(["validate".into(), "-no-color".into()]);
        let o = cx::run(&argv, &cx.path(&d), &[]);
        if o.code == Some(0) {
            continue;
        }
        let out = o.both();
        if let Some(e) = o.error {
            r.notes
                .push(cx.note(NoteKind::ForgeFailure, format!("{bin} could not run ({e})")));
            return r;
        }
        if INIT.is_match(&out) {
            r.notes.push(cx.note(
                NoteKind::Environment,
                format!("{d} is not initialised ({bin} init), so it is not validated"),
            ));
            continue;
        }
        let mut err: Option<String> = None;
        let mut loc: Option<(String, u32)> = None;
        let mut flush = |err: &mut Option<String>, loc: &mut Option<(String, u32)>| {
            if let Some(e) = err.take() {
                r.findings.push(match loc.take() {
                    Some((p, l)) => cx.at(&p, l, format!("Error: {e}")),
                    None => cx.error(format!("{d}: Error: {e}")),
                });
            }
            *loc = None;
        };
        for l in out.lines() {
            if let Some(e) = l.strip_prefix("Error: ") {
                flush(&mut err, &mut loc);
                err = Some(e.to_string());
            } else if err.is_some()
                && loc.is_none()
                && let Some(c) = ON.captures(l)
            {
                loc = Some((join(&d, &c[1]), c[2].parse().unwrap_or(0)));
            } else if l.starts_with("Warning: ") {
                flush(&mut err, &mut loc);
            }
        }
        flush(&mut err, &mut loc);
    }
    r
}

/// `forge_tflint`: the pinned tflint in each directory of a changed .tf / .tofu file (every one
/// with --all), with a config of Forge's own (a project .tflint.hcl cannot disable rules): the
/// terraform ruleset's recommended preset, plus the pinned aws / google / azurerm ruleset when a
/// .terraform.lock.hcl of the repository lists that provider. Every issue counts, whatever its
/// severity; a file tflint cannot parse is reported at its line.
pub fn tflint(cx: &Cx<'_>, _args: &[String]) -> Report {
    re!(
        ISSUE,
        r"^([^\s:]+):([0-9]+):([0-9]+): (Error|Warning|Notice) - (.*)$"
    );
    re!(PARSE, r"^([^\s:]+):([0-9]+):([0-9]+): error - (.*)$");
    re!(FAIL, r"(?m)^Failed to|^Error:");
    let mut r = Report::default();
    let dirs = tf_dirs(cx);
    if dirs.is_empty() {
        return r;
    }
    let bin = match cx.pinned("tflint") {
        Ok(b) => b,
        Err(n) => {
            r.notes.push(n);
            return r;
        }
    };
    let scratch = cx.scratch();
    let base = scratch
        .path()
        .join(format!("infra-tflint.{}", std::process::id()));
    let plugins = base.join("plugins");
    let _ = std::fs::create_dir_all(&plugins);
    let mut hcl =
        "plugin \"terraform\" {\n  enabled = true\n  preset  = \"recommended\"\n}\n".to_string();
    let locks: Vec<String> = cx
        .repo_files(&["*.terraform.lock.hcl"])
        .iter()
        .filter_map(|l| cx.now(l))
        .collect();
    let catalog = forge_tools::Catalog::builtin();
    for p in RULESETS {
        let used = Regex::new(&format!(
            r#"provider "registry\.(terraform\.io|opentofu\.org)/hashicorp/{p}""#
        ))
        .expect("regex");
        if !locks.iter().any(|t| used.is_match(t)) {
            continue;
        }
        let name = format!("tflint-ruleset-{p}");
        let Some(spec) = catalog.get(&name) else {
            continue;
        };
        let path = match cx.pinned(&name) {
            Ok(b) => b,
            Err(n) => {
                r.notes.push(n);
                let _ = std::fs::remove_dir_all(&base);
                return r;
            }
        };
        // where tflint looks for an installed plugin: no tflint --init (GitHub's API)
        let dir = plugins
            .join("github.com/terraform-linters")
            .join(&name)
            .join(spec.version);
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::os::unix::fs::symlink(&path, dir.join(&name));
        hcl.push_str(&format!(
            "plugin \"{p}\" {{\n  enabled = true\n  version = \"{}\"\n  source  = \"github.com/terraform-linters/{name}\"\n}}\n",
            spec.version
        ));
    }
    let cfg = base.join("tflint.hcl");
    let _ = std::fs::write(&cfg, hcl);
    let plugin_dir = path_arg(&plugins);
    for d in dirs {
        let argv = vec![
            path_arg(&bin),
            "--format".into(),
            "compact".into(),
            "--force".into(),
            "--chdir".into(),
            d.clone(),
            "--config".into(),
            path_arg(&cfg),
        ];
        // rulesets are plugin processes reached through a Unix socket under TMPDIR, whose path
        // macOS caps at 104 bytes
        let o = cx::run(
            &argv,
            cx.root(),
            &[("TMPDIR", "/tmp"), ("TFLINT_PLUGIN_DIR", &plugin_dir)],
        );
        let out = o.both();
        let before = r.findings.len();
        for l in out.lines() {
            if let Some(c) = ISSUE.captures(l) {
                r.findings
                    .push(cx.at(&c[1], c[2].parse().unwrap_or(0), c[5].to_string()));
            } else if let Some(c) = PARSE.captures(l) {
                r.findings.push(cx.at(
                    &c[1],
                    c[2].parse().unwrap_or(0),
                    format!("does not parse: {}", &c[4]),
                ));
            }
        }
        if r.findings.len() == before && (o.code != Some(0) || FAIL.is_match(&out)) {
            let why = o.error.clone().unwrap_or_else(|| cx::last_line(&out));
            r.notes.push(cx.note(
                NoteKind::ForgeFailure,
                format!("tflint could not run in {d} ({why})"),
            ));
        }
    }
    let _ = std::fs::remove_dir_all(&base);
    r
}

const TRIVY_TPL: &str = "{{- range . }}{{- $t := .Target }}{{- range .Misconfigurations }}{{ $t }}\t{{ .ID }}\t{{ .CauseMetadata.Resource }}\t{{ .CauseMetadata.StartLine }}\t{{ .Title }}\n{{ end }}{{ end }}";

/// `forge_trivy_config`: the pinned trivy (its bundled checks, no update; an empty ignore file
/// of Forge's, so .trivyignore does not apply) on each directory of a changed .tf / .tofu file,
/// now and as it was at the base: each HIGH or CRITICAL Terraform misconfiguration (check,
/// resource) the change introduced. Not with --all (no base to compare).
pub fn trivy_config(cx: &Cx<'_>, _args: &[String]) -> Report {
    let mut r = Report::default();
    if cx.all() {
        return r;
    }
    let dirs = tf_dirs(cx);
    if dirs.is_empty() {
        return r;
    }
    let bin = match cx.pinned("trivy") {
        Ok(b) => b,
        Err(n) => {
            r.notes.push(n);
            return r;
        }
    };
    let scratch = cx.scratch();
    let cache = cx.cache("trivy", &scratch);
    let work = scratch
        .path()
        .join(format!("infra-trivy.{}", std::process::id()));
    let _ = std::fs::create_dir_all(&work);
    let tpl = work.join("trivy.tpl");
    let ignore = work.join("trivyignore");
    let _ = std::fs::write(&tpl, TRIVY_TPL);
    let _ = std::fs::write(&ignore, "");
    let repo = cx.repo_files(&["."]);
    for (i, d) in dirs.iter().enumerate() {
        // the directory as it was: its files now, with the change undone
        let then_dir = work.join(format!("base{i}"));
        let _ = std::fs::create_dir_all(&then_dir);
        for f in repo.iter().filter(|f| dir_of(f) == *d) {
            let name = f.rsplit('/').next().unwrap_or(f);
            if cx.is_changed(f) {
                if let Some(b) = cx.base_path(f) {
                    let _ = std::fs::copy(b, then_dir.join(name));
                }
            } else if cx.exists(f) {
                let _ = std::fs::copy(cx.path(f), then_dir.join(name));
            }
        }
        for f in cx
            .ctx
            .changed
            .iter()
            .filter(|f| dir_of(f) == *d && !repo.contains(f))
        {
            if let Some(b) = cx.base_path(f) {
                let _ = std::fs::copy(b, then_dir.join(f.rsplit('/').next().unwrap_or(f)));
            }
        }
        let scan = |target: &Path| {
            let argv = vec![
                path_arg(&bin),
                "config".into(),
                "--quiet".into(),
                "--misconfig-scanners".into(),
                "terraform".into(),
                "--severity".into(),
                "HIGH,CRITICAL".into(),
                "--skip-check-update".into(),
                "--format".into(),
                "template".into(),
                "--cache-dir".into(),
                path_arg(&cache),
                "--ignorefile".into(),
                path_arg(&ignore),
                "--template".into(),
                format!("@{}", path_arg(&tpl)),
                path_arg(target),
            ];
            cx::run(&argv, cx.root(), &[])
        };
        let now = scan(&cx.path(d));
        let then = scan(&then_dir);
        if let Some(bad) = [&now, &then].into_iter().find(|o| o.code != Some(0)) {
            let why = bad
                .error
                .clone()
                .unwrap_or_else(|| cx::last_line(&bad.stderr));
            r.notes.push(cx.note(
                NoteKind::ForgeFailure,
                format!("trivy could not scan {d} ({why})"),
            ));
            continue;
        }
        let rows = |s: &str| -> Vec<Vec<String>> {
            s.lines()
                .map(|l| l.split('\t').map(str::to_string).collect::<Vec<_>>())
                .filter(|v| v.len() >= 5)
                .collect()
        };
        let old: HashSet<(String, String)> = rows(&then.stdout)
            .into_iter()
            .map(|v| (v[1].clone(), v[2].clone()))
            .collect();
        for v in rows(&now.stdout) {
            if old.contains(&(v[1].clone(), v[2].clone())) {
                continue;
            }
            r.findings.push(cx.at(
                &join(d, &v[0]),
                0,
                format!("{} {} ({}, line {})", v[1], v[4], v[2], v[3]),
            ));
        }
    }
    let _ = std::fs::remove_dir_all(&work);
    r
}
