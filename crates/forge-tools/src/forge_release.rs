//! Downloading the Forge version a repo's `forge.lock` pins: the launcher's real
//! [`Fetcher`].
//!
//! A release `v<version>` has, per platform, `claude-forge-<version>-<target>.tar.gz`
//! (holding `claude-forge-<version>-<target>/claude-forge`) and one `SHA256SUMS` listing
//! every asset. The fetcher downloads `SHA256SUMS`, then the platform's archive, checks
//! the archive's sha256 against its line, unpacks `claude-forge` and renames it to
//! `<cache>/bin/forge-<version>`, under `<cache>/bin/.forge-<version>.lock`.
//!
//! **What the check proves, and the choice about signatures.** Both files come over HTTPS
//! from the same GitHub release, so the sum guards against a truncated or corrupted
//! download and a mismatched asset, not against someone able to replace release assets.
//! The release pipeline also publishes GitHub artifact attestations (SLSA build
//! provenance) for every archive, which a person can check with
//! `gh attestation verify <archive> --repo mde-pach/claude-forge`. The fetcher does not
//! require them: verifying one needs `gh` with a login, or a Sigstore client and its trust
//! root, neither of which can be assumed on every machine, and a download that silently
//! fails on half the machines would leave checks off. Releases should be made immutable
//! (GitHub's immutable releases setting) so a published version's assets cannot change.
//!
//! The background variant (session start) runs this same fetch in a detached child
//! process: its own process group, stdin/stdout/stderr on `/dev/null`, working directory
//! `/`, so the hook returns at once and prints nothing. The child takes the lock without
//! waiting and exits when another download already holds it.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use forge_home::Home;
use forge_launcher::{FetchError, Fetcher};

use crate::archive::{self, Kind};
use crate::fsutil::{self, FileLock, TempPath};
use crate::http::{self, Http};
use crate::platform::Platform;

/// Where releases are downloaded from: `<base>/v<version>/<asset>`.
pub const RELEASES_URL: &str = "https://github.com/mde-pach/claude-forge/releases/download";

/// The argument the CLI recognises (before anything else) as "run the background fetch".
pub const BACKGROUND_ARG: &str = "__forge-fetch";

/// The executable inside a release archive.
pub const RELEASE_BINARY: &str = "claude-forge";

/// The archive name for a version and platform.
pub fn asset_name(version: &str, platform: &Platform) -> String {
    format!(
        "{RELEASE_BINARY}-{version}-{}.tar.gz",
        platform.forge_target()
    )
}

/// The sha256 a `SHA256SUMS` file lists for `asset` (`<hex>  <name>` or `<hex> *<name>`).
pub fn sum_for(sums: &str, asset: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let (hex, name) = (it.next()?, it.next()?);
        let ok = name.trim_start_matches('*') == asset
            && hex.len() == 64
            && hex.bytes().all(|b| b.is_ascii_hexdigit());
        ok.then(|| hex.to_ascii_lowercase())
    })
}

/// Downloads Forge releases into `<cache>/bin`.
#[derive(Debug)]
pub struct ReleaseFetcher {
    base_url: String,
    platform: Option<Platform>,
    http: Http,
    /// The program and leading arguments the background child is started with.
    background: Option<(PathBuf, Vec<OsString>)>,
}

impl Default for ReleaseFetcher {
    fn default() -> Self {
        ReleaseFetcher::new()
    }
}

impl ReleaseFetcher {
    /// GitHub releases over HTTPS, this platform; the background child is this executable.
    pub fn new() -> ReleaseFetcher {
        ReleaseFetcher {
            base_url: RELEASES_URL.to_owned(),
            platform: Platform::current(),
            http: Http::new(),
            background: None,
        }
    }

    /// For tests: another server, platform, client and background program.
    pub fn with(
        base_url: impl Into<String>,
        platform: Option<Platform>,
        http: Http,
        background: Option<(PathBuf, Vec<OsString>)>,
    ) -> ReleaseFetcher {
        ReleaseFetcher {
            base_url: base_url.into(),
            platform,
            http,
            background,
        }
    }

    fn failed(msg: impl Into<String>) -> FetchError {
        FetchError::Failed(msg.into())
    }

    fn install(&self, home: &Home, version: &str, dest: &Path) -> Result<(), FetchError> {
        let platform = self
            .platform
            .ok_or_else(|| Self::failed("Forge publishes no release for this platform"))?;
        let base = format!("{}/v{version}", self.base_url.trim_end_matches('/'));
        let sums = self
            .http
            .get_text(&format!("{base}/SHA256SUMS"))
            .map_err(|e| {
                Self::failed(format!("cannot download Forge {version}'s SHA256SUMS: {e}"))
            })?;
        let asset = asset_name(version, &platform);
        let want = sum_for(&sums, &asset).ok_or_else(|| {
            Self::failed(format!(
                "Forge {version}'s SHA256SUMS lists no {asset} (no release for {platform})"
            ))
        })?;
        let bin = home.bin_dir();
        let dl = TempPath::new(fsutil::temp_sibling(&bin, "download"));
        let got = self
            .http
            .download(&format!("{base}/{asset}"), dl.path(), http::MAX_DOWNLOAD)
            .map_err(|e| Self::failed(format!("cannot download {asset}: {e}")))?;
        if got != want {
            return Err(Self::failed(format!(
                "{asset} does not match its SHA256SUMS line (expected {want}, got {got}); not installed"
            )));
        }
        let stage = TempPath::new(fsutil::temp_sibling(&bin, "stage"));
        let io = |what: &str, e: io::Error| Self::failed(format!("{what}: {e}"));
        fs::create_dir(stage.path()).map_err(|e| io("cannot unpack", e))?;
        archive::extract(dl.path(), Kind::TarGz, &[RELEASE_BINARY], stage.path())
            .map_err(|e| io(&format!("cannot unpack {asset}"), e))?;
        fs::rename(stage.path().join(RELEASE_BINARY), dest)
            .map_err(|e| io("cannot move it into place", e))?;
        Ok(())
    }

