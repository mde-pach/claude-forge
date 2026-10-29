//! Known vulnerabilities in dependencies (the original `supply-chain.sh`).

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use forge_core::builtin::BuiltinCtx;
use forge_core::{Finding, NoteKind, Report};

use crate::ctx::{Extra, base_copy, is_all};
use crate::util::{last_line, note};
use crate::{CodeBuiltin, Options};

/// One vulnerability osv-scanner reports: package, version, advisory id, fixed version ("" when
/// there is none).
type Vuln = (String, String, String, String);

/// `forge_osv`: for each in-scope lock file of the change, the pinned osv-scanner on it and on
/// its base copy, with an empty config of Forge's (osv-scanner.toml ignores do not apply): each
/// vulnerability (package, version, advisory) it has now and the base did not. Not with `--all`
/// (no base to compare with).
pub struct Osv(pub Arc<Options>);

impl CodeBuiltin for Osv {
    fn name(&self) -> &'static str {
        "osv"
    }

    fn run_with(&self, ctx: &BuiltinCtx, x: &Extra, _args: &[String]) -> Report {
        let mut r = Report::default();
        let files: Vec<&String> = x
            .files
            .iter()
            .filter(|f| ctx.root.join(f).is_file())
            .collect();
        if is_all(ctx) || files.is_empty() {
            return r;
        }
        let bin = match self.0.tools.pinned("osv-scanner") {
            Ok(b) => b,
            Err(n) => {
                r.notes.push(n.for_rule(ctx.rule.clone()));
                return r;
            }
        };
        let Ok(tmp) = tempfile::tempdir() else {
            return r;
        };
        let config = tmp.path().join("osv.toml");
        if std::fs::write(&config, "").is_err() {
            return r;
        }
        for f in files {
            if f.ends_with(".lockb") {
                r.notes.push(note(
                    ctx,
                    NoteKind::NotEnforced,
                    format!("{f} is binary, so osv-scanner cannot read it (bun.lock, the text lock file, can be)"),
                ));
                continue;
            }
            let now = match scan(&bin, &config, &ctx.root, f) {
                Ok(v) => v,
                Err(why) => {
                    r.notes.push(note(
                        ctx,
                        NoteKind::Environment,
                        format!("osv-scanner could not scan {f} ({why})"),
                    ));
                    continue;
                }
            };
            let then = match (base_copy(ctx, f), ctx.base.as_deref()) {
                (Some(_), Some(base)) => match scan(&bin, &config, base, f) {
                    Ok(v) => v,
                    Err(why) => {
                        r.notes.push(note(
                            ctx,
                            NoteKind::Environment,
                            format!("osv-scanner could not scan the base copy of {f} ({why})"),
                        ));
                        continue;
                    }
                },
                _ => BTreeSet::new(),
            };
            let old: BTreeSet<(&str, &str, &str)> = then
                .iter()
                .map(|(p, v, i, _)| (p.as_str(), v.as_str(), i.as_str()))
                .collect();
            for (p, v, id, fixed) in &now {
                if old.contains(&(p.as_str(), v.as_str(), id.as_str())) {
                    continue;
                }
                let msg = if fixed.is_empty() {
                    format!(
                        "{p}@{v} has a known vulnerability, {id}, and no release fixes it yet; use another package or version"
                    )
                } else {
                    format!(
                        "{p}@{v} has a known vulnerability, {id} (fixed in {fixed}); use a fixed version"
                    )
                };
                r.findings
                    .push(Finding::error(&ctx.rule, msg).at(f.as_str(), Some(0)));
            }
        }
        r
    }
}

/// The vulnerabilities osv-scanner reports on `lock` (relative to `dir`).
fn scan(bin: &Path, config: &Path, dir: &Path, lock: &str) -> Result<BTreeSet<Vuln>, String> {
    let name = lock.rsplit('/').next().unwrap_or(lock);
    let arg = if name.starts_with("requirements") && name.ends_with(".txt") {
        format!("requirements.txt:{lock}")
    } else {
        lock.to_string()
    };
    let out = Command::new(bin)
        .args(["scan", "source", "--format", "markdown", "--config"])
        .arg(config)
        .args(["-L", &arg])
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .output()
        .map_err(|e| e.to_string())?;
    // 0 no vulnerability, 1 vulnerabilities, 128 no package in the lock file; 127 any other
    // error (unreadable lock file, api.osv.dev unreachable)
    match out.status.code() {
        Some(0 | 1 | 128) => {}
        _ => {
            let err = String::from_utf8_lossy(&out.stderr);
            let why = err
                .lines()
                .rfind(|l| !l.trim().is_empty())
                .map(|l| l.trim().to_string())
                .unwrap_or_else(|| last_line(&out));
            return Err(why);
        }
    }
    Ok(parse_markdown(&String::from_utf8_lossy(&out.stdout)))
}

/// `| https://osv.dev/<id>[<br/>…] | CVSS | Ecosystem | Package | Version | Fixed Version | Source |`
fn parse_markdown(text: &str) -> BTreeSet<Vuln> {
    let mut out = BTreeSet::new();
    for l in text.lines() {
        let c: Vec<&str> = l.split('|').collect();
        if c.len() < 8 {
            continue;
        }
        let Some(id) = c[1].trim().strip_prefix("https://osv.dev/") else {
            continue;
        };
        let id = id.split("<br/>").next().unwrap_or(id).trim();
        let fixed = match c[6].trim() {
            "--" => "",
            f => f,
        };
        out.insert((
            c[4].trim().to_string(),
            c[5].trim().to_string(),
            id.to_string(),
            fixed.to_string(),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::parse_markdown;

    #[test]
    fn reads_the_markdown_table() {
        let t = "| OSV URL | CVSS | Ecosystem | Package | Version | Fixed Version | Source |\n| --- | --- | --- | --- | --- | --- | --- |\n| https://osv.dev/GHSA-35jh-r3h4-6jhm<br/>https://osv.dev/CVE-2021-23337 | 7.2 | npm | lodash | 4.17.20 | 4.17.21 | package-lock.json |\n| https://osv.dev/GHSA-x | | npm | lodash | 4.17.20 | -- | package-lock.json |\n";
        let v: Vec<_> = parse_markdown(t).into_iter().collect();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].2, "GHSA-35jh-r3h4-6jhm");
        assert_eq!(v[0].3, "4.17.21");
        assert_eq!(v[1].3, "");
    }
}
