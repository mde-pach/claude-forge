//! `ephemeral-exec`: a package launcher that would fetch what it runs from a registry.
//!
//! Flagged: `pnpm dlx`, `pnpx`, `yarn dlx` and `pipx run` always; `npx`, `bunx`, `bun x`,
//! `npm exec` / `npm x`, `uvx` and `uv tool run` when given a package option before the
//! command name (`-p`, `--package`, `-c`, `--call`; uv's `--from`, `--with*`, index options),
//! a spec that is not a bare name (`name@version`, `name==version`, a URL, `user/repo`), or a
//! command not installed where it runs: `node_modules/.bin/<name>` (or
//! `node_modules/<name>/package.json`) for the Node launchers, `.venv/bin/<name>` for uv, from
//! that directory up to the repository root. Only options before the command name belong to
//! the launcher (`npx tsc --noEmit -p tsconfig.json` runs tsc's `-p`); `npm exec` also reads
//! its options after the name, up to `--`, unless `--` precedes it. A package manager's own
//! options before its subcommand are skipped (`npm --yes exec`; `--prefix`, `-C`, `--cwd` move
//! the directory). The create forms run an initializer package `create-<name>` (`@scope` →
//! `@scope/create`, `@scope/x` → `@scope/create-x`): `npm create` / `npm init <x>` and
//! `bun create <x>` are judged as `npm exec` / `bunx create-<x>` (a GitHub `user/repo`
//! template is fetched), `pnpm create` and `yarn create` always fetch; `npm init` without an
//! initializer is not flagged. `deno run|x|serve` on an `npm:`, `jsr:` or URL module is
//! flagged. A command name expanded at run time is flagged when the variable holds a launcher
//! (`N=npx; $N cowsay`), or when its value is unknown and the command line names a launcher.
//!
//! Every command position is checked: `uv run npx cowsay` is `npx cowsay`.

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::ctx::GuardCtx;
use crate::parse::{Dir, Parsed, SimpleCommand, Word, basename, chdir};
use crate::runners::{NODE_LAUNCHER_VALUE, UV_VALUE, pm_option};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Node,
    Npm,
    Uv,
    /// Always fetches.
    Dlx,
}

enum Found {
    Remote(String),
    /// Fetched unless installed: (node or python, where it runs, the name, the launcher).
    Lookup(bool, Dir, String, String),
}

const LAUNCHERS: &[&str] = &[
    "npx", "bunx", "pnpx", "uvx", "pipx", "npm", "pnpm", "yarn", "bun", "deno", "uv",
];

/// The command line names a launcher somewhere (the old pre-filter, for a command name whose
/// value is unknown).
static NAMES_LAUNCHER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(npx|pnpx|bunx|dlx|uvx|uv\s+tool|pipx|deno\s|(npm|pnpm|yarn|bun)\s(.*\s)?(exec|x|create|init|innit)(\s|$))")
        .expect("regex")
});

/// The finding for the first launcher of `parsed` that would fetch what it runs.
pub fn check(parsed: &Parsed, raw: &str, ctx: &GuardCtx) -> Option<String> {
    for cmd in &parsed.commands {
        if let Some(e) = cmd.effective()
            && cmd.words[e].dynamic
            && let Some(f) = dynamic_name(cmd, e, parsed, raw)
        {
            return Some(f);
        }
        let mut found = Vec::new();
        for &s in &cmd.starts {
            if cmd.words[s].dynamic {
                continue;
            }
            at(cmd, s, ctx, &mut found);
        }
        for f in found {
            match f {
                Found::Remote(m) => return Some(m),
                Found::Lookup(node, dir, name, launcher) => {
                    let Dir::Known(d) = dir else {
                        return Some(format!(
                            "{launcher} {name} runs in a directory that is not known in advance, so whether {name} is installed there is unknown: give `cd` a literal path"
                        ));
                    };
                    if installed(node, &d, &name, ctx) {
                        continue;
                    }
                    let w = if node {
                        "node_modules/.bin"
                    } else {
                        ".venv/bin"
                    };
                    let place = if d == ctx.root {
                        "in the repository root".to_owned()
                    } else {
                        format!("from {} up to the repository root", ctx.shown(&d))
                    };
                    return Some(format!(
                        "{name} is not installed where the command runs (no {w}/{name} {place}), so {launcher} would fetch it from a registry and run it"
                    ));
                }
            }
        }
    }
    None
}

