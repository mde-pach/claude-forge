//! Getting a Forge version into `<cache>/bin`. The real downloader (release
//! asset, checksum, atomic install) comes in a later stage; the launcher only
//! depends on this trait.

use std::fmt;
use std::path::PathBuf;

use forge_home::Home;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// This build cannot download (no fetcher wired in, offline mode, ...).
    NotAvailable,
    Failed(String),
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FetchError::NotAvailable => f.write_str("downloading is not available in this build"),
            FetchError::Failed(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for FetchError {}

pub trait Fetcher {
    /// Downloads and installs `version`, returning the path of the
    /// executable `<cache>/bin/forge-<version>`. Blocks.
    fn fetch(&self, home: &Home, version: &str) -> Result<PathBuf, FetchError>;

    /// Starts a download that outlives this process (detached: own process
    /// group, null stdio) and returns at once.
    fn fetch_in_background(&self, home: &Home, version: &str) -> Result<(), FetchError>;
}

/// The stage-1 fetcher: never downloads.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoFetcher;

impl Fetcher for NoFetcher {
    fn fetch(&self, _: &Home, _: &str) -> Result<PathBuf, FetchError> {
        Err(FetchError::NotAvailable)
    }

    fn fetch_in_background(&self, _: &Home, _: &str) -> Result<(), FetchError> {
        Err(FetchError::NotAvailable)
    }
}
