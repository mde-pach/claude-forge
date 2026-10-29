//! Kubernetes and Helm (`templates/engine/kubernetes.sh`): kube-linter, helm lint and template
//! validated by kubeconform, chart versions.

use std::collections::BTreeSet;
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

// ---------------------------------------------------------------- chart versions

/// The top-level `version:` of a Chart.yaml.
fn chart_ver(text: &str) -> String {
    re!(COMMENT, r"\s#.*$");
    text.lines()
        .find_map(|l| l.strip_prefix("version:"))
        .map(|v| {
            COMMENT
                .replace(v.trim_start(), "")
                .chars()
                .filter(|c| !matches!(c, '"' | '\'' | ' ' | '\t' | '\r'))
                .collect()
        })
        .unwrap_or_default()
}

/// ct.yaml's `chart-dirs` (a flow list or a block list).
fn chart_dirs(ct: &str) -> Vec<String> {
    re!(ITEM, r"^\s+-");
    re!(TOP, r"^[^\s#]");
    let mut out = Vec::new();
    let mut on = false;
    for l in ct.lines() {
        if let Some(v) = l.strip_prefix("chart-dirs:") {
            on = true;
            let v = v.trim_start();
            if v.starts_with('[') {
                let v: String = v
                    .chars()
                    .map(|c| if "[]\",'".contains(c) { ' ' } else { c })
                    .collect();
                out.extend(v.split_whitespace().map(str::to_string));
                on = false;
            }
            continue;
        }
        if on && ITEM.is_match(l) {
            let v = l.trim_start().trim_start_matches('-').trim_start();
            let v: String = v.chars().filter(|c| !matches!(c, '"' | '\'')).collect();
            let v = v.trim_end();
            out.push(v.strip_suffix('/').unwrap_or(v).to_string());
            continue;
        }
        if on && TOP.is_match(l) {
            on = false;
        }
    }
    out
}

/// The nearest directory holding a Chart.yaml at or above `p`'s directory.
fn chart_of(cx: &Cx<'_>, p: &str) -> Option<String> {
    let mut d = dir_of(p);
    loop {
        if cx.exists(&join(&d, "Chart.yaml")) {
            return Some(d);
        }
        if d == "." {
            return None;
        }
        d = dir_of(&d);
    }
}

/// `forge_chart_version`: in a chart-testing repository (ct.yaml or ct.yml at the root, its
/// check-version-increment not false), each chart under ct's chart-dirs (default charts) with a
/// changed file keeps the base's Chart.yaml version. A new chart is fine.
pub fn chart_version(cx: &Cx<'_>, _args: &[String]) -> Report {
    re!(OFF, r"(?m)^check-version-increment:\s*false");
    let mut r = Report::default();
    let Some(ct) = ["ct.yaml", "ct.yml"].iter().find_map(|f| cx.now(f)) else {
        return r;
    };
    if OFF.is_match(&ct) || cx.all() {
        return r;
    }
    let mut dirs = chart_dirs(&ct);
    if dirs.is_empty() {
        dirs.push("charts".into());
    }
    let charts: BTreeSet<String> = cx
        .ctx
        .changed
        .iter()
        .filter_map(|f| chart_of(cx, f))
        .collect();
    for c in charts {
        if !dirs.iter().any(|d| c.starts_with(&format!("{d}/"))) {
            continue;
        }
        let file = join(&c, "Chart.yaml");
        let Some(was) = cx.before(&file) else {
            continue;
        };
        let (now, was) = (
            chart_ver(&cx.now(&file).unwrap_or_default()),
            chart_ver(&was),
        );
        if !was.is_empty() && now == was {
            r.findings.push(cx.error(format!(
                "chart {c} changed but its Chart.yaml version is still {now}; bump it (chart-testing checks it, and a published chart is otherwise not released)"
            )));
        }
    }
    r
}

// ---------------------------------------------------------------- kube-linter

