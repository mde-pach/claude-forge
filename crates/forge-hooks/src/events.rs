//! SessionStart, UserPromptSubmit, PostToolUse, the typecheck, Stop, Notification, SessionEnd.

use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

use forge_core::{Note, NoteKind, Report};
use forge_engine::{DiagFile, Lang};
use serde_json::json;

use crate::checker::{StopOutcome, StopStatus};
use crate::input::HookInput;
use crate::proc::{GIT_CAP, git};
use crate::snapshot::{self, BashSnapshots};
use crate::{Output, Project, Runtime, engine, state, testedit};

// ---- SessionStart

pub fn session_start(rt: &Runtime, input: &HookInput) -> Output {
    // the person must act on a pinned version that is not there (synchronous hook: shown to them)
    let system: Vec<String> = rt
        .launcher_notes
        .iter()
        .filter(|n| matches!(n, forge_launcher::Note::VersionUnavailable { .. }))
        .map(ToString::to_string)
        .collect();
    let Some(p) = rt.project(input) else {
        return with_system(Output::silent(), &system);
    };
    let context = session_context(&p.root);
    let cache = p.rules(&rt.home);
    let mut notes = Vec::new();
    // Forge's uv on the agent's PATH where the project uses uv (or no packaging at all)
    if let (Some(c), Some(file)) = (&cache, &rt.env.env_file) {
        if matches!(c.python_pm.as_deref(), Some("uv" | "none")) {
            if let Err(e) = forge_tools::append_path_export(file, &rt.home.path_dir()) {
                notes.push(Note::new(
                    NoteKind::Setup,
                    format!(
                        "Could not put Forge's uv on the session's PATH ({}: {e}).",
                        file.display()
                    ),
                ));
            }
        }
    }
    if let Some(c) = &cache {
        let starting = forge_engine::socket_addr(&rt.home.engine_socket(&p.id))
            .with_extension("sock.lock")
            .exists();
        if let Some(client) = engine::running(&rt.home, &p.id, Duration::from_millis(200)) {
            let _ = client.register(&p.session);
        } else if !c.typed.is_empty() && rt.env.engine && !starting {
            let r = forge_engine::spawn_detached(
                Command::new(&rt.exe)
                    .args(["engine", "start", "--root"])
                    .arg(&p.root)
                    .args(["--session", &p.session])
                    .current_dir(&p.root),
            );
            if let Err(e) = r {
                notes.push(Note::new(
                    NoteKind::ForgeFailure,
                    format!("The types engine could not be started: {e}."),
                ));
            }
        }
    }
    sweep(&p);
    p.note(&notes);
    let mut out = serde_json::Map::new();
    if !context.is_empty() {
        out.insert(
            "hookSpecificOutput".into(),
            json!({"hookEventName": "SessionStart", "additionalContext": context}),
        );
    }
    if !system.is_empty() {
        out.insert("systemMessage".into(), json!(system.join("\n")));
    }
    if out.is_empty() {
        Output::silent()
    } else {
        Output::json(serde_json::Value::Object(out))
    }
}

fn with_system(o: Output, system: &[String]) -> Output {
    if system.is_empty() {
        o
    } else {
        Output::json(json!({"systemMessage": system.join("\n")}))
    }
}

/// At most 20 lines: the branch, the changed files (10), the last commits (5).
fn session_context(root: &Path) -> String {
    let mut lines = Vec::new();
    let st = snapshot::status_paths_only(root);
    if let Some((branch, head, paths)) = st {
        let branch = branch.unwrap_or_else(|| match &head {
            Some(h) => format!("(detached at {})", &h[..h.len().min(7)]),
            None => "(no commits yet)".into(),
        });
        let n = paths.len();
        lines.push(format!(
            "Git branch {branch}, {n} changed file{}{}",
            if n == 1 { "" } else { "s" },
            if n > 0 { ":" } else { "." }
        ));
        for f in paths.iter().take(10) {
            lines.push(format!("  {f}"));
        }
        if n > 10 {
            lines.push(format!("  ({} more)", n - 10));
        }
    }
    if let Some(log) = git(root, &["log", "-5", "--format=%h %s"], GIT_CAP) {
        let log = String::from_utf8_lossy(&log);
        let commits: Vec<&str> = log.lines().filter(|l| !l.is_empty()).collect();
        if !commits.is_empty() {
            lines.push("Last commits:".into());
            for c in commits {
                lines.push(format!("  {}", c.chars().take(72).collect::<String>()));
            }
        }
    }
    lines.truncate(20);
    lines.join("\n")
}

