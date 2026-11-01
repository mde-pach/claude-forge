//! `package-add` and `python-pm`: commands that install packages.
//!
//! An install is read from a command position: `npm|pnpm|yarn|bun install|i|add`, `yarn|pnpm
//! global add`, `pip|pip3|pipX.Y install`, `python -m pip install`, `uv pip install`,
//! `uv|poetry|pdm add`, `pipenv install`, `pipx install`, `uv tool install` (through wrappers
//! and runners: `sudo pip install`, `uv run pip install`). Its packages are its positionals
//! that are not local paths (`.`, `./pkg`, `/x`, `~/x`, `file:`, `link:`); requirement and
//! constraint files (`-r`, `-c`) and editable installs (`-e`) name no package. `--dry-run`
//! installs nothing. Read-only subcommands (`pip show|list|freeze|download|index|inspect`,
//! `npm ls|view|outdated`, `uv pip list|tree|show|freeze`, `uv tree`…) are not installs.
//!
//! - `package-add` flags an install that names a package.
//! - `python-pm` holds pip (`pip install`, `python -m pip install`, `uv pip install`) into the
//!   project's environment to the project's way of declaring dependencies (`python.pm`). Where
//!   the packages are uv's (`uv`), every such install is refused: it goes through `uv add` or
//!   `uv sync`. Where a `pyproject.toml` declares the project and no manager has left its mark
//!   (`pyproject`), or nothing declares dependencies yet (`scripts`), an install that names a
//!   package is refused (the package is declared first); the project's own installs pass
//!   (`pip install -e .`, `pip install -r requirements-dev.txt`). An install aimed at another
//!   environment is not the project's and passes: `--target`, `--prefix`, `--root`,
//!   `--system`, `--python <path>` / `-p <path>` outside the repository, a pip or python run by
//!   a path outside the repository (`/tmp/venv/bin/pip install x`), `VIRTUAL_ENV=` outside the
//!   repository, or a command run from outside the repository. (`package-add` still asks
//!   about those.)
//! - `js-pm` flags an install with a JavaScript package manager that is not the project's
//!   (`js.pm`): `npm install|i|ci|add`, `pnpm add|install|i|update|up|remove|rm`, `yarn` alone (or
//!   with `--immutable` / `--frozen-lockfile`) and `yarn add|install|up|upgrade|remove`, `bun
//!   install|i|add|a`. Global installs, and commands run from outside the repository, pass.

use std::path::{Path, PathBuf};

use crate::args::Args;
use crate::ctx::{GuardCtx, JsPm, PythonPm};
use crate::parse::{Dir, Parsed, SimpleCommand, basename, normalize};
use crate::runners::{UV_VALUE, is_python, pm_option};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Node,
    /// pip, pip3, python -m pip.
    Pip,
    UvPip,
    /// uv add, poetry add, pdm add, pipenv install.
    ProjectAdd,
    /// pipx install, uv tool install.
    ToolInstall,
}

#[derive(Debug, Clone)]
pub struct Install<'a> {
    pub tool: Tool,
    /// `pip install`, `uv add`…: what the command is.
    pub what: String,
    pub packages: Vec<&'a str>,
    pub requirements: Vec<&'a str>,
    /// `-e`, or a local path.
    pub local: bool,
    pub dry_run: bool,
    /// Aimed at an environment that is not the project's.
    pub elsewhere: bool,
}

const PIP_VALUE: &[&str] = &[
    "-r",
    "--requirement",
    "-c",
    "--constraint",
    "-e",
    "--editable",
    "-i",
    "--index-url",
    "--extra-index-url",
    "-f",
    "--find-links",
    "-t",
    "--target",
    "--prefix",
    "--root",
    "--python",
    "-p",
    "--src",
    "--upgrade-strategy",
    "--platform",
    "--python-version",
    "--implementation",
    "--abi",
    "--only-binary",
    "--no-binary",
    "--progress-bar",
    "--log",
    "--cache-dir",
    "--trusted-host",
    "--proxy",
    "--retries",
    "--timeout",
    "--exists-action",
    "--cert",
    "--client-cert",
    "--global-option",
    "--config-settings",
    "-C",
    "--report",
    "--index-strategy",
    "--keyring-provider",
    "--resolution",
    "--prerelease",
    "--exclude-newer",
    "--link-mode",
    "--override",
    "--overrides",
    "--constraints",
    "--build-constraints",
    "--extra",
    "--group",
    "--default-index",
    "--index",
    "--python-platform",
    "--torch-backend",
    "--directory",
    "--project",
];

