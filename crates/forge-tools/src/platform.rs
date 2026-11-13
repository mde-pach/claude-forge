//! The machine a download is for.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Os {
    Linux,
    Darwin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Arch {
    X86_64,
    Aarch64,
}

/// An OS, an architecture and, on Linux, whether the C library is musl.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Platform {
    pub os: Os,
    pub arch: Arch,
    /// Linux with musl (Alpine). Only Bun ships separate musl builds; uv and Forge are
    /// static musl binaries that run on both.
    pub musl: bool,
}

impl Platform {
    /// The platform this binary was built for, `None` where Forge ships no downloads.
    pub fn current() -> Option<Platform> {
        let os = if cfg!(target_os = "linux") {
            Os::Linux
        } else if cfg!(target_os = "macos") {
            Os::Darwin
        } else {
            return None;
        };
        let arch = if cfg!(target_arch = "x86_64") {
            Arch::X86_64
        } else if cfg!(target_arch = "aarch64") {
            Arch::Aarch64
        } else {
            return None;
        };
        Some(Platform {
            os,
            arch,
            musl: os == Os::Linux && linux_is_musl(),
        })
    }

    /// The key of the pin tables: `uname -s`-`uname -m` (macOS arm is `arm64`, as uname says).
    pub fn key(&self) -> &'static str {
        match (self.os, self.arch) {
            (Os::Linux, Arch::X86_64) => "Linux-x86_64",
            (Os::Linux, Arch::Aarch64) => "Linux-aarch64",
            (Os::Darwin, Arch::X86_64) => "Darwin-x86_64",
            (Os::Darwin, Arch::Aarch64) => "Darwin-arm64",
        }
    }

    /// The keys to look up, best first: `<key>-musl` before `<key>` on musl Linux.
    pub fn lookup_keys(&self) -> Vec<String> {
        let mut v = Vec::with_capacity(2);
        if self.musl {
            v.push(format!("{}-musl", self.key()));
        }
        v.push(self.key().to_owned());
        v
    }

    /// The Rust target of Forge's own release for this platform (static musl on Linux).
    pub fn forge_target(&self) -> &'static str {
        match (self.os, self.arch) {
            (Os::Linux, Arch::X86_64) => "x86_64-unknown-linux-musl",
            (Os::Linux, Arch::Aarch64) => "aarch64-unknown-linux-musl",
            (Os::Darwin, Arch::X86_64) => "x86_64-apple-darwin",
            (Os::Darwin, Arch::Aarch64) => "aarch64-apple-darwin",
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())?;
        if self.musl {
            f.write_str(" (musl)")?;
        }
        Ok(())
    }
}

/// musl's dynamic loader is `/lib/ld-musl-<arch>.so.1`; glibc has none of that name.
fn linux_is_musl() -> bool {
    std::fs::read_dir("/lib").is_ok_and(|rd| {
        rd.filter_map(Result::ok)
            .any(|e| e.file_name().to_string_lossy().starts_with("ld-musl-"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        let p = Platform {
            os: Os::Linux,
            arch: Arch::Aarch64,
            musl: true,
        };
        assert_eq!(p.lookup_keys(), ["Linux-aarch64-musl", "Linux-aarch64"]);
        assert_eq!(p.forge_target(), "aarch64-unknown-linux-musl");
        let m = Platform {
            os: Os::Darwin,
            arch: Arch::Aarch64,
            musl: false,
        };
        assert_eq!(m.lookup_keys(), ["Darwin-arm64"]);
        assert!(Platform::current().is_some());
    }
}
