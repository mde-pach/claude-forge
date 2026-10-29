//! The checker's machinery: running tools (`forge_do`, `forge_exec_tool`, the pinned ones),
//! advice once per turn (`forge_once`, `forge_advise`), engine helpers and ratchets, file roles,
//! and the small readers commands use (`forge_toml_get`, `forge_pkg_deps`, `forge_db_url`…).

use std::io::Write;
use std::process::{Command, Stdio};

use forge_core::NoteKind;

use super::{Invocation, Registry, finish};
use crate::adapters::{Adapter, DoCtx, picked, run_do};
use crate::tools::ToolCmd;

/// What the agent reads after a finding it may restore or have accepted (`forge_accept_hint`).
pub const ACCEPT_HINT: &str = "Restore it, or ask the user to accept this change.";
/// The same after a loosened setting (`forge_ratchet`).
pub const RATCHET_HINT: &str = "Restore the setting, or ask the user to accept this change.";

pub(super) fn register(r: &mut Registry) {
    r.register_raw("do", forge_do);
    r.register_raw("exec_tool", exec_tool);
    r.register_raw("ty", ty);
    r.register_raw("terraform", terraform);
    r.register_raw("kustomize", kustomize);
    r.register_raw("pinned", pinned);
    r.register_raw("helper", helper);
    r.register_raw("ratchet", ratchet);
    r.register_raw("accept_hint", accept_hint);
    r.register_raw("once", once);
    r.register_raw("advise", advise);
    r.register_raw("role", role);
    r.register_raw("role_files", role_files);
    r.register_raw("state_file", state_file);
    r.register_raw("scope_files", scope_files);
    r.register_raw("toml_get", toml_get);
    r.register_raw("pkg_deps", pkg_deps);
    r.register_raw("db_url", db_url);
    r.register_raw("local_db", local_db);
}

fn exec(cmd: &ToolCmd, args: &[String], cwd: &std::path::Path) -> i32 {
    let st = Command::new(&cmd.argv[0])
        .args(&cmd.argv[1..])
        .args(args)
        .current_dir(cwd)
        .status();
    match st {
        Ok(s) => s.code().unwrap_or(128 + 9),
        Err(_) => 127,
    }
}

/// `forge_do <tool> <op> [args…]` (§16).
fn forge_do(inv: &Invocation<'_>, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let (Some(tool), Some(op)) = (inv.args.first(), inv.args.get(1)) else {
        let _ = writeln!(err, "usage: forge_do <tool> <operation> [arguments]");
        return 2;
    };
    let ctx = DoCtx {
        root: inv.root(),
        cwd: &inv.cwd,
        rule: inv.rule(),
        tools: inv.tools.as_ref(),
        stdin: None,
    };
    let outcome = run_do(tool, op, &inv.args[2..], &ctx, out, err);
    inv.ctx.write_notes(&outcome.notes);
    outcome.status
}

/// `forge_exec_tool <name> [args…]`: the project's own tool; 127 when absent.
fn exec_tool(inv: &Invocation<'_>, _out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some(name) = inv.args.first() else {
        return 2;
    };
    match inv.tools.project(inv.root(), &inv.cwd, name) {
        Some(cmd) => exec(&cmd, &inv.args[1..], &inv.cwd),
        None => {
            let _ = writeln!(err, "forge: {name} not installed");
            127
        }
    }
}

/// Runs Forge's pinned `tool`; 127, and a note, when it cannot be had.
fn run_pinned(inv: &Invocation<'_>, tool: &str, args: &[String]) -> i32 {
    let version = Adapter::load(tool)
        .and_then(|a| a.pinned)
        .unwrap_or_default();
    match inv.tools.pinned(tool, &version) {
        Ok(cmd) => exec(&cmd, args, &inv.cwd),
        Err(why) => {
            inv.note(NoteKind::MissingTool, format!("skipped: {why}"));
            127
        }
    }
}

/// `forge_ty [args…]`: Forge's pinned ty (§15.2).
fn ty(inv: &Invocation<'_>, _out: &mut dyn Write, _err: &mut dyn Write) -> i32 {
    run_pinned(inv, "ty", &inv.args)
}

/// `forge_pinned <tool> [args…]`.
fn pinned(inv: &Invocation<'_>, _out: &mut dyn Write, _err: &mut dyn Write) -> i32 {
    let Some(tool) = inv.args.first() else {
        return 2;
    };
    run_pinned(inv, tool, &inv.args[1..])
}

/// `forge_terraform [args…]`: the project's Terraform CLI (tofu in an OpenTofu project) from PATH.
fn terraform(inv: &Invocation<'_>, _out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let bin = picked("terraform", inv.root(), inv.tools.as_ref());
    match inv.tools.on_path(&bin) {
        Some(p) => exec(&ToolCmd::exe(p), &inv.args, &inv.cwd),
        None => {
            let _ = writeln!(err, "forge: {bin} not installed");
            127
        }
    }
}