/// `forge_kube_manifest`: a .yaml / .yml whose first document has top-level apiVersion and kind,
/// outside a Helm chart's templates/.
fn kube_manifest(cx: &Cx<'_>, p: &str) -> bool {
    re!(SEP, r"^(---|\.\.\.)(\s|$)");
    re!(SKIP, r"^\s*(#|$)");
    if !(p.ends_with(".yaml") || p.ends_with(".yml")) {
        return false;
    }
    let t = if p.starts_with("templates/") {
        Some(0)
    } else {
        p.find("/templates/").map(|i| i + 1)
    };
    if let Some(i) = t
        && cx.exists(&format!("{}Chart.yaml", &p[..i]))
    {
        return false;
    }
    let Some(text) = cx.now(p) else {
        return false;
    };
    let (mut body, mut a, mut k) = (false, false, false);
    for l in text.lines() {
        if SEP.is_match(l) {
            if body {
                break;
            }
            continue;
        }
        if SKIP.is_match(l) || l.starts_with('%') {
            continue;
        }
        body = true;
        a |= l.starts_with("apiVersion:");
        k |= l.starts_with("kind:");
    }
    a && k
}

/// A path a tool printed, relative to the root.
fn relative(cx: &Cx<'_>, p: &str) -> String {
    let roots = [
        cx.root().to_path_buf(),
        std::fs::canonicalize(cx.root()).unwrap_or_default(),
    ];
    for r in roots {
        let pre = format!("{}/", r.display());
        if r.as_os_str().len() > 1
            && let Some(rest) = p.strip_prefix(&pre)
        {
            return rest.to_string();
        }
    }
    p.strip_prefix("./").unwrap_or(p).to_string()
}

/// `forge_kube_linter [--no-new-ignores] <check>...`: the pinned kube-linter with only the named
/// checks (a config of Forge's own: the project's .kube-linter.yaml does not apply) on the
/// changed manifests (every one with --all), each linted with the manifests of its directory
/// (dangling-service and mismatching-selector need the objects next to it). --no-new-ignores: a
/// changed manifest also may not gain ignore-check.kube-linter.io annotations.
pub fn kube_linter(cx: &Cx<'_>, args: &[String]) -> Report {
    let mut r = Report::default();
    let (ignores, checks) = match args.first() {
        Some(a) if a == "--no-new-ignores" => (true, &args[1..]),
        _ => (false, args),
    };
    let changed: Vec<String> = cx
        .scope_files()
        .into_iter()
        .filter(|f| kube_manifest(cx, f))
        .collect();
    if changed.is_empty() {
        return r;
    }
    if ignores {
        // lines naming one, as grep -c counted them
        let count = |t: &str| {
            t.lines()
                .filter(|l| l.contains("ignore-check.kube-linter.io/"))
                .count()
        };
        for f in &changed {
            let now = count(&cx.now(f).unwrap_or_default());
            let then = cx
                .base_path(f)
                .and_then(|b| std::fs::read_to_string(b).ok())
                .map(|t| count(&t))
                .unwrap_or(0);
            if now > then {
                r.findings.push(cx.at(
                    f,
                    0,
                    format!(
                        "adds ignore-check.kube-linter.io annotations ({then} -> {now}); fix the manifest instead of silencing the check"
                    ),
                ));
            }
        }
    }
    let bin = match cx.pinned("kube-linter") {
        Ok(b) => b,
        Err(n) => {
            r.notes.push(n);
            return r;
        }
    };
    let dirs: BTreeSet<String> = changed.iter().map(|f| dir_of(f)).collect();
    let files: Vec<String> = cx
        .repo_files(&["*.yaml", "*.yml"])
        .into_iter()
        .filter(|f| dirs.contains(&dir_of(f)) && kube_manifest(cx, f))
        .collect();
    let scratch = cx.scratch();
    let cfg = scratch
        .path()
        .join(format!("infra-kl.{}.yaml", std::process::id()));
    let mut text = "checks:\n  doNotAutoAddDefaults: true\n  include:\n".to_string();
    for c in checks {
        text.push_str(&format!("    - {c}\n"));
    }
    let _ = std::fs::write(&cfg, text);
    let mut argv = cx::with(
        &bin,
        vec![
            "lint".into(),
            "--format".into(),
            "plain".into(),
            "--with-color=false".into(),
            "--config".into(),
            path_arg(&cfg),
        ],
    );
    argv.extend(files);
    let o = cx::run(&argv, cx.root(), &[]);
    let _ = std::fs::remove_file(&cfg);
    let out = o.both();
    re!(FOUND_N, r"Error: found [0-9]+ lint errors");
    // 1 is findings ("Error: found <n> lint errors") or a run that could not lint
    if o.code != Some(0) && !FOUND_N.is_match(&out) {
        let why = o.error.clone().unwrap_or_else(|| cx::last_line(&out));
        r.notes.push(cx.note(
            NoteKind::ForgeFailure,
            format!("kube-linter could not run ({why})"),
        ));
        return r;
    }
    re!(OBJ, r": \(object: ([^)]*)\) ");
    re!(CHECK, r"\(check: ([^,)]*)");
    re!(TAIL, r" \(check: .*$");
    for l in out.lines() {
        let Some(m) = OBJ.captures(l) else { continue };
        let whole = m.get(0).expect("match");
        let f = relative(cx, &l[..whole.start()]);
        if !changed.contains(&f) {
            continue;
        }
        let rest = &l[whole.end()..];
        let check = CHECK
            .captures(rest)
            .map(|c| c[1].to_string())
            .unwrap_or_default();
        let msg = TAIL.replace(rest, "");
        r.findings
            .push(cx.at(&f, 0, format!("{check}: {msg} ({})", &m[1])));
    }
    r
}

