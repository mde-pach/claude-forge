//! The setup `forge apply` offers a project (§3.5): what the active rules expect of the project
//! and it lacks, each as one [`Offer`]: the edits it makes (shown as a diff) and the commands it
//! runs (`uv lock`, `uv add --dev …`), asked once (`--yes` accepts). A value the project set is
//! never changed. Declined, or without a terminal and without `--yes`, it is left for
//! `forge status` to report ([`pending`]). The setup is the person's: the agent is never sent to
//! add it (a change is held only to what it broke, §10.5); once there, the project's own tools
//! enforce it and the rules keep the agent from turning it off.

use std::collections::BTreeSet;
use std::io::IsTerminal;
use std::path::Path;

use forge_builtins_data::python::with_uv_cooldown;
use forge_builtins_data::test_setup::{self, NO_NETWORK, NO_NETWORK_FILE, PytestSetup, Setting};
use forge_packs::model::Level;

use super::io::{Error, confirm, show_diff, write_text};
use super::{Ctx, Flags};

/// What one offer does, in order; a command that fails stops it there.
pub enum Step {
    /// A file written: `added` says what for the person (empty: the file is created).
    Write {
        file: String,
        before: Option<String>,
        after: String,
        added: String,
    },
    /// A command run in the project; it says what it did, that there was nothing to do, or why
    /// it failed.
    Run(Box<dyn Fn(&Path) -> Ran>),
}

pub enum Ran {
    Done(String),
    Nothing,
    Failed(String),
}

pub struct Offer {
    /// Whose setup (`uv`, `npm`, `pytest`, `the test runner`).
    pub name: String,
    /// What it does, as the question and `forge status` say it.
    pub what: String,
    /// Why, in the question: what fails loudly once it is there.
    pub why: Option<String>,
    pub steps: Vec<Step>,
}

/// The offers for this project, in the order apply asks them.
pub fn offers(ctx: &Ctx) -> Vec<Offer> {
    if ctx.level != Level::Project {
        return Vec::new();
    }
    [uv(ctx), js_pm(ctx), pytest(ctx), js_tests(ctx)]
        .into_iter()
        .flatten()
        .chain(testcontainers(ctx))
        .chain(dependabot(ctx))
        .chain(dockerignore(ctx))
        .chain(bun_text_lock(ctx))
        .collect()
}

/// What `forge status` reports: each offer still open, `<name>: <what>`.
pub fn pending(ctx: &Ctx) -> Vec<String> {
    offers(ctx)
        .into_iter()
        .map(|o| format!("{}: {}", o.name, o.what))
        .collect()
}

/// Asks each offer and makes the accepted ones.
pub fn ask(ctx: &Ctx, flags: &Flags) -> Result<(), Error> {
    for o in offers(ctx) {
        make(ctx, flags, o)?;
    }
    Ok(())
}

fn make(ctx: &Ctx, flags: &Flags, o: Offer) -> Result<(), Error> {
    if !flags.yes && !std::io::stdin().is_terminal() {
        println!(
            "forge: the project's {} setup is incomplete; re-run with --yes to {}",
            o.name, o.what
        );
        return Ok(());
    }
    for s in &o.steps {
        if let Step::Write {
            file,
            before,
            after,
            ..
        } = s
        {
            show_diff(before.as_deref(), Some(after), file);
        }
    }
    let question = match &o.why {
        Some(w) => format!("{} ({w})?", o.what),
        None => format!("{}?", o.what),
    };
    if !confirm(&question, flags.yes)? {
        return Ok(());
    }
    let mut done = Vec::new();
    for s in &o.steps {
        match s {
            Step::Write {
                file, after, added, ..
            } => {
                write_text(&ctx.base.join(file), after, false)?;
                done.push(if added.is_empty() {
                    format!("{file} created")
                } else {
                    format!("{file}: added {added}")
                });
            }
            Step::Run(run) => match run(&ctx.base) {
                Ran::Done(d) => done.push(d),
                Ran::Nothing => {}
                Ran::Failed(why) => {
                    println!("forge: {why}");
                    break;
                }
            },
        }
    }
    if !done.is_empty() {
        println!("forge: {}; commit them with Forge's files", done.join(", "));
    }
    Ok(())
}

fn active(ctx: &Ctx, id: &str) -> bool {
    ctx.plan.active.iter().any(|st| st.rule.id == id)
}

fn pack(ctx: &Ctx, id: &str) -> bool {
    ctx.plan.packs.iter().any(|p| p.id == id)
}

fn read(ctx: &Ctx, f: &str) -> Option<String> {
    std::fs::read_to_string(ctx.base.join(f)).ok()
}

