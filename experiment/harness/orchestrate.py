"""Run the experiment from a frozen manifest: resumable, parallel, complete replicates in order
(DESIGN.md §8). Infra/parity errors are retried up to 3 attempts and never scored.

usage: python3 harness/orchestrate.py <manifest.json> <tasks_dir> <run_dir> [--jobs 4] [--reps 3] [--only task_id,...]
"""
import argparse
import hashlib
import json
import os
import random
import subprocess
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor

HERE = os.path.dirname(os.path.abspath(__file__))
PINS = json.load(open(os.path.join(HERE, "..", "config", "pins.json")))
MAX_ATTEMPTS = 3
LOCK = threading.Lock()
# disk-heavy workspaces (~3 GB each): at most this many concurrent trials per repo
REPO_CAP = {"better-auth": 2}
SEMS = {r: threading.Semaphore(n) for r, n in REPO_CAP.items()}


def sha_tree(d):
    h = hashlib.sha256()
    for root, _, fs in sorted(os.walk(d)):
        for f in sorted(fs):
            p = os.path.join(root, f)
            h.update(os.path.relpath(p, d).encode() + b"\0" + open(p, "rb").read() + b"\0")
    return h.hexdigest()


def schedule(task_ids, reps, seed):
    """Replicate-major order; tasks shuffled per replicate; arm order per (task, rep) by coin flip."""
    order = []
    for rep in range(1, reps + 1):
        rng = random.Random(f"{seed}-{rep}")
        ts = sorted(task_ids)
        rng.shuffle(ts)
        for t in ts:
            arms = ["baseline", "forge"]
            if rng.random() < 0.5:
                arms.reverse()
            order += [(t, arm, rep) for arm in arms]
    return order


def status_of(run_dir, t, arm, rep):
    p = os.path.join(run_dir, t, arm, f"r{rep}", "meta.json")
    if not os.path.exists(p):
        return None
    return json.load(open(p)).get("status")


def attempts_file(run_dir):
    return os.path.join(run_dir, "_attempts.json")


def bump_attempt(run_dir, key):
    with LOCK:
        p = attempts_file(run_dir)
        d = json.load(open(p)) if os.path.exists(p) else {}
        d[key] = d.get(key, 0) + 1
        json.dump(d, open(p, "w"), indent=1)
        return d[key]


def get_attempts(run_dir, key):
    p = attempts_file(run_dir)
    return (json.load(open(p)) if os.path.exists(p) else {}).get(key, 0)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("manifest")
    ap.add_argument("tasks_dir")
    ap.add_argument("run_dir")
    ap.add_argument("--jobs", type=int, default=4)
    ap.add_argument("--reps", type=int, default=PINS["replicates"])
    ap.add_argument("--only", default="")
    a = ap.parse_args()
    man = json.load(open(a.manifest))
    # the frozen dataset must be unchanged
    for t in man["tasks"]:
        got = sha_tree(os.path.join(a.tasks_dir, t["task_id"]))
        if got != t["sha256"]:
            sys.exit(f"dataset changed since freeze: {t['task_id']}")
    os.makedirs(a.run_dir, exist_ok=True)
    json.dump({"manifest": os.path.abspath(a.manifest), "manifest_sha256": hashlib.sha256(open(a.manifest, "rb").read()).hexdigest(),
               "pins": PINS, "jobs": a.jobs, "reps": a.reps, "started": time.time()},
              open(os.path.join(a.run_dir, f"_run_{int(time.time())}.json"), "w"), indent=1)
    ids = [t["task_id"] for t in man["tasks"]]
    repo_of = {t["task_id"]: t["repo"] for t in man["tasks"]}
    if a.only:
        ids = [i for i in ids if i in a.only.split(",")]
    order = schedule(ids, a.reps, PINS["seed"])
    stop = threading.Event()

    def work(item):
        t, arm, rep = item
        key = f"{t}/{arm}/r{rep}"
        while not stop.is_set():
            st = status_of(a.run_dir, t, arm, rep)
            if st == "scored":
                return
            n = get_attempts(a.run_dir, key)
            if n >= MAX_ATTEMPTS:
                return
            if os.path.exists(os.path.join(a.run_dir, "STOP")):
                stop.set()
                return
            att = bump_attempt(a.run_dir, key)
            sem = SEMS.get(repo_of[t])
            if sem:
                sem.acquire()
            try:
                r = subprocess.run([sys.executable, os.path.join(HERE, "run_trial.py"), os.path.join(a.tasks_dir, t), arm,
                                str(rep), a.run_dir, "--attempt", str(att)], capture_output=True, text=True)
            finally:
                if sem:
                    sem.release()
            line = (r.stdout.strip().splitlines() or [r.stderr[-500:]])[-1]
            with LOCK:
                with open(os.path.join(a.run_dir, "_log.jsonl"), "a") as f:
                    f.write(json.dumps({"ts": time.time(), "key": key, "attempt": att, "out": line}) + "\n")
            print(key, att, line, flush=True)
            st = status_of(a.run_dir, t, arm, rep)
            if st in ("infra_error", "parity_error"):
                meta = json.load(open(os.path.join(a.run_dir, t, arm, f"r{rep}", "meta.json")))
                # keep every failed attempt's metadata for the infra report
                os.makedirs(os.path.join(a.run_dir, "_failed_attempts"), exist_ok=True)
                json.dump(meta, open(os.path.join(a.run_dir, "_failed_attempts", key.replace("/", "__") + f"__a{att}.json"), "w"), indent=1)
                if "usage limit" in (meta.get("infra_reason") or "").lower() or "rate" in (meta.get("infra_reason") or "").lower():
                    time.sleep(600)  # back off on usage/rate limits
                else:
                    time.sleep(30)

    with ThreadPoolExecutor(a.jobs) as ex:
        list(ex.map(work, order))


if __name__ == "__main__":
    main()
