"""Shared loading code for the Forge experiment analysis (stdlib only).

Results layout (harness/INTERFACES.md):
    results/raw/<run_id>/<task_id>/<arm>/r<k>/{meta.json, outcome.json, edits.json, forge/hook_summary.json, ...}

`results_dir` may point at `results/`, `results/raw/` or a single `results/raw/<run_id>/`: every
`meta.json` below it is picked up. Identity is taken from the meta fields (task_id, arm, rep),
falling back to the path. Retries: if several attempts of one (task, arm, rep) exist (either
overwritten in place with `attempt` = N, or kept in sub-directories), the trial is represented by
its scored attempt if there is one, otherwise by the latest attempt; `attempts` = highest attempt
number seen (or the number of attempt records, whichever is larger).
"""

from __future__ import annotations

import glob
import json
import os
import re

ARMS = ("baseline", "forge")
_REP_RE = re.compile(r"^r(\d+)$")


def read_json(path, default=None):
    try:
        with open(path, encoding="utf-8") as f:
            return json.load(f)
    except (OSError, ValueError):
        return default


def load_dataset(dataset_dir):
    """Return {task_id: task.json dict}. Uses the newest dataset/manifest.vN.json if present."""
    tasks_dir = os.path.join(dataset_dir, "tasks")
    manifests = sorted(
        glob.glob(os.path.join(dataset_dir, "manifest.v*.json")),
        key=lambda p: int(re.search(r"manifest\.v(\d+)\.json$", p).group(1))
        if re.search(r"manifest\.v(\d+)\.json$", p) else -1,
    )
    if manifests:
        ids = [t["task_id"] for t in read_json(manifests[-1], {}).get("tasks", [])]
    else:
        ids = sorted(d for d in os.listdir(tasks_dir) if os.path.isdir(os.path.join(tasks_dir, d)))
    tasks = {}
    for tid in ids:
        t = read_json(os.path.join(tasks_dir, tid, "task.json"), {}) or {}
        t.setdefault("task_id", tid)
        t.setdefault("repo", tid.split("__")[0])
        t.setdefault("type", "other")
        tasks[tid] = t
    return tasks


def _identity_from_path(meta_path):
    """Infer (task_id, arm, rep) from .../<task>/<arm>/r<k>[/...]/meta.json."""
    parts = os.path.normpath(os.path.dirname(meta_path)).split(os.sep)
    for i in range(len(parts) - 1, 1, -1):
        m = _REP_RE.match(parts[i])
        if m and parts[i - 1] in ARMS:
            return parts[i - 2], parts[i - 1], int(m.group(1)), os.sep.join(parts[: i + 1])
    return None, None, None, os.path.dirname(meta_path)


def load_trials(results_dir):
    """Return (trials, running).

    trials: {(task_id, arm, rep): record}, record = {meta, outcome, edits, hook, dir, attempts,
             attempt_records:[status...], mtime}
    running: list of (task_id, arm, rep) whose r<k> directory exists without any meta.json
             (or whose meta says status == "running").
    """
    groups = {}
    for meta_path in glob.glob(os.path.join(results_dir, "**", "meta.json"), recursive=True):
        meta = read_json(meta_path)
        if not isinstance(meta, dict):
            continue
        p_task, p_arm, p_rep, rep_dir = _identity_from_path(meta_path)
        task = meta.get("task_id") or p_task
        arm = meta.get("arm") or p_arm
        rep = meta.get("rep", p_rep)
        if task is None or arm not in ARMS or rep is None:
            continue
        rep = int(str(rep).lstrip("r"))
        groups.setdefault((task, arm, rep), []).append((meta_path, meta, rep_dir))

    trials, running = {}, []
    for key, recs in groups.items():
        statuses = [m.get("status") for _, m, _ in recs]
        if all(s == "running" for s in statuses):
            running.append(key)
            continue
        done = [r for r in recs if r[1].get("status") != "running"]
        scored = [r for r in done if r[1].get("status") == "scored"]
        pick_from = scored or done
        meta_path, meta, _ = max(pick_from, key=lambda r: (int(r[1].get("attempt") or 1), os.path.getmtime(r[0])))
        d = os.path.dirname(meta_path)
        attempts = max([int(m.get("attempt") or 1) for _, m, _ in recs] + [len(done)])
        trials[key] = {
            "meta": meta,
            "outcome": read_json(os.path.join(d, "outcome.json")),
            "edits": read_json(os.path.join(d, "edits.json")),
            "hook": read_json(os.path.join(d, "forge", "hook_summary.json")),
            "dir": d,
            "attempts": attempts,
            "attempt_records": statuses,
            "mtime": os.path.getmtime(meta_path),
        }

    # r<k> directories that have no meta.json at all -> running (started, not finished)
    for rep_dir in glob.glob(os.path.join(results_dir, "**", "r[0-9]*"), recursive=True):
        if not os.path.isdir(rep_dir):
            continue
        task, arm, rep, _ = _identity_from_path(os.path.join(rep_dir, "meta.json"))
        if task is None:
            continue
        key = (task, arm, rep)
        if key not in trials and key not in running:
            running.append(key)
    return trials, sorted(running)


def status_of(rec):
    return rec["meta"].get("status") or "unknown"


def resolved_of(rec):
    """Primary outcome; None if not a scored trial with an outcome."""
    if status_of(rec) != "scored" or not rec.get("outcome"):
        return None
    return bool(rec["outcome"].get("resolved"))


def total_cost(meta):
    """cost_usd is the total incl. Forge's reviewer; fall back to agent + reviewer."""
    if meta.get("cost_usd") is not None:
        return float(meta["cost_usd"])
    parts = [meta.get("cost_usd_agent"), meta.get("cost_usd_reviewer")]
    if all(p is None for p in parts):
        return None
    return float(sum(p or 0.0 for p in parts))