/// A command run in the project; its last stderr line on failure.
fn command(argv: &[String], dir: &Path) -> Result<(), String> {
    let shown = argv.join(" ");
    match std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .output()
    {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr);
            let why = err
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("")
                .trim()
                .to_string();
            Err(format!("`{shown}` failed ({why})"))
        }
        Err(e) => Err(format!("`{shown}` could not run ({e})")),
    }
}

/// A list for the person: `a`, `a and b`, `a, b and c`.
fn and(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [a] => a.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// uv's cooldown in `pyproject.toml` (`python/uv/exclude-newer`), and a `uv.lock` matching it
/// (`python/uv/lock-sync`), which `uv lock` creates or updates after the cooldown changed what
/// resolves. Not where the project keeps uv's settings in `uv.toml`.
fn uv(ctx: &Ctx) -> Option<Offer> {
    if ctx.base.join("uv.toml").is_file() {
        return None;
    }
    let before = read(ctx, "pyproject.toml")?;
    let cooldown = active(ctx, "python/uv/exclude-newer")
        .then(|| with_uv_cooldown(&before))
        .flatten();
    let no_lock = !ctx.base.join("uv.lock").is_file();
    let lock = active(ctx, "python/uv/lock-sync") && (no_lock || cooldown.is_some());
    if cooldown.is_none() && !lock {
        return None;
    }
    let what = match (&cooldown, lock, no_lock) {
        (Some(_), true, true) => {
            "add uv's cooldown to pyproject.toml and create uv.lock with `uv lock`"
        }
        (Some(_), true, false) => {
            "add uv's cooldown to pyproject.toml and update uv.lock with `uv lock`"
        }
        (Some(_), false, _) => "add uv's cooldown to pyproject.toml",
        (None, _, _) => "create uv.lock with `uv lock`",
    };
    let why = cooldown
        .is_some()
        .then(|| "uv then resolves only versions published at least 7 days ago".to_string());
    let mut steps = Vec::new();
    if let Some(after) = cooldown {
        steps.push(Step::Write {
            file: "pyproject.toml".into(),
            before: Some(before),
            after,
            added: "[tool.uv] exclude-newer and required-version".into(),
        });
    }
    if lock {
        steps.push(Step::Run(Box::new(move |dir| {
            match command(&["uv".into(), "lock".into(), "--quiet".into()], dir) {
                Ok(()) => Ran::Done(format!(
                    "uv.lock {}",
                    if no_lock { "created" } else { "updated" }
                )),
                Err(e) => Ran::Failed(format!("{e}; run it yourself once pyproject.toml resolves")),
            }
        })));
    }
    Some(Offer {
        name: "uv".into(),
        what: what.into(),
        why,
        steps,
    })
}

/// The JavaScript package manager's setup, as [`uv`]'s: the cooldown settings
/// (`typescript/<pm>/release-age`, [`forge_builtins_code::cooldown::setup`]), and a lock
/// (`typescript/<pm>/lock-sync`): created when the project has none, and updated when the
/// cooldown changed package.json and the lock no longer matches it.
fn js_pm(ctx: &Ctx) -> Option<Offer> {
    use forge_check::builtins::{AdapterTools, BuiltinTools, ToolRun};
    let pm = ["npm", "pnpm", "yarn", "bun"].into_iter().find(|pm| {
        active(ctx, &format!("typescript/{pm}/release-age"))
            || active(ctx, &format!("typescript/{pm}/lock-sync"))
    })?;
    let lock = forge_builtins_data::lock::js(pm)?;
    let lock_sync = active(ctx, &format!("typescript/{pm}/lock-sync"));
    let edits = if active(ctx, &format!("typescript/{pm}/release-age")) {
        forge_builtins_code::cooldown::setup(pm, &|f| read(ctx, f))
    } else {
        Vec::new()
    };
    let has_lock = lock.files.iter().any(|f| ctx.base.join(f).is_file());
    let create = lock_sync && !has_lock && ctx.base.join("package.json").is_file();
    if edits.is_empty() && !create {
        return None;
    }
    let files: Vec<&str> = edits.iter().map(|e| e.file).collect();
    let cooldown = format!("add {pm}'s cooldown to {}", files.join(" and "));
    let what = match (edits.is_empty(), create) {
        (false, true) => format!(
            "{cooldown} and create {} with `{}`",
            lock.default, lock.command
        ),
        (false, false) => cooldown,
        (true, _) => format!("create {} with `{}`", lock.default, lock.command),
    };
    let why = (!edits.is_empty())
        .then(|| format!("{pm} then installs only versions published at least 7 days ago"));
    // a lock the cooldown's package.json change left behind is updated too
    let may_be_stale = has_lock && files.contains(&"package.json") && lock_sync;
    let mut steps: Vec<Step> = edits
        .into_iter()
        .map(|e| Step::Write {
            file: e.file.into(),
            before: read(ctx, e.file),
            after: e.text,
            added: e.added,
        })
        .collect();
    if create || may_be_stale {
        steps.push(Step::Run(Box::new(move |base| {
            let tools = AdapterTools::new(
                crate::provisioning::builtin_tools(),
                forge_tools::lock_pins(),
            );
            if !create
                && !matches!(tools.run(base, base, pm, "lock-check", &[]), ToolRun::Ran(o) if o.status != 0)
            {
                return Ran::Nothing;
            }
            match tools.run(base, base, pm, "lock", &[]) {
                ToolRun::Ran(o) if o.status == 0 => Ran::Done(format!(
                    "{} {}",
                    lock.files
                        .iter()
                        .find(|f| base.join(f).is_file())
                        .unwrap_or(&lock.default),
                    if create { "created" } else { "updated" }
                )),
                ToolRun::Ran(o) => {
                    let why = o
                        .stderr
                        .lines()
                        .rev()
                        .find(|l| !l.trim().is_empty())
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    Ran::Failed(format!(
                        "`{}` failed ({why}); run it yourself once package.json resolves",
                        lock.command
                    ))
                }
                ToolRun::Missing(why) | ToolRun::Unsupported(why) => Ran::Failed(format!(
                    "`{}` could not run ({why}); run it yourself",
                    lock.command
                )),
            }
        })));
    }
    Some(Offer {
        name: pm.into(),
        what,
        why,
        steps,
    })
}

/// The Python dependencies the project declares (normalised names): pyproject.toml's tables and
/// the root requirements files.
fn python_deps(ctx: &Ctx) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if let Some(v) =
        read(ctx, "pyproject.toml").and_then(|t| forge_detect::deps::parse_toml(&t).ok())
    {
        out.extend(forge_detect::deps::pyproject_deps(&v).into_keys());
    }
    if let Ok(rd) = std::fs::read_dir(&ctx.base) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with("requirements")
                && n.ends_with(".txt")
                && let Ok(t) = std::fs::read_to_string(e.path())
            {
                out.extend(forge_detect::deps::requirements_deps(&t).into_keys());
            }
        }
    }
    out
}

