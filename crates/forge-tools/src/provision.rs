//! Installing the pinned tools into the cache and finding them again.
//!
//! Layout under `<cache>/tools/<name>/<version>/`:
//!
//! | source        | contents                                   | executable        |
//! |---------------|--------------------------------------------|-------------------|
//! | uv            | `uv`, `uvx`                                | `uv`              |
//! | Bun           | `bun`                                      | `bun`             |
//! | release asset | the binary                                 | `<binary>`        |
//! | uv tool       | `env/` (uv's tool dir), `bin/`, marker     | `bin/<binary>`    |
//! | npm package   | `node_modules/`, `bin/<binary>`, marker    | `bin/<binary>`    |
//!
//! Downloads are built in a temporary sibling and renamed into place, so their directory
//! exists only when complete. uv and Bun installs cannot be moved after the fact (they
//! record absolute paths), so they are built in place and count as installed only once
//! their `.forge-installed` marker is written last. Every install runs under the tool
//! version's lock file. `<cache>/path/` holds only `uv` and `uvx` symlinks to the pinned uv.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use forge_core::{Note, NoteKind};
use forge_home::Home;

use crate::archive::{self, Kind};
use crate::catalog::{Catalog, Source, TOOLS_PYTHON, ToolSpec};
use crate::env_file::sh_quote;
use crate::fsutil::{self, FileLock, TempPath};
use crate::http::{self, Http};
use crate::platform::Platform;

/// Written last into a uv-tool or npm install: its presence means the install completed.
pub const MARKER: &str = ".forge-installed";

/// One tool after [`Provisioner::ensure_all`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provisioned {
    pub name: String,
    pub version: String,
    pub path: PathBuf,
    /// Installed by this call (rather than found in the cache).
    pub installed_now: bool,
}

/// What [`Provisioner::ensure_all`] did: the tools it has, and a note for each it has not.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProvisionReport {
    pub tools: Vec<Provisioned>,
    pub notes: Vec<Note>,
}

/// Installs and finds the tools of a [`Catalog`] in a [`Home`]'s cache.
#[derive(Debug)]
pub struct Provisioner {
    home: Home,
    catalog: Catalog,
    platform: Option<Platform>,
    http: Http,
}

impl Provisioner {
    /// Forge's tool table, this machine's platform, HTTPS downloads.
    pub fn new(home: Home) -> Provisioner {
        Provisioner::with(home, Catalog::builtin(), Platform::current(), Http::new())
    }

    pub fn with(
        home: Home,
        catalog: Catalog,
        platform: Option<Platform>,
        http: Http,
    ) -> Provisioner {
        Provisioner {
            home,
            catalog,
            platform,
            http,
        }
    }

