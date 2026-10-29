//! `forge status` and `forge explain`.

use forge_detect::list_repo_files;
use forge_packs::glob::glob_to_regex;
use forge_packs::model::{Level, Origin, PackSource};
use forge_render::changes::role_summary;
use forge_render::report::{
    ToolProbe, audit_rules, enforcement, enforcement_summary, format_audit,
};
use serde::Serialize;
use serde_json::json;

use super::io::Error;
use super::{
    Ctx, FORGE_VERSION, Flags, base_of, context, load_packs, person_notes, pin_differences,
    precommit,
};

#[derive(Debug, Serialize)]
struct RoleFiles {
    name: String,
    summary: String,
    patterns: Vec<String>,
    sources: Vec<String>,
    matching: usize,
    notes: Vec<String>,
    rules: Vec<String>,
}

fn role_files(ctx: &Ctx) -> Vec<RoleFiles> {
    if ctx.level != Level::Project || ctx.plan.roles.is_empty() {
        return Vec::new();
    }
    let files = list_repo_files(&ctx.base);
    ctx.plan
        .roles
        .iter()
        .map(|r| {
            let inc: Vec<_> = r
                .patterns
                .iter()
                .filter(|p| !p.exclude)
                .map(|p| glob_to_regex(&p.glob))
                .collect();
            let exc: Vec<_> = r
                .patterns
                .iter()
                .filter(|p| p.exclude)
                .map(|p| glob_to_regex(&p.glob))
                .collect();
            let matching = files
                .iter()
                .filter(|f| {
                    inc.iter().any(|re| re.is_match(f).unwrap_or(false))
                        && !exc.iter().any(|re| re.is_match(f).unwrap_or(false))
                })
                .count();
            let mut sources: Vec<String> = Vec::new();
            for p in &r.patterns {
                if !sources.contains(&p.source) {
                    sources.push(p.source.clone());
                }
            }
            RoleFiles {
                name: r.name.clone(),
                summary: role_summary(r),
                patterns: ctx.plan.role_globs[&r.name].clone(),
                sources,
                matching,
                notes: r.notes.clone(),
                rules: r.rules.clone(),
            }
        })
        .collect()
}

/// The remote's default branch (origin/HEAD), else "main".
fn default_branch(ctx: &Ctx) -> String {
    std::process::Command::new("git")
        .args([
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ])
        .current_dir(&ctx.base)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .strip_prefix("origin/")
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "main".into())
}