    /// The fetch, with the lock taken without waiting: `Ok(None)` when another process
    /// holds it (that one is downloading).
    pub fn fetch_unless_busy(
        &self,
        home: &Home,
        version: &str,
    ) -> Result<Option<PathBuf>, FetchError> {
        let (dest, lock_path) = paths(home, version)?;
        let Some(_lock) = FileLock::try_acquire(&lock_path)
            .map_err(|e| Self::failed(format!("cannot lock {}: {e}", lock_path.display())))?
        else {
            return Ok(None);
        };
        if !fsutil::is_executable(&dest) {
            self.install(home, version, &dest)?;
        }
        Ok(Some(dest))
    }
}

fn paths(home: &Home, version: &str) -> Result<(PathBuf, PathBuf), FetchError> {
    let dest = home
        .forge_binary(version)
        .map_err(|e| FetchError::Failed(e.to_string()))?;
    let lock = home.bin_dir().join(format!(".forge-{version}.lock"));
    fs::create_dir_all(home.bin_dir()).map_err(|e| {
        FetchError::Failed(format!("cannot create {}: {e}", home.bin_dir().display()))
    })?;
    Ok((dest, lock))
}

impl Fetcher for ReleaseFetcher {
    fn fetch(&self, home: &Home, version: &str) -> Result<PathBuf, FetchError> {
        let (dest, lock_path) = paths(home, version)?;
        if fsutil::is_executable(&dest) {
            return Ok(dest);
        }
        let _lock = FileLock::acquire(&lock_path)
            .map_err(|e| Self::failed(format!("cannot lock {}: {e}", lock_path.display())))?;
        if !fsutil::is_executable(&dest) {
            self.install(home, version, &dest)?;
        }
        Ok(dest)
    }

    fn fetch_in_background(&self, home: &Home, version: &str) -> Result<(), FetchError> {
        forge_home::check_version(version).map_err(|e| Self::failed(e.to_string()))?;
        let (program, prefix) = match &self.background {
            Some((p, a)) => (p.clone(), a.clone()),
            None => (
                std::env::current_exe()
                    .map_err(|e| Self::failed(format!("cannot find this executable: {e}")))?,
                vec![OsString::from(BACKGROUND_ARG)],
            ),
        };
        use std::os::unix::process::CommandExt;
        Command::new(program)
            .args(prefix)
            .current_dir("/")
            .env(ENV_VERSION, version)
            .env(ENV_CACHE, home.cache_root())
            .env(ENV_STATE, home.state_root())
            .env(ENV_BASE, &self.base_url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .map(drop)
            .map_err(|e| Self::failed(format!("cannot start the background download: {e}")))
    }
}

const ENV_VERSION: &str = "FORGE_FETCH_VERSION";
const ENV_CACHE: &str = "FORGE_FETCH_CACHE";
const ENV_STATE: &str = "FORGE_FETCH_STATE";
const ENV_BASE: &str = "FORGE_FETCH_BASE_URL";

/// The background child's work, its parameters read from the environment its parent set.
/// The CLI calls it when argv[1] is [`BACKGROUND_ARG`], before the launcher. Exits 0 when
/// the binary is there (or another process is getting it), 1 otherwise; prints nothing.
pub fn run_background_fetch() -> ExitCode {
    run_background_fetch_with(Http::new())
}

/// [`run_background_fetch`] with a given client (tests allow plain HTTP).
pub fn run_background_fetch_with(http: Http) -> ExitCode {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
    let (Some(version), Some(cache), Some(state)) =
        (var(ENV_VERSION), var(ENV_CACHE), var(ENV_STATE))
    else {
        return ExitCode::from(2);
    };
    let base = var(ENV_BASE)
        .and_then(|b| b.into_string().ok())
        .unwrap_or_else(|| RELEASES_URL.to_owned());
    let Some(version) = version.to_str() else {
        return ExitCode::from(2);
    };
    let home = Home::new(cache.into(), state.into());
    let fetcher = ReleaseFetcher::with(base, Platform::current(), http, None);
    match fetcher.fetch_unless_busy(&home, version) {
        Ok(_) => ExitCode::SUCCESS,
        Err(_) => ExitCode::from(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums() {
        let a = "a".repeat(64);
        let text = format!(
            "{a}  claude-forge-1.0.0-x.tar.gz\n{}  *other\n",
            "B".repeat(64)
        );
        assert_eq!(sum_for(&text, "claude-forge-1.0.0-x.tar.gz"), Some(a));
        assert_eq!(sum_for(&text, "other"), Some("b".repeat(64)));
        assert_eq!(sum_for(&text, "missing"), None);
        assert_eq!(sum_for("zz  f\n", "f"), None);
    }
}
