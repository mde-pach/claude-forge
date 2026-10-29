//! Measures launcher overhead.
//!
//!     cargo run --release -p forge-launcher --example launch_bench [runs]
//!
//! Process timings (each spawned from this driver, interleaved, `runs` each):
//!   direct   — spawn `<cache>/bin/forge-9.9.9` (a copy of this binary) directly;
//!              its launcher check matches the lock and it runs in-process.
//!   noop     — spawn this binary, exit at once: bare process startup.
//!   inproc   — spawn this binary as launcher, lock matches: startup + resolve.
//!   exec     — spawn this binary as launcher at another version: resolve,
//!              exec forge-9.9.9, which resolves again and runs in-process.
//! Overhead of the launcher = exec − direct (what a hook pays on top of
//! running the right binary), and inproc − noop (resolve cost alone).
//!
//! In-process timings: repo discovery + lock read + resolve, and the lock
//! scanner versus the full `toml` parser on a realistic 4 KB lock.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use forge_home::{Home, find_repo_root};
use forge_launcher::{NoFetcher, launch_from_env, parse_lock, read_lock, resolve};

const LOCKED: &str = "9.9.9";

fn main() {
    if let Ok(role) = std::env::var("BENCH_ROLE") {
        if role == "launcher" {
            // A copy installed as <cache>/bin/forge-<v> reports version <v>.
            let me = std::env::current_exe().unwrap();
            let current = match me.file_name().and_then(|n| n.to_str()) {
                Some(n) if n.starts_with("forge-") => n["forge-".len()..].to_owned(),
                _ => std::env::var("BENCH_CURRENT").unwrap(),
            };
            let _ = launch_from_env(&current, &NoFetcher);
        }
        return;
    }
    let runs: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(500);

    let t = tempfile::tempdir().unwrap();
    let base = t.path().canonicalize().unwrap();
    let home = Home::new(base.join("cache"), base.join("cache/state"));
    let repo = base.join("repo");
    let deep = repo.join("src/a/b");
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    let lock_text = realistic_lock();
    std::fs::write(repo.join("forge.lock"), &lock_text).unwrap();
    std::fs::create_dir_all(home.bin_dir()).unwrap();
    let target = home.forge_binary(LOCKED).unwrap();
    let me = std::env::current_exe().unwrap();
    // The target is a copy of this binary: a real Rust binary that, like
    // forge-<v>, runs its own launcher check (which then matches).
    std::fs::copy(&me, &target).unwrap();
    let spawn = |prog: &Path, role: Option<&str>, current: &str| {
        let mut c = Command::new(prog);
        c.args(["hook", "pre-tool-use"])
            .env("FORGE_HOME", home.cache_root())
            .env("CLAUDE_PROJECT_DIR", &deep)
            .env("BENCH_CURRENT", current)
            .env_remove("FORGE_LAUNCHED_VERSION")
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        match role {
            Some(r) => c.env("BENCH_ROLE", r),
            None => c.env_remove("BENCH_ROLE"),
        };
        let s = Instant::now();
        let st = c.status().unwrap();
        let d = s.elapsed();
        assert!(st.success());
        d
    };

    let names = ["direct", "noop", "inproc", "exec"];
    let mut samples: [Vec<Duration>; 4] = Default::default();
    for i in 0..runs + 20 {
        let r = [
            spawn(&target, Some("launcher"), LOCKED),
            spawn(&me, Some("noop"), LOCKED),
            spawn(&me, Some("launcher"), LOCKED),
            spawn(&me, Some("launcher"), "1.0.0"),
        ];
        if i >= 20 {
            // first 20 rounds warm the page cache
            for (s, d) in samples.iter_mut().zip(r) {
                s.push(d);
            }
        }
    }
    println!("process timings, {runs} runs each (median / p99):");
    let mut stats = Vec::new();
    for (name, s) in names.iter().zip(samples.iter_mut()) {
        let (m, p) = med_p99(s);
        stats.push((m, p));
        println!("  {name:<7} {:>8.3} ms / {:>8.3} ms", ms(m), ms(p));
    }
    let diff = |a: usize, b: usize| {
        (
            ms(stats[a].0) - ms(stats[b].0),
            ms(stats[a].1) - ms(stats[b].1),
        )
    };
    let (m, p) = diff(3, 0);
    println!("  launcher overhead (exec - direct):  median {m:.3} ms, p99 {p:.3} ms");
    let (m, p) = diff(2, 1);
    println!("  resolve cost     (inproc - noop):   median {m:.3} ms, p99 {p:.3} ms");

    // In-process: the work launch() does before exec.
    let n = 20_000;
    let mut d = Vec::with_capacity(n);
    for _ in 0..n {
        let s = Instant::now();
        let r = find_repo_root(&deep).unwrap().unwrap();
        let lock = read_lock(&r.lock_path()).unwrap();
        let dec = resolve("1.0.0", lock.as_ref(), None, &home);
        d.push(s.elapsed());
        std::hint::black_box(dec);
    }
    let (m, p) = med_p99(&mut d);
    println!(
        "in-process discover+read+resolve: median {:.1} us / p99 {:.1} us",
        us(m),
        us(p)
    );

    let mut a = Vec::with_capacity(n);
    let mut b = Vec::with_capacity(n);
    for _ in 0..n {
        let s = Instant::now();
        std::hint::black_box(parse_lock(std::hint::black_box(&lock_text)).unwrap());
        a.push(s.elapsed());
        let s = Instant::now();
        let v: toml::Table = std::hint::black_box(&lock_text).parse().unwrap();
        std::hint::black_box(v.get("forge"));
        b.push(s.elapsed());
    }
    let (am, ap) = med_p99(&mut a);
    let (bm, bp) = med_p99(&mut b);
    println!(
        "lock parse ({} bytes): scanner {:.2} us / p99 {:.2} us;  toml crate {:.1} us / p99 {:.1} us",
        lock_text.len(),
        us(am),
        us(ap),
        us(bm),
        us(bp)
    );
}

fn realistic_lock() -> String {
    let mut s = String::from("# Written by forge apply. Do not edit.\nschema = 1\n");
    s.push_str(&format!("forge = \"{LOCKED}\"\n"));
    for i in 0..40 {
        s.push_str(&format!(
            "\n[[packs]]\nid = \"pack-{i}\"\nversion = \"1.{i}.0\"\nsha256 = \"{}\"\nfiles = [\"rules/a.md\", \"rules/b.md\"]\n",
            "ab".repeat(32)
        ));
    }
    s
}

fn med_p99(v: &mut [Duration]) -> (Duration, Duration) {
    v.sort();
    let p = ((v.len() as f64) * 0.99).ceil() as usize - 1;
    (v[v.len() / 2], v[p.min(v.len() - 1)])
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

fn us(d: Duration) -> f64 {
    d.as_secs_f64() * 1e6
}
