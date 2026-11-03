//! The project as it was at the base (`forge_base_tree`), in a temporary directory, for running
//! a typecheck or the failing tests there (§10.1 step 4, §10.6).
//!
//! The files git sees now (tracked and untracked, not ignored) are copied, with the change
//! undone from the base copies (files added by the change left out). Of the ignored paths:
//! Python virtual environments are linked, the project's own code they import (editable
//! installs) taken from the copy (`PYTHONPATH`); each `node_modules` is rebuilt from links to its
//! packages, a link to a package of the repository pointing to that package in the copy; caches
//! test runs rewrite are left out; everything else (`.env`, a SQLite file, `dist/`) is copied.
//! The repository, its index and its working tree are never written.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::change::{ChangeSet, Git, split_z};

/// Ignored files over this size in all are not copied: the comparison is not possible.
pub const COPY_LIMIT_BYTES: u64 = 256 * 1024 * 1024;

const CACHES: &[&str] = &[
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".hypothesis",
    ".coverage",
    "coverage",
    "htmlcov",
    ".nyc_output",
    ".vite",
    ".vitest",
    ".turbo",
    ".eslintcache",
];

/// A base tree.
pub struct BaseTree {
    pub dir: tempfile::TempDir,
    /// Paths to put first on PYTHONPATH there.
    pub pythonpath: Vec<PathBuf>,
}

impl BaseTree {
    pub fn path(&self) -> &Path {
        self.dir.path()
    }
}

fn dir_size(p: &Path) -> u64 {
    let Ok(m) = std::fs::symlink_metadata(p) else {
        return 0;
    };
    if m.is_file() {
        return m.len();
    }
    if !m.is_dir() {
        return 0;
    }
    std::fs::read_dir(p)
        .map(|rd| rd.flatten().map(|e| dir_size(&e.path())).sum())
        .unwrap_or(0)
}