/// Old snapshots: this session's Bash snapshots older than a day, other sessions untouched for
/// three days.
fn sweep(p: &Project) {
    BashSnapshots::new(&p.root, &p.session_dir, &p.state).cleanup(Some(Duration::from_secs(86400)));
    let Ok(rd) = std::fs::read_dir(p.session_dir.parent().unwrap_or(&p.state)) else {
        return;
    };
    let now = SystemTime::now();
    for e in rd.flatten() {
        if e.path() == p.session_dir {
            continue;
        }
        let old = e.metadata().and_then(|m| m.modified()).is_ok_and(|t| {
            now.duration_since(t).unwrap_or_default() > Duration::from_secs(3 * 86400)
        });
        if old {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

// ---- UserPromptSubmit

pub fn prompt(rt: &Runtime, input: &HookInput) -> Output {
    let Some(p) = rt.project(input) else {
        return Output::silent();
    };
    if !state::ensure_dir(&p.session_dir) {
        return Output::silent();
    }
    let st = snapshot::status(&p.root);
    match &st {
        Some(st) => {
            snapshot::write_turn(&p.root, &p.turn_dir(), st);
        }
        None => {
            let _ = std::fs::remove_dir_all(p.turn_dir());
        }
    }
    let _ = std::fs::remove_file(p.session_dir.join("blocks"));
    if let Some(prompt) = &input.prompt {
        let cut: String = prompt.chars().take(8000).collect();
        state::write_atomic(&p.session_dir.join("prompt"), cut.as_bytes());
    }
    // the engine resyncs what changed outside Claude's edits since the last prompt
    if let (Some(st), Some(client)) = (
        &st,
        engine::running(&rt.home, &p.id, Duration::from_millis(200)),
    ) {
        let head_file = p.state.join("engine-head");
        let new = st.head.clone().unwrap_or_default();
        let mut paths: Vec<String> = st.dirty.keys().cloned().collect();
        if let Some(old) = state::read_trim(&head_file).filter(|o| *o != new && !new.is_empty()) {
            if let Some(out) = git(&p.root, &["diff", "--name-only", "-z", &old, &new], GIT_CAP) {
                paths.extend(
                    out.split(|b| *b == 0)
                        .filter(|s| !s.is_empty())
                        .map(|s| String::from_utf8_lossy(s).into_owned()),
                );
            }
        }
        if !paths.is_empty() {
            let _ = client.changed(&paths);
        }
        state::write_atomic(&head_file, new.as_bytes());
    }
    Output::silent()
}

// ---- PostToolUse

/// The files the tool call changed (repository-relative), for the edit-time checks.
fn changed_files(rt: &Runtime, input: &HookInput, p: &Project) -> Vec<String> {
    let cwd = rt.cwd(input);
    match input.tool() {
        "Bash" => {
            let Some(key) = input.tool_key() else {
                return Vec::new();
            };
            let snaps = BashSnapshots::new(&p.root, &p.session_dir, &p.state);
            let Some(before) = state::read_trim(&snaps.tree_file(&key)) else {
                return Vec::new();
            };
            let files = if before == "skipped" {
                Vec::new()
            } else {
                match snaps.tree(&key, false, rt.env.snapshot_cap) {
                    Some(after) => snaps
                        .changed(&before, &after)
                        .into_iter()
                        .filter(|f| p.root.join(f).is_file())
                        .collect(),
                    None => Vec::new(),
                }
            };
            let mut list = files.join("\n");
            list.push('\n');
            state::write_atomic(&snaps.files_file(&key), list.as_bytes());
            snaps.drop_key(&key);
            files
        }
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => {
            let path = input
                .input_str("file_path")
                .or_else(|| input.input_str("notebook_path"));
            let Some(rel) = path.and_then(|f| p.rel(f, &cwd)) else {
                return Vec::new();
            };
            testedit::post(p, input, &rel);
            if snapshot::excluded(&rel) || !p.root.join(&rel).is_file() {
                return Vec::new();
            }
            vec![rel]
        }
        _ => Vec::new(),
    }
}

/// Most files one edit-time run checks; the Stop check covers the rest.
const EDIT_FILES_MAX: usize = 20;

pub fn post_tool_use(rt: &Runtime, input: &HookInput) -> Output {
    let Some(p) = rt.project(input) else {
        return Output::silent();
    };
    let mut files = changed_files(rt, input, &p);
    if files.is_empty() {
        return Output::silent();
    }
    files.truncate(EDIT_FILES_MAX);
    let snap = p.snapshot();
    let report = std::panic::catch_unwind(AssertUnwindSafe(|| {
        rt.checker.post_edit(&p.root, &files, snap.as_deref())
    }))
    .unwrap_or_else(|_| failure("the checks after an edit stopped unexpectedly"));
    p.note(&report.notes);
    if report.findings.is_empty() {
        return Output::silent();
    }
    let event = match input.hook_event_name.as_deref() {
        Some("PostToolUseFailure") => "PostToolUseFailure",
        _ => "PostToolUse",
    };
    Output::json(json!({
        "hookSpecificOutput": {"hookEventName": event, "additionalContext": report.agent_text(20)}
    }))
}

fn failure(what: &str) -> Report {
    Report {
        findings: Vec::new(),
        notes: vec![Note::new(NoteKind::ForgeFailure, format!("Forge: {what}."))],
    }
}

// ---- PostToolUse typecheck (asyncRewake)

fn lang_of(path: &str) -> Option<Lang> {
    let ext = Path::new(path).extension()?.to_str()?;
    match ext {
        "py" | "pyi" => Some(Lang::Python),
        "ts" | "tsx" | "mts" | "cts" => Some(Lang::Typescript),
        _ => None,
    }
}

/// Whether Claude Code runs this asyncRewake hook in the foreground: `claude -p` with a prompt
/// (entrypoint `sdk-cli`) and no streaming input on an ancestor's command line. Every edit would
/// then wait for the typecheck; the Stop check runs it once instead. Kept per session.
fn rewake_blocks(rt: &Runtime, p: &Project) -> bool {
    match rt.env.edit_typecheck.as_deref() {
        Some("always") => return false,
        Some("never") => return true,
        _ => {}
    }
    if rt.env.entrypoint.as_deref() != Some("sdk-cli") {
        return false;
    }
    let cache = p.session_dir.join("rewake");
    match state::read_trim(&cache).as_deref() {
        Some("blocks") => return true,
        Some("runs") => return false,
        _ => {}
    }
    let ps = |pid: u32, field: &str| -> Option<String> {
        let out = Command::new("ps")
            .args(["-o", &format!("{field}="), "-p", &pid.to_string()])
            .output()
            .ok()?;
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let mut pid = std::os::unix::process::parent_id();
    let mut streams = false;
    for _ in 0..12 {
        if pid <= 1 {
            break;
        }
        let args = ps(pid, "args").unwrap_or_default();
        let a = format!(" {args} ");
        if a.contains(" --input-format stream-json ")
            || a.contains(" --input-format=stream-json ")
            || a.contains(" --sdk-url")
        {
            streams = true;
            break;
        }
        pid = ps(pid, "ppid").and_then(|s| s.parse().ok()).unwrap_or(0);
    }
    state::write_atomic(&cache, if streams { b"runs\n" } else { b"blocks\n" });
    !streams
}

pub fn typecheck(rt: &Runtime, input: &HookInput) -> Output {
    let Some(p) = rt.project(input) else {
        return Output::silent();
    };
    if rewake_blocks(rt, &p) {
        return Output::silent();
    }
    let cwd = rt.cwd(input);
    let files: Vec<String> = if input.tool() == "Bash" {
        let Some(key) = input.tool_key() else {
            return Output::silent();
        };
        let snaps = BashSnapshots::new(&p.root, &p.session_dir, &p.state);
        let list = snaps.files_file(&key);
        // PostToolUse, run alongside, writes the files the command changed
        let until = Instant::now() + Duration::from_secs(60);
        while !list.is_file() && snaps.tree_file(&key).is_file() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(50));
        }
        let text = std::fs::read_to_string(&list).unwrap_or_default();
        let _ = std::fs::remove_file(&list);
        text.lines()
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect()
    } else {
        match input.input_str("file_path").and_then(|f| p.rel(f, &cwd)) {
            Some(f) if !snapshot::excluded(&f) => vec![f],
            _ => Vec::new(),
        }
    };
    let Some(cache) = p.rules(&rt.home) else {
        return Output::silent();
    };
    let mut by_lang: Vec<(Lang, Vec<String>)> = Vec::new();
    for f in files {
        let Some(l) = lang_of(&f) else { continue };
        if !cache.typecheck.contains(&l) {
            continue;
        }
        match by_lang.iter_mut().find(|(x, _)| *x == l) {
            Some((_, v)) => v.push(f),
            None => by_lang.push((l, vec![f])),
        }
    }
    if by_lang.is_empty() {
        return Output::silent();
    }
    let Some(client) = engine::running(&rt.home, &p.id, Duration::from_secs(120)) else {
        return Output::silent();
    };
    let tmp = p
        .session_dir
        .join(format!("typecheck.{}", std::process::id()));
    let mut lines: Vec<String> = Vec::new();
    let mut notes = Vec::new();
    for (lang, files) in by_lang {
        let diag_files: Vec<DiagFile> = files
            .iter()
            .map(|f| DiagFile {
                path: f.clone(),
                base: base_copy(&p, f, &tmp),
            })
            .collect();
        match client.diagnostics(lang, &diag_files, false) {
            Ok(d) => lines.extend(d.iter().map(|d| d.concise())),
            Err(forge_engine::EngineError::Loading) => {}
            Err(e) => notes.push(Note::new(
                NoteKind::ForgeFailure,
                format!("The {lang} typecheck after an edit did not run: {e}."),
            )),
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    // a declared dependency the environment lacks is not the edit's error (§10.4)
    if lines
        .iter()
        .any(|l| forge_check::pydeps::unresolved_module(l).is_some())
    {
        let (kept, missing) = forge_check::pydeps::Declared::of(&p.root).split(&lines.join("\n"));
        if !missing.is_empty() {
            notes.push(forge_check::pydeps::missing_note(&missing, ""));
            lines = kept.lines().map(str::to_owned).collect();
        }
    }
    p.note(&notes);
    if lines.is_empty() {
        return Output::silent();
    }
    let n = lines.len();
    lines.truncate(20);
    if n > 20 {
        lines.push(format!("({} more)", n - 20));
    }
    Output {
        stdout: String::new(),
        stderr: format!("This edit added type errors:\n{}\n", lines.join("\n")),
        code: 2,
    }
}

/// `file` as it was at the start of the turn, copied under `tmp`; None when it is new.
fn base_copy(p: &Project, file: &str, tmp: &Path) -> Option<PathBuf> {
    let dst = tmp.join(file);
    state::ensure_dir(dst.parent()?);
    let mut rev = "HEAD".to_string();
    if let Some(s) = p.snapshot() {
        let dirty = std::fs::read_to_string(s.join("dirty")).unwrap_or_default();
        if let Some(h) = dirty
            .lines()
            .find_map(|l| l.split_once('\t').filter(|(q, _)| *q == file).map(|x| x.1))
        {
            if h == "-" {
                return None;
            }
            return std::fs::copy(s.join("files").join(file), &dst)
                .ok()
                .map(|_| dst);
        }
        if let Some(h) = state::read_trim(&s.join("head")) {
            rev = h;
        }
    }
    let bytes = git(
        &p.root,
        &["cat-file", "blob", &format!("{rev}:{file}")],
        GIT_CAP,
    )?;
    std::fs::write(&dst, bytes).ok().map(|_| dst)
}

// ---- Stop

/// Blocks in one turn after which the stop is let through.
const MAX_BLOCKS: u32 = 3;

pub fn stop(rt: &Runtime, input: &HookInput) -> Output {
    let Some(p) = rt.project(input) else {
        return Output::silent();
    };
    if !state::ensure_dir(&p.session_dir) {
        return Output::silent();
    }
    let counter = p.session_dir.join("blocks");
    if !input.stop_hook_active {
        // not a continuation of a blocked stop: a new count
        let _ = std::fs::remove_file(&counter);
    }
    let Some(st) = snapshot::status(&p.root) else {
        return Output::silent();
    };
    let snap = p.snapshot();
    let hash = st.tree_hash();
    let (pass, fail) = match &snap {
        Some(s) => {
            if snapshot::turn_ids(s).is_some_and(|(_, tree)| tree == hash) {
                // nothing changed since the prompt: whatever is dirty is not this turn's work
                return Output::silent();
            }
            (s.join("pass"), s.join("fail"))
        }
        None => (
            p.session_dir.join("last-pass"),
            p.session_dir.join("last-fail"),
        ),
    };
    let read = |f: &Path| state::read_trim(f);
    let reason_file = fail.with_extension("reason");
    if read(&pass).as_deref() == Some(hash.as_str()) {
        return Output::silent();
    }
    let same_failure = read(&fail).as_deref() == Some(hash.as_str());
    let count: u32 = read(&counter).and_then(|c| c.parse().ok()).unwrap_or(0);
    if count >= MAX_BLOCKS {
        let _ = std::fs::remove_file(&counter);
        p.note(&[Note::new(
            NoteKind::Environment,
            format!("The Stop check failed {MAX_BLOCKS} times in one turn, so Claude was let stop with the failures left; run `forge check` to see them."),
        )]);
        return Output::silent();
    }
    let block = |text: String| -> Output {
        state::write_atomic(&counter, (count + 1).to_string().as_bytes());
        Output::json(json!({"decision": "block", "reason": cap(&text)}))
    };
    if same_failure && reason_file.is_file() {
        // Claude was shown these findings and stopped again without changing anything: it chose
        // to leave them (the user may have asked for that code). Blocking again with the same
        // reason only costs turns; the person is told what was left.
        let _ = std::fs::remove_file(&counter);
        p.note(&[Note::new(
            NoteKind::Environment,
            "Claude stopped with the Stop check's findings left after being shown them once; run `forge check` to see them.",
        )]);
        return Output::silent();
    }
    let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
        rt.checker.stop(&p.root, snap.as_deref())
    }))
    .unwrap_or_else(|_| StopOutcome {
        report: failure("the Stop check stopped unexpectedly"),
        status: StopStatus::Pass,
    });
    p.note(&outcome.report.notes);
    // the tree as the checks left it (formatters may have run)
    let now = snapshot::status(&p.root).map_or(hash, |s| s.tree_hash());
    let text = cap(&outcome.report.agent_text(40));
    let has_findings = !outcome.report.findings.is_empty();
    match outcome.status {
        StopStatus::Fail if outcome.report.fails() => {
            state::write_atomic(&fail, now.as_bytes());
            state::write_atomic(&reason_file, text.as_bytes());
            block(text)
        }
        // tests that fail with and without the change are the person's (a note), never a
        // block; advice next to them is still given once
        StopStatus::AdviceOnly | StopStatus::Unverified if has_findings => {
            // advice is given once: the same tree then passes
            state::write_atomic(&pass, now.as_bytes());
            let _ = std::fs::remove_file(&fail);
            block(text)
        }
        _ => {
            // passed, or failed with nothing for the agent (Forge's own failures never block)
            state::write_atomic(&pass, now.as_bytes());
            for f in [&fail, &reason_file, &counter] {
                let _ = std::fs::remove_file(f);
            }
            Output::silent()
        }
    }
}

/// A Stop reason: at most 40 lines and 6000 characters.
fn cap(text: &str) -> String {
    let mut out = String::new();
    for (i, l) in text.lines().enumerate() {
        if i >= 40 || out.len() + l.len() + 1 > 6000 {
            break;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(l);
    }
    out
}

// ---- Notification

pub fn notify(rt: &Runtime, input: &HookInput) -> Output {
    let msg: String = input
        .message
        .as_deref()
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(200)
        .collect();
    let msg = if msg.trim().is_empty() {
        "Claude Code needs your attention".to_string()
    } else {
        msg
    };
    let seq = match rt.env.term_program.as_deref() {
        Some("iTerm.app" | "WezTerm") => format!("\u{1b}]9;{msg}\u{7}"),
        _ => format!("\u{1b}]777;notify;Claude Code;{msg}\u{7}"),
    };
    Output::json(json!({ "terminalSequence": seq }))
}

// ---- SessionEnd

pub fn session_end(rt: &Runtime, input: &HookInput) -> Output {
    let Some(p) = rt.project(input) else {
        return Output::silent();
    };
    if let Some(c) = engine::running(&rt.home, &p.id, Duration::from_secs(2)) {
        let _ = c.unregister(&p.session);
    }
    let _ = std::fs::remove_dir_all(p.session_dir.join("bash"));
    Output::silent()
}
