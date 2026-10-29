//! The builtins of the data packs, ported from the engine's shell modules `sql.sh`, `python.sh`,
//! `env.sh`, `release.sh` and `docs.sh`: migration
//! linting and immutability (Squawk, Alembic, Prisma, Drizzle, TypeORM, sqlfluff), models a
//! migration would miss, type stubs, uv's release cooldown, `.env` templates, hand-edited
//! versions and release notes, immutable ADRs and docs that mention changed identifiers.
//!
//! Each is a [`CheckBuiltin`] named after the shell function without `forge_`: it reads
//! forge-core's [`BuiltinCtx`] and the checker's [`CheckCtx`] (the check's files, the run's
//! mode, the file roles). Tools come from forge-check's [`BuiltinTools`]: the project's own
//! (alembic, sqlfluff, typeorm) as the checker's adapters drive them, and Forge's pinned Squawk.
//! A tool that is absent, or a run that fails, is a [`forge_core::Note`] for the person; the
//! check passes.
//!
//! Two builtins produce data for the pack command around them rather than findings for the
//! agent: `alembic_files` (revision files, one per line, for `$(…)`) and `prisma_drops` (the
//! `path:line: statement` lines a pack command filters through the user's accepted changes).
//! Their lines are findings without a path, so they print exactly as they are.

mod ctx;
mod docs;
mod env;
mod python;
mod release;
mod sql;
mod util;

use std::sync::Arc;
use std::time::Duration;

use forge_check::builtins::{BuiltinTools, CheckBuiltin, CheckCtx, NoTools};
use forge_core::Report;
use forge_core::builtin::BuiltinCtx;

pub use ctx::glob_matches;
pub use forge_check::builtins::{RunMode as Mode, ToolOutput, ToolRun};

/// How the builtins reach tools.
#[derive(Clone)]
pub struct Options {
    /// The project's own tools (alembic, sqlfluff, typeorm…) and Forge's pinned Squawk.
    pub tools: Arc<dyn BuiltinTools>,
    /// The minimum age of a version that the package managers' cooldown settings must ask for.
    pub cooldown: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            tools: Arc::new(NoTools),
            cooldown: Duration::from_secs(259_200),
        }
    }
}

type RunFn = fn(&Run<'_>, &[String]) -> Report;

struct DataBuiltin {
    name: &'static str,
    run: RunFn,
    opts: Arc<Options>,
}

impl CheckBuiltin for DataBuiltin {
    fn name(&self) -> &'static str {
        self.name
    }

    fn run(&self, ctx: &BuiltinCtx, check: &CheckCtx, args: &[String]) -> Report {
        (self.run)(
            &Run {
                ctx,
                check,
                opts: &self.opts,
            },
            args,
        )
    }
}

pub(crate) use ctx::Run;

/// The builtins' names and functions.
const ALL: &[(&str, RunFn)] = &[
    ("sqlfluff", sql::sqlfluff),
    ("squawk_files", sql::squawk_files),
    ("squawk_role", sql::squawk_role),
    ("alembic_files", sql::alembic_files),
    ("squawk_alembic", sql::squawk_alembic),
    ("alembic_batch", sql::alembic_batch),
    ("alembic_fk_index", sql::alembic_fk_index),
    ("alembic_data_downgrade", sql::alembic_data_downgrade),
    ("models_registered", sql::models_registered),
    ("prisma_drops", sql::prisma_drops),
    ("drizzle_migration_present", sql::drizzle_migration_present),
    ("drizzle_applied", sql::drizzle_applied),
    ("drizzle_squawk", sql::drizzle_squawk),
    ("applied_migrations", sql::applied_migrations),
    ("typeorm_pending", sql::typeorm_pending),
    ("stub_sync", python::stub_sync),
    ("uv_cooldown", python::uv_cooldown),
    ("env_example", env::env_example),
    ("version_changed", release::version_changed),
    ("release_notes", release::release_notes),
    ("adr_edits", docs::adr_edits),
    ("docs_mentions", docs::docs_mentions),
];

/// The names of every builtin of this crate.
pub fn names() -> Vec<&'static str> {
    ALL.iter().map(|(n, _)| *n).collect()
}

/// Every builtin of this crate.
pub fn builtins(opts: Options) -> Vec<Box<dyn CheckBuiltin>> {
    let opts = Arc::new(opts);
    ALL.iter()
        .map(|(name, run)| {
            Box::new(DataBuiltin {
                name,
                run: *run,
                opts: opts.clone(),
            }) as Box<dyn CheckBuiltin>
        })
        .collect()
}
