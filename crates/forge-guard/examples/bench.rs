//! Times `GuardSet::evaluate` over the guard corpus with every guard rule loaded:
//! `cargo run --release -p forge-guard --example bench`.
//!
//! Warm: the set built and every command evaluated once, then `ROUNDS` rounds over the corpus.
//! Cold: what one hook process pays before its first answer, the packs already loaded (their
//! cache is the loader's): reading the converted rules, building the set, the first parse
//! (tree-sitter's parser) and the regexes the command needs.

use std::time::{Duration, Instant};

use forge_guard::{GuardCtx, GuardRule, GuardSet, converted};
use forge_packs::Options;

include!("../tests/common/corpus_data.rs");

const ROUNDS: usize = 200;

fn pct(xs: &mut [Duration], q: f64) -> Duration {
    xs.sort();
    let i = ((xs.len() as f64 * q).ceil() as usize).clamp(1, xs.len()) - 1;
    xs[i]
}

fn main() {
    let packs = forge_packs::load_builtin(Options::default()).expect("packs");
    let rules: Vec<&forge_packs::Rule> = packs
        .packs
        .values()
        .flat_map(|p| &p.rules)
        .filter(|r| r.enforce.iter().any(|t| t.kind() == "guard"))
        .collect();
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonical");
    std::fs::create_dir_all(root.join(".git")).expect(".git");
    for f in [
        "node_modules/.bin/tsc",
        "node_modules/.bin/vitest",
        "packages/app/src/a.test.ts",
    ] {
        let p = root.join(f);
        std::fs::create_dir_all(p.parent().expect("parent")).expect("dirs");
        std::fs::write(p, "").expect("write");
    }
    let ctx = GuardCtx::new(root.clone(), root.clone());
    let commands: Vec<&str> = MUST_TRIGGER
        .iter()
        .map(|c| c.0)
        .chain(MUST_NOT_TRIGGER.iter().copied())
        .chain(FALSE_POSITIVES.iter().copied())
        .collect();

    // cold
    let t0 = Instant::now();
    let conv: Vec<GuardRule> = rules.iter().filter_map(|r| converted::rule(r)).collect();
    let set = GuardSet::new_lazy(conv).expect("set");
    let built = t0.elapsed();
    let first = Instant::now();
    let _ = set.evaluate(
        "git status && bun test --coverage src/app 2>&1 | tail -n 40",
        &ctx,
    );
    let first = first.elapsed();
    println!(
        "cold (converted rules: {} rules): build {:?}, first evaluate {:?}",
        set.rules().count(),
        built,
        first
    );

    for (label, set) in [
        ("converted (structural)", set),
        (
            "pack patterns per simple command",
            GuardSet::from_pack_rules(rules.iter().copied()).expect("legacy"),
        ),
    ] {
        for c in &commands {
            let _ = set.evaluate(c, &ctx);
        }
        let mut times = Vec::with_capacity(ROUNDS * commands.len());
        for _ in 0..ROUNDS {
            for c in &commands {
                let t = Instant::now();
                let _ = std::hint::black_box(set.evaluate(c, &ctx));
                times.push(t.elapsed());
            }
        }
        let n = times.len();
        let median = pct(&mut times, 0.5);
        let p99 = pct(&mut times, 0.99);
        let max = *times.last().expect("times");
        println!(
            "{label}: {n} evaluations of {} commands: median {median:?}, p99 {p99:?}, max {max:?}",
            commands.len()
        );
    }
}