// ---------------------------------------------------------------- Helm

const CRDS: &str = "https://raw.githubusercontent.com/datreeio/CRDs-catalog/main/{{.Group}}/{{.ResourceKind}}_{{.ResourceAPIVersion}}.json";

/// `forge_kubeconform <label> <file>`: the pinned kubeconform in strict mode against the schemas
/// of Kubernetes and the CRDs catalog. A document whose apiVersion has no schema is a finding when
/// its group is one of Kubernetes' own (a removed apiVersion), else a note.
fn kubeconform(cx: &Cx<'_>, label: &str, file: &Path, r: &mut Report) {
    let bin = match cx.pinned("kubeconform") {
        Ok(b) => b,
        Err(n) => {
            r.notes.push(n);
            return;
        }
    };
    let scratch = cx.scratch();
    let cache = cx.cache("kubeconform", &scratch);
    let argv = cx::with(
        &bin,
        vec![
            "-strict".into(),
            "-output".into(),
            "json".into(),
            "-schema-location".into(),
            "default".into(),
            "-schema-location".into(),
            CRDS.into(),
            "-cache".into(),
            path_arg(&cache),
            path_arg(file),
        ],
    );
    let o = cx::run(&argv, cx.root(), &[]);
    let json: Option<serde_json::Value> = serde_json::from_str(&o.stdout).ok();
    let resources = json
        .as_ref()
        .and_then(|j| j["resources"].as_array().cloned())
        .unwrap_or_default();
    // invalid documents exit 1 with their JSON; an exit with no JSON could not validate
    if o.code != Some(0) && json.is_none() {
        let why = o.error.clone().unwrap_or_else(|| cx::last_line(&o.stderr));
        r.notes.push(cx.note(
            NoteKind::ForgeFailure,
            format!("{label}: kubeconform could not run ({why})"),
        ));
        return;
    }
    re!(
        PREFIX,
        r"^problem validating schema\. Check JSON formatting: jsonschema validation failed with .* - "
    );
    for d in resources {
        let s = |k: &str| d[k].as_str().unwrap_or("").to_string();
        let (kind, name, version, status) = (s("kind"), s("name"), s("version"), s("status"));
        let msg = PREFIX.replace(&s("msg"), "").into_owned();
        match status.as_str() {
            "statusInvalid" => r
                .findings
                .push(cx.error(format!("{label}: {kind} {name}: {msg}"))),
            "statusError" => {
                let group = version.rsplit_once('/').map(|x| x.0).unwrap_or("");
                if msg.starts_with("could not find schema") {
                    if group.is_empty() || !group.contains('.') || group.ends_with(".k8s.io") {
                        r.findings.push(cx.error(format!(
                            "{label}: {kind} {name}: apiVersion {version} is not served by current Kubernetes (no schema); use its current apiVersion"
                        )));
                    } else {
                        r.notes.push(cx.note(
                            NoteKind::NotEnforced,
                            format!("{label}: no schema for {kind} ({version})"),
                        ));
                    }
                } else if msg.starts_with("failed downloading schema") {
                    r.notes.push(cx.note(
                        NoteKind::Environment,
                        format!(
                            "{label}: the schema for {kind} could not be downloaded (offline?)"
                        ),
                    ));
                } else {
                    r.findings
                        .push(cx.error(format!("{label}: {kind} {name}: {msg}")));
                }
            }
            _ => {}
        }
    }
}