/// `forge_kustomize <dir>`: the kustomization's build (kustomize, else `kubectl kustomize`).
fn kustomize(inv: &Invocation<'_>, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut args = vec!["kustomize".to_string(), "build".to_string()];
    args.extend(inv.args.iter().cloned());
    forge_do(&inv.with_args(args), out, err)
}

fn run_helper(inv: &Invocation<'_>, out: &mut dyn Write, hint: Option<&str>) -> i32 {
    let Some(script) = inv.args.get(1) else {
        return 2;
    };
    let Some(h) = inv.registry.helper(script) else {
        inv.note(
            NoteKind::NotEnforced,
            format!("not enforced: the engine helper {script} is not available yet in this Forge"),
        );
        return 0;
    };
    // the script's name first: it picks the Python or the JavaScript half of a helper
    let mut report = h.run(&inv.ctx, &inv.args[1..]);
    if hint.is_some() {
        // the ratchet's own hint gives way to the one said after its findings
        report
            .findings
            .retain(|f| !(f.path.is_none() && f.message == ACCEPT_HINT));
    }
    let mut buf = Vec::new();
    let code = finish(inv, report, &mut buf);
    let _ = out.write_all(&buf);
    if code == 1
        && let Some(hint) = hint
        && !buf.is_empty()
    {
        let _ = writeln!(out, "{hint}");
    }
    code
}

/// `forge_helper <runtime> <script> [args…]`: an engine helper (a Rust port of
/// `.forge/helpers/<script>`; the runtime is no longer needed).
fn helper(inv: &Invocation<'_>, out: &mut dyn Write, _err: &mut dyn Write) -> i32 {
    run_helper(inv, out, None)
}

/// `forge_ratchet <runtime> <script>`: the config ratchet (§10.4); each finding is a loosened
/// setting, unless the user accepted it.
fn ratchet(inv: &Invocation<'_>, out: &mut dyn Write, _err: &mut dyn Write) -> i32 {
    run_helper(inv, out, Some(RATCHET_HINT))
}

/// `forge_accept_hint`: after a finding the user may accept ("… || forge_accept_hint").
fn accept_hint(_inv: &Invocation<'_>, out: &mut dyn Write, _err: &mut dyn Write) -> i32 {
    let _ = writeln!(out, "{ACCEPT_HINT}");
    1
}

/// Runs a command given as words: a builtin in this process, anything else with the shell.
fn run_words(inv: &Invocation<'_>, words: &[String]) -> (i32, String) {
    let Some(first) = words.first() else {
        return (0, String::new());
    };
    if let Some(name) = first.strip_prefix("forge_")
        && (inv.registry.has(name) || crate::shell::BUILTIN_NAMES.contains(&name))
    {
        let sub = inv.with_args(words[1..].to_vec());
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = inv.registry.dispatch(name, &sub, &mut out, &mut err);
        return (code, String::from_utf8_lossy(&out).into_owned());
    }
    let script = format!(
        "{}\n{}",
        crate::shell::preamble(inv.rule(), &[]),
        words
            .iter()
            .map(|w| format!("'{}'", w.replace('\'', "'\\''")))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let out = Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .current_dir(&inv.cwd)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    match out {
        Ok(o) => (
            o.status.code().unwrap_or(1),
            String::from_utf8_lossy(&o.stdout).into_owned(),
        ),
        Err(_) => (127, String::new()),
    }
}

/// `forge_once <command…>` (§10.7): each line the command prints is a finding. At Stop it is
/// advice reported once per finding and turn; elsewhere any output fails the check.
fn once(inv: &Invocation<'_>, out: &mut dyn Write, _err: &mut dyn Write) -> i32 {
    let (_, text) = run_words(inv, &inv.args);
    let mut lines = Vec::new();
    for l in text.lines() {
        // a Forge line (a note in the old protocol) is the person's, not a finding
        if l.starts_with("forge:") {
            inv.note(
                NoteKind::NotEnforced,
                l.split_once(' ').map_or(l, |x| x.1).to_string(),
            );
        } else {
            lines.push(l.to_string());
        }
    }
    if lines.iter().all(|l| l.trim().is_empty()) {
        return 0;
    }
    let Some(snap) = inv.snapshot.as_ref().filter(|_| inv.turn()) else {
        let _ = writeln!(out, "{}", lines.join("\n"));
        return 1;
    };
    let seen_file = snap.join("reported");
    let seen = std::fs::read_to_string(&seen_file).unwrap_or_default();
    let rule = inv.rule();
    let new: Vec<&String> = lines
        .iter()
        .filter(|l| !l.is_empty() && !seen.lines().any(|s| *s == format!("{rule}\t{l}")))
        .collect();
    if new.is_empty() {
        return 0;
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&seen_file)
    {
        for l in &new {
            let _ = writeln!(f, "{rule}\t{l}");
        }
    }
    inv.mark_advised();
    let _ = writeln!(out, "{}", lines.join("\n"));
    1
}

/// `forge_advise <command…>`: `forge_once` at Stop only; pre-commit and CI never run advice.
fn advise(inv: &Invocation<'_>, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if !inv.turn() {
        return 0;
    }
    once(inv, out, err)
}

