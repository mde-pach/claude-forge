//! Taking executables out of a verified release asset.
//!
//! Only regular files whose last path component is one of the wanted names are read, and
//! each is written to `<dest>/<name>` under that fixed name, so an archive's paths
//! (`../`, absolute, symlinks) never decide where anything lands.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    TarGz,
    Zip,
    /// The asset is the executable itself.
    Raw,
}

impl Kind {
    pub fn of(asset_name: &str) -> Kind {
        if asset_name.ends_with(".tar.gz") || asset_name.ends_with(".tgz") {
            Kind::TarGz
        } else if asset_name.ends_with(".zip") {
            Kind::Zip
        } else {
            Kind::Raw
        }
    }
}

/// Writes each of `names` from `archive` into `dest` (an existing directory), mode 755.
/// Fails when one is missing.
pub fn extract(archive: &Path, kind: Kind, names: &[&str], dest: &Path) -> io::Result<()> {
    let mut found = BTreeSet::new();
    match kind {
        Kind::Raw => {
            let [name] = names else {
                return Err(io::Error::other("a raw asset holds exactly one executable"));
            };
            fs::copy(archive, dest.join(name))?;
            found.insert(*name);
        }
        Kind::TarGz => {
            let gz = flate2::read::GzDecoder::new(File::open(archive)?);
            let mut tar = tar::Archive::new(gz);
            for entry in tar.entries()? {
                let mut entry = entry?;
                if !entry.header().entry_type().is_file() {
                    continue;
                }
                let path = entry.path()?.into_owned();
                let Some(base) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if let Some(name) = names.iter().find(|n| **n == base) {
                    if found.insert(*name) {
                        write_member(&mut entry, &dest.join(name))?;
                    }
                }
            }
        }
        Kind::Zip => {
            let mut zip = zip::ZipArchive::new(File::open(archive)?).map_err(io::Error::other)?;
            for i in 0..zip.len() {
                let mut file = zip.by_index(i).map_err(io::Error::other)?;
                if !file.is_file() {
                    continue;
                }
                let Ok(raw) = file.name().map(|n| n.into_owned()) else {
                    continue;
                };
                let base = raw.rsplit(['/', '\\']).next().unwrap_or("");
                if let Some(name) = names.iter().find(|n| **n == base) {
                    if found.insert(*name) {
                        write_member(&mut file, &dest.join(name))?;
                    }
                }
            }
        }
    }
    for n in names {
        if !found.contains(n) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("the archive holds no {n}"),
            ));
        }
        set_executable(&dest.join(n))?;
    }
    Ok(())
}

fn write_member(r: &mut impl Read, to: &Path) -> io::Result<()> {
    let mut out = File::create(to)?;
    io::copy(r, &mut out)?;
    out.sync_all()
}

pub fn set_executable(p: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(p, fs::Permissions::from_mode(0o755))
    }
    #[cfg(not(unix))]
    {
        let _ = p;
        Ok(())
    }
}
