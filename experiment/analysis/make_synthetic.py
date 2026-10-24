#!/usr/bin/env python3
"""Write a synthetic results tree + dataset to test analyze.py and status.py.

    python3 analysis/make_synthetic.py --out /tmp/synth --effect 0.10 --seed 1
    python3 analysis/analyze.py /tmp/synth/results /tmp/synth/dataset --out /tmp/synth/out

Generative model: per-task baseline success probability p_i ~ Beta(0.6, 0.6) (many tasks near
0 or 1, as with real agents); forge probability = clip(p_i + effect, 0, 1). Each trial is
Bernoulli. Infra errors happen per attempt with probability `--infra`; up to 3 attempts. In the
last replicate some forge trials are missing (never run), one trial exhausts its retries, and one
trial is "running" (directory without meta.json), so the last replicate is incomplete and the
primary analysis must use the earlier replicates only. truth.json records the planted values,
including the realised expected difference after clipping.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import random

ARMS = ("baseline", "forge")
TYPES = ("fix", "feat", "refactor", "other")


def wjson(path, obj):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        json.dump(obj, f, indent=1)


def poisson(rng, lam):
    L, k, p = math.exp(-lam), 0, 1.0
    while True:
        p *= rng.random()
        if p <= L:
            return k
        k += 1


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--tasks", type=int, default=60)
    ap.add_argument("--repos", type=int, default=6)
    ap.add_argument("--reps", type=int, default=3)
    ap.add_argument("--effect", type=float, default=0.10)
    ap.add_argument("--infra", type=float, default=0.06)
    ap.add_argument("--missing-last-rep", type=int, default=12,
                    help="forge trials of the last replicate never run")
    ap.add_argument("--seed", type=int, default=1)
    a = ap.parse_args()
    rng = random.Random(a.seed)

    ds_dir = os.path.join(a.out, "dataset")
    res_dir = os.path.join(a.out, "results", "raw", "synth-run")
    repos = [f"repo{j}" for j in range(a.repos)]
    tasks, p_base = [], {}
    for i in range(a.tasks):
        repo = repos[i % a.repos]
        tid = f"{repo}__{i:07x}"
        t = {"task_id": tid, "repo": repo, "commit": f"{i:040x}", "parent": f"{i + 1:040x}",
             "date": "2026-08-01", "subject": "synthetic", "type": rng.choice(TYPES),
             "source_files": ["src/x.py"], "test_files": ["tests/test_x.py"],
             "f2p": [f"tests/test_x.py::t{j}" for j in range(3)], "p2p": [], "audit": {"kept": [], "dropped": []}}
        wjson(os.path.join(ds_dir, "tasks", tid, "task.json"), t)
        tasks.append(t)
        p_base[tid] = rng.betavariate(0.6, 0.6)
    wjson(os.path.join(ds_dir, "manifest.v1.json"),
          {"version": 1, "created": "2026-10-24", "rules_sha256": "0" * 64, "pins": {},
           "tasks": [{"task_id": t["task_id"], "sha256": "0" * 64} for t in tasks]})

    p_arm = {(tid, "baseline"): p for tid, p in p_base.items()}
    p_arm.update({(tid, "forge"): min(1.0, max(0.0, p + a.effect)) for tid, p in p_base.items()})
    true_diff = sum(p_arm[(t, "forge")] - p_arm[(t, "baseline")] for t in p_base) / len(p_base)

    last = a.reps
    missing = set(rng.sample([t["task_id"] for t in tasks], a.missing_last_rep))
    exhausted = rng.choice([t["task_id"] for t in tasks if t["task_id"] not in missing])
    running = rng.choice([t["task_id"] for t in tasks if t["task_id"] not in missing and t["task_id"] != exhausted])
    parity = rng.choice([t["task_id"] for t in tasks
                         if t["task_id"] not in missing and t["task_id"] not in (exhausted, running)])
    clock = 1_790_000_000.0
    order = [(k, t["task_id"], arm) for k in range(1, a.reps + 1) for t in tasks for arm in ARMS]
    for k, tid, arm in order:
        d = os.path.join(res_dir, tid, arm, f"r{k}")
        if k == last and arm == "forge" and tid in missing:
            continue
        if k == last and arm == "forge" and tid == running:
            os.makedirs(d, exist_ok=True)
            continue
        attempt, status, reason = 1, "scored", None
        if k == last and arm == "baseline" and tid == exhausted:
            attempt, status, reason = 3, "infra_error", "api_overloaded"
        elif k == last and arm == "forge" and tid == parity:
            attempt, status, reason = 3, "parity_error", "workspace hash mismatch"
        else:
            while attempt < 3 and rng.random() < a.infra:
                attempt += 1
        resolved = rng.random() < p_arm[(tid, arm)]
        timed_out = status == "scored" and rng.random() < 0.03
        cost_agent = rng.lognormvariate(math.log(1.2), 0.6) * (1.15 if arm == "forge" else 1.0)
        cost_rev = rng.lognormvariate(math.log(0.15), 0.5) if arm == "forge" else 0.0
        dur = rng.lognormvariate(math.log(600), 0.5) * (1.2 if arm == "forge" else 1.0)
        meta = {"task_id": tid, "arm": arm, "rep": k, "attempt": attempt, "status": status,
                "infra_reason": reason, "cost_usd": round(cost_agent + cost_rev, 4),
                "cost_usd_agent": round(cost_agent, 4), "cost_usd_reviewer": round(cost_rev, 4),
                "num_turns": int(rng.lognormvariate(math.log(35), 0.4)),
                "duration_s": round(2700.0 if timed_out else dur, 1), "exit_code": 0, "timed_out": timed_out,
                "model": "claude-synthetic", "claude_version": "2.x.synthetic",
                "forge_commit": "abc1234" if arm == "forge" else None}
        wjson(os.path.join(d, "meta.json"), meta)
        clock += rng.uniform(60, 900)
        os.utime(os.path.join(d, "meta.json"), (clock, clock))
        if status != "scored":
            continue
        wjson(os.path.join(d, "outcome.json"), {
            "resolved": resolved, "f2p": {}, "p2p": {},
            "f2p_frac": 1.0 if resolved else round(rng.choice([0, 0, 1 / 3, 2 / 3]), 4),
            "p2p_regressions": 0 if resolved else poisson(rng, 0.4),
            "lint_new": poisson(rng, 0.3 if arm == "forge" else 1.0),
            "diff_lines": int(rng.lognormvariate(math.log(60), 0.7))})
        et = poisson(rng, 12)
        wjson(os.path.join(d, "edits.json"),
              {"edit_tool_calls": et, "shell_edit_calls": poisson(rng, 1 if arm == "forge" else 3)})
        if arm == "forge":
            wjson(os.path.join(d, "forge", "hook_summary.json"),
                  {"active": rng.random() < 0.97, "blocks": poisson(rng, 1.5),
                   "feedback_injections": poisson(rng, 4), "reviews": poisson(rng, 1)})
        else:
            wjson(os.path.join(d, "forge", "hook_summary.json"),
                  {"active": False, "blocks": 0, "feedback_injections": 0, "reviews": 0})

    wjson(os.path.join(a.out, "truth.json"),
          {"effect_param": a.effect, "true_mean_diff_after_clip": true_diff, "seed": a.seed,
           "missing_last_rep_forge": sorted(missing), "exhausted": exhausted, "parity": parity, "running": running})
    print(f"wrote {a.out}: {a.tasks} tasks, {a.reps} reps, planted effect {a.effect:+.2f} "
          f"(expected diff after clipping {true_diff:+.3f})")


if __name__ == "__main__":
    main()