/// `forge_role <@role> <path>`: whether the path is one of the role's files.
fn role(inv: &Invocation<'_>, _out: &mut dyn Write, _err: &mut dyn Write) -> i32 {
    match (inv.args.first(), inv.args.get(1)) {
        (Some(r), Some(p)) if inv.roles.matches(r, p) => 0,
        _ => 1,
    }
}

/// `forge_role_files <@role>`: the repository's files (tracked, and untracked but not ignored)
/// of a file role.
fn role_files(inv: &Invocation<'_>, out: &mut dyn Write, _err: &mut dyn Write) -> i32 {
    let Some(r) = inv.args.first() else { return 0 };
    let git = crate::change::Git::new(inv.root());
    let files = git
        .run(&["ls-files", "-z", "-co", "--exclude-standard"])
        .unwrap_or_default();
    for f in crate::change::split_z(&files) {
        if inv.roles.matches(r, &f) {
            let _ = writeln!(out, "{f}");
        }
    }
    0
}

/// `forge_state_file <name>`: an absolute path for a tool's cache file (tsc's build info), in
/// Forge's state for this repository, never in the project; one per workspace package.
fn state_file(inv: &Invocation<'_>, out: &mut dyn Write, _err: &mut dyn Write) -> i32 {
    let Some(name) = inv.args.first() else {
        return 2;
    };
    let root = inv
        .ws_root
        .clone()
        .unwrap_or_else(|| inv.root().to_path_buf());
    let pkg = inv
        .ws_root
        .as_ref()
        .and_then(|w| inv.cwd.strip_prefix(w).ok())
        .map(|p| p.to_string_lossy().replace('/', "-"))
        .filter(|p| !p.is_empty());
    let dir = forge_home::Home::from_env()
        .ok()
        .and_then(|h| {
            forge_home::RepoId::for_root(&root)
                .ok()
                .map(|id| h.repo_state_dir(&id).join("tool-state"))
        })
        .unwrap_or_else(|| std::env::temp_dir().join("forge-tool-state"));
    let _ = std::fs::create_dir_all(&dir);
    let file = match pkg {
        Some(p) => format!("{p}.{name}"),
        None => name.clone(),
    };
    let _ = write!(out, "{}", dir.join(file).display());
    0
}

/// `forge_scope_files`: the check's files, else every tracked file in its scope.
fn scope_files(inv: &Invocation<'_>, out: &mut dyn Write, _err: &mut dyn Write) -> i32 {
    for f in inv.scope_files() {
        let _ = writeln!(out, "{f}");
    }
    0
}

/// `forge_toml_get <file> <section> <key>`: a one-line value, quotes and comments removed.
fn toml_get(inv: &Invocation<'_>, out: &mut dyn Write, _err: &mut dyn Write) -> i32 {
    let (Some(file), Some(section), Some(key)) =
        (inv.args.first(), inv.args.get(1), inv.args.get(2))
    else {
        return 0;
    };
    let Ok(text) = std::fs::read_to_string(inv.cwd.join(file)) else {
        return 0;
    };
    if let Some(v) = toml_value(&text, section, key) {
        let _ = writeln!(out, "{v}");
    }
    0
}

pub(crate) fn toml_value(text: &str, want: &str, key: &str) -> Option<String> {
    let mut section = String::new();
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with('[') {
            let s = t.trim_start_matches('[').trim_start();
            section = s.split(']').next().unwrap_or("").trim_end().to_string();
            continue;
        }
        if section != want || !t.starts_with(key) {
            continue;
        }
        let rest = t[key.len()..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let rest = rest.trim_start();
        let v = if let Some(r) = rest.strip_prefix('"') {
            r.split('"').next().unwrap_or("").to_string()
        } else if let Some(r) = rest.strip_prefix('\'') {
            r.split('\'').next().unwrap_or("").to_string()
        } else {
            rest.split('#').next().unwrap_or("").trim_end().to_string()
        };
        return Some(v);
    }
    None
}

/// `forge_pkg_deps <package.json>`: `section<TAB>name<TAB>spec` for each dependency,
/// devDependency and optionalDependency, sorted.
fn pkg_deps(inv: &Invocation<'_>, out: &mut dyn Write, _err: &mut dyn Write) -> i32 {
    let Some(file) = inv.args.first() else {
        return 0;
    };
    for l in package_deps(&inv.cwd.join(file)) {
        let _ = writeln!(out, "{l}");
    }
    0
}

