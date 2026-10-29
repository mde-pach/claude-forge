//! JavaScript packages: the release cooldown, install scripts, lock file sources, major versions
//! of related packages (the original `javascript.sh`).

use std::process::Command;
use std::sync::Arc;

use forge_core::builtin::BuiltinCtx;
use forge_core::{Finding, NoteKind, Report};
use serde_json::Value;

use crate::ctx::{Extra, base_copy, existing, read};
use crate::util::{last_line, note};
use crate::{CodeBuiltin, Options};

fn is_package_json(p: &str) -> bool {
    p == "package.json" || p.ends_with("/package.json")
}

/// `forge_bun_cooldown`: bunfig.toml `[install] minimumReleaseAge` is at least the cooldown.
pub struct BunCooldown(pub Arc<Options>);

impl CodeBuiltin for BunCooldown {
    fn name(&self) -> &'static str {
        "bun_cooldown"
    }

    fn run_with(&self, ctx: &BuiltinCtx, _x: &Extra, _args: &[String]) -> Report {
        let want = self.0.cooldown_seconds;
        let value = read(&ctx.root, "bunfig.toml").and_then(|t| install_value(&t));
        let mut r = Report::default();
        if let Some(Value::Number(n)) = &value
            && n.as_u64().is_some_and(|v| v >= want)
        {
            return r;
        }
        let shown = match &value {
            None => "unset".to_string(),
            Some(Value::String(s)) => format!("\"{s}\""),
            Some(v) => v.to_string(),
        };
        r.findings.push(Finding::error(
            &ctx.rule,
            format!(
                "bunfig.toml: [install] minimumReleaseAge is {shown}; set it to at least {want} ({}, in seconds) so a freshly published (possibly compromised) version is not installed. Packages you must update sooner go in minimumReleaseAgeExcludes.",
                duration(want)
            ),
        ));
        r
    }
}

/// `[install] minimumReleaseAge` of a bunfig.toml (None: absent or unreadable).
fn install_value(text: &str) -> Option<Value> {
    let t: toml::Table = text.parse().ok()?;
    let v = t.get("install")?.get("minimumReleaseAge")?;
    serde_json::to_value(v).ok()
}

/// "3 days", "36 hours", "90 seconds".
pub fn duration(secs: u64) -> String {
    let plural = |n: u64, w: &str| format!("{n} {w}{}", if n == 1 { "" } else { "s" });
    if secs % 86_400 == 0 {
        plural(secs / 86_400, "day")
    } else if secs % 3600 == 0 {
        plural(secs / 3600, "hour")
    } else {
        plural(secs, "second")
    }
}

/// `forge_trusted_deps`: no package.json of the change gains trustedDependencies entries.
pub struct TrustedDeps;