    pub fn home(&self) -> &Home {
        &self.home
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    fn spec(&self, name: &str) -> Option<ToolSpec> {
        self.catalog.get(name).copied()
    }

    fn dir(&self, spec: &ToolSpec) -> Option<PathBuf> {
        self.home.tool_dir(spec.name, spec.version).ok()
    }

    fn exe(spec: &ToolSpec, dir: &Path) -> PathBuf {
        match spec.source {
            Source::Uv { .. } => dir.join("uv"),
            Source::Bun { .. } => dir.join("bun"),
            Source::Release { binary, .. } => dir.join(binary),
            Source::UvTool { binary, .. } | Source::Npm { binary, .. } => {
                dir.join("bin").join(binary)
            }
        }
    }

    fn complete(spec: &ToolSpec, dir: &Path) -> bool {
        let exe = Self::exe(spec, dir);
        match spec.source {
            Source::Uv { .. } => {
                fsutil::is_executable(&exe) && fsutil::is_executable(&dir.join("uvx"))
            }
            Source::Bun { .. } | Source::Release { .. } => fsutil::is_executable(&exe),
            Source::UvTool { .. } | Source::Npm { .. } => {
                dir.join(MARKER).is_file() && fsutil::is_executable(&exe)
            }
        }
    }

    /// The pinned uv (installed or not).
    fn uv_spec(&self) -> Option<ToolSpec> {
        self.catalog
            .find(|s| matches!(s, Source::Uv { .. }))
            .copied()
    }

    fn bun_spec(&self) -> Option<ToolSpec> {
        self.catalog
            .find(|s| matches!(s, Source::Bun { .. }))
            .copied()
    }

    /// The installed pinned `uvx`, next to `uv`.
    pub fn resolve_uvx(&self) -> Option<PathBuf> {
        let uv = self.resolve(self.uv_spec()?.name)?;
        Some(uv.with_file_name("uvx"))
    }

    /// Makes sure the pinned uv is installed and `<cache>/path` holds exactly `uv` and
    /// `uvx` pointing at it. What `forge apply` and session start call.
    pub fn ensure_uv_on_path(&self) -> Result<PathBuf, Note> {
        let spec = self
            .uv_spec()
            .ok_or_else(|| Note::new(NoteKind::ForgeFailure, "Forge's tool table has no uv."))?;
        let uv = self.ensure(spec.name)?;
        self.link_path_dir(&uv).map_err(|e| {
            io_note(
                &spec,
                &format!("cannot link it into {}", self.home.path_dir().display()),
                e,
            )
        })?;
        Ok(uv)
    }

    fn link_path_dir(&self, uv: &Path) -> io::Result<()> {
        let path_dir = self.home.path_dir();
        fs::create_dir_all(&path_dir)?;
        for name in ["uv", "uvx"] {
            let target = uv.with_file_name(name);
            let link = path_dir.join(name);
            if fs::read_link(&link).is_ok_and(|t| t == target) {
                continue;
            }
            fsutil::replace_symlink(&target, &link)?;
        }
        // Only uv and uvx: anything else there would also land on the agent's PATH.
        for entry in fs::read_dir(&path_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            if name != "uv" && name != "uvx" {
                let p = entry.path();
                let _ = fs::remove_dir_all(&p).or_else(|_| fs::remove_file(&p));
            }
        }
        Ok(())
    }

    /// Ensures each tool (for `forge apply`). Never stops at the first failure.
    pub fn ensure_all<'a>(&self, names: impl IntoIterator<Item = &'a str>) -> ProvisionReport {
        let mut report = ProvisionReport::default();
        for name in names {
            let Some(spec) = self.spec(name) else {
                report.notes.push(unknown(name));
                continue;
            };
            let before = self.resolve(name).is_some();
            let r = if matches!(spec.source, Source::Uv { .. }) {
                self.ensure_uv_on_path()
            } else {
                self.ensure(name)
            };
            match r {
                Ok(path) => report.tools.push(Provisioned {
                    name: spec.name.to_owned(),
                    version: spec.version.to_owned(),
                    path,
                    installed_now: !before,
                }),
                Err(n) => report.notes.push(n),
            }
        }
        report
    }

    fn install(&self, spec: &ToolSpec, dir: &Path) -> Result<(), Note> {
        match spec.source {
            Source::Uv { .. } => self.install_asset(spec, dir, &["uv", "uvx"]),
            Source::Bun { .. } => self.install_asset(spec, dir, &["bun"]),
            Source::Release { binary, .. } => self.install_asset(spec, dir, &[binary]),
            Source::UvTool {
                package,
                binary,
                binary_sha256,
            } => {
                let uv_spec = self
                    .uv_spec()
                    .ok_or_else(|| needs(spec, "uv", "Forge's tool table has no uv"))?;
                let uv = self.ensure(uv_spec.name)?;
                self.install_uv_tool(spec, dir, &uv, package, binary, binary_sha256)
            }
            Source::Npm { package, binary } => {
                let bun_spec = self
                    .bun_spec()
                    .ok_or_else(|| needs(spec, "Bun", "Forge's tool table has no Bun"))?;
                let bun = self.ensure(bun_spec.name)?;
                self.install_npm(spec, dir, &bun, package, binary)
            }
        }
    }