/// What the tests connect to on this machine: a datastore or a local server. pytest-socket then
/// allows connections to this machine (`--allow-hosts`), so the offer does not break such a suite.
const LOCAL_SERVICES: &[&str] = &[
    "psycopg",
    "psycopg2",
    "psycopg2-binary",
    "psycopg-binary",
    "asyncpg",
    "aiopg",
    "pymysql",
    "mysqlclient",
    "aiomysql",
    "asyncmy",
    "redis",
    "aioredis",
    "valkey",
    "pymongo",
    "motor",
    "beanie",
    "elasticsearch",
    "opensearch-py",
    "cassandra-driver",
    "pika",
    "aio-pika",
    "kafka-python",
    "aiokafka",
    "confluent-kafka",
    "nats-py",
    "celery",
    "django",
    "pytest-django",
    "pytest-postgresql",
    "pytest-mysql",
    "pytest-redis",
    "pytest-mongodb",
    "testcontainers",
    "pytest-docker",
    "pytest-httpserver",
    "pytest-localserver",
    "pytest-xprocess",
    "uvicorn",
    "minio",
    "moto",
];

/// The filterwarnings entries the active `python/pytest/*-deprecations-error` rules expect.
const DEPRECATIONS: &[(&str, &str)] = &[
    (
        "python/pytest/django-deprecations-error",
        "error::django.utils.deprecation.RemovedInNextVersionWarning",
    ),
    (
        "python/pytest/drf-deprecations-error",
        "error::rest_framework.deprecation.RemovedInNextDRFVersionWarning",
    ),
    (
        "python/pytest/pydantic-deprecations-error",
        "error::pydantic.warnings.PydanticDeprecationWarning",
    ),
];

