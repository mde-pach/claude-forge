//! The repository: leaked secrets and credential files, dotenv files, build artifacts, generated
//! files, editorconfig (the original `repository.sh`).

use std::collections::BTreeSet;
use std::process::Command;
use std::sync::{Arc, LazyLock};

use forge_core::builtin::BuiltinCtx;
use forge_core::{Finding, NoteKind, Report};
use forge_packs::model::{Stage, Target};
use regex::Regex;

use crate::ctx::{Extra, existing, is_all, read};
use crate::scope::RuleScope;
use crate::util::{attr, git, git_lines, last_line, note};
use crate::{CodeBuiltin, Options};

macro_rules! re {
    ($name:ident, $src:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($src).expect("regex"));
    };
}

/// Forge's gitleaks config: gitleaks' default rules without generic-api-key (it decides on
/// identifier names and entropy), plus vendor key formats the defaults lack.
pub const GITLEAKS_CONFIG: &str = r#"title = "forge"

[extend]
useDefault = true
disabledRules = ["generic-api-key"]

[[rules]]
id = "langsmith-api-key"
description = "LangSmith API key"
regex = "\\blsv2_(?:pt|sk)_[a-f0-9]{32}_[a-f0-9]{10}\\b"
keywords = ["lsv2_"]

[[rules]]
id = "pinecone-api-key"
description = "Pinecone API key"
regex = "\\bpcsk_[A-Za-z0-9_]{40,}\\b"
keywords = ["pcsk_"]
"#;

/// `forge_gitleaks`: each secret the pinned gitleaks finds in the in-scope files, scanned as
/// copies with Forge's config only (the project's .gitleaks.toml, .gitleaksignore and inline
/// gitleaks:allow comments do not apply), as `path:line rule: description`, redacted. The
/// checker keeps those on added lines.
pub struct Gitleaks(pub Arc<Options>);

impl CodeBuiltin for Gitleaks {
    fn name(&self) -> &'static str {
        "gitleaks"
    }

    fn run_with(&self, ctx: &BuiltinCtx, x: &Extra, _args: &[String]) -> Report {
        let mut r = Report::default();
        let files: Vec<String> = x
            .scope_files(ctx)
            .into_iter()
            .filter(|f| ctx.root.join(f).is_file())
            .collect();
        if files.is_empty() {
            return r;
        }
        let bin = match self.0.tools.pinned("gitleaks") {
            Ok(b) => b,
            Err(n) => {
                r.notes.push(n.for_rule(ctx.rule.clone()));
                return r;
            }
        };
        let Ok(tmp) = tempfile::tempdir() else {
            return r;
        };
        let (scan, ignore) = (tmp.path().join("scan"), tmp.path().join("ignore"));
        let config = tmp.path().join("config.toml");
        let tmpl = tmp.path().join("report.tmpl");
        let report = tmp.path().join("report");
        let setup = (|| -> std::io::Result<()> {
            std::fs::create_dir_all(&scan)?;
            std::fs::create_dir_all(&ignore)?;
            for f in &files {
                let dst = scan.join(f);
                if let Some(p) = dst.parent() {
                    std::fs::create_dir_all(p)?;
                }
                std::fs::copy(ctx.root.join(f), dst)?;
            }
            std::fs::write(&config, GITLEAKS_CONFIG)?;
            std::fs::write(
                &tmpl,
                "{{range .}}{{.File}}:{{.StartLine}} {{.RuleID}}: {{.Description}}\n{{end}}",
            )
        })();
        if let Err(e) = setup {
            r.notes.push(note(
                ctx,
                NoteKind::ForgeFailure,
                format!("gitleaks could not run (cannot copy the files to scan: {e})"),
            ));
            return r;
        }
        let out = Command::new(&bin)
            .args([
                "dir",
                "--no-banner",
                "--no-color",
                "--redact",
                "-l",
                "error",
                "--ignore-gitleaks-allow",
                "--exit-code",
                "0",
                "--report-format",
                "template",
                "-c",
            ])
            .arg(&config)
            .arg("-i")
            .arg(&ignore)
            .arg("--report-template")
            .arg(&tmpl)
            .arg("--report-path")
            .arg(&report)
            .arg(".")
            .current_dir(&scan)
            .output();
        match out {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                r.notes.push(note(
                    ctx,
                    NoteKind::ForgeFailure,
                    format!("gitleaks could not run ({})", last_line(&o)),
                ));
                return r;
            }
            Err(e) => {
                r.notes.push(note(
                    ctx,
                    NoteKind::ForgeFailure,
                    format!("gitleaks could not run ({e})"),
                ));
                return r;
            }
        }
        let found: BTreeSet<String> = std::fs::read_to_string(&report)
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.strip_prefix("./").unwrap_or(l).to_string())
            .collect();
        re!(LOC, r"^(.+?):([0-9]+) (.*)$");
        for l in found {
            match LOC.captures(&l) {
                Some(c) => r
                    .findings
                    .push(Finding::error(&ctx.rule, &c[3]).at(&c[1], c[2].parse::<u32>().ok())),
                None => r.findings.push(Finding::error(&ctx.rule, l)),
            }
        }
        r
    }
}

