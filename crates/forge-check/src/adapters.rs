//! Tool adapters (§16) as data: `adapters/<tool>.yaml`, embedded in the binary and parsed at run
//! time with the packs' strict YAML loader. A rule names an operation of a tool
//! (`forge_do ruff lint {files}`); the adapter of the installed version says how that version
//! does it, or that it cannot.
//!
//! Each file is one adapter: `tool`, `pinned` (Forge's own tools: the version is the pin),
//! `verified` (the newest release it is proven on), `runner` (`project`, the default: the
//! project's own install; `path`; `pinned`), `pick` (which adapter drives the project's tool
//! when it is one of several: `opentofu`, `kustomize-or-kubectl`), `version_args`, `package`
//! (read the version from that npm package's package.json), `usage` (the exit statuses and
//! output line of a usage error, or null), `ranges` (oldest first: `from` inclusive, `to`
//! exclusive, `ops`: per operation the arguments after the tool's name, or null: this range
//! cannot), and `codes` (the operation listing the release's rule codes and their history).
//! The header comment of each file says where each boundary is documented.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use forge_core::{Note, NoteKind};
use include_dir::{Dir, include_dir};
use indexmap::IndexMap;
use serde::Deserialize;

use crate::tools::{ToolCmd, ToolResolver};

static ADAPTER_FILES: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/adapters");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Runner {
    #[default]
    Project,
    Path,
    Pinned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Pick {
    /// terraform, or tofu in a repository that uses OpenTofu.
    Opentofu,
    /// kustomize when it is on PATH, else kubectl (`kubectl kustomize`).
    KustomizeOrKubectl,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Usage {
    pub exit: Vec<i32>,
    /// A POSIX ERE over one output line.
    pub pattern: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Range {
    pub from: String,
    #[serde(default)]
    pub to: Option<String>,
    pub source: String,
    pub ops: IndexMap<String, Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodeHistory {
    pub since: String,
    pub codes: Vec<String>,
    pub names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Codes {
    pub op: String,
    pub history: BTreeMap<String, CodeHistory>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Adapter {
    pub tool: String,
    #[serde(default)]
    pub pinned: Option<String>,
    pub verified: String,
    #[serde(default)]
    pub runner: Runner,
    #[serde(default)]
    pub pick: Option<Pick>,
    #[serde(default)]
    pub version_args: Option<String>,
    #[serde(default)]
    pub package: Option<String>,
    #[serde(default)]
    pub direct: Option<String>,
    pub usage: Option<Usage>,
    pub ranges: Vec<Range>,
    #[serde(default)]
    pub codes: Option<Codes>,
}

/// What a version of a tool does for an operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpLookup {
    /// The arguments after the tool's name.
    Args(String),
    /// Its range cannot; `need`: the first version at or after it that can.
    Cannot { need: Option<String> },
    /// No range covers the version; `first`: the oldest one covered.
    NotCovered { first: String },
}

impl Adapter {
    /// The adapter of `tool`, parsed from its data file.
    pub fn load(tool: &str) -> Option<Adapter> {
        let file = ADAPTER_FILES.get_file(format!("{tool}.yaml"))?;
        parse(file.contents_utf8()?).ok()
    }

    /// Every adapter, by tool name.
    pub fn all() -> Result<Vec<Adapter>, String> {
        let mut out = Vec::new();
        for f in ADAPTER_FILES.files() {
            let name = f.path().display().to_string();
            let text = f
                .contents_utf8()
                .ok_or_else(|| format!("{name}: not UTF-8"))?;
            let a = parse(text).map_err(|e| format!("{name}: {e}"))?;
            if format!("{}.yaml", a.tool) != name {
                return Err(format!("{name}: holds the adapter of {}", a.tool));
            }
            out.push(a);
        }
        out.sort_by(|a, b| a.tool.cmp(&b.tool));
        Ok(out)
    }

    /// How `version` does `op`.
    pub fn lookup(&self, version: &str, op: &str) -> OpLookup {
        let first = self
            .ranges
            .iter()
            .map(|r| r.from.as_str())
            .min_by(|a, b| cmp_version(a, b))
            .unwrap_or("0")
            .to_string();
        let below = |r: &Range| {
            r.to.as_deref()
                .is_none_or(|to| cmp_version(version, to) == Ordering::Less)
        };
        let covering = self
            .ranges
            .iter()
            .find(|r| below(r) && cmp_version(version, &r.from) != Ordering::Less);
        let Some(range) = covering else {
            return OpLookup::NotCovered { first };
        };
        match range.ops.get(op) {
            Some(Some(args)) => OpLookup::Args(args.clone()),
            _ => {
                let need = self
                    .ranges
                    .iter()
                    .filter(|r| below(r) && matches!(r.ops.get(op), Some(Some(_))))
                    .map(|r| r.from.clone())
                    .min_by(|a, b| cmp_version(a, b));
                OpLookup::Cannot { need }
            }
        }
    }

    /// The operations any range has.
    pub fn ops(&self) -> Vec<String> {
        let mut ops: Vec<String> = self
            .ranges
            .iter()
            .flat_map(|r| r.ops.keys().cloned())
            .collect();
        ops.sort();
        ops.dedup();
        ops
    }

    pub fn version_args(&self) -> &str {
        self.version_args.as_deref().unwrap_or("--version")
    }
}

fn parse(text: &str) -> Result<Adapter, String> {
    let parsed = forge_packs::yaml::parse_strict(text).map_err(|e| e.to_string())?;
    serde_json::from_value(parsed.value).map_err(|e| e.to_string())
}

/// Compares versions by their first three numbers (`forge_version_lt`).
pub fn cmp_version(a: &str, b: &str) -> Ordering {
    let parts = |v: &str| -> [u64; 3] {
        let mut out = [0; 3];
        for (i, p) in v.split('.').take(3).enumerate() {
            let digits: String = p.chars().take_while(|c| c.is_ascii_digit()).collect();
            out[i] = digits.parse().unwrap_or(0);
        }
        out
    };
    parts(a).cmp(&parts(b))
}

/// The adapter's words with the caller's arguments: `{1}`, `{2}`… by rank, `{args}` the others;
/// without `{args}`, the others follow.
pub fn build_args(adapter_args: &str, caller: &[String]) -> Vec<String> {
    let words: Vec<&str> = adapter_args.split(' ').filter(|w| !w.is_empty()).collect();
    let rank = |w: &str| -> Option<usize> {
        let n = w.strip_prefix('{')?.strip_suffix('}')?;
        (n.len() == 1)
            .then(|| n.parse::<usize>().ok())
            .flatten()
            .filter(|n| *n >= 1)
    };
    let used: Vec<usize> = words.iter().filter_map(|w| rank(w)).collect();
    let others: Vec<String> = caller
        .iter()
        .enumerate()
        .filter(|(i, _)| !used.contains(&(i + 1)))
        .map(|(_, a)| a.clone())
        .collect();
    let mut out = Vec::new();
    let mut put = false;
    for w in &words {
        if *w == "{args}" {
            out.extend(others.iter().cloned());
            put = true;
        } else if let Some(k) = rank(w) {
            if let Some(a) = caller.get(k - 1) {
                out.push(a.clone());
            }
        } else {
            out.push(w.to_string());
        }
    }
    if !put {
        out.extend(others);
    }
    out
}

/// `forge_tf_pick`: tofu when the repository uses OpenTofu (a `*.tofu` file, or a
/// `.terraform.lock.hcl` naming registry.opentofu.org providers), else terraform.
pub fn opentofu_pick(root: &Path) -> &'static str {
    let out = Command::new(crate::change::git_exe())
        .current_dir(root)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            "*.tofu",
            "*.terraform.lock.hcl",
        ])
        .stderr(Stdio::null())
        .output();
    let Ok(out) = out else { return "terraform" };
    for f in crate::change::split_z(&out.stdout) {
        if f.ends_with(".tofu") {
            return "tofu";
        }
        if std::fs::read_to_string(root.join(&f))
            .is_ok_and(|t| t.contains("registry.opentofu.org/"))
        {
            return "tofu";
        }
    }
    "terraform"
}

/// The adapter that drives the project's `tool` (its pick's choice).
pub fn picked(tool: &str, root: &Path, tools: &dyn ToolResolver) -> String {
    match Adapter::load(tool).and_then(|a| a.pick) {
        Some(Pick::Opentofu) => opentofu_pick(root).to_string(),
        Some(Pick::KustomizeOrKubectl) => {
            if tools.on_path("kustomize").is_some() {
                "kustomize".into()
            } else {
                "kubectl".into()
            }
        }
        None => tool.to_string(),
    }
}

/// The first `x.y[.z]` a tool prints for `args`.
pub fn ask_version(cmd: &ToolCmd, args: &str) -> Option<String> {
    let mut c = Command::new(&cmd.argv[0]);
    c.args(&cmd.argv[1..])
        .args(args.split(' ').filter(|w| !w.is_empty()))
        .stdin(Stdio::null());
    let out = c.output().ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    first_version(&text)
}

pub fn first_version(text: &str) -> Option<String> {
    static V: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"[0-9]+\.[0-9]+(\.[0-9]+)?").expect("regex")
    });
    V.find(text).map(|m| m.as_str().to_string())
}

