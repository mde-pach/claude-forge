//! Load time of the built-in packs: parsed (no cache), cache miss (parse and write), cache
//! hit. `cargo run --release -p forge-packs --example load_time`.

use std::time::{Duration, Instant};

use forge_packs::{CacheOutcome, Options};

fn median(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v[v.len() / 2]
}

fn main() {
    let runs = 20;
    let dir = std::env::temp_dir().join(format!("forge-packs-load-time-{}", std::process::id()));
    let cache = Options {
        cache_dir: Some(&dir),
    };
    let time = |opts: Options, before: &dyn Fn()| {
        let mut out = Vec::new();
        for _ in 0..runs {
            before();
            let t = Instant::now();
            let set = forge_packs::load_builtin(opts).expect("built-in packs load");
            out.push(t.elapsed());
            std::hint::black_box(set);
        }
        median(out)
    };
    let parsed = time(Options::default(), &|| {});
    let miss = time(cache, &|| {
        let _ = std::fs::remove_dir_all(&dir);
    });
    let hit = time(cache, &|| {});
    assert_eq!(
        forge_packs::load_builtin(cache).unwrap().cache.builtin,
        CacheOutcome::Hit
    );
    let size = std::fs::metadata(dir.join("builtin-packs.msgpack"))
        .map(|m| m.len())
        .unwrap_or(0);
    println!(
        "median of {runs}: parsed {parsed:?}, cache miss {miss:?}, cache hit {hit:?} (entry {size} bytes)"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
