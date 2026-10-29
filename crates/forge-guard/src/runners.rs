//! Runners: commands that run another command named in their arguments (`npx prisma …`,
//! `uv run alembic …`, `python manage.py …`). The word where that command starts is a command
//! position too, so `bunx prisma migrate reset` is matched as `prisma migrate reset`.

use crate::parse::{Word, basename};

/// Options of a Node launcher that take the next word as their value.
pub const NODE_LAUNCHER_VALUE: &[&str] = &[
    "-p",
    "--package",
    "-c",
    "--call",
    "-w",
    "--workspace",
    "--prefix",
    "--registry",
    "--cache",
    "--userconfig",
    "--allow-scripts",
    "--loglevel",
];

/// Options of `uv run` / `uvx` / `uv tool run` that take the next word as their value.
pub const UV_VALUE: &[&str] = &[
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
    "-p",
    "--python",
    "-C",
    "--config-setting",
    "--directory",
    "--project",
    "--cache-dir",
    "--config-file",
    "--color",
    "--resolution",
    "--prerelease",
    "--exclude-newer",
    "--index-strategy",
    "--keyring-provider",
    "--link-mode",
    "--constraints",
    "-c",
    "--overrides",
    "--build-constraints",
    "--env-file",
    "--python-preference",
    "--extra",
    "--group",
    "--only-group",
    "--no-group",
    "--package",
    "--script",
];

/// A package manager's options before its subcommand: 1 takes a value, 2 takes a directory.
pub fn pm_option(pm: &str, o: &str) -> u8 {
    match pm {
        "npm" => {
            if o == "--prefix" {
                2
            } else {
                u8::from(matches!(
                    o,
                    "-w" | "--workspace"
                        | "--registry"
                        | "--cache"
                        | "--userconfig"
                        | "--globalconfig"
                        | "--loglevel"
                ))
            }
        }
        "pnpm" => {
            if o == "-C" || o == "--dir" {
                2
            } else {
                u8::from(matches!(
                    o,
                    "-F" | "--filter" | "--reporter" | "--loglevel" | "--workspace-concurrency"
                ))
            }
        }
        "yarn" => {
            if o == "--cwd" {
                2
            } else {
                0
            }
        }
        "bun" => {
            if o == "--cwd" {
                2
            } else {
                u8::from(matches!(
                    o,
                    "-c" | "--config" | "--env-file" | "-r" | "--preload"
                ))
            }
        }
        _ => 0,
    }
}

/// pnpm / yarn subcommands: anything else is a binary of the project (`pnpm turbo build`).
const PNPM_YARN_BUILTINS: &[&str] = &[
    "add",
    "install",
    "i",
    "remove",
    "rm",
    "uninstall",
    "un",
    "update",
    "up",
    "upgrade",
    "run",
    "test",
    "t",
    "start",
    "exec",
    "dlx",
    "create",
    "init",
    "list",
    "ls",
    "why",
    "outdated",
    "audit",
    "publish",
    "pack",
    "link",
    "unlink",
    "store",
    "config",
    "set",
    "get",
    "info",
    "view",
    "import",
    "rebuild",
    "prune",
    "patch",
    "patch-commit",
    "deploy",
    "fetch",
    "env",
    "setup",
    "global",
    "workspaces",
    "workspace",
    "cache",
    "dedupe",
    "licenses",
    "bin",
    "root",
    "server",
    "approve-builds",
    "self-update",
    "node",
    "help",
    "version",
    "versions",
    "login",
    "logout",
    "owner",
    "tag",
    "team",
    "npm",
    "constraints",
    "plugin",
    "explain",
    "search",
    "stage",
    "unplug",
    "recursive",
];

/// The index of the command a runner at `e` runs, if `words[e]` is a runner.
pub fn inner(words: &[Word], e: usize) -> Option<usize> {
    let w = |i: usize| words.get(i).map(|w| w.text.as_str());
    let b = basename(w(e)?);
    match b {
        "npx" | "bunx" | "pnpx" => skip_options(words, e + 1, NODE_LAUNCHER_VALUE),
        "uvx" => skip_options(words, e + 1, UV_VALUE),
        "uv" => match (w(e + 1), w(e + 2)) {
            (Some("run"), _) => skip_options(words, e + 2, UV_VALUE),
            (Some("tool"), Some("run")) => skip_options(words, e + 3, UV_VALUE),
            _ => None,
        },
        "pipx" if w(e + 1) == Some("run") => skip_options(words, e + 2, &["--spec", "--python"]),
        "poetry" | "pdm" | "pipenv" | "hatch" | "rye" | "pixi" | "conda" | "mamba"
        | "micromamba"
            if w(e + 1) == Some("run") =>
        {
            skip_options(
                words,
                e + 2,
                &["-n", "--name", "-p", "--prefix", "-e", "--environment"],
            )
        }
        "npm" | "pnpm" | "yarn" | "bun" => {
            let mut j = e + 1;
            while let Some(o) = w(j) {
                if !(o.starts_with('-') && o.len() > 1) {
                    break;
                }
                j += if pm_option(b, o) > 0 { 2 } else { 1 };
            }
            let sub = w(j)?;
            match (b, sub) {
                ("npm", "exec" | "x") | ("bun", "x") | ("pnpm", "exec" | "dlx") => {
                    skip_options(words, j + 1, NODE_LAUNCHER_VALUE)
                }
                ("yarn", "exec" | "dlx") => skip_options(words, j + 1, NODE_LAUNCHER_VALUE),
                // a script or a binary of the project (`bun run vitest -u`)
                (_, "run" | "run-script") => skip_options(words, j + 1, &[]),
                ("pnpm" | "yarn", s) if !PNPM_YARN_BUILTINS.contains(&s) => Some(j),
                _ => None,
            }
        }
        _ if is_python(b) => {
            let mut j = e + 1;
            while let Some(o) = w(j) {
                match o {
                    "-m" => return words.get(j + 1).map(|_| j + 1),
                    "-c" | "-" => return None,
                    "-W" | "-X" | "--check-hash-based-pycs" => j += 2,
                    _ if o.starts_with('-') && o.len() > 1 => {
                        // -mpip, -c…
                        if o.starts_with("-c") {
                            return None;
                        }
                        j += 1
                    }
                    _ => return Some(j),
                }
            }
            None
        }
        _ => None,
    }
}

/// `python`, `python3`, `python3.12`, `pypy3`.
pub fn is_python(b: &str) -> bool {
    let rest = b.strip_prefix("python").or_else(|| b.strip_prefix("pypy"));
    rest.is_some_and(|r| r.chars().all(|c| c.is_ascii_digit() || c == '.'))
}

/// The first word from `j` that is not an option (or an option's value); `--` ends options.
fn skip_options(words: &[Word], mut j: usize, with_value: &[&str]) -> Option<usize> {
    while let Some(o) = words.get(j).map(|w| w.text.as_str()) {
        if o == "--" {
            return words.get(j + 1).map(|_| j + 1);
        }
        if !(o.starts_with('-') && o.len() > 1) {
            return Some(j);
        }
        j += if with_value.contains(&o) { 2 } else { 1 };
    }
    None
}