/// The version an npm package's package.json says, found up from the executable's real path.
fn package_version(bin: &Path, package: &str) -> Option<String> {
    let real = std::fs::canonicalize(bin).ok()?;
    let mut dir = real.parent();
    while let Some(d) = dir {
        let pj = d.join("package.json");
        if let Ok(text) = std::fs::read_to_string(&pj)
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&text)
            && v.get("name").and_then(|n| n.as_str()) == Some(package)
        {
            return v
                .get("version")
                .and_then(|x| x.as_str())
                .and_then(first_version);
        }
        dir = d.parent();
    }
    None
}

/// Versions asked once per executable, kept while it is unchanged (§16.3, a fact).
fn version_cache_file() -> Option<PathBuf> {
    let home = forge_home::Home::from_env().ok()?;
    Some(home.state_root().join("facts").join("tool-versions.json"))
}

fn stamp(bin: &Path) -> String {
    let m = |p: &Path| {
        std::fs::metadata(p)
            .and_then(|m| m.modified().map(|t| (t, m.len())))
            .map(|(t, l)| {
                format!(
                    "{}:{l}",
                    t.duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_nanos())
                        .unwrap_or(0)
                )
            })
            .unwrap_or_default()
    };
    let real = std::fs::canonicalize(bin).unwrap_or_else(|_| bin.to_path_buf());
    format!(
        "{}|{}|{}|{}",
        bin.display(),
        m(bin),
        real.display(),
        m(&real)
    )
}