/// The tracked files and the changed files that exist, sorted.
fn tracked_and_changed(ctx: &BuiltinCtx) -> Vec<String> {
    let mut all: BTreeSet<String> = git_lines(&ctx.root, &["ls-files"]).into_iter().collect();
    all.extend(existing(ctx));
    all.into_iter()
        .filter(|f| ctx.root.join(f).is_file())
        .collect()
}

re!(
    CREDENTIAL,
    r"(^|/)([^/]+\.(p12|pfx|jks|keystore|key)|id_(rsa|ecdsa|ed25519)|\.pypirc|\.netrc|\.git-credentials)$"
);

/// `forge_credential_files`: each tracked or added credential container file, and each .npmrc
/// line giving `_authToken`, `_auth` or `_password` a literal value.
pub struct CredentialFiles;

impl CodeBuiltin for CredentialFiles {
    fn name(&self) -> &'static str {
        "credential_files"
    }

    fn run_with(&self, ctx: &BuiltinCtx, _x: &Extra, _args: &[String]) -> Report {
        re!(NPM_AUTH, r"(_authToken|_auth|_password)\s*=\s*[^$\s]");
        let mut r = Report::default();
        for f in tracked_and_changed(ctx) {
            if CREDENTIAL.is_match(&f) {
                r.findings.push(
                    Finding::error(
                        &ctx.rule,
                        "is a credential file; keep it out of the repository (git rm --cached, .gitignore) and load it from a secret store",
                    )
                    .at(&f, Some(0)),
                );
            }
            if f == ".npmrc" || f.ends_with("/.npmrc") {
                for (i, l) in read(&ctx.root, &f).unwrap_or_default().lines().enumerate() {
                    if NPM_AUTH.is_match(l) {
                        r.findings.push(
                            Finding::error(
                                &ctx.rule,
                                "sets a registry credential literally; use ${NPM_TOKEN}-style environment references",
                            )
                            .at(&f, Some(i as u32 + 1)),
                        );
                    }
                }
            }
        }
        r
    }
}

/// `forge_env_files`: each tracked or added dotenv file (role @env-files); then, when a template
/// or a .gitignore is part of the change (or with `--all`), each template's directory whose .env
/// git would not ignore.
pub struct EnvFiles;

impl CodeBuiltin for EnvFiles {
    fn name(&self) -> &'static str {
        "env_files"
    }

    fn run_with(&self, ctx: &BuiltinCtx, x: &Extra, _args: &[String]) -> Report {
        let mut r = Report::default();
        let files = tracked_and_changed(ctx);
        for f in files.iter().filter(|f| x.roles.matches("@env-files", f)) {
            r.findings.push(
                Finding::error(
                    &ctx.rule,
                    "is a dotenv file; keep it out of the repository (git rm --cached, .gitignore) and commit a template (.env.example) instead",
                )
                .at(f.as_str(), Some(0)),
            );
        }
        if !is_all(ctx)
            && !ctx.changed.iter().any(|f| {
                f == ".gitignore"
                    || f.ends_with("/.gitignore")
                    || x.roles.matches("@env-template", f)
            })
        {
            return r;
        }
        for t in files.iter().filter(|f| x.roles.matches("@env-template", f)) {
            let env = match t.rsplit_once('/') {
                Some((d, _)) => format!("{d}/.env"),
                None => ".env".to_string(),
            };
            if git(&ctx.root, &["check-ignore", "-q", "--no-index", "--", &env]).is_some() {
                continue;
            }
            r.findings.push(
                Finding::error(
                    &ctx.rule,
                    format!("is not git-ignored next to its template {t}; add .env to .gitignore"),
                )
                .at(env, None),
            );
        }
        r
    }
}

/// `forge_artifacts <@role>`: each file of the change in the role (dependency, cache and build
/// output) or under a directory holding pyvenv.cfg (a virtualenv) or CACHEDIR.TAG (a cache),
/// reported once per shallowest such directory.
pub struct Artifacts;

impl CodeBuiltin for Artifacts {
    fn name(&self) -> &'static str {
        "artifacts"
    }

    fn run_with(&self, ctx: &BuiltinCtx, x: &Extra, args: &[String]) -> Report {
        let role = args.first().map_or("@build-artifacts", String::as_str);
        let mut hits = BTreeSet::new();
        for f in existing(ctx) {
            let mut dir = String::new();
            let mut hit = None;
            let parts: Vec<&str> = f.split('/').collect();
            for seg in &parts[..parts.len() - 1] {
                if !dir.is_empty() {
                    dir.push('/');
                }
                dir.push_str(seg);
                let d = ctx.root.join(&dir);
                if d.join("pyvenv.cfg").is_file()
                    || d.join("CACHEDIR.TAG").is_file()
                    || x.roles.matches(role, &format!("{dir}/"))
                {
                    hit = Some(format!("{dir}/"));
                    break;
                }
            }
            if hit.is_none() && x.roles.matches(role, &f) {
                hit = Some(f.clone());
            }
            hits.extend(hit);
        }
        let mut r = Report::default();
        for p in hits {
            r.findings.push(
                Finding::error(
                    &ctx.rule,
                    "is not ignored: it is dependency, cache or build output; add it to .gitignore (and git rm -r --cached it if it was committed)",
                )
                .at(p, None),
            );
        }
        r
    }
}