const NODE_INSTALL_VALUE: &[&str] = &[
    "-w",
    "--workspace",
    "--prefix",
    "--registry",
    "--save-prefix",
    "--tag",
    "--omit",
    "--include",
    "--min-release-age",
    "--cache",
    "-F",
    "--filter",
    "-C",
    "--dir",
    "--cwd",
    "--reporter",
    "--network-concurrency",
    "--allow-scripts",
    "--install-strategy",
];

const ADD_VALUE: &[&str] = &[
    "--group",
    "-G",
    "--optional",
    "--extra",
    "--package",
    "--index",
    "--rev",
    "--tag",
    "--branch",
    "--source",
    "--python",
    "-p",
    "--directory",
    "--project",
    "-r",
    "--requirements",
    "--constraints",
    "-c",
    "--marker",
    "-m",
    "--bounds",
    "--categories",
    "--suffix",
    "--pip-args",
    "--index-url",
];

fn is_pip(b: &str) -> bool {
    b.strip_prefix("pip")
        .is_some_and(|r| r.chars().all(|c| c.is_ascii_digit() || c == '.'))
}

fn is_local(a: &str) -> bool {
    a.starts_with(['.', '/', '~']) || a.starts_with("file:") || a.starts_with("link:")
}

/// The install at command position `p`, if there is one.
pub fn install_at<'a>(cmd: &'a SimpleCommand, p: usize, ctx: &GuardCtx) -> Option<Install<'a>> {
    let w = &cmd.words;
    let t = |i: usize| w.get(i).map(|x| x.text.as_str()).unwrap_or("");
    let b = basename(t(p));
    let (tool, what, from, table): (Tool, String, usize, &[&str]) = if is_pip(b) {
        (t(p + 1) == "install").then_some(())?;
        (Tool::Pip, format!("{b} install"), p + 2, PIP_VALUE)
    } else if is_python(b) && t(p + 1) == "-m" && is_pip(t(p + 2)) {
        (t(p + 3) == "install").then_some(())?;
        (Tool::Pip, format!("{b} -m pip install"), p + 4, PIP_VALUE)
    } else if b == "uv" && t(p + 1) == "pip" {
        (t(p + 2) == "install").then_some(())?;
        (Tool::UvPip, "uv pip install".into(), p + 3, PIP_VALUE)
    } else if b == "uv" && t(p + 1) == "tool" && t(p + 2) == "install" {
        (Tool::ToolInstall, "uv tool install".into(), p + 3, UV_VALUE)
    } else if matches!(b, "uv" | "poetry" | "pdm") && t(p + 1) == "add" {
        (Tool::ProjectAdd, format!("{b} add"), p + 2, ADD_VALUE)
    } else if b == "pipenv" && t(p + 1) == "install" {
        (Tool::ProjectAdd, "pipenv install".into(), p + 2, ADD_VALUE)
    } else if b == "pipx" && t(p + 1) == "install" {
        (Tool::ToolInstall, "pipx install".into(), p + 2, ADD_VALUE)
    } else if matches!(b, "npm" | "pnpm" | "yarn" | "bun") {
        let mut j = p + 1;
        while t(j).starts_with('-') && t(j).len() > 1 {
            j += if pm_option(b, t(j)) > 0 { 2 } else { 1 };
        }
        let mut sub = t(j);
        if matches!(b, "yarn" | "pnpm") && sub == "global" {
            j += 1;
            sub = t(j);
            (sub == "add").then_some(())?;
        }
        let ok = match b {
            "npm" => matches!(
                sub,
                "install"
                    | "i"
                    | "add"
                    | "in"
                    | "ins"
                    | "inst"
                    | "insta"
                    | "instal"
                    | "isnt"
                    | "isnta"
                    | "isntal"
                    | "isntall"
            ),
            "bun" => matches!(sub, "install" | "i" | "add" | "a"),
            _ => matches!(sub, "install" | "i" | "add"),
        };
        ok.then_some(())?;
        (Tool::Node, format!("{b} {sub}"), j + 1, NODE_INSTALL_VALUE)
    } else {
        return None;
    };
    let extra: Vec<String> = table.iter().map(|s| s.to_string()).collect();
    let args = Args::read("", &w[from.min(w.len())..], &extra);
    let mut inst = Install {
        tool,
        what,
        packages: vec![],
        requirements: vec![],
        local: false,
        dry_run: args.options.iter().any(|(o, _)| *o == "--dry-run"),
        elsewhere: false,
    };
    for &(o, v) in &args.options {
        match (o, v) {
            ("-r" | "--requirement" | "--requirements", Some(v)) => inst.requirements.push(v),
            ("-e" | "--editable", _) => inst.local = true,
            ("--with", Some(v)) if tool == Tool::ToolInstall => inst.packages.push(v),
            ("--target" | "-t" | "--prefix" | "--root" | "--system", _) => inst.elsewhere = true,
            ("--python" | "-p", Some(v)) if v.contains('/') => {
                if !inside(ctx, &cmd.dir, v) {
                    inst.elsewhere = true;
                }
            }
            _ => {}
        }
    }
    for a in &args.positionals {
        if is_local(a) {
            inst.local = true;
        } else {
            inst.packages.push(a);
        }
    }
    // the tool itself run by a path outside the repository (a venv elsewhere)
    let run_by = if is_python(b) || is_pip(b) { t(p) } else { "" };
    if run_by.contains('/') && !inside(ctx, &cmd.dir, run_by) {
        inst.elsewhere = true;
    }
    if let Some((_, v)) = cmd.assigns.iter().find(|(n, _)| n == "VIRTUAL_ENV")
        && !inside(ctx, &cmd.dir, v)
    {
        inst.elsewhere = true;
    }
    if let Dir::Known(d) = &cmd.dir
        && !ctx.in_repo(d)
    {
        inst.elsewhere = true;
    }
    Some(inst)
}