/// How many versions the facts file keeps: the least recently used go first.
const VERSIONS_KEPT: usize = 200;

/// A use refreshes an entry's time only when it is older than this, so a hit rarely writes.
const USE_GRANULARITY_SECS: u64 = 3600;

/// One remembered version and when it was last used (seconds since the epoch).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
struct VersionFact {
    version: String,
    used: u64,
}

type VersionFacts = BTreeMap<String, VersionFact>;

/// The version of `key` in `facts`, its use recorded; `true` when `facts` changed.
fn recall(facts: &mut VersionFacts, key: &str, now: u64) -> (Option<String>, bool) {
    match facts.get_mut(key) {
        Some(f) => {
            let stale = now.saturating_sub(f.used) >= USE_GRANULARITY_SECS;
            if stale {
                f.used = now;
            }
            (Some(f.version.clone()), stale)
        }
        None => (None, false),
    }
}

/// Remembers `version` for `key`, then keeps the [`VERSIONS_KEPT`] most recently used.
fn remember(facts: &mut VersionFacts, key: String, version: String, now: u64) {
    facts.insert(key, VersionFact { version, used: now });
    if facts.len() > VERSIONS_KEPT {
        let mut by_use: Vec<(u64, String)> =
            facts.iter().map(|(k, f)| (f.used, k.clone())).collect();
        by_use.sort();
        for (_, k) in by_use.into_iter().take(facts.len() - VERSIONS_KEPT) {
            facts.remove(&k);
        }
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn cached_version(
    bin: &Path,
    args: &str,
    compute: impl FnOnce() -> Option<String>,
) -> Option<String> {
    let key = format!("{}|{args}", stamp(bin));
    let file = version_cache_file();
    // an older or unreadable file is an empty cache
    let mut facts: VersionFacts = file
        .as_ref()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let now = now_secs();
    let (found, changed) = recall(&mut facts, &key, now);
    let v = match found {
        Some(v) if !changed => return Some(v),
        Some(v) => v,
        None => {
            let v = compute()?;
            remember(&mut facts, key, v.clone(), now);
            v
        }
    };
    if let Some(f) = file {
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let tmp = f.with_extension(format!("tmp.{}", std::process::id()));
        if std::fs::write(&tmp, serde_json::to_string(&facts).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(&tmp, &f);
        }
    }
    Some(v)
}

/// Where the tool of an adapter is, and its version.
pub fn locate(
    a: &Adapter,
    tools: &dyn ToolResolver,
    root: &Path,
    cwd: &Path,
) -> Result<(ToolCmd, String), String> {
    if let Some(pin) = &a.pinned {
        let cmd = tools.pinned(&a.tool, pin)?;
        return Ok((cmd, pin.clone()));
    }
    let cmd = match a.runner {
        Runner::Project => tools
            .project(root, cwd, &a.tool)
            .ok_or_else(|| format!("{} is not installed", a.tool))?,
        Runner::Path => tools
            .on_path(&a.tool)
            .map(ToolCmd::exe)
            .ok_or_else(|| format!("{} is not installed", a.tool))?,
        Runner::Pinned => tools.pinned(&a.tool, &a.verified)?,
    };
    let args = a.version_args().to_string();
    let v = cached_version(&cmd.bin, &args, || {
        a.package
            .as_deref()
            .and_then(|p| package_version(&cmd.bin, p))
            .or_else(|| ask_version(&cmd, &args))
    })
    .ok_or_else(|| format!("the version of {} could not be read", a.tool))?;
    Ok((cmd, v))
}

/// What `forge_do` did.
#[derive(Debug, Default)]
pub struct DoOutcome {
    pub status: i32,
    pub notes: Vec<Note>,
}

/// An operation of a tool, resolved: what runs and with which arguments.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub adapter: Adapter,
    /// The adapter's tool (after `pick`).
    pub name: String,
    pub version: String,
    pub cmd: ToolCmd,
    /// The arguments after the command's words (the operation's, then the caller's).
    pub args: Vec<String>,
}

impl Resolved {
    /// The whole command line.
    pub fn argv(&self) -> Vec<String> {
        let mut v = self.cmd.argv.clone();
        v.extend(self.args.iter().cloned());
        v
    }

    /// Why the tool rejected the command line (a usage error, which says the adapter does not
    /// fit the installed version), from its exit status and output; None when it did not.
    pub fn usage_error(
        &self,
        op: &str,
        status: i32,
        stdout: &[u8],
        stderr: &[u8],
    ) -> Option<String> {
        let u = self.adapter.usage.as_ref()?;
        if !u.exit.contains(&status) || op.ends_with("-stdin") {
            return None;
        }
        let re = regex::Regex::new(&crate::grep::ere_to_regex(&u.pattern)).ok()?;
        let (out, err) = (
            String::from_utf8_lossy(stdout),
            String::from_utf8_lossy(stderr),
        );
        let text = strip_ansi(&format!("{out}{err}"));
        if !text.lines().any(|l| re.is_match(l)) {
            return None;
        }
        let errtext = strip_ansi(&format!("{err}{out}"));
        let why = errtext
            .lines()
            .find(|l| {
                let ll = l.to_ascii_lowercase();
                ll.starts_with("error") || ll.contains(": error")
            })
            .or_else(|| errtext.lines().find(|l| re.is_match(l)))
            .unwrap_or("")
            .trim_start_matches("error: ")
            .trim_start_matches("Error: ")
            .to_string();
        Some(format!(
            "skipped: {} {} rejected the command ({} {}): {why}",
            self.name,
            self.version,
            self.name,
            self.args.join(" ")
        ))
    }
}

/// How `op` of `tool` runs here with `args`, as its adapter says for the installed version;
/// a note (the tool is absent, its version cannot, no adapter covers it) when it cannot.
pub fn resolve(tool: &str, op: &str, args: &[String], ctx: &DoCtx<'_>) -> Result<Resolved, Note> {
    let note = |kind: NoteKind, text: String| {
        let n = Note::new(kind, text);
        if ctx.rule.is_empty() {
            n
        } else {
            n.for_rule(ctx.rule.to_string())
        }
    };
    let name = picked(tool, ctx.root, ctx.tools);
    let Some(a) = Adapter::load(&name) else {
        return Err(note(
            NoteKind::ForgeFailure,
            format!("Forge has no adapter for {name}"),
        ));
    };
    let (cmd, version) = locate(&a, ctx.tools, ctx.root, ctx.cwd)
        .map_err(|why| note(NoteKind::MissingTool, format!("skipped: {why}")))?;
    let words = match a.lookup(&version, op) {
        OpLookup::Args(w) => w,
        OpLookup::Cannot { need } => {
            let need = need
                .map(|n| format!(" (it needs {name} {n} or later)"))
                .unwrap_or_default();
            return Err(note(
                NoteKind::NotEnforced,
                format!("not enforced: {name} {version} cannot {op}{need}"),
            ));
        }
        OpLookup::NotCovered { first } => {
            return Err(note(
                NoteKind::NotEnforced,
                format!(
                    "not enforced: no Forge adapter covers {name} {version} (Forge drives {name} {first} and later)"
                ),
            ));
        }
    };
    let mut args = build_args(&words, args);
    if name == "ty" {
        args.extend(ty_level_args(ctx.cwd, ctx.root));
    }
    Ok(Resolved {
        args,
        adapter: a,
        name,
        version,
        cmd,
    })
}

/// The rule levels Forge's pinned ty is run with: `--error <rule>` for each rule the checker
/// lists (`FORGE_TY_RULES`), where the project leaves ty to its defaults. A project that
/// configures ty itself (`ty.toml`, `[tool.ty]`) is the authority on its levels: nothing is
/// added, and the audit says what its config lacks (§11.2). ty 0.0.86: `ty check --error <RULE>`.
pub fn ty_level_args(cwd: &Path, root: &Path) -> Vec<String> {
    let Some(rules) = std::env::var_os(crate::builtins::ENV_TY_RULES) else {
        return Vec::new();
    };
    if forge_detect::place::ty_config(cwd, root).is_some() {
        return Vec::new();
    }
    error_args(rules.to_string_lossy().lines())
}

/// `--error <rule>` for each rule.
pub fn error_args<'a>(rules: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    rules
        .into_iter()
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .flat_map(|r| ["--error".to_string(), r.to_string()])
        .collect()
}

