//! Downloads: one HTTP client, streamed to a file while hashing.
//!
//! The client is built on first use, so a process that never downloads (every hook but
//! session start) pays nothing for it. TLS is rustls with the platform's certificate
//! verifier, so a company's own root CA works; `HTTPS_PROXY`/`ALL_PROXY`/`NO_PROXY` are
//! honoured. In production only `https://` URLs are fetched and redirects cannot
//! downgrade to plain HTTP.

use std::fmt;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use sha2::{Digest, Sha256};

/// The largest download Forge accepts (Bun's archive is about 40 MB, trivy's about 60 MB).
pub const MAX_DOWNLOAD: u64 = 512 * 1024 * 1024;
/// The largest text file (a `SHA256SUMS`).
pub const MAX_TEXT: u64 = 1024 * 1024;

#[derive(Debug)]
pub enum DownloadError {
    /// Could not connect, TLS failed, timed out: most likely offline.
    Network(String),
    /// The server answered with this status (404: no such release asset).
    Status(u16),
    /// The response was larger than the limit.
    TooLarge(u64),
    /// Writing the file failed.
    Io(io::Error),
}

impl fmt::Display for DownloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DownloadError::Network(e) => write!(f, "network error: {e}"),
            DownloadError::Status(s) => write!(f, "the server answered HTTP {s}"),
            DownloadError::TooLarge(n) => write!(f, "the download is larger than {n} bytes"),
            DownloadError::Io(e) => write!(f, "cannot write the download: {e}"),
        }
    }
}

impl std::error::Error for DownloadError {}

impl From<ureq::Error> for DownloadError {
    fn from(e: ureq::Error) -> Self {
        match e {
            ureq::Error::StatusCode(s) => DownloadError::Status(s),
            ureq::Error::Io(e) => DownloadError::Network(e.to_string()),
            other => DownloadError::Network(other.to_string()),
        }
    }
}

/// An HTTP client for downloads.
pub struct Http {
    https_only: bool,
    agent: OnceLock<ureq::Agent>,
}

impl fmt::Debug for Http {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Http")
            .field("https_only", &self.https_only)
            .finish_non_exhaustive()
    }
}

impl Default for Http {
    fn default() -> Self {
        Http::new()
    }
}

impl Http {
    /// HTTPS only: what Forge uses for real downloads.
    pub fn new() -> Http {
        Http {
            https_only: true,
            agent: OnceLock::new(),
        }
    }

    /// Plain HTTP allowed: for tests against a local server.
    pub fn allowing_http() -> Http {
        Http {
            https_only: false,
            agent: OnceLock::new(),
        }
    }

    fn agent(&self) -> &ureq::Agent {
        self.agent.get_or_init(|| {
            let tls = ureq::tls::TlsConfig::builder()
                .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                .build();
            ureq::Agent::config_builder()
                .https_only(self.https_only)
                .tls_config(tls)
                .proxy(ureq::Proxy::try_from_env())
                .timeout_connect(Some(Duration::from_secs(15)))
                // Generous: a large asset on a slow link. Stalls are caught by the per-read timeout.
                .timeout_global(Some(Duration::from_secs(15 * 60)))
                .timeout_recv_body(Some(Duration::from_secs(60)))
                .user_agent(concat!("claude-forge/", env!("CARGO_PKG_VERSION")))
                .build()
                .into()
        })
    }

    /// Streams `url` into `dest` (created or truncated) and returns the sha256 (lower-case hex)
    /// of what was written. Fails past `max_bytes`.
    pub fn download(
        &self,
        url: &str,
        dest: &Path,
        max_bytes: u64,
    ) -> Result<String, DownloadError> {
        let resp = self.agent().get(url).call()?;
        let mut reader = resp
            .into_body()
            .into_with_config()
            .limit(max_bytes.saturating_add(1))
            .reader();
        let mut file = File::create(dest).map_err(DownloadError::Io)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        let mut total: u64 = 0;
        loop {
            let n = match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(classify_read_error(e, max_bytes)),
            };
            total += n as u64;
            if total > max_bytes {
                return Err(DownloadError::TooLarge(max_bytes));
            }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n]).map_err(DownloadError::Io)?;
        }
        file.sync_all().map_err(DownloadError::Io)?;
        Ok(hex(&hasher.finalize()))
    }

    /// A small text file (a checksum list).
    pub fn get_text(&self, url: &str) -> Result<String, DownloadError> {
        let resp = self.agent().get(url).call()?;
        let mut reader = resp
            .into_body()
            .into_with_config()
            .limit(MAX_TEXT + 1)
            .reader();
        let mut out = Vec::new();
        reader
            .read_to_end(&mut out)
            .map_err(|e| classify_read_error(e, MAX_TEXT))?;
        if out.len() as u64 > MAX_TEXT {
            return Err(DownloadError::TooLarge(MAX_TEXT));
        }
        String::from_utf8(out).map_err(|_| DownloadError::Network("the file is not text".into()))
    }
}

fn classify_read_error(e: io::Error, max: u64) -> DownloadError {
    // ureq reports its body limit as an io error wrapping Error::BodyExceedsLimit.
    if e.to_string().contains("limit") {
        DownloadError::TooLarge(max)
    } else {
        DownloadError::Network(e.to_string())
    }
}

pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// sha256 of a file, lower-case hex.
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match f.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => h.update(&buf[..n]),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(hex(&h.finalize()))
}