/// pytest's setup where the pytest pack is active: the plugins that make hidden problems fail
/// (pytest-socket: a test that reaches the network; pytest-randomly: one that depends on the
/// order; pytest-timeout: one that hangs, with its traceback), added as dev dependencies with
/// the project's package manager (uv, Poetry, PDM, Pipenv; with none, only the settings of the
/// plugins it already has), their settings, strict markers and settings, strict xfail, and the
/// deprecation filters the active rules expect. A plugin the settings switch off
/// (`-p no:<plugin>`) is the project's choice, as is a setting it has.
fn pytest(ctx: &Ctx) -> Option<Offer> {
    if !pack(ctx, "python/pytest") {
        return None;
    }
    let cfg = test_setup::pytest_config(&|f| read(ctx, f))?;
    let deps = python_deps(ctx);
    let add_cmd: Option<Vec<&str>> = if pack(ctx, "python/uv") {
        Some(vec!["uv", "add", "--dev"])
    } else if pack(ctx, "python/poetry") {
        Some(vec!["poetry", "add", "--group", "dev"])
    } else if pack(ctx, "python/pdm") {
        Some(vec!["pdm", "add", "-dG", "dev"])
    } else if pack(ctx, "python/pipenv") {
        Some(vec!["pipenv", "install", "--dev"])
    } else {
        None
    };
    let mut add: Vec<String> = Vec::new();
    let mut want = PytestSetup::default();
    let mut plugin = |name: &str, plugin: &str, settings_unset: bool| -> bool {
        let have = deps.contains(name);
        if cfg.disables(plugin) || !settings_unset || !(have || add_cmd.is_some()) {
            return false;
        }
        if !have {
            add.push(name.to_string());
        }
        true
    };
    if plugin(
        "pytest-socket",
        "socket",
        !["--disable-socket", "--allow-hosts", "--force-enable-socket"]
            .iter()
            .any(|o| cfg.has_opt(o)),
    ) {
        want.addopts.extend([
            "--disable-socket".to_string(),
            "--allow-unix-socket".to_string(),
        ]);
        let local = LOCAL_SERVICES.iter().any(|d| deps.contains(*d))
            || !forge_detect::compose::dependency_services(&ctx.base, &not_dependencies(ctx))
                .is_empty();
        if local {
            want.addopts
                .push("--allow-hosts=127.0.0.1,localhost,::1".to_string());
        }
    }
    // pytest-randomly works by being installed; pytest-random-order is the project's choice
    if !deps.contains("pytest-random-order") {
        plugin("pytest-randomly", "randomly", true);
    }
    if plugin(
        "pytest-timeout",
        "timeout",
        !cfg.has_key("timeout") && !cfg.has_opt("--timeout"),
    ) {
        want.keys.push(("timeout", Setting::Int(120)));
    }
    if !cfg.strict_markers() {
        want.addopts.push("--strict-markers".into());
    }
    if !cfg.strict_config() {
        want.addopts.push("--strict-config".into());
    }
    if !cfg.strict_xfail() {
        want.keys.push(("xfail_strict", Setting::Bool(true)));
    }
    for (rule, entry) in DEPRECATIONS {
        if active(ctx, rule) && !cfg.filters(entry) {
            want.filterwarnings.push((*entry).to_string());
        }
    }
    let file = cfg.file.path();
    let before = read(ctx, file).unwrap_or_default();
    let after = test_setup::with_pytest_setup(&before, &cfg, &want);
    if after.is_none() {
        // a file it cannot edit: only what needs no setting (pytest-randomly) is offered
        add.retain(|a| a == "pytest-randomly");
        want = PytestSetup::default();
        if add.is_empty() {
            return None;
        }
    }
    let mut settings: Vec<String> = want.addopts.iter().map(|o| o.to_string()).collect();
    settings.extend(want.keys.iter().map(|(k, _)| k.to_string()));
    if !want.filterwarnings.is_empty() {
        settings.push("filterwarnings".into());
    }
    let mut steps = Vec::new();
    let mut what = Vec::new();
    if let (false, Some(cmd)) = (add.is_empty(), &add_cmd) {
        let mut argv: Vec<String> = cmd.iter().map(|s| s.to_string()).collect();
        argv.extend(add.iter().cloned());
        let shown = cmd.join(" ");
        what.push(format!("add {} with `{shown}`", and(&add)));
        let names = and(&add);
        steps.push(Step::Run(Box::new(move |dir| match command(&argv, dir) {
            Ok(()) => Ran::Done(format!("{names} added")),
            Err(e) => Ran::Failed(format!(
                "{e}; pytest's settings were not changed, since they need the plugins"
            )),
        })));
    }
    if after.is_some() {
        what.push(format!("set {} in {file}", and(&settings)));
    }
    let mut fails = Vec::new();
    if want.addopts.iter().any(|o| o == "--disable-socket") {
        fails.push("reaches the network");
    }
    if add.iter().any(|a| a == "pytest-randomly") {
        fails.push("depends on the order the tests run in");
    }
    if want.keys.iter().any(|(k, _)| *k == "timeout") {
        fails.push("hangs");
    }
    if want.addopts.iter().any(|o| o.starts_with("--strict-")) {
        fails.push("uses an unknown marker or setting");
    }
    if want.keys.iter().any(|(k, _)| *k == "xfail_strict") {
        fails.push("is marked xfail and passes");
    }
    if !want.filterwarnings.is_empty() {
        fails.push("uses a deprecated API");
    }
    if let Some(after) = after {
        steps.push(Step::Write {
            file: file.into(),
            before: read(ctx, file),
            after,
            added: format!("pytest's {}", and(&settings)),
        });
    }
    Some(Offer {
        name: "pytest".into(),
        what: what.join(" and "),
        why: Some(format!(
            "a test then fails when it {}",
            and(&fails.iter().map(|s| s.to_string()).collect::<Vec<_>>())
        )),
        steps,
    })
}