    fn install_asset(&self, spec: &ToolSpec, dir: &Path, names: &[&str]) -> Result<(), Note> {
        let platform = self.platform.ok_or_else(|| no_platform(spec, None))?;
        let (base, asset) = spec
            .asset_for(&platform.lookup_keys())
            .ok_or_else(|| no_platform(spec, Some(platform)))?;
        let parent = dir.parent().expect("tool dirs have a parent");
        let url = format!("{base}{}", asset.name);
        let dl = TempPath::new(fsutil::temp_sibling(parent, "download"));
        let got = self
            .http
            .download(&url, dl.path(), http::MAX_DOWNLOAD)
            .map_err(|e| {
                Note::new(
                    NoteKind::Environment,
                    format!(
                        "Could not download {} {} ({url}): {e}. The checks that use it are skipped until it is downloaded; `forge apply` tries again.",
                        spec.name, spec.version
                    ),
                )
            })?;
        if got != asset.sha256 {
            return Err(Note::new(
                NoteKind::Setup,
                format!(
                    "The downloaded {} {} ({url}) does not match its pinned sha256 {} (got {got}), so it was not installed. The checks that use it are skipped.",
                    spec.name, spec.version, asset.sha256
                ),
            ));
        }
        let stage = TempPath::new(fsutil::temp_sibling(parent, "stage"));
        fs::create_dir(stage.path())
            .map_err(|e| io_note(spec, "cannot create its directory", e))?;
        archive::extract(dl.path(), Kind::of(asset.name), names, stage.path()).map_err(|e| {
            Note::new(
                NoteKind::Setup,
                format!(
                    "Could not unpack {} {} ({url}): {e}. The checks that use it are skipped.",
                    spec.name, spec.version
                ),
            )
        })?;
        fsutil::replace_dir(stage, dir).map_err(|e| io_note(spec, "cannot move it into place", e))
    }

    fn fresh_dir(spec: &ToolSpec, dir: &Path) -> Result<(), Note> {
        if dir.exists() {
            fs::remove_dir_all(dir)
                .map_err(|e| io_note(spec, "cannot remove an incomplete install", e))?;
        }
        fs::create_dir_all(dir).map_err(|e| io_note(spec, "cannot create its directory", e))
    }

    fn install_uv_tool(
        &self,
        spec: &ToolSpec,
        dir: &Path,
        uv: &Path,
        package: &str,
        binary: &str,
        binary_sha256: &[(&str, &str)],
    ) -> Result<(), Note> {
        Self::fresh_dir(spec, dir)?;
        let tools = self.home.tools_dir();
        let mut cmd = Command::new(uv);
        cmd.args(["tool", "install", "--quiet", "--managed-python", "--python"])
            .arg(TOOLS_PYTHON)
            .arg(format!("{package}=={}", spec.version))
            // Run from the cache, so no project's uv.toml or pyproject applies.
            .current_dir(dir)
            .env("UV_TOOL_DIR", dir.join("env"))
            .env("UV_TOOL_BIN_DIR", dir.join("bin"))
            .env("UV_CACHE_DIR", tools.join(".uv-cache"))
            .env("UV_PYTHON_INSTALL_DIR", tools.join(".python"))
            .env("UV_NO_PROGRESS", "1")
            .env_remove("VIRTUAL_ENV")
            .env_remove("UV_PYTHON")
            .env_remove("UV_SYSTEM_PYTHON");
        run(spec, "uv", cmd)?;
        let exe = Self::exe(spec, dir);
        if !fsutil::is_executable(&exe) {
            let _ = fs::remove_dir_all(dir);
            return Err(Note::new(
                NoteKind::Setup,
                format!(
                    "uv installed {package} {} but it has no `{binary}` executable; the checks that use it are skipped.",
                    spec.version
                ),
            ));
        }
        if !binary_sha256.is_empty() {
            self.check_binary(spec, dir, &exe, binary_sha256)?;
        }
        write_marker(spec, dir)
    }

