//! The parsed-packs cache: a loaded tree serialized (MessagePack) under a directory the
//! caller chooses, keyed by a hash of its inputs and of this build. A missing, stale or
//! corrupt entry is reparsed and rewritten; the cache never changes what is loaded.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::LoadError;
use crate::loader::Loaded;

/// Bumped when the entry's layout changes in a way the build id would not show.
const FORMAT: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheOutcome {
    /// No cache directory was given.
    Disabled,
    /// Read from the cache.
    Hit,
    /// No entry: parsed and written.
    Miss,
    /// An entry for other inputs or another build: parsed and rewritten.
    Stale,
    /// An entry that does not decode: parsed and rewritten.
    Corrupt,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    format: u32,
    key: String,
    loaded: Loaded,
}

#[derive(Deserialize)]
struct Header {
    format: u32,
    key: String,
}

/// The key of a set of inputs: this build, then each file's path and bytes.
pub fn key<'a>(files: impl IntoIterator<Item = (String, std::borrow::Cow<'a, [u8]>)>) -> String {
    let mut h = blake3::Hasher::new();
    h.update(crate::BUILD_ID.as_bytes());
    for (path, bytes) in files {
        h.update(&(path.len() as u64).to_le_bytes());
        h.update(path.as_bytes());
        h.update(&(bytes.len() as u64).to_le_bytes());
        h.update(&bytes);
    }
    h.finalize().to_hex().to_string()
}

/// The loaded tree from `<dir>/<name>.msgpack` when its key is `key`, else from `load`
/// (then written there, best effort: a cache that cannot be written is no error).
pub fn cached(
    dir: Option<&Path>,
    name: &str,
    key: &str,
    load: impl FnOnce() -> Result<Loaded, LoadError>,
) -> Result<(Loaded, CacheOutcome), LoadError> {
    let Some(dir) = dir else {
        return Ok((load()?, CacheOutcome::Disabled));
    };
    let file = dir.join(format!("{name}.msgpack"));
    let outcome = match std::fs::read(&file) {
        Err(_) => CacheOutcome::Miss,
        Ok(bytes) => match rmp_serde::from_slice::<Entry>(&bytes) {
            Ok(entry) if entry.format == FORMAT && entry.key == key => {
                return Ok((entry.loaded, CacheOutcome::Hit));
            }
            Ok(_) => CacheOutcome::Stale,
            // another build's layout reads as stale when its header still decodes
            Err(_) => match rmp_serde::from_slice::<Header>(&bytes) {
                Ok(h) if h.format != FORMAT || h.key != key => CacheOutcome::Stale,
                _ => CacheOutcome::Corrupt,
            },
        },
    };
    let loaded = load()?;
    let entry = Entry {
        format: FORMAT,
        key: key.to_string(),
        loaded,
    };
    if let Ok(bytes) = rmp_serde::to_vec_named(&entry) {
        let _ = write_atomic(dir, &file, &bytes);
    }
    Ok((entry.loaded, outcome))
}

/// Writes through a temporary file renamed into place, so readers never see half an entry.
fn write_atomic(dir: &Path, file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let tmp = file.with_extension(format!("tmp.{}", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, file).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}