fn copy_rec(src: &Path, dst: &Path) -> std::io::Result<()> {
    let m = std::fs::symlink_metadata(src)?;
    if let Some(d) = dst.parent() {
        std::fs::create_dir_all(d)?;
    }
    if m.file_type().is_symlink() {
        let to = std::fs::read_link(src)?;
        std::os::unix::fs::symlink(to, dst)
    } else if m.is_dir() {
        std::fs::create_dir_all(dst)?;
        for e in std::fs::read_dir(src)?.flatten() {
            copy_rec(&e.path(), &dst.join(e.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(src, dst).map(|_| ())
    }
}

fn link(to: &Path, at: &Path) -> std::io::Result<()> {
    if let Some(d) = at.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::os::unix::fs::symlink(to, at)
}

/// The files git sees in `root` (tracked, and untracked but not ignored) that `keep` accepts,
/// as they are in the base, copied into `dest`: a changed file from its base copy (`inbase`,
/// `base_copy`), a file the change added left out. The first step of [`build`], and the whole
/// copy for a tool that reads only source files (`forge_condor`).
pub fn copy_sources(
    root: &Path,
    dest: &Path,
    is_changed: &dyn Fn(&str) -> bool,
    inbase: &[String],
    base_copy: &dyn Fn(&str) -> Option<PathBuf>,
    keep: &dyn Fn(&str) -> bool,
) -> Result<(), String> {
    let files = Git::new(root)
        .run(&[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .map_err(|_| "the files of the tree could not be listed".to_string())?;
    for f in split_z(&files) {
        if is_changed(&f) || !keep(&f) {
            continue;
        }
        let src = root.join(&f);
        if src.symlink_metadata().is_ok() {
            copy_rec(&src, &dest.join(&f)).map_err(|e| format!("{f} could not be copied ({e})"))?;
        }
    }
    for f in inbase.iter().filter(|f| keep(f)) {
        if let Some(b) = base_copy(f) {
            copy_rec(&b, &dest.join(f)).map_err(|e| format!("{f} could not be copied ({e})"))?;
        }
    }
    Ok(())
}

/// Builds the base tree; `Err` says why the copy cannot be made or isolated (for the person).
pub fn build(change: &ChangeSet) -> Result<BaseTree, String> {
    let root = std::fs::canonicalize(&change.root).unwrap_or_else(|_| change.root.clone());
    let git = Git::new(&root);
    let dir = tempfile::Builder::new()
        .prefix("forge-base-tree.")
        .tempdir()
        .map_err(|e| format!("the copy of the tree could not be made ({e})"))?;
    let dest = dir.path().to_path_buf();
    copy_sources(
        &root,
        &dest,
        &|f| change.is_changed(f),
        &change.inbase,
        &|f| change.base_copy(f),
        &|_| true,
    )?;
    let ignored = git
        .run(&[
            "ls-files",
            "-z",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
        ])
        .unwrap_or_default();
    let mut venvs = Vec::new();
    let mut copy = Vec::new();
    for f in split_z(&ignored) {
        let f = f.trim_end_matches('/').to_string();
        if f.is_empty() || dest.join(&f).symlink_metadata().is_ok() {
            continue;
        }
        let name = f.rsplit('/').next().unwrap_or(&f);
        if CACHES.contains(&name)
            || name.starts_with(".coverage.")
            || name.ends_with(".tsbuildinfo")
        {
            continue;
        }
        let src = root.join(&f);
        if src.is_dir() && src.join("pyvenv.cfg").is_file() {
            link(&src, &dest.join(&f)).map_err(|e| e.to_string())?;
            venvs.push(f);
        } else if name == "node_modules" && src.is_dir() {
            modules(&root, &f, &dest).map_err(|e| format!("{f} could not be linked ({e})"))?;
        } else {
            copy.push(f);
        }
    }
    let size: u64 = copy.iter().map(|f| dir_size(&root.join(f))).sum();
    if size > COPY_LIMIT_BYTES {
        return Err(format!(
            "the files git ignores here ({} KiB, e.g. {}) are too large to copy",
            size / 1024,
            copy[0]
        ));
    }
    for f in &copy {
        let _ = copy_rec(&root.join(f), &dest.join(f));
    }
    let pythonpath = if venvs.is_empty() {
        Vec::new()
    } else {
        python_paths(&root, &venvs, &dest)?
    };
    Ok(BaseTree { dir, pythonpath })
}

/// `forge_base_modules`: `dest/<nm>` as a directory of links to the packages of `<nm>`.
fn modules(root: &Path, nm: &str, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest.join(nm))?;
    for e in std::fs::read_dir(root.join(nm))?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if matches!(name.as_str(), ".vite" | ".vitest" | ".cache") {
            continue;
        }
        let rel = format!("{nm}/{name}");
        let ft = e.file_type()?;
        if ft.is_dir() && name.starts_with('@') {
            std::fs::create_dir_all(dest.join(&rel))?;
            for s in std::fs::read_dir(e.path())?.flatten() {
                let srel = format!("{rel}/{}", s.file_name().to_string_lossy());
                link_package(root, &srel, dest)?;
            }
            continue;
        }
        link_package(root, &rel, dest)?;
    }
    Ok(())
}

/// `forge_base_link`: a link to the package, or, for a link to a directory of the repository
/// outside node_modules (a workspace package), to that directory in the copy.
fn link_package(root: &Path, rel: &str, dest: &Path) -> std::io::Result<()> {
    let src = root.join(rel);
    let mut to = src.clone();
    if src.symlink_metadata()?.file_type().is_symlink()
        && let Ok(real) = std::fs::canonicalize(&src)
        && real.is_dir()
        && let Ok(inside) = real.strip_prefix(root)
        && !format!("/{}/", inside.display()).contains("/node_modules/")
    {
        to = dest.join(inside);
    }
    link(&to, &dest.join(rel))
}

/// The project modules a venv's python could import from the working tree (`FORGE_PY_LEAKS`).
const PY_LEAKS: &str = r#"import importlib.util, os, sys
root, venv = os.path.realpath(sys.argv[1]), os.path.realpath(sys.argv[2])
def real(p):
    p = os.path.realpath(p)
    return (p == root or p.startswith(root + os.sep)) and not (p == venv or p.startswith(venv + os.sep))
names = set()
for p in sys.path:
    if p and os.path.isdir(p) and real(p):
        for e in os.listdir(p):
            if e.endswith(".py") and e[:-3].isidentifier(): names.add(e[:-3])
            elif e.isidentifier() and os.path.isfile(os.path.join(p, e, "__init__.py")): names.add(e)
for m in list(sys.modules.values()):
    mapping = getattr(m, "MAPPING", None)
    if (getattr(m, "__name__", "") or "").startswith("__editable__") and isinstance(mapping, dict):
        names.update(k for k, v in mapping.items() if isinstance(v, str) and real(v))
leak = []
for n in sorted(x for x in names if not x.startswith("__")):
    try:
        s = importlib.util.find_spec(n)
    except Exception:
        continue
    if s is None: continue
    where = s.origin if s.origin not in (None, "namespace", "built-in", "frozen") else next(iter(s.submodule_search_locations or []), None)
    if where and real(where): leak.append(n)
print(" ".join(leak[:5]))
"#;

/// `forge_base_python`: the repository paths the venvs' .pth files and editable finders add,
/// as their copies; then each venv's python checks it imports no project module from the
/// working tree.
fn python_paths(root: &Path, venvs: &[String], dest: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths: Vec<PathBuf> = Vec::new();
    let mapping = regex::Regex::new(r#"'(/[^']+)'|"(/[^"]+)""#).expect("regex");
    for v in venvs {
        for lib in ["lib", "lib64"] {
            let Ok(rd) = std::fs::read_dir(root.join(v).join(lib)) else {
                continue;
            };
            for py in rd.flatten() {
                let sp = py.path().join("site-packages");
                let Ok(entries) = std::fs::read_dir(&sp) else {
                    continue;
                };
                for e in entries.flatten() {
                    let n = e.file_name().to_string_lossy().to_string();
                    let text = std::fs::read_to_string(e.path()).unwrap_or_default();
                    let mut found: Vec<PathBuf> = Vec::new();
                    if n.ends_with(".pth") {
                        found.extend(
                            text.lines()
                                .filter(|l| !l.starts_with("import") && !l.starts_with('#'))
                                .map(PathBuf::from),
                        );
                    } else if n.starts_with("__editable__") && n.ends_with("finder.py") {
                        for l in text.lines().filter(|l| l.starts_with("MAPPING")) {
                            for c in mapping.captures_iter(l) {
                                let p = c
                                    .get(1)
                                    .or_else(|| c.get(2))
                                    .map(|m| m.as_str())
                                    .unwrap_or("");
                                if let Some(parent) = Path::new(p).parent() {
                                    found.push(parent.to_path_buf());
                                }
                            }
                        }
                    }
                    for p in found {
                        if let Ok(rel) = p.strip_prefix(root) {
                            let to = dest.join(rel);
                            if !paths.contains(&to) {
                                paths.push(to);
                            }
                        }
                    }
                }
            }
        }
    }
    let pp = std::env::join_paths(
        paths.iter().chain(
            std::env::var_os("PYTHONPATH")
                .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
                .unwrap_or_default()
                .iter(),
        ),
    )
    .unwrap_or_default();
    for v in venvs {
        let python = root.join(v).join("bin/python");
        if !python.exists() {
            continue;
        }
        let venv_real = std::fs::canonicalize(root.join(v)).unwrap_or_else(|_| root.join(v));
        let out = Command::new(&python)
            .current_dir(dest)
            .env("PYTHONPATH", &pp)
            .args(["-c", PY_LEAKS])
            .arg(root)
            .arg(&venv_real)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|_| format!("could not check which code {v}/bin/python imports"))?;
        if !out.status.success() {
            return Err(format!("could not check which code {v}/bin/python imports"));
        }
        let leak = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !leak.is_empty() {
            return Err(format!(
                "{v}/bin/python imports {leak} from the working tree, not from the copy"
            ));
        }
    }
    Ok(paths)
}
