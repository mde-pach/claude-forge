//! Provisioning: everything Forge downloads into its cache, each download checked against
//! a pinned sha256.
//!
//! * [`catalog`]: the one table of pinned tools (uv, Bun, the uv-installed Python tools,
//!   the npm-only tools, the release binaries) — [`catalog::lock_pins`] is what `forge
//!   apply` writes into `forge.lock`.
//! * [`Provisioner`] / [`ToolResolver`]: `resolve(tool)` finds an installed tool without
//!   downloading (hot path); `ensure(tool)` installs it first when missing, returning a
//!   person-facing [`forge_core::Note`] when it cannot.
//! * [`env_file`]: the SessionStart step that puts Forge's uv on the agent's PATH through
//!   `CLAUDE_ENV_FILE`.
//! * [`ReleaseFetcher`]: the launcher's downloader for the Forge version `forge.lock` pins.
//!
//! Nothing here writes to stdout or stderr: progress and failures are values (notes,
//! errors) for `forge apply` and `forge status` to show the person. The agent never sees
//! them.

#![cfg(unix)]

pub mod archive;
pub mod catalog;
pub mod env_file;
pub mod forge_release;
pub mod fsutil;
pub mod http;
pub mod platform;
pub mod provision;

pub use catalog::{Catalog, Source, ToolSpec, lock_pins, pin_differences};
pub use env_file::{add_forge_path_to_session, append_path_export};
pub use forge_release::{ReleaseFetcher, run_background_fetch};
pub use http::Http;
pub use platform::Platform;
pub use provision::{ProvisionReport, Provisioned, Provisioner, ToolResolver};