pub fn status(flags: &Flags, json_out: bool, probe: &dyn ToolProbe) -> Result<u8, Error> {
    let ctx = context(flags)?;
    let p = &ctx.plan;
    let audits = if ctx.level == Level::Project {
        audit_rules(p, probe)
    } else {
        Vec::new()
    };
    let missing = probe.missing_tools(p);
    let rules = enforcement(p, &audits, &missing);
    let versions = probe.tool_versions(p);
    let ch = ctx.changes()?;
    let notes = person_notes(&ctx, probe);
    let mut drift: Vec<String> = Vec::new();
    drift.extend(
        ch.drift
            .iter()
            .map(|d| format!("{d} modified since the last apply")),
    );
    drift.extend(
        ch.conflicts
            .iter()
            .map(|c| format!("{c} is not a forge file")),
    );
    drift.extend(
        ch.rules_changed
            .iter()
            .map(|id| format!("rule {id} changed since the last apply")),
    );
    drift.extend(ch.roles_changed.iter().cloned());
    for e in &ch.missing_entries {
        drift.push(format!("{}: missing {} {}", e.file, e.path, e.value));
    }
    for f in &ch.block_drift {
        drift.push(format!(
            "{f}: forge's pre-commit block modified since the last apply"
        ));
    }
    for b in &ch.blocks {
        drift.push(format!(
            "{}: forge's pre-commit block is missing or out of date (apply updates it)",
            b.path
        ));
    }
    drift.extend(ch.overridden.iter().cloned());
    if ctx.migration.is_some() {
        drift.push(
            "the repository has forge 0.5's layout (forge apply moves it to the new one)".into(),
        );
    }
    if ctx.lock.is_some() && (!ch.writes.is_empty() || !ch.deletes.is_empty()) {
        drift.push(format!(
            "{} file(s) differ from what apply would write (forge diff)",
            ch.writes.len() + ch.deletes.len()
        ));
    }
    let pre = (ctx.level == Level::Project).then(|| precommit::state(&ctx));
    let pins = pin_differences(&ctx);
    if json_out {
        let enforced = rules.iter().filter(|r| r.enforced).count();
        let v = json!({
            "forge_version": FORGE_VERSION,
            "level": ctx.level.as_str(),
            "base": ctx.base,
            "applied": ctx.lock.as_ref().map(|l| l.forge.clone()),
            "packs": p.packs,
            "inactive_packs": p.removed_packs,
            "counts": {"active": rules.len(), "enforced": enforced, "not_enforced": rules.len() - enforced},
            "rules": rules,
            "disabled": p.disabled,
            "gated": p.gated,
            "switched_off": p.switched_off,
            "deprecated": p.deprecated,
            "audits": audits,
            "missing_tools": missing,
            "tool_versions": versions,
            "tool_pins": pins.iter().map(|(t, l, o)| json!({"tool": t, "locked": l, "forge": o})).collect::<Vec<_>>(),
            "pre_commit": pre,
            "drift": drift,
            "files": role_files(&ctx),
            "notes": notes,
            "warnings": p.warnings,
        });
        println!("{}", serde_json::to_string_pretty(&v).expect("JSON"));
        return Ok(0);
    }
    println!(
        "forge {FORGE_VERSION}: {} level, {}",
        ctx.level,
        ctx.base.display()
    );
    match &ctx.lock {
        Some(l) => println!("applied with forge {}", l.forge),
        None => println!("not applied yet (run forge apply)"),
    }
    println!();
    println!("Packs:");
    for pk in &p.packs {
        println!("  {} ({})", pk.id, pk.why);
    }
    for pk in &p.removed_packs {
        println!("  {}: not active ({})", pk.id, pk.why);
    }
    for w in &p.warnings {
        println!("forge: {w}");
    }
    println!();
    println!("Rules: {}", enforcement_summary(&rules));
    for st in &p.active {
        let local = if st.rule.origin == Origin::Local {
            " (local)"
        } else {
            ""
        };
        println!("  {}{local}", st.rule.id);
        for e in &st.enforced {
            println!("    {e}");
        }
        for n in &st.not_enforced {
            println!("    {n}");
        }
    }
    if !p.disabled.is_empty() {
        println!("\nDisabled:");
        for d in &p.disabled {
            println!("  {}: {}", d.id, d.reason);
        }
    }
    if !p.gated.is_empty() {
        println!("\nInactive (version gate):");
        for g in &p.gated {
            println!("  {}: needs {}", g.id, g.needs);
        }
    }
    if !p.switched_off.is_empty() {
        println!("\nSwitched off by packs:");
        for o in &p.switched_off {
            let by = if o.rules.is_empty() {
                String::new()
            } else {
                format!("; not asked for by {}", o.rules.join(", "))
            };
            println!("  {} {} ({}): {}{by}", o.tool, o.expects, o.pack, o.reason);
        }
    }
    for d in &p.deprecated {
        println!("  {}: deprecated, replaced by {}", d.id, d.replaced_by);
    }
    let roles = role_files(&ctx);
    if !roles.is_empty() {
        println!("\nFiles:");
        for r in roles {
            let none = if r.matching == 0 {
                "; matches no file here"
            } else {
                ""
            };
            println!("  @{}: {}{none}", r.name, r.summary);
            for n in r.notes {
                println!("    {n}");
            }
        }
    }
    if !audits.is_empty() {
        println!("\nAudit:");
        for a in &audits {
            println!("  {}", format_audit(a).replace('\n', "\n  "));
        }
    }
    println!();
    if drift.is_empty() {
        println!("Drift: none");
    } else {
        println!("Drift:");
        for d in &drift {
            println!("  {d}");
        }
    }
    if missing.is_empty() {
        println!("Missing tools: none");
    } else {
        println!("Missing tools:");
        for m in &missing {
            println!("  {m}");
        }
    }
    if ctx.level == Level::Project {
        if versions.is_empty() {
            println!("Tool versions: nothing reported");
        } else {
            println!("Tool versions:");
            for v in &versions {
                println!("  {v}");
            }
        }
        if ctx.lock.is_some() {
            if pins.is_empty() {
                println!("Tool pins: as this Forge pins them");
            } else {
                println!("Tool pins (forge.lock, then this Forge's; forge apply rewrites them):");
                for (t, locked, ours) in &pins {
                    let side = |v: &str| {
                        if v.is_empty() {
                            "not pinned".to_string()
                        } else {
                            v.to_string()
                        }
                    };
                    println!("  {t}: {} -> {}", side(locked), side(ours));
                }
            }
        }
        println!();
        let pre = pre.expect("project");
        match (&pre.file, pre.installed) {
            (Some(f), true) => {
                println!("Pre-commit: {f}{}", if pre.forge { " (forge)" } else { "" })
            }
            _ => println!("Pre-commit: {}", precommit::not_checked(&ctx, &pre)),
        }
        println!(
            "CI step, pull requests: claude-forge check --since origin/{}",
            default_branch(&ctx)
        );
        println!("CI step, the default branch: claude-forge check --all");
    }
    if !notes.is_empty() {
        println!("\nNotes:");
        for n in &notes {
            println!("  {n}");
        }
    }
    Ok(0)
}