/// Whether `path`, from `dir`, is inside the repository (unknown: assumed inside).
fn inside(ctx: &GuardCtx, dir: &Dir, path: &str) -> bool {
    let p = if let Some(rest) = path.strip_prefix("~/") {
        match &ctx.home {
            Some(h) => h.join(rest),
            None => return true,
        }
    } else if path.starts_with('/') {
        PathBuf::from(path)
    } else if path.contains('$') {
        return true;
    } else {
        match dir {
            Dir::Known(d) => d.join(path),
            Dir::Unknown => return true,
        }
    };
    normalize(Path::new(&p)).starts_with(&ctx.root)
}

fn installs<'a>(parsed: &'a Parsed, ctx: &'a GuardCtx) -> impl Iterator<Item = Install<'a>> {
    parsed
        .commands
        .iter()
        .flat_map(move |c| c.starts.iter().filter_map(move |&p| install_at(c, p, ctx)))
}

fn list(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [a] => (*a).to_owned(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// `package-add`: the first install that adds a named package.
pub fn package_add(parsed: &Parsed, ctx: &GuardCtx) -> Option<String> {
    installs(parsed, ctx)
        .find(|i| !i.dry_run && !i.packages.is_empty())
        .map(|i| format!("it adds {}", list(&i.packages)))
}

/// `python-pm`: the reason and the detail for pip into the project's environment.
pub fn python_pm(parsed: &Parsed, ctx: &GuardCtx) -> Option<(String, String)> {
    let pm = ctx.python_pm?;
    if !matches!(pm, PythonPm::Uv | PythonPm::Pyproject | PythonPm::Scripts) {
        return None;
    }
    let i = installs(parsed, ctx).find(|i| {
        matches!(i.tool, Tool::Pip | Tool::UvPip)
            && !i.dry_run
            && !i.elsewhere
            && if pm == PythonPm::Uv {
                !i.packages.is_empty() || !i.requirements.is_empty() || i.local
            } else {
                !i.packages.is_empty()
            }
    })?;
    let names: Vec<String> = i.packages.iter().map(|p| shell_quote(p)).collect();
    let mut add = String::from("uv add");
    for r in &i.requirements {
        add.push_str(" -r ");
        add.push_str(r);
    }
    for p in &names {
        add.push(' ');
        add.push_str(p);
    }
    let only_local = i.packages.is_empty() && i.requirements.is_empty();
    let quoted = |f: &dyn Fn(&str) -> String| -> String {
        let all: Vec<String> = i.packages.iter().map(|p| format!("`{}`", f(p))).collect();
        list(&all.iter().map(String::as_str).collect::<Vec<_>>())
    };
    Some(match pm {
        PythonPm::Uv => (
            "This project uses uv: `uv add <pkg>` (`--dev` for tooling).".into(),
            if only_local {
                "`uv sync` installs the project itself".into()
            } else {
                format!("`{add}`")
            },
        ),
        PythonPm::Pyproject => (
            "This project declares its dependencies in pyproject.toml: add the package to `[project] dependencies` there (tooling: `[dependency-groups]`), then install the project.".into(),
            format!(
                "add {} to [project] dependencies in pyproject.toml",
                quoted(&|p| p.to_string())
            ),
        ),
        _ => (
            "This project declares no dependencies yet; declare the package first: add `<pkg>==<version>` to requirements.txt, then `pip install -r requirements.txt`; or create the project with `uv init --bare`, then `uv add <pkg>`.".into(),
            format!(
                "add {} to requirements.txt, then `pip install -r requirements.txt`; or `uv init --bare && uv add {}`",
                quoted(&|p| if p.contains("==") {
                    p.to_string()
                } else {
                    let name = p.split(['<', '>', '=', '!', '~', ';']).next().unwrap_or(p);
                    format!("{}==<version>", name.trim())
                }),
                names.join(" ")
            ),
        ),
    })
}

/// `js-pm`: the reason and the detail for an install with another JavaScript package manager.
pub fn js_pm(parsed: &Parsed, ctx: &GuardCtx) -> Option<(String, String)> {
    let pm = match ctx.js_pm? {
        JsPm::Npm => "npm",
        JsPm::Pnpm => "pnpm",
        JsPm::Yarn => "yarn",
        JsPm::Bun => "bun",
    };
    let (other, what) = parsed.commands.iter().find_map(|c| {
        if let Dir::Known(d) = &c.dir
            && !ctx.in_repo(d)
        {
            return None;
        }
        c.starts.iter().find_map(|&p| {
            let w = &c.words;
            let t = |i: usize| w.get(i).map(|x| x.text.as_str()).unwrap_or("");
            let b = basename(t(p));
            if !matches!(b, "npm" | "pnpm" | "yarn" | "bun") || b == pm {
                return None;
            }
            let mut j = p + 1;
            let mut flags = Vec::new();
            while t(j).starts_with('-') && t(j).len() > 1 {
                flags.push(t(j));
                j += if pm_option(b, t(j)) > 0 { 2 } else { 1 };
            }
            let sub = t(j);
            let installs = match b {
                "npm" => matches!(
                    sub,
                    "install"
                        | "i"
                        | "ci"
                        | "add"
                        | "in"
                        | "ins"
                        | "inst"
                        | "insta"
                        | "instal"
                        | "isnt"
                        | "isnta"
                        | "isntal"
                        | "isntall"
                ),
                "pnpm" => matches!(
                    sub,
                    "add" | "install" | "i" | "update" | "up" | "remove" | "rm"
                ),
                "yarn" => {
                    matches!(sub, "add" | "install" | "up" | "upgrade" | "remove")
                        || (j >= w.len()
                            && flags
                                .iter()
                                .all(|f| matches!(*f, "--immutable" | "--frozen-lockfile")))
                }
                _ => matches!(sub, "install" | "i" | "add" | "a"),
            };
            // a global install is not the project's
            let global = flags.iter().any(|f| matches!(*f, "-g" | "--global"))
                || w.iter()
                    .skip(j + 1)
                    .any(|x| matches!(x.text.as_str(), "-g" | "--global"));
            let installs = installs && !global;
            let shown = if sub.is_empty() {
                b.to_string()
            } else {
                format!("{b} {sub}")
            };
            installs.then_some((b, shown))
        })
    })?;
    let use_ = match pm {
        "npm" => "`npm install` (`npm install <pkg>` to add one)",
        "pnpm" => "`pnpm install` (`pnpm add <pkg>` to add one)",
        "yarn" => "`yarn install` (`yarn add <pkg>` to add one)",
        _ => "`bun install` (`bun add <pkg>` to add one)",
    };
    Some((
        format!(
            "This project's packages are managed with {pm}: {use_}; installing with {other} would write another lock file."
        ),
        format!("`{what}` installs with {other}"),
    ))
}

fn shell_quote(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_.=@/:+,[]".contains(c))
    {
        s.to_owned()
    } else {
        format!("'{s}'")
    }
}