/// Where a runner reads its test setup, for the person: the setting names and the setup file.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Runner {
    Vitest,
    Bun,
    Jest,
}

impl Runner {
    /// The setup file it loads, as its config names it, and the file Forge creates.
    fn setup(self) -> (String, &'static str, &'static str) {
        use test_setup::{NO_NETWORK_JEST, NO_NETWORK_JEST_FILE};
        match self {
            Runner::Jest => (
                format!("<rootDir>/{NO_NETWORK_JEST_FILE}"),
                NO_NETWORK_JEST_FILE,
                NO_NETWORK_JEST,
            ),
            _ => (format!("./{NO_NETWORK_FILE}"), NO_NETWORK_FILE, NO_NETWORK),
        }
    }

    /// The settings it added, as the person reads them in `file`.
    fn added(self, file: &str, e: &test_setup::RunnerEdit) -> String {
        let (setup, _, _) = self.setup();
        let prefix = if file == "package.json" { "jest." } else { "" };
        let mut parts = Vec::new();
        if e.setup {
            parts.push(match self {
                Runner::Vitest => format!("test.setupFiles {setup}"),
                Runner::Bun => format!("[test] preload {setup}"),
                Runner::Jest => format!("{prefix}setupFilesAfterEnv {setup}"),
            });
        }
        if e.shuffle {
            parts.push(match self {
                Runner::Vitest => "test.sequence.shuffle".to_string(),
                Runner::Bun => "[test] randomize".to_string(),
                Runner::Jest => format!("{prefix}randomize"),
            });
        }
        and(&parts)
    }
}

/// The version of an npm package the project runs: its install's, else the lowest its
/// package.json admits (`""`: unknown).
fn npm_version(ctx: &Ctx, deps: &forge_detect::deps::Deps, name: &str) -> String {
    read(ctx, &format!("node_modules/{name}/package.json"))
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("version").and_then(|v| v.as_str()).map(str::to_owned))
        .or_else(|| deps.get(name).cloned())
        .unwrap_or_default()
}

