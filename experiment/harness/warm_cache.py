#!/usr/bin/env python3
"""Fill the shared host package caches (/work/cache/*) for one repository.

    python3 harness/warm_cache.py <repo_id> [--limit N] [--pilot M] [--jobs J] [--no-verify]

For the first N (default 60) main-window candidates and the first M (default 10) pilot
candidates, both the parent and the commit are considered. Commits are deduplicated by the
hash of their lockfile(s) (yaml `lockfiles`, default uv.lock / pnpm-lock.yaml / bun.lock),
and the repo's `warm` command (an online install) is run once per distinct lockfile, WITH
network (docker bridge + proxy CA), with /work/cache/<kind> mounted read-write.
Then the offline `install` is verified for the first main candidate's parent (network none,
caches read-only). A JSON record is written to harness/warm/<repo_id>.json.
"""
from __future__ import annotations

import argparse
import concurrent.futures as cf
import hashlib
import json
import shutil
import subprocess
import sys
import time
import uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import repo_env  # noqa: E402

EXP = Path(__file__).resolve().parent.parent
CAND = EXP / "dataset" / "candidates"
OUT = Path(__file__).resolve().parent / "warm"
CA = "/root/.ccr/ca-bundle.crt"
CA_CTR = "/etc/forge-proxy-ca.crt"
NET_ENV = {
    "SSL_CERT_FILE": CA_CTR, "REQUESTS_CA_BUNDLE": CA_CTR, "NODE_EXTRA_CA_CERTS": CA_CTR,
    "GIT_SSL_CAINFO": CA_CTR, "UV_NATIVE_TLS": "1", "PIP_CERT": CA_CTR,
}
DEFAULT_LOCKS = ["uv.lock", "pnpm-lock.yaml", "bun.lock", "bun.lockb", "package-lock.json"]


def git_show(mirror: str, commit: str, path: str) -> bytes | None:
    r = subprocess.run(["git", "-C", mirror, "show", f"{commit}:{path}"], capture_output=True)
    return r.stdout if r.returncode == 0 else None


def lock_key(cfg: dict, commit: str) -> str:
    h = hashlib.sha256(cfg.get("warm", "").encode())
    for lf in cfg.get("lockfiles") or DEFAULT_LOCKS:
        data = git_show(cfg["mirror"], commit, lf)
        h.update(f"\0{lf}\0".encode())
        h.update(data if data is not None else b"<missing>")
    return h.hexdigest()


def cache_kinds(cfg: dict) -> list[str]:
    return list(cfg.get("caches") or (["uv"] if cfg.get("lang") == "python" else ["pnpm", "npm", "bun"]))


def du(path: Path) -> str:
    r = subprocess.run(["du", "-sh", str(path)], capture_output=True, text=True)
    return r.stdout.split()[0] if r.returncode == 0 and r.stdout else "?"


def disk_free_gb() -> float:
    return shutil.disk_usage("/").free / 1e9


def warm_one(repo_id: str, cfg: dict, commit: str) -> dict:
    name = f"warm-{repo_id}-{commit[:8]}-{uuid.uuid4().hex[:6]}"
    mounts = [(str(repo_env.HOST_CACHE / k), f"/cache/{k}", "rw") for k in cache_kinds(cfg)]
    mounts.append((CA, CA_CTR, "ro"))
    t0 = time.time()
    repo_env.start_container(repo_id, name, network="bridge", mounts=mounts, env=NET_ENV)
    try:
        repo_env.materialize(name, repo_id, commit)
        r = repo_env.exec_(name, cfg["warm"], timeout=int(cfg.get("install_timeout", 3600)))
        ok = r.returncode == 0
        tail = (r.stdout + r.stderr)[-2000:]
    except Exception as e:  # noqa: BLE001
        ok, tail = False, repr(e)
    finally:
        repo_env.remove(name)
    return {"commit": commit, "ok": ok, "seconds": round(time.time() - t0), "log_tail": tail if not ok else tail[-300:]}


# Forge 091c0d3 pins (src/util.ts), invoked exactly as templates/scripts/{lib,engine}.sh do. The
# base image has them in its own cache; Python repos point the agent's UV_CACHE_DIR at /cache/uv,
# so they are warmed there too.
FORGE_UVX = [
    'uvx -q --from "ast-grep-cli==0.45.3" ast-grep --version',
    'uvx -q "zizmor@1.30.1" --version',
    "uvx -q --from \"hadolint-bin==2.15.1\" python -c 'import shutil; print(shutil.which(\"hadolint\"))'",
    'uvx -q --from "squawk-cli==2.70.0" squawk --version',
]


def warm_forge_tools(repo_id: str, cfg: dict) -> dict:
    name = f"warm-{repo_id}-forgetools-{uuid.uuid4().hex[:6]}"
    mounts = [(str(repo_env.HOST_CACHE / "uv"), "/cache/uv", "rw"), (CA, CA_CTR, "ro")]
    env = dict(NET_ENV, UV_CACHE_DIR="/cache/uv", UV_OFFLINE="false")
    repo_env.start_container(repo_id, name, network="bridge", mounts=mounts, env=env)
    try:
        r = repo_env.exec_(name, " && ".join(FORGE_UVX), timeout=900, workdir="/tmp")
    finally:
        repo_env.remove(name)
    return {"ok": r.returncode == 0, "out": (r.stdout + r.stderr)[-600:]}