fn dynamic_name(cmd: &SimpleCommand, e: usize, parsed: &Parsed, raw: &str) -> Option<String> {
    let t = cmd.words[e].text.as_str();
    let var = t
        .strip_prefix("${")
        .and_then(|r| r.strip_suffix('}'))
        .or_else(|| t.strip_prefix('$'))
        .filter(|v| v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
    let value = var.and_then(|v| {
        cmd.assigns
            .iter()
            .rev()
            .find(|(n, _)| n == v)
            .map(|(_, x)| x.as_str())
            .or_else(|| parsed.variable(v))
    });
    let flagged = match value {
        Some(v) => {
            let first = v.split_whitespace().next().unwrap_or("");
            LAUNCHERS.contains(&basename(first))
        }
        None => NAMES_LAUNCHER.is_match(raw),
    };
    flagged.then(|| {
        format!("the command name {t} is expanded when the command runs, so what it runs cannot be checked")
    })
}

fn installed(node: bool, start: &Path, name: &str, ctx: &GuardCtx) -> bool {
    let mut d = start.to_path_buf();
    loop {
        let hit = if node {
            ctx.lookup.exists(&d.join("node_modules/.bin").join(name))
                || ctx
                    .lookup
                    .exists(&d.join("node_modules").join(name).join("package.json"))
        } else {
            ctx.lookup.is_executable(&d.join(".venv/bin").join(name))
        };
        if hit {
            return true;
        }
        if d == ctx.root || ctx.lookup.exists(&d.join(".git")) || !d.pop() {
            return false;
        }
    }
}

fn at(cmd: &SimpleCommand, k: usize, ctx: &GuardCtx, out: &mut Vec<Found>) {
    let w = &cmd.words;
    let t = |i: usize| w.get(i).map(|x| x.text.as_str()).unwrap_or("");
    let mut ld = cmd.dir.clone();
    let b = basename(t(k));
    let mut l = Launch {
        w,
        ld: ld.clone(),
        out,
    };
    match b {
        "npx" | "bunx" => return l.launcher(k + 1, b, Kind::Node),
        "pnpx" => return l.launcher(k + 1, b, Kind::Dlx),
        "uvx" => return l.launcher(k + 1, b, Kind::Uv),
        "uv" if t(k + 1) == "tool" && t(k + 2) == "run" => {
            return l.launcher(k + 3, "uv tool run", Kind::Uv);
        }
        "pipx" if t(k + 1) == "run" => return l.launcher(k + 2, "pipx run", Kind::Dlx),
        "deno" => return l.deno(k + 1),
        "npm" | "pnpm" | "yarn" | "bun" => {}
        _ => return,
    }
    let mut j = k + 1;
    while j < w.len() && t(j).starts_with('-') && t(j).len() > 1 {
        let o = t(j);
        let opt = pm_option(b, o);
        if opt == 2 {
            ld = match w.get(j + 1) {
                Some(x) => chdir(&ld, x, ctx.home.as_deref()),
                None => Dir::Unknown,
            };
        } else if let Some(v) = ["--prefix=", "--dir=", "--cwd="]
            .iter()
            .find_map(|p| o.strip_prefix(p))
        {
            ld = chdir(&ld, &Word::plain(v), ctx.home.as_deref());
        }
        if opt > 0 {
            j += 1;
        }
        j += 1;
    }
    l.ld = ld;
    let x = t(j);
    match (b, x) {
        ("npm", "exec" | "x") => l.launcher(j + 1, "npm exec", Kind::Npm),
        ("bun", "x") => l.launcher(j + 1, "bun x", Kind::Node),
        ("pnpm" | "yarn", "dlx") => l.launcher(j + 1, &format!("{b} dlx"), Kind::Dlx),
        (_, "create") | ("npm", "init" | "innit") => l.initializer(j + 1, &format!("{b} {x}"), b),
        _ => {}
    }
}

struct Launch<'a, 'o> {
    w: &'a [Word],
    ld: Dir,
    out: &'o mut Vec<Found>,
}

fn bare_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

impl Launch<'_, '_> {
    fn t(&self, i: usize) -> &str {
        self.w.get(i).map(|x| x.text.as_str()).unwrap_or("")
    }

    fn launcher(&mut self, mut j: usize, name: &str, kind: Kind) {
        const UV_PKG: &[&str] = &[
            "--from",
            "--with",
            "--with-editable",
            "--with-requirements",
            "-w",
            "--index",
            "--index-url",
            "--default-index",
            "--extra-index-url",
            "-i",
            "-f",
            "--find-links",
        ];
        let n = self.w.len();
        let mut pkg: Option<String> = None;
        let mut dd = false;
        while j < n {
            let o = self.t(j).to_owned();
            if o == "--" {
                dd = true;
                j += 1;
                break;
            }
            if !o.starts_with('-') || o == "-" {
                break;
            }
            if kind == Kind::Uv {
                if UV_PKG.contains(&o.as_str()) {
                    pkg = Some(o);
                    j += 1;
                } else if UV_PKG
                    .iter()
                    .any(|p| p.starts_with("--") && o.starts_with(&format!("{p}=")))
                {
                    pkg = Some(o);
                } else if UV_VALUE.contains(&o.as_str()) {
                    j += 1;
                }
            } else if matches!(o.as_str(), "-p" | "--package" | "-c" | "--call") {
                pkg = Some(o);
                j += 1;
            } else if o.starts_with("--package=") || o.starts_with("--call=") {
                pkg = Some(o);
            } else if NODE_LAUNCHER_VALUE.contains(&o.as_str()) {
                j += 1;
            }
            j += 1;
        }
        let cmd = self.t(j).to_owned();
        if kind == Kind::Npm && !dd {
            for o in self.w.iter().skip(j + 1).map(|x| x.text.as_str()) {
                if o == "--" {
                    break;
                }
                let head = o.split('=').next().unwrap_or(o);
                if matches!(head, "-p" | "--package" | "-c" | "--call") {
                    pkg = Some(o.to_owned());
                }
            }
        }
        if kind == Kind::Dlx {
            let what = if cmd.is_empty() { "a package" } else { &cmd };
            self.out.push(Found::Remote(format!(
                "{name} fetches {what} from a registry and runs it"
            )));
            return;
        }
        if let Some(p) = pkg {
            let p = p.split('=').next().unwrap_or(&p).to_owned();
            self.out.push(Found::Remote(format!(
                "{name} {p} fetches a package from a registry and runs it"
            )));
            return;
        }
        if cmd.is_empty() {
            return;
        }
        self.classify(&cmd, name, kind);
    }

    fn classify(&mut self, cmd: &str, name: &str, kind: Kind) {
        if kind == Kind::Uv {
            if bare_name(cmd) {
                self.out.push(Found::Lookup(
                    false,
                    self.ld.clone(),
                    cmd.to_owned(),
                    name.to_owned(),
                ));
            } else {
                self.out.push(Found::Remote(format!(
                    "{name} {cmd} names a version or source that is fetched from a registry"
                )));
            }
            return;
        }
        if cmd.starts_with(['.', '/', '~']) {
            return;
        }
        let scoped = cmd
            .strip_prefix('@')
            .and_then(|r| r.split_once('/'))
            .is_some_and(|(a, b)| bare_name(a) && bare_name(b));
        if scoped || bare_name(cmd) {
            self.out.push(Found::Lookup(
                true,
                self.ld.clone(),
                cmd.to_owned(),
                name.to_owned(),
            ));
        } else {
            self.out.push(Found::Remote(format!(
                "{name} {cmd} names a version, URL or repository that is fetched from a registry"
            )));
        }
    }

    fn initializer(&mut self, mut j: usize, name: &str, pm: &str) {
        let n = self.w.len();
        while j < n {
            let o = self.t(j);
            if o == "--" {
                j += 1;
                break;
            }
            if !(o.starts_with('-') && o.len() > 1) {
                break;
            }
            if matches!(o, "-w" | "--workspace" | "--scope" | "--init-module") {
                j += 1;
            }
            j += 1;
        }
        if j >= n {
            return;
        }
        let mut x = self.t(j).to_owned();
        if pm == "bun" && x.starts_with(['.', '/', '~']) {
            return;
        }
        if pm == "bun"
            && x.split_once('/')
                .is_some_and(|(a, b)| bare_name(a) && bare_name(b))
        {
            self.out.push(Found::Remote(format!(
                "{name} {x} fetches the template from GitHub and runs it"
            )));
            return;
        }
        let mut v = String::new();
        if let Some(i) = x[1.min(x.len())..].find('@').map(|i| i + 1) {
            v = x[i..].to_owned();
            x.truncate(i);
        }
        let pkg = if let Some(scope) = x.strip_prefix('@') {
            match scope.split_once('/') {
                None => format!("@{scope}/create"),
                Some((s, rest)) => format!("@{s}/create-{rest}"),
            }
        } else {
            format!("create-{x}")
        };
        if pm == "pnpm" || pm == "yarn" {
            self.out.push(Found::Remote(format!(
                "{name} fetches {pkg} from a registry and runs it"
            )));
            return;
        }
        self.classify(&format!("{pkg}{v}"), name, Kind::Node);
    }

    fn deno(&mut self, mut j: usize) {
        if !matches!(self.t(j), "run" | "x" | "serve") {
            return;
        }
        let n = self.w.len();
        j += 1;
        while j < n {
            let o = self.t(j);
            if o == "--" {
                j += 1;
                break;
            }
            if !(o.starts_with('-') && o.len() > 1) {
                break;
            }
            if matches!(
                o,
                "-c" | "--config"
                    | "--import-map"
                    | "--lock"
                    | "--cert"
                    | "--location"
                    | "--seed"
                    | "--ext"
                    | "--env-file"
                    | "--inspect"
                    | "--watch"
            ) && !self.t(j + 1).starts_with('-')
            {
                j += 1;
            }
            j += 1;
        }
        let m = self.t(j);
        if j < n
            && ["npm:", "jsr:", "http:", "https:"]
                .iter()
                .any(|p| m.starts_with(p))
        {
            self.out.push(Found::Remote(format!(
                "deno {m} fetches the module from a registry and runs it"
            )));
        }
    }
}