/// The JavaScript test runners' setup: Vitest's (`typescript/vitest/run`), `bun test`'s
/// (`typescript/bun/test`) and Jest's (`typescript/jest/run`). Tests run in a random order
/// (Vitest's `sequence.shuffle`, bunfig's `randomize`, Jest's `randomize` from 29.5), and reach
/// no host but this machine: [`NO_NETWORK_FILE`] (Jest: [`test_setup::NO_NETWORK_JEST_FILE`],
/// from Jest 28, whose node environment first has the fetch globals nock needs), loaded before
/// each test file (Vitest's `setupFiles`, bunfig's `preload`, Jest's `setupFilesAfterEnv`), makes
/// nock (a dev dependency added with the project's package manager) refuse any other. Not where
/// the project has msw, whose `onUnhandledRequest: "error"` is its own way
/// (`typescript/msw/unhandled-error`). A config that is code is edited only in the shapes
/// [`test_setup`] understands; otherwise nothing is offered for it.
fn js_tests(ctx: &Ctx) -> Option<Offer> {
    let vitest = active(ctx, "typescript/vitest/run");
    let bun = active(ctx, "typescript/bun/test");
    let jest = active(ctx, "typescript/jest/run");
    if !vitest && !bun && !jest {
        return None;
    }
    let pkg: serde_json::Value = read(ctx, "package.json")
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let deps = forge_detect::deps::npm_deps(&pkg);
    let pm = ["npm", "pnpm", "yarn", "bun"]
        .into_iter()
        .find(|pm| pack(ctx, &format!("typescript/{pm}")));
    let has_nock = deps.contains_key("nock");
    let network = !deps.contains_key("msw") && (has_nock || pm.is_some());
    let mut edits: Vec<(Runner, String, Option<String>, test_setup::RunnerEdit)> = Vec::new();
    if vitest {
        let names = |stem: &str| {
            ["ts", "mts", "cts", "js", "mjs", "cjs"].map(|e| format!("{stem}.config.{e}"))
        };
        let own = names("vitest")
            .into_iter()
            .chain(names("vite"))
            .find(|f| ctx.base.join(f).is_file());
        let text = own.as_ref().and_then(|f| read(ctx, f));
        if own.is_none() || text.is_some() {
            let file = own.unwrap_or_else(|| "vitest.config.ts".into());
            let (setup, _, _) = Runner::Vitest.setup();
            if let Some(e) = test_setup::with_vitest_setup(
                text.as_deref(),
                network.then_some(setup.as_str()),
                true,
            ) {
                edits.push((Runner::Vitest, file, text, e));
            }
        }
    }
    if bun {
        let text = read(ctx, "bunfig.toml");
        let (setup, _, _) = Runner::Bun.setup();
        if let Some(e) = test_setup::with_bun_test_setup(
            text.as_deref().unwrap_or(""),
            network.then_some(setup.as_str()),
            true,
        ) {
            edits.push((Runner::Bun, "bunfig.toml".into(), text, e));
        }
    }
    if jest {
        use forge_detect::versions::satisfies;
        let v = npm_version(ctx, &deps, "jest");
        // jest.config.* first: Jest refuses a project with both it and package.json's "jest"
        let file = ["ts", "js", "mjs", "cjs", "cts", "mts", "json"]
            .map(|e| format!("jest.config.{e}"))
            .into_iter()
            .find(|f| ctx.base.join(f).is_file())
            .unwrap_or_else(|| "package.json".into());
        let (setup, _, _) = Runner::Jest.setup();
        if let Some(text) = read(ctx, &file)
            && let Some(e) = test_setup::with_jest_setup(
                &file,
                &text,
                (network && satisfies(&v, Some(">=28.0.0"))).then_some(setup.as_str()),
                satisfies(&v, Some(">=29.5.0")),
            )
        {
            edits.push((Runner::Jest, file, Some(text), e));
        }
    }
    if edits.is_empty() {
        return None;
    }
    let loads: Vec<Runner> = edits
        .iter()
        .filter(|(_, _, _, e)| e.setup)
        .map(|(r, ..)| *r)
        .collect();
    let mut steps = Vec::new();
    let mut what = Vec::new();
    if !loads.is_empty()
        && !has_nock
        && let Some(pm) = pm
    {
        let argv: Vec<String> = match pm {
            "npm" => vec!["npm", "install", "--save-dev", "nock@^14"],
            "bun" => vec!["bun", "add", "--dev", "nock@^14"],
            other => vec![other, "add", "--dev", "nock@^14"],
        }
        .into_iter()
        .map(str::to_owned)
        .collect();
        what.push(format!(
            "add nock with `{}`",
            argv[..argv.len() - 1].join(" ")
        ));
        steps.push(Step::Run(Box::new(move |dir| match command(&argv, dir) {
            Ok(()) => Ran::Done("nock added".into()),
            Err(e) => Ran::Failed(format!(
                "{e}; the test setup was not changed, since it needs nock"
            )),
        })));
    }
    let mut created = BTreeSet::new();
    for r in &loads {
        let (_, file, text) = r.setup();
        if !ctx.base.join(file).exists() && created.insert(file) {
            what.push(format!("create {file}"));
            steps.push(Step::Write {
                file: file.into(),
                before: None,
                after: text.into(),
                added: String::new(),
            });
        }
    }
    let mut fails = Vec::new();
    if !loads.is_empty() {
        fails.push("reaches a host other than this machine".to_string());
    }
    if edits.iter().any(|(_, _, _, e)| e.shuffle) {
        fails.push("depends on the order the tests run in".to_string());
    }
    for (runner, file, before, e) in edits {
        let added = runner.added(&file, &e);
        what.push(if before.is_none() {
            format!("create {file} with {added}")
        } else {
            format!("add {added} to {file}")
        });
        steps.push(Step::Write {
            file,
            before,
            after: e.text,
            added,
        });
    }
    Some(Offer {
        name: "test runner".into(),
        what: and(&what),
        why: Some(format!("a test then fails when it {}", and(&fails))),
        steps,
    })
}

/// The repository's files of a role (`dockerfile`), as the active rules see it.
fn role_files(ctx: &Ctx, role: &str) -> Vec<String> {
    use forge_packs::glob::glob_to_regex;
    let Some(globs) = ctx.plan.role_globs.get(role) else {
        return Vec::new();
    };
    let (exc, inc): (Vec<_>, Vec<_>) = globs.iter().partition(|g| g.starts_with('!'));
    let inc: Vec<_> = inc.iter().map(|g| glob_to_regex(g)).collect();
    let exc: Vec<_> = exc.iter().map(|g| glob_to_regex(&g[1..])).collect();
    forge_detect::list_repo_files(&ctx.base)
        .into_iter()
        .filter(|f| {
            inc.iter().any(|re| re.is_match(f).unwrap_or(false))
                && !exc.iter().any(|re| re.is_match(f).unwrap_or(false))
        })
        .collect()
}

/// Whether the repository has files of a role (`python-tests`), as the active rules see it.
fn has_role_files(ctx: &Ctx, role: &str) -> bool {
    !role_files(ctx, role).is_empty()
}