fn trusted(text: Option<String>) -> Vec<String> {
    text.and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.get("trustedDependencies").cloned())
        .and_then(|v| match v {
            Value::Array(a) => Some(
                a.iter()
                    .map(|x| match x {
                        Value::String(s) => s.clone(),
                        o => o.to_string(),
                    })
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

impl CodeBuiltin for TrustedDeps {
    fn name(&self) -> &'static str {
        "trusted_deps"
    }

    fn run_with(&self, ctx: &BuiltinCtx, _x: &Extra, _args: &[String]) -> Report {
        let mut r = Report::default();
        for f in existing(ctx).iter().filter(|f| is_package_json(f)) {
            let before = trusted(base_copy(ctx, f).and_then(|p| std::fs::read_to_string(p).ok()));
            let new: Vec<String> = trusted(read(&ctx.root, f))
                .into_iter()
                .filter(|d| !before.contains(d))
                .collect();
            if !new.is_empty() {
                r.findings.push(Finding::error(
                    &ctx.rule,
                    format!(
                        "{f}: trustedDependencies adds {} (its install scripts would run); remove {}: only the user approves install scripts",
                        new.join(" "),
                        if new.len() == 1 { "it" } else { "them" }
                    ),
                ));
            }
        }
        r
    }
}

/// `forge_lockfile_lint`: the pinned lockfile-lint on each in-scope package-lock.json: every
/// package resolves from registry.npmjs.org, over https, with a sha512 integrity.
pub struct LockfileLint(pub Arc<Options>);

impl CodeBuiltin for LockfileLint {
    fn name(&self) -> &'static str {
        "lockfile_lint"
    }

    fn run_with(&self, ctx: &BuiltinCtx, x: &Extra, _args: &[String]) -> Report {
        let mut r = Report::default();
        let locks: Vec<String> = x
            .scope_files(ctx)
            .into_iter()
            .filter(|f| {
                (f == "package-lock.json" || f.ends_with("/package-lock.json"))
                    && ctx.root.join(f).is_file()
            })
            .collect();
        if locks.is_empty() {
            return r;
        }
        let bin = match self.0.tools.pinned("lockfile-lint") {
            Ok(b) => b,
            Err(n) => {
                r.notes.push(n.for_rule(ctx.rule.clone()));
                return r;
            }
        };
        for f in locks {
            let out = Command::new(&bin)
                .args([
                    "--type",
                    "npm",
                    "--allowed-hosts",
                    "npm",
                    "--validate-https",
                    "--validate-integrity",
                    "--path",
                    &f,
                ])
                .current_dir(&ctx.root)
                .env("NO_COLOR", "1")
                .output();
            let out = match out {
                Ok(o) => o,
                Err(e) => {
                    r.notes.push(note(
                        ctx,
                        NoteKind::ForgeFailure,
                        format!("lockfile-lint could not run ({e})"),
                    ));
                    return r;
                }
            };
            if out.status.success() {
                continue;
            }
            let text = format!(
                "{}\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            let found = lockfile_lint_findings(&text);
            if found.is_empty() {
                r.notes.push(note(
                    ctx,
                    NoteKind::ForgeFailure,
                    format!("lockfile-lint could not run ({})", last_line(&out)),
                ));
                return r;
            }
            for (pkg, what, want, got) in found {
                r.findings.push(
                    Finding::error(
                        &ctx.rule,
                        format!("{pkg}: {what} (expected {want}, actual {got})"),
                    )
                    .at(&f, Some(0)),
                );
            }
        }
        r
    }
}

/// lockfile-lint's "detected … for package: <pkg>" blocks: package, problem, expected, actual.
fn lockfile_lint_findings(text: &str) -> Vec<(String, String, String, String)> {
    let ansi = regex::Regex::new(r"\x1b\[[0-9;]*[A-Za-z]").expect("regex");
    let hash = regex::Regex::new(r"-[0-9a-f]{40}$").expect("regex");
    let text = ansi.replace_all(text, "");
    let mut out = vec![];
    let (mut pkg, mut what, mut want) = (String::new(), String::new(), String::new());
    for l in text.lines() {
        if let Some(rest) = l.strip_prefix("detected ") {
            let (w, p) = rest.split_once(" for package: ").unwrap_or((rest, rest));
            what = w.to_string();
            pkg = hash.replace(p, "").into_owned();
        } else if let Some(v) = l.trim_start().strip_prefix("expected: ") {
            want = v.to_string();
        } else if let Some(v) = l.trim_start().strip_prefix("actual: ") {
            out.push((pkg.clone(), what.clone(), want.clone(), v.to_string()));
        }
    }
    out
}

/// The entries of dependencies, devDependencies and optionalDependencies: section, name, spec.
pub fn package_deps(text: &str) -> Vec<(String, String, String)> {
    let Ok(Value::Object(m)) = serde_json::from_str::<Value>(text) else {
        return vec![];
    };
    let mut out = vec![];
    for s in ["dependencies", "devDependencies", "optionalDependencies"] {
        if let Some(Value::Object(d)) = m.get(s) {
            for (k, v) in d {
                if let Value::String(spec) = v {
                    out.push((s.to_string(), k.clone(), spec.clone()));
                }
            }
        }
    }
    out.sort();
    out
}

/// The lowest major version a range allows (the smallest major its `||` alternatives start
/// at); None when it names no lower version (`*`, `latest`, `workspace:`, a git source, `<5`).
pub fn lowest_major(spec: &str) -> Option<u64> {
    let re = regex::Regex::new(r"^(?:\^|~|>=|=|v)?[ \t]*v?([0-9]+)(?:[.]|[ \t]|$)").expect("regex");
    spec.split("||")
        .map(|a| re.captures(a.trim()).and_then(|c| c[1].parse::<u64>().ok()))
        .try_fold(None::<u64>, |best, m| {
            let m = m?;
            Some(Some(best.map_or(m, |b| b.min(m))))
        })
        .flatten()
}

/// `forge_dep_major_match <package> <types-package>...`: in each in-scope package.json declaring
/// both, the lowest majors their ranges allow are the same.
pub struct DepMajorMatch;

impl CodeBuiltin for DepMajorMatch {
    fn name(&self) -> &'static str {
        "dep_major_match"
    }

    fn run_with(&self, ctx: &BuiltinCtx, x: &Extra, args: &[String]) -> Report {
        let mut r = Report::default();
        let Some((pkg, types)) = args.split_first() else {
            return r;
        };
        for f in x.scope_files(ctx).iter().filter(|f| is_package_json(f)) {
            let Some(text) = read(&ctx.root, f) else {
                continue;
            };
            let deps = package_deps(&text);
            let spec = |name: &str| {
                deps.iter()
                    .rfind(|(_, n, _)| n == name)
                    .map(|(_, _, s)| s.clone())
            };
            let Some(have) = spec(pkg) else { continue };
            let pm = lowest_major(&have);
            for t in types {
                let Some(tspec) = spec(t) else { continue };
                let (Some(pm), Some(tm)) = (pm, lowest_major(&tspec)) else {
                    r.notes.push(note(
                        ctx,
                        NoteKind::NotEnforced,
                        format!(
                            "{f}: {pkg} \"{have}\" and {t} \"{tspec}\" are not both version ranges, so their majors were not compared"
                        ),
                    ));
                    continue;
                };
                if pm != tm {
                    r.findings.push(
                        Finding::error(
                            &ctx.rule,
                            format!(
                                "{t} \"{tspec}\" (major {tm}) describes another API than {pkg} \"{have}\" (major {pm}): declare {t}@^{pm}.0.0 (or the {pkg} release you run)"
                            ),
                        )
                        .at(f.as_str(), Some(0)),
                    );
                }
            }
        }
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowest_majors() {
        for (s, want) in [
            ("^4.17.1", Some(4)),
            ("~4.1", Some(4)),
            ("4.x", Some(4)),
            (">=4 <6", Some(4)),
            ("5 || 4", Some(4)),
            ("5", Some(5)),
            ("v5.1.0", Some(5)),
            ("*", None),
            ("latest", None),
            ("workspace:*", None),
            ("<5", None),
            ("4 || latest", None),
        ] {
            assert_eq!(lowest_major(s), want, "{s}");
        }
    }

    #[test]
    fn durations_and_lockfile_lint_output() {
        assert_eq!(duration(259_200), "3 days");
        assert_eq!(duration(3600), "1 hour");
        let out = "detected invalid host(s) for package: left-pad@1.3.0-0123456789abcdef0123456789abcdef01234567\n    expected: registry.npmjs.org\n    actual: evil.example.com\n\n \u{1b}[31merror\u{1b}[0m";
        assert_eq!(
            lockfile_lint_findings(out),
            vec![(
                "left-pad@1.3.0".to_string(),
                "invalid host(s)".to_string(),
                "registry.npmjs.org".to_string(),
                "evil.example.com".to_string()
            )]
        );
    }
}