    /// For a wheel that repackages an upstream binary: it must be that binary.
    fn check_binary(
        &self,
        spec: &ToolSpec,
        dir: &Path,
        exe: &Path,
        shas: &[(&str, &str)],
    ) -> Result<(), Note> {
        let platform = self.platform.ok_or_else(|| no_platform(spec, None))?;
        let keys = platform.lookup_keys();
        let Some(want) = keys
            .iter()
            .find_map(|k| shas.iter().find(|(p, _)| p == k).map(|(_, s)| *s))
        else {
            let _ = fs::remove_dir_all(dir);
            return Err(no_platform(spec, Some(platform)));
        };
        let got = http::sha256_file(exe).map_err(|e| io_note(spec, "cannot read it", e))?;
        if got != want {
            let _ = fs::remove_dir_all(dir);
            return Err(Note::new(
                NoteKind::Setup,
                format!(
                    "The {} {} binary uv installed does not match the upstream release's sha256 {want} (got {got}), so it was removed. The checks that use it are skipped.",
                    spec.name, spec.version
                ),
            ));
        }
        Ok(())
    }

    fn install_npm(
        &self,
        spec: &ToolSpec,
        dir: &Path,
        bun: &Path,
        package: &str,
        binary: &str,
    ) -> Result<(), Note> {
        Self::fresh_dir(spec, dir)?;
        let manifest = format!(
            "{{\"name\":\"forge-tool-{}\",\"private\":true,\"dependencies\":{{\"{package}\":\"{}\"}}}}\n",
            spec.name, spec.version
        );
        fs::write(dir.join("package.json"), manifest)
            .map_err(|e| io_note(spec, "cannot write its package.json", e))?;
        let mut cmd = Command::new(bun);
        cmd.args([
            "install",
            "--production",
            "--ignore-scripts",
            "--no-progress",
            "--no-summary",
        ])
        .current_dir(dir)
        .env(
            "BUN_INSTALL_CACHE_DIR",
            self.home.tools_dir().join(".bun-cache"),
        )
        .env("NO_COLOR", "1");
        run(spec, "Bun", cmd)?;
        let script = dir.join("node_modules").join(".bin").join(binary);
        if !script.exists() {
            let _ = fs::remove_dir_all(dir);
            return Err(Note::new(
                NoteKind::Setup,
                format!(
                    "Bun installed {package} {} but it has no `{binary}` command; the checks that use it are skipped.",
                    spec.version
                ),
            ));
        }
        let (Some(bun_s), Some(script_s)) = (bun.to_str(), script.to_str()) else {
            return Err(io_note(
                spec,
                "its path is not UTF-8",
                io::Error::other("non-UTF-8 cache path"),
            ));
        };
        let wrapper = format!(
            "#!/bin/sh\nexec {} --bun {} \"$@\"\n",
            sh_quote(bun_s),
            sh_quote(script_s)
        );
        let exe = Self::exe(spec, dir);
        fs::create_dir_all(dir.join("bin"))
            .and_then(|()| fs::write(&exe, wrapper))
            .and_then(|()| archive::set_executable(&exe))
            .map_err(|e| io_note(spec, "cannot write its launcher", e))?;
        write_marker(spec, dir)
    }
}

impl Provisioner {
    /// The tool's executable when it is already installed. Never downloads: a couple of
    /// `stat` calls, safe on a hook's hot path.
    pub fn resolve(&self, tool: &str) -> Option<PathBuf> {
        let spec = self.spec(tool)?;
        let dir = self.dir(&spec)?;
        Self::complete(&spec, &dir).then(|| Self::exe(&spec, &dir))
    }

    /// The tool's executable, installing it (and uv or Bun first, when it needs them) if
    /// it is missing. May download: call it only where a person waits anyway (`forge
    /// apply`, session start in the background, `forge check`). A failure is a note for
    /// the person: the checks that need the tool are skipped, never failed.
    pub fn ensure(&self, tool: &str) -> Result<PathBuf, Note> {
        let spec = self.spec(tool).ok_or_else(|| unknown(tool))?;
        let dir = self.dir(&spec).ok_or_else(|| {
            Note::new(
                NoteKind::ForgeFailure,
                format!("Forge's tool table pins {tool} to an invalid version."),
            )
        })?;
        if Self::complete(&spec, &dir) {
            return Ok(Self::exe(&spec, &dir));
        }
        let parent = dir.parent().expect("tool dirs have a parent");
        let _lock = FileLock::acquire(&parent.join(format!(".{}.lock", spec.version)))
            .map_err(|e| io_note(&spec, "cannot lock its directory", e))?;
        // Another process may have installed it while we waited.
        if !Self::complete(&spec, &dir) {
            self.install(&spec, &dir)?;
        }
        Ok(Self::exe(&spec, &dir))
    }
}