/// `forge_helm`: for each Helm chart with changed files (every chart with --all), `helm lint
/// --strict` and its `helm template` output validated by kubeconform. A chart whose dependencies
/// are not built is a note. Findings are the chart's, not attributed to lines.
pub fn helm(cx: &Cx<'_>, _args: &[String]) -> Report {
    re!(LINT_LINE, r"^\[(ERROR|WARNING)\]|^Error");
    re!(SUMMARY, r"^Error: [0-9]+ chart\(s\) linted");
    let mut r = Report::default();
    let charts: BTreeSet<String> = cx
        .scope_files()
        .iter()
        .filter_map(|f| chart_of(cx, f))
        .collect();
    if charts.is_empty() {
        return r;
    }
    let scratch = cx.scratch();
    for c in charts {
        let o = match cx.project("helm", "lint", std::slice::from_ref(&c), cx.root()) {
            Ok(o) => o,
            Err(n) => {
                r.notes.push(n);
                return r;
            }
        };
        let out = o.both();
        if out.contains("chart directory is missing these dependencies") {
            r.notes.push(cx.note(
                NoteKind::Environment,
                format!("{c}: its dependencies are not built (helm dependency build {c})"),
            ));
            continue;
        }
        if o.code != Some(0) {
            if let Some(e) = o.error {
                r.notes
                    .push(cx.note(NoteKind::ForgeFailure, format!("helm could not run ({e})")));
                return r;
            }
            for l in out
                .lines()
                .filter(|l| LINT_LINE.is_match(l) && !SUMMARY.is_match(l))
            {
                r.findings.push(cx.error(format!("{c}: helm lint: {l}")));
            }
            continue;
        }
        let o = match cx.project("helm", "template", std::slice::from_ref(&c), cx.root()) {
            Ok(o) => o,
            Err(n) => {
                r.notes.push(n);
                return r;
            }
        };
        if o.code != Some(0) {
            let err = cx::strip_ansi(&o.stderr);
            let first = err
                .lines()
                .find(|l| l.starts_with("Error"))
                .or_else(|| err.lines().find(|l| !l.trim().is_empty()))
                .unwrap_or("")
                .to_string();
            r.findings
                .push(cx.error(format!("{c}: helm template: {first}")));
            continue;
        }
        let file = scratch
            .path()
            .join(format!("infra-helm.{}.yaml", std::process::id()));
        if std::fs::write(&file, &o.stdout).is_ok() {
            kubeconform(cx, &c, &file, &mut r);
            let _ = std::fs::remove_file(&file);
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_dirs_and_versions() {
        assert_eq!(
            chart_dirs("chart-dirs:\n  - deploy/charts/\n  - 'x'\nother: 1\n"),
            ["deploy/charts", "x"]
        );
        assert_eq!(chart_dirs("chart-dirs: [a, \"b\"]\n"), ["a", "b"]);
        assert_eq!(
            chart_ver("apiVersion: v2\nversion: \"0.3.1\" # x\n"),
            "0.3.1"
        );
    }
}
