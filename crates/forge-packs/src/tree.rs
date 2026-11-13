//! A read-only file tree the loader walks: the built-in packs embedded in the binary, or a
//! directory on disk (`.forge/packs`, `.forge/rules`). Paths are relative, "/"-separated,
//! "" for the root.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;

use include_dir::{Dir, DirEntry};

pub trait Tree {
    /// The entries of a directory, sorted by name, with whether each is a directory; None
    /// when `dir` is not a directory.
    fn list(&self, dir: &str) -> Option<Vec<(String, bool)>>;
    /// A file's bytes; None when it is not a file.
    fn read(&self, file: &str) -> Result<Option<Cow<'_, [u8]>>, String>;

    fn is_dir(&self, dir: &str) -> bool {
        self.list(dir).is_some()
    }

    /// Every file below the root, sorted.
    fn all_files(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut stack = vec![String::new()];
        while let Some(dir) = stack.pop() {
            for (name, is_dir) in self.list(&dir).unwrap_or_default() {
                let path = join(&dir, &name);
                if is_dir {
                    stack.push(path);
                } else {
                    out.push(path);
                }
            }
        }
        out.sort();
        out
    }
}

pub fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// The packs compiled into the binary, indexed by path (include_dir's own lookups compare
/// paths component by component, entry by entry).
pub struct Embedded {
    dirs: HashMap<String, Vec<(String, bool)>>,
    files: HashMap<String, &'static [u8]>,
}

impl Embedded {
    pub fn new(root: &'static Dir<'static>) -> Self {
        let mut tree = Embedded {
            dirs: HashMap::new(),
            files: HashMap::new(),
        };
        tree.index("", root);
        tree
    }

    fn index(&mut self, at: &str, dir: &'static Dir<'static>) {
        let mut entries = Vec::new();
        for e in dir.entries() {
            let Some(name) = e
                .path()
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
            else {
                continue;
            };
            let path = join(at, &name);
            match e {
                DirEntry::Dir(d) => {
                    entries.push((name, true));
                    self.index(&path, d);
                }
                DirEntry::File(f) => {
                    entries.push((name, false));
                    self.files.insert(path, f.contents());
                }
            }
        }
        entries.sort();
        self.dirs.insert(at.to_string(), entries);
    }
}

impl Tree for Embedded {
    fn list(&self, dir: &str) -> Option<Vec<(String, bool)>> {
        self.dirs.get(dir).cloned()
    }

    fn read(&self, file: &str) -> Result<Option<Cow<'_, [u8]>>, String> {
        Ok(self.files.get(file).map(|b| Cow::Borrowed(*b)))
    }
}

/// A directory on disk.
pub struct Disk(pub PathBuf);

impl Tree for Disk {
    fn list(&self, dir: &str) -> Option<Vec<(String, bool)>> {
        let entries = std::fs::read_dir(self.0.join(dir)).ok()?;
        let mut out: Vec<(String, bool)> = entries
            .flatten()
            .map(|e| {
                // metadata follows symbolic links, as statSync does
                let is_dir = std::fs::metadata(e.path())
                    .map(|m| m.is_dir())
                    .unwrap_or(false);
                (e.file_name().to_string_lossy().into_owned(), is_dir)
            })
            .collect();
        out.sort();
        Some(out)
    }

    fn read(&self, file: &str) -> Result<Option<Cow<'_, [u8]>>, String> {
        let path = self.0.join(file);
        if !path.is_file() {
            return Ok(None);
        }
        std::fs::read(&path)
            .map(|b| Some(Cow::Owned(b)))
            .map_err(|e| format!("{}: {e}", path.display()))
    }
}