fn run(spec: &ToolSpec, by: &str, mut cmd: Command) -> Result<(), Note> {
    let out = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| {
            Note::new(
                NoteKind::ForgeFailure,
                format!(
                    "Could not run {by} to install {} {}: {e}.",
                    spec.name, spec.version
                ),
            )
        })?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let why = stderr
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("no error message");
    Err(Note::new(
        NoteKind::Environment,
        format!(
            "{by} could not install {} {} ({why}). The checks that use it are skipped until it is installed; `forge apply` tries again.",
            spec.name, spec.version
        ),
    ))
}

fn write_marker(spec: &ToolSpec, dir: &Path) -> Result<(), Note> {
    fs::write(
        dir.join(MARKER),
        format!("{} {}\n", spec.name, spec.version),
    )
    .map_err(|e| io_note(spec, "cannot finish the install", e))
}

fn unknown(name: &str) -> Note {
    Note::new(
        NoteKind::ForgeFailure,
        format!("Forge has no pinned tool named {name}."),
    )
}

fn needs(spec: &ToolSpec, what: &str, why: &str) -> Note {
    Note::new(
        NoteKind::ForgeFailure,
        format!("{} {} needs {what}: {why}.", spec.name, spec.version),
    )
}

fn no_platform(spec: &ToolSpec, platform: Option<Platform>) -> Note {
    let on = platform.map_or_else(|| "this platform".to_owned(), |p| p.to_string());
    Note::new(
        NoteKind::MissingTool,
        format!(
            "Forge has no pinned {} {} for {on}; the checks that use it are skipped.",
            spec.name, spec.version
        ),
    )
}

fn io_note(spec: &ToolSpec, what: &str, e: io::Error) -> Note {
    Note::new(
        NoteKind::ForgeFailure,
        format!(
            "Could not install {} {} into Forge's cache: {what} ({e}).",
            spec.name, spec.version
        ),
    )
}

/// Forge's pinned tools from the cache, as forge-core's [`forge_core::tools::ToolResolver`]
/// answers them: what the checker and the engine ask. `pinned` never downloads; `ensure`
/// installs a missing tool only when `download` is set (`forge apply`, `forge check`, Stop).
#[derive(Debug)]
pub struct PinnedTools {
    pub provisioner: Provisioner,
    pub download: bool,
}

impl PinnedTools {
    pub fn new(provisioner: Provisioner, download: bool) -> PinnedTools {
        PinnedTools {
            provisioner,
            download,
        }
    }

    /// The tool of this Forge's table at `version`; another version is not Forge's pin.
    fn spec_at(&self, name: &str, version: &str) -> Result<ToolSpec, String> {
        let spec = self
            .provisioner
            .spec(name)
            .ok_or_else(|| format!("Forge has no pinned tool named {name}"))?;
        if spec.version != version {
            return Err(format!(
                "{name} {version} is not this Forge's pin ({name} {})",
                spec.version
            ));
        }
        Ok(spec)
    }
}

impl forge_core::tools::ToolResolver for PinnedTools {
    fn pinned(&self, name: &str, version: &str) -> Result<forge_core::tools::ToolCmd, String> {
        let spec = self.spec_at(name, version)?;
        self.provisioner
            .resolve(spec.name)
            .map(forge_core::tools::ToolCmd::exe)
            .ok_or_else(|| format!("{name} {version} is not installed in Forge's cache"))
    }

    fn ensure(&self, name: &str, version: &str) -> Result<forge_core::tools::ToolCmd, String> {
        if !self.download {
            return self.pinned(name, version);
        }
        let spec = self.spec_at(name, version)?;
        self.provisioner
            .ensure(spec.name)
            .map(forge_core::tools::ToolCmd::exe)
            .map_err(|n| n.text)
    }
}