/// `forge_generated_edits`: each changed in-scope file marked generated (linguist-generated in
/// .gitattributes, or `@generated` / `Code generated … DO NOT EDIT.` in its first 15 lines).
pub struct GeneratedEdits;

impl CodeBuiltin for GeneratedEdits {
    fn name(&self) -> &'static str {
        "generated_edits"
    }

    fn run_with(&self, ctx: &BuiltinCtx, x: &Extra, _args: &[String]) -> Report {
        re!(HEADER, r"@generated|Code generated .* DO NOT EDIT\.");
        let mut r = Report::default();
        for f in &x.files {
            let Some(text) = read(&ctx.root, f) else {
                continue;
            };
            let why = if attr(&ctx.root, "linguist-generated", f) == "true" {
                ".gitattributes marks it linguist-generated"
            } else if text.lines().take(15).any(|l| HEADER.is_match(l)) {
                "its header says it is generated"
            } else {
                continue;
            };
            r.findings.push(
                Finding::error(
                    &ctx.rule,
                    format!(
                        "is generated ({why}): change its source or generator and regenerate it, rather than editing it by hand"
                    ),
                )
                .at(f.as_str(), None),
            );
        }
        r
    }
}

/// `forge_editorconfig`: when the repository has an .editorconfig, the pinned
/// editorconfig-checker on the in-scope files no format check formats, with a config of Forge's
/// own (the project's .editorconfig-checker.json excludes and disables do not apply).
pub struct Editorconfig(pub Arc<Options>);

impl CodeBuiltin for Editorconfig {
    fn name(&self) -> &'static str {
        "editorconfig"
    }

    fn run_with(&self, ctx: &BuiltinCtx, x: &Extra, _args: &[String]) -> Report {
        let mut r = Report::default();
        if !ctx.root.join(".editorconfig").is_file() {
            return r;
        }
        let files = unformatted(ctx, x, x.scope_files(ctx));
        if files.is_empty() {
            return r;
        }
        let bin = match self.0.tools.pinned("editorconfig-checker") {
            Ok(b) => b,
            Err(n) => {
                r.notes.push(n.for_rule(ctx.rule.clone()));
                return r;
            }
        };
        let Ok(tmp) = tempfile::tempdir() else {
            return r;
        };
        let config = tmp.path().join("ec.json");
        if std::fs::write(&config, "{}\n").is_err() {
            return r;
        }
        let out = Command::new(&bin)
            .args(["-no-color", "-format", "gcc", "-config"])
            .arg(&config)
            .args(&files)
            .current_dir(&ctx.root)
            .output();
        let out = match out {
            Ok(o) => o,
            Err(e) => {
                r.notes.push(note(
                    ctx,
                    NoteKind::ForgeFailure,
                    format!("editorconfig-checker could not run ({e})"),
                ));
                return r;
            }
        };
        match out.status.code() {
            Some(0) => return r,
            Some(1) => {}
            _ => {
                r.notes.push(note(
                    ctx,
                    NoteKind::ForgeFailure,
                    format!("editorconfig-checker could not run ({})", last_line(&out)),
                ));
                return r;
            }
        }
        re!(GCC, r"^([^\s:]+):([0-9]+):([0-9]+): (?:error: )?(.*)$");
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        for l in text.lines() {
            if let Some(c) = GCC.captures(l) {
                r.findings
                    .push(Finding::error(&ctx.rule, &c[4]).at(&c[1], c[2].parse::<u32>().ok()));
            }
        }
        r
    }
}

/// The files no active format check covers (a formatter owns the others).
fn unformatted(ctx: &BuiltinCtx, x: &Extra, files: Vec<String>) -> Vec<String> {
    if x.checks.is_empty() || files.is_empty() {
        return files;
    }
    let cache = forge_home::Home::from_env().ok().map(|h| h.packs_dir());
    let Ok(set) = forge_packs::load(
        Some(&ctx.root),
        forge_packs::Options {
            cache_dir: cache.as_deref(),
        },
    ) else {
        return files;
    };
    let scopes: Vec<RuleScope> = set
        .packs
        .values()
        .flat_map(|p| p.rules.iter())
        .chain(set.local_rules.iter())
        .filter(|rule| x.checks.contains(&rule.id))
        .filter(|rule| {
            rule.enforce
                .iter()
                .any(|t| matches!(t, Target::Check(c) if c.stage == Stage::Format))
        })
        .map(|rule| RuleScope::of_check(rule, &x.roles, None))
        .collect();
    files
        .into_iter()
        .filter(|f| !scopes.iter().any(|s| s.matches(f)))
        .collect()
}
