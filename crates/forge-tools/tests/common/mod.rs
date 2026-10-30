//! A local HTTP server and archive builders for the download tests.
#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use sha2::{Digest, Sha256};

#[derive(Clone)]
pub enum Route {
    Body(Vec<u8>),
    Status(u16),
    /// Announces the full length, sends only the first `n` bytes, then closes.
    Truncated(Vec<u8>, usize),
    /// Waits before answering (to make concurrent requests overlap).
    Slow(Vec<u8>, Duration),
}

#[derive(Clone, Default)]
struct State {
    routes: HashMap<String, Route>,
    hits: HashMap<String, usize>,
}

pub struct Server {
    port: u16,
    state: Arc<Mutex<State>>,
}

impl Server {
    pub fn start() -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let state = Arc::new(Mutex::new(State::default()));
        let s = state.clone();
        thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(conn) = conn else { continue };
                let s = s.clone();
                thread::spawn(move || serve(conn, &s));
            }
        });
        Server { port, state }
    }

    pub fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base())
    }

    pub fn put(&self, path: &str, route: Route) {
        self.state
            .lock()
            .unwrap()
            .routes
            .insert(path.to_owned(), route);
    }

    pub fn hits(&self, path: &str) -> usize {
        *self.state.lock().unwrap().hits.get(path).unwrap_or(&0)
    }
}

fn serve(conn: TcpStream, state: &Mutex<State>) {
    let mut reader = BufReader::new(conn.try_clone().unwrap());
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let path = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).is_err() || h == "\r\n" || h.is_empty() {
            break;
        }
    }
    let route = {
        let mut st = state.lock().unwrap();
        *st.hits.entry(path.clone()).or_insert(0) += 1;
        st.routes.get(&path).cloned()
    };
    let mut conn = conn;
    let respond = |conn: &mut TcpStream, status: u16, body: &[u8], send: usize| {
        let head = format!(
            "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = conn.write_all(head.as_bytes());
        let _ = conn.write_all(&body[..send]);
        let _ = conn.flush();
    };
    match route {
        None => respond(&mut conn, 404, b"", 0),
        Some(Route::Status(s)) => respond(&mut conn, s, b"", 0),
        Some(Route::Body(b)) => respond(&mut conn, 200, &b, b.len()),
        Some(Route::Truncated(b, n)) => respond(&mut conn, 200, &b, n),
        Some(Route::Slow(b, d)) => {
            thread::sleep(d);
            respond(&mut conn, 200, &b, b.len());
        }
    }
}

pub fn sha256(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

/// A .tar.gz of `(path, contents, mode)` entries.
pub fn tar_gz(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut tar = tar::Builder::new(gz);
    for (path, data, mode) in entries {
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(*mode);
        h.set_cksum();
        tar.append_data(&mut h, path, *data).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap()
}

/// A .zip (stored) of `(path, contents)` entries.
pub fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .unix_permissions(0o755);
    for (path, data) in entries {
        w.start_file(*path, opts).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

/// Leaks a string so test catalogs can hold `&'static str` like the real one.
pub fn leak(s: String) -> &'static str {
    s.leak()
}

/// Every file name in a directory (for checking no temporary file was left behind).
pub fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}