/// `forge_do <tool> <op> [args]` (§16.1): runs `op` by the installed tool as its adapter says,
/// with the tool's stdout and stderr passed on once it has exited. A version that cannot (or
/// that no range covers), a tool that is absent, and a usage error of the command line are
/// notes for the person, and status 0: never a finding.
pub fn run_do(
    tool: &str,
    op: &str,
    args: &[String],
    ctx: &DoCtx<'_>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> DoOutcome {
    let note = |kind: NoteKind, text: String| {
        let n = Note::new(kind, text);
        if ctx.rule.is_empty() {
            n
        } else {
            n.for_rule(ctx.rule.to_string())
        }
    };
    let r = match resolve(tool, op, args, ctx) {
        Ok(r) => r,
        Err(n) => {
            return DoOutcome {
                status: 0,
                notes: vec![n],
            };
        }
    };
    let name = &r.name;
    let mut c = Command::new(&r.cmd.argv[0]);
    c.args(&r.cmd.argv[1..]).args(&r.args).current_dir(ctx.cwd);
    c.stdin(if ctx.stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::inherit()
    });
    let child = c.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
    let mut child = match child {
        Ok(ch) => ch,
        Err(e) => {
            return DoOutcome {
                status: 0,
                notes: vec![note(
                    NoteKind::MissingTool,
                    format!("skipped: {name} could not run: {e}"),
                )],
            };
        }
    };
    if let (Some(input), Some(mut sin)) = (ctx.stdin.clone(), child.stdin.take()) {
        std::thread::spawn(move || {
            let _ = sin.write_all(&input);
        });
    }
    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => {
            return DoOutcome {
                status: 0,
                notes: vec![note(
                    NoteKind::ForgeFailure,
                    format!("{name} could not run: {e}"),
                )],
            };
        }
    };
    let status = output.status.code().unwrap_or(128 + 9);
    if let Some(why) = r.usage_error(op, status, &output.stdout, &output.stderr) {
        return DoOutcome {
            status: 0,
            notes: vec![note(NoteKind::NotEnforced, why)],
        };
    }
    let _ = out.write_all(&output.stdout);
    let _ = err.write_all(&output.stderr);
    DoOutcome {
        status,
        notes: Vec::new(),
    }
}