/// Testcontainers, where the repository has tests and its Compose files run a service the code
/// depends on ([`forge_detect::compose::dependency_services`]): the tests then start that service
/// themselves, in a throwaway container, instead of relying on one someone left running. The
/// dependency is added with the project's package manager, with the modules of those services:
/// PyPI `testcontainers[<extras>]` for Python tests, npm `testcontainers` and
/// `@testcontainers/<module>` for JavaScript ones. Forge never writes the fixtures.
fn testcontainers(ctx: &Ctx) -> Vec<Offer> {
    let services = forge_detect::compose::dependency_services(&ctx.base, &not_dependencies(ctx));
    if services.is_empty() {
        return Vec::new();
    }
    let kinds: BTreeSet<&str> = services
        .iter()
        .filter_map(|s| forge_detect::compose::service_kind(&s.image))
        .collect();
    let names: Vec<String> = services.iter().map(|s| s.name.clone()).collect();
    let why = format!(
        "the tests then start {} themselves, in a throwaway container, instead of needing one already running",
        and(&names)
    );
    let mut out = Vec::new();
    let python_cmd: Option<Vec<&str>> = if pack(ctx, "python/uv") {
        Some(vec!["uv", "add", "--dev"])
    } else if pack(ctx, "python/poetry") {
        Some(vec!["poetry", "add", "--group", "dev"])
    } else if pack(ctx, "python/pdm") {
        Some(vec!["pdm", "add", "-dG", "dev"])
    } else if pack(ctx, "python/pipenv") {
        Some(vec!["pipenv", "install", "--dev"])
    } else {
        None
    };
    if let Some(cmd) = python_cmd
        && !python_deps(ctx).contains("testcontainers")
        && has_role_files(ctx, "python-tests")
    {
        let extras: BTreeSet<&str> = kinds
            .iter()
            .map(|k| if *k == "mariadb" { "mysql" } else { *k })
            .collect();
        let spec = if extras.is_empty() {
            "testcontainers".to_string()
        } else {
            format!(
                "testcontainers[{}]",
                extras.into_iter().collect::<Vec<_>>().join(",")
            )
        };
        out.push(dependency_offer(&cmd, vec![spec], why.clone()));
    }
    let js_pm = ["npm", "pnpm", "yarn", "bun"]
        .into_iter()
        .find(|pm| pack(ctx, &format!("typescript/{pm}")));
    let pkg: serde_json::Value = read(ctx, "package.json")
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    if let Some(pm) = js_pm
        && !forge_detect::deps::npm_deps(&pkg).contains_key("testcontainers")
        && has_role_files(ctx, "ts-tests")
    {
        let mut specs = vec!["testcontainers".to_string()];
        for k in &kinds {
            let module = match *k {
                "postgres" => "postgresql",
                "mssql" => "mssqlserver",
                "memcached" | "opensearch" => continue,
                other => other,
            };
            specs.push(format!("@testcontainers/{module}"));
        }
        let cmd: Vec<&str> = match pm {
            "npm" => vec!["npm", "install", "--save-dev"],
            other => vec![other, "add", "--dev"],
        };
        out.push(dependency_offer(&cmd, specs, why));
    }
    out
}

/// An offer that adds test dependencies with the package manager's command.
fn dependency_offer(cmd: &[&str], specs: Vec<String>, why: String) -> Offer {
    let shown = cmd.join(" ");
    let mut argv: Vec<String> = cmd.iter().map(|s| s.to_string()).collect();
    argv.extend(specs.iter().cloned());
    let list = and(&specs);
    let done = format!("{list} added");
    Offer {
        name: "Testcontainers".into(),
        what: format!("add {list} with `{shown}`"),
        why: Some(why),
        steps: vec![Step::Run(Box::new(move |dir| match command(&argv, dir) {
            Ok(()) => Ran::Done(done.clone()),
            Err(e) => Ran::Failed(e),
        }))],
    }
}

/// Dependabot's cooldown where zizmor's `dependabot-cooldown` audit runs (`github-actions/zizmor`)
/// and `.github/dependabot.yml` has an update without one: `cooldown: {default-days: 7}` on each,
/// as the package managers' cooldowns, so a freshly published (possibly compromised) version is
/// not proposed at once.
fn dependabot(ctx: &Ctx) -> Option<Offer> {
    use forge_builtins_infra::dependabot::{DAYS, FILE, with_cooldown};
    if !active(ctx, "github-actions/zizmor") {
        return None;
    }
    let before = read(ctx, FILE)?;
    let after = with_cooldown(&before)?;
    Some(Offer {
        name: "Dependabot".into(),
        what: format!("add a {DAYS}-day cooldown to each update in {FILE}"),
        why: Some(format!(
            "Dependabot then proposes only versions published at least {DAYS} days ago"
        )),
        steps: vec![Step::Write {
            file: FILE.into(),
            before: Some(before),
            after,
            added: format!("cooldown default-days {DAYS}"),
        }],
    })
}

