#!/usr/bin/env python3
"""Compact live-progress JSON for the experiment status page.

    python3 analysis/status.py <results_dir> <dataset_dir> [--reps 3]

Prints: trials done/total per arm (done = a final status: scored, or infra/parity error after
retries), running count, per-arm resolved rate so far (scored trials, trial level), cost so far
(all finished trials incl. reviewer), infra errors, and the last 10 finished trials by meta.json
modification time. Descriptive only: no inference here (see analyze.py).
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from common import ARMS, load_dataset, load_trials, resolved_of, status_of, total_cost  # noqa: E402


def build(results_dir, dataset_dir, reps):
    tasks = load_dataset(dataset_dir)
    trials, running = load_trials(results_dir)
    trials = {k: v for k, v in trials.items() if k[0] in tasks}
    arms = {}
    for arm in ARMS:
        recs = [r for (t, a, k), r in trials.items() if a == arm]
        scored = [r for r in recs if status_of(r) == "scored"]
        res = [resolved_of(r) for r in scored if resolved_of(r) is not None]
        costs = [total_cost(r["meta"]) for r in recs]
        arms[arm] = {
            "done": len(recs),
            "total": len(tasks) * reps,
            "scored": len(scored),
            "resolved": sum(res),
            "resolved_rate": round(sum(res) / len(res), 4) if res else None,
            "cost_usd": round(sum(c for c in costs if c is not None), 2),
            "infra_errors": sum(1 for r in recs if status_of(r) in ("infra_error", "parity_error")),
            "retried_attempts": sum(r["attempts"] - 1 for r in recs),
            "running": sum(1 for (t, a, k) in running if a == arm and t in tasks),
        }
    last = sorted(trials.items(), key=lambda kv: kv[1]["mtime"], reverse=True)[:10]
    return {
        "generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "n_tasks": len(tasks),
        "reps": reps,
        "done": sum(x["done"] for x in arms.values()),
        "total": sum(x["total"] for x in arms.values()),
        "running": sum(x["running"] for x in arms.values()),
        "cost_usd": round(sum(x["cost_usd"] for x in arms.values()), 2),
        "arms": arms,
        "last": [{"task": t, "arm": a, "rep": k, "status": status_of(r), "resolved": resolved_of(r),
                  "cost": total_cost(r["meta"]), "duration_s": r["meta"].get("duration_s"),
                  "finished": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(r["mtime"]))}
                 for (t, a, k), r in last],
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("results_dir")
    ap.add_argument("dataset_dir")
    ap.add_argument("--reps", type=int, default=3, help="planned replicates (DESIGN.md section 8)")
    a = ap.parse_args()
    print(json.dumps(build(a.results_dir, a.dataset_dir, a.reps), separators=(",", ":")))


if __name__ == "__main__":
    main()