def verify_offline(repo_id: str, commit: str, cfg: dict) -> dict:
    name = f"verify-{repo_id}-{uuid.uuid4().hex[:6]}"
    t0 = time.time()
    repo_env.start_container(repo_id, name, network="none")
    checks = {}
    try:
        repo_env.materialize(name, repo_id, commit)
        ok, log = repo_env.install(name, repo_id)
        if ok and "uv" in cache_kinds(cfg):
            # What the agent / Forge hooks will run afterwards, offline, in the workspace.
            for cmd in FORGE_UVX + ['uv run python -c "print(0)"', "git status --porcelain -- uv.lock"]:
                r = repo_env.exec_(name, cmd, timeout=300)
                checks[cmd] = {"rc": r.returncode, "out": (r.stdout + r.stderr).strip()[-300:]}
            ok = ok and all(v["rc"] == 0 for v in checks.values()) \
                and not checks["git status --porcelain -- uv.lock"]["out"]
    finally:
        repo_env.remove(name)
    return {"commit": commit, "ok": ok, "seconds": round(time.time() - t0), "checks": checks,
            "log_tail": log[-1500:]}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("repo_id")
    ap.add_argument("--limit", type=int, default=60)
    ap.add_argument("--pilot", type=int, default=10)
    ap.add_argument("--jobs", type=int, default=2)
    ap.add_argument("--no-verify", action="store_true")
    ap.add_argument("--force", action="store_true", help="re-run lockfiles already warmed ok (per harness/warm/<id>.json)")
    ap.add_argument("--min-free-gb", type=float, default=6.0, help="stop warming below this free disk")
    a = ap.parse_args()
    cfg = repo_env.load_repos()[a.repo_id]
    if not cfg.get("warm"):
        print(f"{a.repo_id}: no `warm` command in yaml", file=sys.stderr)
        return 2
    for k in cache_kinds(cfg):
        (repo_env.HOST_CACHE / k).mkdir(parents=True, exist_ok=True)
    main_c = json.loads((CAND / f"{a.repo_id}.main.json").read_text())["candidates"][: a.limit]
    pilot_c = json.loads((CAND / f"{a.repo_id}.pilot.json").read_text())["candidates"][: a.pilot]
    commits: list[str] = []
    for c in main_c + pilot_c:
        for sha in (c["parent"], c["commit"]):
            if sha not in commits:
                commits.append(sha)
    groups: dict[str, list[str]] = {}
    for sha in commits:
        groups.setdefault(lock_key(cfg, sha), []).append(sha)
    prev_file = OUT / f"{a.repo_id}.json"
    done_keys: set[str] = set()
    if prev_file.exists() and not a.force:
        done_keys = set(json.loads(prev_file.read_text()).get("warmed_keys", []))
    reps = [v[0] for k, v in groups.items() if k not in done_keys]
    skipped = len(groups) - len(reps)
    print(f"{a.repo_id}: {len(main_c)} main + {len(pilot_c)} pilot candidates, {len(commits)} commits, "
          f"{len(groups)} distinct lockfiles ({skipped} already warmed); free {disk_free_gb():.1f} GB", flush=True)
    results: list[dict] = []
    stopped_low_disk = False
    with cf.ThreadPoolExecutor(max_workers=a.jobs) as ex:
        futs = {}
        for sha in reps:
            futs[ex.submit(warm_one, a.repo_id, cfg, sha)] = sha
        for i, f in enumerate(cf.as_completed(futs), 1):
            if f.cancelled():
                continue
            res = f.result()
            results.append(res)
            print(f"  [{i}/{len(reps)}] {res['commit'][:10]} ok={res['ok']} {res['seconds']}s "
                  f"cache={du(repo_env.HOST_CACHE / cache_kinds(cfg)[0])} free={disk_free_gb():.1f}GB", flush=True)
            if not res["ok"]:
                print(res["log_tail"], flush=True)
            if disk_free_gb() < a.min_free_gb and not stopped_low_disk:
                stopped_low_disk = True
                print("  LOW DISK: cancelling remaining warm-ups", flush=True)
                for g in futs:
                    g.cancel()
    record = {
        "repo": a.repo_id, "date": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "limit_main": a.limit, "limit_pilot": a.pilot, "n_commits": len(commits),
        "n_lockfiles": len(groups), "stopped_low_disk": stopped_low_disk,
        "warmed_keys": sorted(done_keys | {k for k, v in groups.items()
                                           if any(r["ok"] and r["commit"] == v[0] for r in results)}),
        "warm": [{k: v for k, v in r.items() if k != "log_tail" or not r["ok"]} for r in results],
        "lock_groups": {k[:16]: v for k, v in groups.items()},
        "cache_sizes": {k: du(repo_env.HOST_CACHE / k) for k in cache_kinds(cfg)},
    }
    if "uv" in cache_kinds(cfg):
        ft = warm_forge_tools(a.repo_id, cfg)
        record["forge_tools"] = ft
        print(f"forge tools into /work/cache/uv: ok={ft['ok']}", flush=True)
        if not ft["ok"]:
            print(ft["out"])
    if not a.no_verify and main_c:
        v = verify_offline(a.repo_id, main_c[0]["parent"], cfg)
        record["verify_offline"] = v
        print(f"verify offline install @ {v['commit'][:10]}: ok={v['ok']} {v['seconds']}s", flush=True)
        if not v["ok"]:
            print(v["log_tail"], json.dumps(v["checks"], indent=1))
    OUT.mkdir(exist_ok=True)
    (OUT / f"{a.repo_id}.json").write_text(json.dumps(record, indent=1) + "\n")
    bad = [r for r in results if not r["ok"]]
    print(f"done: {len(results) - len(bad)}/{len(reps)} ok; cache {record['cache_sizes']}; "
          f"free {disk_free_gb():.1f} GB")
    return 0 if not bad and record.get("verify_offline", {}).get("ok", True) else 1


if __name__ == "__main__":
    sys.exit(main())