/// The secret files out of each Docker build context, where `docker/dockerignore` is active and
/// the repository has a Dockerfile: the `.dockerignore` it reads (`<Dockerfile>.dockerignore`,
/// the one in its directory, else the root one, which is created when there is none) gets the
/// dotenv and credential patterns it lacks ([`forge_builtins_infra::dockerignore`]). A build that
/// copies the context then never puts them in an image layer. Guard: `docker/dockerignore-ratchet`.
fn dockerignore(ctx: &Ctx) -> Option<Offer> {
    use forge_builtins_infra::dockerignore::with_secret_excludes;
    if !active(ctx, "docker/dockerignore") {
        return None;
    }
    let mut targets = BTreeSet::new();
    for df in role_files(ctx, "dockerfile") {
        let dir = df.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let local = if dir.is_empty() {
            ".dockerignore".to_string()
        } else {
            format!("{dir}/.dockerignore")
        };
        let target = [format!("{df}.dockerignore"), local]
            .into_iter()
            .find(|c| ctx.base.join(c).is_file())
            .unwrap_or_else(|| ".dockerignore".into());
        targets.insert(target);
    }
    let mut steps = Vec::new();
    let mut files = Vec::new();
    let mut created = false;
    for file in targets {
        let before = read(ctx, &file);
        if before.is_none() && ctx.base.join(&file).exists() {
            continue;
        }
        let Some((after, _)) = with_secret_excludes(before.as_deref()) else {
            continue;
        };
        created |= before.is_none();
        steps.push(Step::Write {
            added: if before.is_some() {
                "the secret files it did not exclude".into()
            } else {
                String::new()
            },
            file: file.clone(),
            before,
            after,
        });
        files.push(file);
    }
    if steps.is_empty() {
        return None;
    }
    let verb = if created && files.len() == 1 {
        format!("create {} excluding", files[0])
    } else {
        format!("exclude in {}", and(&files))
    };
    Some(Offer {
        name: "Docker".into(),
        what: format!(
            "{verb} .env files (not the templates), private keys, keystores and credential files"
        ),
        why: Some("a build that copies the context then never puts them in an image".into()),
        steps,
    })
}

/// The text lock file, where `typescript/bun/text-lockfile` is active and the project has a
/// binary `bun.lockb` (and no `bun.lock` yet): bun's own migration (the adapter's `text-lock`,
/// `bun install --save-text-lockfile --frozen-lockfile --lockfile-only`, Bun 1.1.43 and later:
/// it writes bun.lock from bun.lockb, installs nothing and fails on a bun.lockb that no longer
/// matches package.json), then bun.lockb is deleted (Bun 1.2 does it itself).
fn bun_text_lock(ctx: &Ctx) -> Option<Offer> {
    use forge_check::builtins::{AdapterTools, BuiltinTools, ToolRun};
    if !active(ctx, "typescript/bun/text-lockfile")
        || !ctx.base.join("bun.lockb").is_file()
        || ctx.base.join("bun.lock").exists()
    {
        return None;
    }
    let shown = "bun install --save-text-lockfile --frozen-lockfile --lockfile-only";
    Some(Offer {
        name: "Bun".into(),
        what: format!(
            "replace bun.lockb with the text bun.lock (`{shown}`, then delete bun.lockb)"
        ),
        why: Some("reviews and merges then read the lock file".into()),
        steps: vec![Step::Run(Box::new(move |base| {
            let tools = AdapterTools::new(
                crate::provisioning::builtin_tools(),
                forge_tools::lock_pins(),
            );
            match tools.run(base, base, "bun", "text-lock", &[]) {
                ToolRun::Ran(o) if o.status == 0 && base.join("bun.lock").is_file() => {
                    let lockb = base.join("bun.lockb");
                    if lockb.exists() && std::fs::remove_file(&lockb).is_err() {
                        return Ran::Failed(
                            "bun.lock created, but bun.lockb could not be deleted; delete it yourself"
                                .into(),
                        );
                    }
                    Ran::Done("bun.lock created from bun.lockb, bun.lockb deleted".into())
                }
                ToolRun::Ran(o) => {
                    let why = o
                        .stderr
                        .lines()
                        .chain(o.stdout.lines())
                        .rev()
                        .find(|l| l.trim_start().starts_with("error"))
                        .or_else(|| o.stderr.lines().rev().find(|l| !l.trim().is_empty()))
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    Ran::Failed(format!(
                        "`{shown}` failed ({why}); bring bun.lockb in line with package.json (bun install), then run it yourself"
                    ))
                }
                ToolRun::Missing(why) | ToolRun::Unsupported(why) => {
                    Ran::Failed(format!("`{shown}` could not run ({why}); run it yourself"))
                }
            }
        }))],
    })
}

/// The Compose services the project's `forge.yaml` says are not dependencies
/// (`docker.compose.not-dependencies`).
fn not_dependencies(ctx: &Ctx) -> Vec<String> {
    ctx.config
        .as_ref()
        .and_then(|c| c.docker.as_ref())
        .and_then(|d| d.compose.as_ref())
        .and_then(|c| c.not_dependencies.clone())
        .unwrap_or_default()
}