/// Where `forge_do` runs.
pub struct DoCtx<'a> {
    pub root: &'a Path,
    pub cwd: &'a Path,
    pub rule: &'a str,
    pub tools: &'a dyn ToolResolver,
    /// Bytes for the tool's stdin (`format-stdin`); None: inherited.
    pub stdin: Option<Vec<u8>>,
}

pub fn strip_ansi(s: &str) -> String {
    static ANSI: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new("\x1b\\[[0-9;]*[A-Za-z]").expect("regex"));
    ANSI.replace_all(s, "").into_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_facts_drop_the_least_recently_used() {
        use super::{VERSIONS_KEPT, VersionFacts, recall, remember};
        let mut facts = VersionFacts::new();
        // the oldest entry, and its key sorts first: an alphabetical cap would drop it
        remember(&mut facts, "a-used".into(), "2.0".into(), 50);
        for i in 0..VERSIONS_KEPT - 1 {
            remember(&mut facts, format!("z{i:03}"), "1.0".into(), 100 + i as u64);
        }
        assert_eq!(facts.len(), VERSIONS_KEPT);
        // a use refreshes it (its time is older than the granularity)
        let (v, changed) = recall(&mut facts, "a-used", 10_000);
        assert_eq!((v.as_deref(), changed), (Some("2.0"), true));
        // a use soon after does not rewrite the file
        assert!(!recall(&mut facts, "a-used", 10_001).1);
        remember(&mut facts, "b-new".into(), "3.0".into(), 10_002);
        remember(&mut facts, "c-new".into(), "3.0".into(), 10_003);
        assert_eq!(facts.len(), VERSIONS_KEPT);
        // the two least recently used went; the used one and the new ones stay
        assert!(
            ["a-used", "b-new", "c-new", "z002"]
                .iter()
                .all(|k| facts.contains_key(*k))
        );
        assert!(!facts.contains_key("z000") && !facts.contains_key("z001"));
    }

    use super::*;

    #[test]
    fn every_adapter_loads() {
        let all = Adapter::all().unwrap();
        assert_eq!(all.len(), 51);
        for a in &all {
            assert!(!a.ranges.is_empty(), "{}", a.tool);
            // oldest first, without gaps
            for w in a.ranges.windows(2) {
                assert_eq!(w[0].to.as_deref(), Some(w[1].from.as_str()), "{}", a.tool);
            }
            assert!(
                a.ranges.last().unwrap().to.is_none(),
                "{}: last range open",
                a.tool
            );
            if let Some(u) = &a.usage {
                regex::Regex::new(&crate::grep::ere_to_regex(&u.pattern))
                    .unwrap_or_else(|e| panic!("{}: {e}", a.tool));
            }
        }
        let ruff = Adapter::load("ruff").unwrap();
        assert!(ruff.codes.as_ref().unwrap().history.contains_key("ANN001"));
    }

    #[test]
    fn ranges_and_ops() {
        let t = Adapter::load("terraform").unwrap();
        assert_eq!(t.lookup("1.5.0", "format"), OpLookup::Args("fmt".into()));
        assert_eq!(
            t.lookup("1.2.0", "format"),
            OpLookup::Cannot {
                need: Some("1.3.0".into())
            }
        );
        assert_eq!(
            t.lookup("0.15.0", "format"),
            OpLookup::NotCovered {
                first: "1.0.0".into()
            }
        );
        let r = Adapter::load("ruff").unwrap();
        assert_eq!(
            r.lookup("0.1.10", "settings-warnings"),
            OpLookup::Cannot {
                need: Some("0.2.0".into())
            }
        );
    }

    #[test]
    fn args_by_rank() {
        let c = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            build_args("check --x", &c(&["a", "b"])),
            c(&["check", "--x", "a", "b"])
        );
        assert_eq!(
            build_args("format --stdin-filename {args} -", &c(&["f.py"])),
            c(&["format", "--stdin-filename", "f.py", "-"])
        );
        assert_eq!(
            build_args("lint -c {1}", &c(&["cfg", "x", "y"])),
            c(&["lint", "-c", "cfg", "x", "y"])
        );
        assert_eq!(
            build_args("a {2} {args}", &c(&["x", "y", "z"])),
            c(&["a", "y", "x", "z"])
        );
    }

    #[test]
    fn versions() {
        assert_eq!(cmp_version("0.10.0", "0.9.1"), Ordering::Greater);
        assert_eq!(cmp_version("1.2", "1.2.0"), Ordering::Equal);
        assert_eq!(first_version("ruff 0.6.9\n").as_deref(), Some("0.6.9"));
    }
}