pub(crate) fn package_deps(file: &std::path::Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(file) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for s in ["dependencies", "devDependencies", "optionalDependencies"] {
        if let Some(m) = v.get(s).and_then(|x| x.as_object()) {
            for (name, spec) in m {
                out.push(format!("{s}\t{name}\t{}", spec.as_str().unwrap_or("")));
            }
        }
    }
    out.sort();
    out
}

pub fn database_url(cwd: &std::path::Path) -> Option<String> {
    if let Ok(u) = std::env::var("DATABASE_URL")
        && !u.is_empty()
    {
        return Some(u);
    }
    if let Ok(env) = std::fs::read_to_string(cwd.join(".env")) {
        let mut last = None;
        for l in env.lines() {
            let t = l.trim_start();
            let t = t.strip_prefix("export").map(str::trim_start).unwrap_or(t);
            if let Some(rest) = t.strip_prefix("DATABASE_URL") {
                let rest = rest.trim_start();
                if let Some(v) = rest.strip_prefix('=') {
                    let v = v.trim();
                    let v = v
                        .strip_prefix('"')
                        .and_then(|x| x.strip_suffix('"'))
                        .or_else(|| v.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')))
                        .unwrap_or(v);
                    last = Some(v.to_string());
                }
            }
        }
        if let Some(v) = last.filter(|v| !v.is_empty()) {
            return Some(v);
        }
    }
    let ini = std::fs::read_to_string(cwd.join("alembic.ini")).ok()?;
    ini.lines().find_map(|l| {
        let t = l.trim_start().strip_prefix("sqlalchemy.url")?.trim_start();
        Some(t.strip_prefix('=')?.trim().to_string())
    })
}

/// `forge_db_url`: DATABASE_URL from the environment, then .env, then alembic.ini.
fn db_url(inv: &Invocation<'_>, out: &mut dyn Write, _err: &mut dyn Write) -> i32 {
    if let Some(u) = database_url(&inv.cwd) {
        let _ = write!(out, "{u}");
    }
    0
}

/// Whether the configured database is on this machine (localhost, 127.0.0.1, ::1, an empty host
/// or a Unix socket).
pub fn is_local_db(cwd: &std::path::Path) -> bool {
    let Some(url) = database_url(cwd) else {
        return false;
    };
    if url.contains("host=/") || url.to_ascii_lowercase().contains("host=%2f") {
        return true;
    }
    let rest = match url.find("://") {
        Some(i) => &url[i + 3..],
        None => url.as_str(),
    };
    let rest = rest.split(['/', '?']).next().unwrap_or("");
    let host = rest.rsplit('@').next().unwrap_or("");
    let host = if let Some(h) = host.strip_prefix('[') {
        h.split(']').next().unwrap_or("").to_string()
    } else {
        host.split(':').next().unwrap_or("").to_string()
    };
    matches!(host.as_str(), "" | "localhost" | "127.0.0.1" | "::1")
        || host.to_ascii_lowercase().starts_with("%2f")
}

/// `forge_local_db`: status 0 when the database is local.
fn local_db(inv: &Invocation<'_>, _out: &mut dyn Write, _err: &mut dyn Write) -> i32 {
    if is_local_db(&inv.cwd) { 0 } else { 1 }
}

impl<'a> Invocation<'a> {
    /// The same context with other arguments.
    pub fn with_args(&self, args: Vec<String>) -> Invocation<'a> {
        Invocation {
            ctx: self.ctx.clone(),
            cx: self.cx.clone(),
            args,
            registry: self.registry,
            tools: self.tools.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_values() {
        let t = "[project]\nname = \"x\"\nrequires-python = \">=3.11\" # c\n[tool.x]\nk = 3 # n\n";
        assert_eq!(
            toml_value(t, "project", "requires-python").as_deref(),
            Some(">=3.11")
        );
        assert_eq!(toml_value(t, "tool.x", "k").as_deref(), Some("3"));
        assert_eq!(toml_value(t, "project", "nope"), None);
    }

    #[test]
    fn local_databases() {
        let d = tempfile::tempdir().unwrap();
        for (url, local) in [
            ("postgresql://u:p@localhost:5432/db", true),
            ("postgresql://u:p@db.example.com/db", false),
            ("postgresql:///db?host=/var/run/postgresql", true),
            ("mysql://[::1]:3306/x", true),
        ] {
            std::fs::write(d.path().join(".env"), format!("DATABASE_URL=\"{url}\"\n")).unwrap();
            // the process environment would win: these tests run without it
            if std::env::var_os("DATABASE_URL").is_none() {
                assert_eq!(is_local_db(d.path()), local, "{url}");
            }
        }
    }
}
