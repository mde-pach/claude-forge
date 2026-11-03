//! Lock files and atomic installs.
//!
//! Every install of `tools/<name>/<version>` (or `bin/forge-<version>`) happens under an
//! exclusive `flock` on a sibling lock file, so two sessions, an apply and a background
//! download never install the same thing at once: the second waits, then finds it done.
//! The lock is released by the kernel when the process dies, so a crash leaves no stale
//! lock. Files are built in a temporary sibling and renamed into place, so a reader sees
//! either nothing or the complete install.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// An exclusive lock, held until dropped.
#[derive(Debug)]
pub struct FileLock {
    _file: File,
}

impl FileLock {
    /// Waits for the lock.
    pub fn acquire(path: &Path) -> io::Result<FileLock> {
        let file = open_lock_file(path)?;
        flock(&file, libc::LOCK_EX)?;
        Ok(FileLock { _file: file })
    }

    /// Takes the lock if it is free; `None` when another process holds it.
    pub fn try_acquire(path: &Path) -> io::Result<Option<FileLock>> {
        let file = open_lock_file(path)?;
        match flock(&file, libc::LOCK_EX | libc::LOCK_NB) {
            Ok(()) => Ok(Some(FileLock { _file: file })),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e),
        }
    }
}

fn open_lock_file(path: &Path) -> io::Result<File> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
}

fn flock(file: &File, op: libc::c_int) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    loop {
        // SAFETY: flock on a file descriptor we own; it has no memory effects.
        let rc = unsafe { libc::flock(file.as_raw_fd(), op) };
        if rc == 0 {
            return Ok(());
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

/// A unique name for a temporary sibling: `.<stem>.<pid>.<n>.tmp`.
pub fn temp_sibling(dir: &Path, stem: &str) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    dir.join(format!(
        ".{stem}.{}.{}{nanos}.tmp",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Removes a temporary file or directory when dropped, unless [`TempPath::keep`] was called.
#[derive(Debug)]
pub struct TempPath {
    path: Option<PathBuf>,
}

impl TempPath {
    pub fn new(path: PathBuf) -> TempPath {
        TempPath { path: Some(path) }
    }

    pub fn path(&self) -> &Path {
        self.path.as_deref().expect("temp path already kept")
    }

    /// The path is now owned elsewhere (renamed into place).
    pub fn keep(mut self) {
        self.path = None;
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        if let Some(p) = self.path.take() {
            let _ = fs::remove_dir_all(&p).or_else(|_| fs::remove_file(&p));
        }
    }
}

/// Moves the directory `staged` to `dest`, replacing an incomplete `dest`.
pub fn replace_dir(staged: TempPath, dest: &Path) -> io::Result<()> {
    if dest.exists() {
        fs::remove_dir_all(dest)?;
    }
    fs::rename(staged.path(), dest)?;
    staged.keep();
    Ok(())
}

/// Points the symlink `link` at `target`, atomically (a new link renamed over the old one).
pub fn replace_symlink(target: &Path, link: &Path) -> io::Result<()> {
    let dir = link
        .parent()
        .ok_or_else(|| io::Error::other("a link needs a directory"))?;
    fs::create_dir_all(dir)?;
    let tmp = TempPath::new(temp_sibling(dir, "link"));
    std::os::unix::fs::symlink(target, tmp.path())?;
    fs::rename(tmp.path(), link)?;
    tmp.keep();
    Ok(())
}

pub fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}