pub fn explain(id: &str, flags: &Flags, probe: &dyn ToolProbe) -> Result<u8, Error> {
    let level = if flags.user {
        Level::User
    } else {
        Level::Project
    };
    let base = base_of(level).ok();
    let set = match &base {
        Some(b) => load_packs(level, b)?,
        None => load_packs(Level::User, std::path::Path::new("."))?,
    };
    let rule = set
        .local_rules
        .iter()
        .find(|r| r.id == id)
        .or_else(|| {
            set.packs
                .values()
                .flat_map(|p| &p.rules)
                .find(|r| r.id == id)
        })
        .ok_or_else(|| Error::new(format!("forge: unknown rule {id}")))?
        .clone();
    let pack_source = set.packs.get(&rule.pack).map(|p| p.source);
    let text = match (rule.origin, pack_source) {
        (Origin::Pack, Some(PackSource::Builtin)) => {
            forge_packs::builtin_file(&rule.file).map(|b| String::from_utf8_lossy(b).into_owned())
        }
        _ => base
            .as_ref()
            .and_then(|b| std::fs::read_to_string(b.join(&rule.file)).ok()),
    }
    .unwrap_or_default();
    println!("# {}", rule.file);
    print!("{text}");
    println!();
    if base.is_none() {
        println!("Not in a git repository: no project-specific enforcement to show.");
        return Ok(0);
    }
    let ctx = context(flags)?;
    let p = &ctx.plan;
    if let Some(d) = p.disabled.iter().find(|d| d.id == id) {
        println!("Disabled here: {}", d.reason);
    } else if let Some(g) = p.gated.iter().find(|g| g.id == id) {
        println!("Not active here: the rule needs {}.", g.needs);
    } else if let Some(st) = p.active.iter().find(|s| s.rule.id == id) {
        println!("Enforced here:");
        for e in &st.enforced {
            println!("  {e}");
        }
        for n in &st.not_enforced {
            println!("  {n}");
        }
        if st.enforced.is_empty() {
            println!("  nowhere");
        }
        if ctx.level == Level::Project {
            for a in audit_rules(p, probe).iter().filter(|a| a.rule == id) {
                println!("{}", format_audit(a));
            }
        }
    } else {
        println!(
            "Not active here: pack {} is not active at {} level.",
            rule.pack, ctx.level
        );
    }
    Ok(0)
}
