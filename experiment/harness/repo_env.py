#!/usr/bin/env python3
"""Per-repository container environment (contract: harness/INTERFACES.md).

Generic over languages: everything repo-specific comes from harness/repos/<repo_id>.yaml.

Module API (stable, see INTERFACES.md):
    load_repos, start_container, materialize, install, write_files, run_tests, lint, exec_, remove
Extras (optional, additive):
    run_tests_detailed(...) -> TestRun(results, log, returncode, junit_found, timed_out)
    parse_junit(xml_text, flavor, targets) -> dict[str, str]
    read_file(name, path) -> bytes | None

Test ids are normalised as "<file>::<name>":
  * pytest: the pytest node id ("tests/test_x.py::TestA::test_b[1]"); a module that fails to
    collect is reported as "<file>" (no "::") with status "error".
  * vitest: "<file>::<describe> > <test>"; a file that fails to load is reported as "<file>"
    with status "error".
  Ids that were requested but are absent from the result were not run: callers should treat
  them as "error".

Optional yaml keys beyond the contract (all have defaults):
  junit_flavor: pytest | vitest     default: pytest for lang python, vitest for typescript
  warm: "<sh>"                      online install used by warm_cache.py to fill /work/cache
  install_timeout: seconds          default 1800
  lockfiles: [paths]                files whose content keys the dependency set (warm_cache)
  cache_overlay: [kinds]            caches mounted as a per-container overlay (writable view,
                                    shared lower layer never modified); default [uv] for python,
                                    [] otherwise. Other caches are plain read-only binds.

CLI (smoke test):
  python3 harness/repo_env.py smoke <repo_id> <commit> <test_file...> [--overlay-from <commit>]
      [--network none|trialnet|bridge] [--keep]
"""
from __future__ import annotations

import io
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import tarfile
import time
import uuid
import xml.etree.ElementTree as ET
from dataclasses import dataclass, field
from pathlib import Path

try:
    import yaml  # PyYAML
except ImportError:  # pragma: no cover
    yaml = None

HARNESS = Path(__file__).resolve().parent
REPOS_DIR = HARNESS / "repos"
HOST_CACHE = Path("/work/cache")
OVERLAY_ROOT = Path("/work/runs/cache-overlay")
CACHE_KINDS = ("uv", "pnpm", "npm", "bun", "pip")
WORKDIR = "/work"
DEV_UID = "1000"
DEV_GID = "1000"
CPUS = "2"
MEMORY = "6g"
# What bubblewrap (Claude Code's Bash sandbox, enabled by `forge apply --user`) needs inside Docker
# (DECISIONS D7, D17): namespaces (seccomp/apparmor/SYS_ADMIN) AND an unmasked /proc to mount
# (systempaths=unconfined). Identical in both arms.
SECURITY_EXTRA = ["--security-opt", "seccomp=unconfined", "--security-opt", "apparmor=unconfined",
                  "--cap-add", "SYS_ADMIN", "--security-opt", "systempaths=unconfined"]
GIT_IDENTITY = {
    "GIT_AUTHOR_NAME": "dev", "GIT_AUTHOR_EMAIL": "dev@example.com",
    "GIT_COMMITTER_NAME": "dev", "GIT_COMMITTER_EMAIL": "dev@example.com",
    "GIT_AUTHOR_DATE": "2000-01-01T00:00:00Z", "GIT_COMMITTER_DATE": "2000-01-01T00:00:00Z",
}
LABEL = "forge-exp.repo_env=1"

_repos_cache: dict[str, dict] | None = None
_image_path_cache: dict[str, str] = {}


# --------------------------------------------------------------------------- config

def load_repos() -> dict[str, dict]:
    """All repo configs: {repo_id: cfg}, from harness/repos/*.yaml (each file maps id -> cfg)."""
    global _repos_cache
    if _repos_cache is not None:
        return _repos_cache
    if yaml is None:
        raise RuntimeError("PyYAML is required (pip install pyyaml)")
    out: dict[str, dict] = {}
    for f in sorted(REPOS_DIR.glob("*.yaml")):
        data = yaml.safe_load(f.read_text()) or {}
        for rid, cfg in data.items():
            if rid in out:
                raise ValueError(f"duplicate repo id {rid} in {f}")
            cfg = dict(cfg or {})
            cfg.setdefault("env", {})
            cfg["env"] = {str(k): str(v) for k, v in (cfg["env"] or {}).items()}
            cfg.setdefault("junit_flavor", "vitest" if cfg.get("lang") == "typescript" else "pytest")
            cfg["_file"] = str(f)
            out[rid] = cfg
    _repos_cache = out
    return out


def _cfg(repo_id: str) -> dict:
    repos = load_repos()
    if repo_id not in repos:
        raise KeyError(f"unknown repo {repo_id}; known: {sorted(repos)}")
    return repos[repo_id]


def _image_path(image: str) -> str:
    if image not in _image_path_cache:
        r = subprocess.run(["docker", "image", "inspect", "--format", "{{json .Config.Env}}", image],
                           capture_output=True, text=True)
        path = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
        if r.returncode == 0:
            for kv in json.loads(r.stdout or "[]") or []:
                if kv.startswith("PATH="):
                    path = kv[5:]
        else:
            raise RuntimeError(f"image {image} not found: {r.stderr.strip()}")
        _image_path_cache[image] = path
    return _image_path_cache[image]


# --------------------------------------------------------------------------- containers

def start_container(repo_id: str, name: str, *, network: str = "none",
                    mounts: list[tuple[str, str, str]] | None = None,
                    env: dict[str, str] | None = None, security_extra: bool = False) -> str:
    """Start `sleep infinity` as uid 1000 in /work. Returns the container name.

    Host caches /work/cache/<x> are mounted at /cache/<x> unless `mounts` already targets that
    path (warm-up mounts them rw): kinds in yaml `cache_overlay` (default [uv] for python) as a
    per-container overlay whose lower layer is the shared cache (never modified), the others as
    read-only binds. Always pair with remove(name), which also deletes the overlay.
    """
    if network not in ("none", "trialnet", "bridge"):
        raise ValueError(f"network must be none|trialnet|bridge, got {network!r}")
    cfg = _cfg(repo_id)
    image = cfg["image"]
    mounts = list(mounts or [])
    targets = {m[1].rstrip("/") for m in mounts}
    args = ["docker", "run", "-d", "--name", name, "--label", LABEL,
            "--user", f"{DEV_UID}:{DEV_GID}", "--workdir", WORKDIR,
            "--cpus", CPUS, "--memory", MEMORY, "--init",
            "--network", network]
    if security_extra:
        args += SECURITY_EXTRA
    overlay = set(cfg.get("cache_overlay", ["uv"] if cfg.get("lang") == "python" else []))
    for kind in CACHE_KINDS:
        host = HOST_CACHE / kind
        if not host.is_dir() or f"/cache/{kind}" in targets:
            continue
        if kind in overlay:
            # The shared cache is the read-only lower layer of a per-container overlay: the
            # container sees a writable /cache/<kind> (uv needs to create lock files), its
            # writes go to a private upper dir, and /work/cache/<kind> is never modified.
            args += ["--mount", f"type=volume,src={_overlay_volume(name, kind, host)},dst=/cache/{kind}"]
        else:
            args += ["--mount", f"type=bind,src={host},dst=/cache/{kind},readonly"]
    for host, ctr, mode in mounts:
        if mode not in ("ro", "rw"):
            raise ValueError(f"mount mode must be ro|rw: {mode}")
        spec = f"type=bind,src={host},dst={ctr}" + (",readonly" if mode == "ro" else "")
        args += ["--mount", spec]
    full_env = {"HOME": "/home/dev"}
    full_env.update(cfg.get("env", {}))
    pp = cfg.get("path_prepend")
    full_env["PATH"] = f"{pp}:{_image_path(image)}" if pp else _image_path(image)
    full_env.update({str(k): str(v) for k, v in (env or {}).items()})
    for k in sorted(full_env):
        args += ["-e", f"{k}={full_env[k]}"]
    args += [image, "sleep", "infinity"]
    r = subprocess.run(args, capture_output=True, text=True)
    if r.returncode != 0:
        remove(name)
        raise RuntimeError(f"docker run failed ({r.returncode}): {r.stderr.strip()}")
    return name


def exec_(name: str, cmd, user: str = "dev", timeout: float | None = None,
          env: dict[str, str] | None = None, *, input: bytes | str | None = None,
          workdir: str = WORKDIR, text: bool = True) -> subprocess.CompletedProcess:
    """Run `cmd` (str -> sh -c, or argv list) in the container. On timeout the command is
    killed inside the container (coreutils `timeout`), returncode 124/137."""
    argv = ["sh", "-c", cmd] if isinstance(cmd, str) else list(cmd)
    if timeout:
        argv = ["timeout", "-k", "10", str(int(timeout))] + argv
    d = ["docker", "exec", "-i" if input is not None else "", "-u", user, "-w", workdir]
    d = [a for a in d if a]
    for k, v in (env or {}).items():
        d += ["-e", f"{k}={v}"]
    d += [name] + argv
    if isinstance(input, str):
        input = input.encode()
    try:
        r = subprocess.run(d, input=input, capture_output=True,
                           timeout=(timeout + 60) if timeout else None)
    except subprocess.TimeoutExpired as e:
        r = subprocess.CompletedProcess(d, 124, e.stdout or b"", (e.stderr or b"") + b"\n[host timeout]")
    if text:
        r = subprocess.CompletedProcess(r.args, r.returncode,
                                        (r.stdout or b"").decode("utf-8", "replace"),
                                        (r.stderr or b"").decode("utf-8", "replace"))
    return r


def _overlay_volume(name: str, kind: str, lower: Path) -> str:
    base = OVERLAY_ROOT / name / kind
    shutil.rmtree(base, ignore_errors=True)
    (base / "upper").mkdir(parents=True)
    (base / "work").mkdir()
    os.chown(base / "upper", int(DEV_UID), int(DEV_GID))
    vol = f"{name}-cache-{kind}"
    subprocess.run(["docker", "volume", "rm", "-f", vol], capture_output=True)
    r = subprocess.run(["docker", "volume", "create", "--label", LABEL, "--label", f"forge-exp.container={name}",
                        "--driver", "local", "--opt", "type=overlay", "--opt", "device=overlay",
                        "--opt", f"o=lowerdir={lower},upperdir={base / 'upper'},workdir={base / 'work'}", vol],
                       capture_output=True, text=True)
    if r.returncode != 0:
        raise RuntimeError(f"overlay volume for {lower} failed: {r.stderr.strip()}")
    return vol


def remove(name: str) -> None:
    """Remove the container, its cache overlay volumes and their upper dirs."""
    subprocess.run(["docker", "rm", "-f", "-v", name], capture_output=True)
    vols = subprocess.run(["docker", "volume", "ls", "-q", "--filter", f"label=forge-exp.container={name}"],
                          capture_output=True, text=True).stdout.split()
    if vols:
        subprocess.run(["docker", "volume", "rm", "-f", *vols], capture_output=True)
    shutil.rmtree(OVERLAY_ROOT / name, ignore_errors=True)


# --------------------------------------------------------------------------- workspace

def materialize(name: str, repo_id: str, commit: str) -> None:
    """Tree of `commit` (from the host mirror) -> /work, as a fresh single-commit repo."""
    cfg = _cfg(repo_id)
    mirror = cfg["mirror"]
    attrs = subprocess.run(["git", "-C", mirror, "ls-tree", "-r", "--name-only", commit],
                           capture_output=True, text=True, check=True).stdout.splitlines()
    for p in attrs:
        if p == ".gitattributes" or p.endswith("/.gitattributes"):
            body = subprocess.run(["git", "-C", mirror, "show", f"{commit}:{p}"],
                                  capture_output=True, text=True).stdout
            if re.search(r"\bexport-(ignore|subst)\b", body):
                raise RuntimeError(f"{repo_id}@{commit}: {p} uses export-ignore/subst; "
                                   "git archive would not reproduce the tree")
    clean = exec_(name, f"find {WORKDIR} -mindepth 1 -maxdepth 1 -exec rm -rf {{}} +")
    if clean.returncode != 0:
        raise RuntimeError(f"cleaning /work failed: {clean.stderr}")
    arch = subprocess.Popen(["git", "-C", mirror, "archive", "--format=tar", commit],
                            stdout=subprocess.PIPE)
    ext = subprocess.run(["docker", "exec", "-i", "-u", "dev", name, "tar", "-x", "-C", WORKDIR],
                         stdin=arch.stdout, capture_output=True)
    arch.stdout.close()
    if arch.wait() != 0 or ext.returncode != 0:
        raise RuntimeError(f"materialize {commit} failed: {ext.stderr.decode(errors='replace')}")
    r = exec_(name, "git -c init.defaultBranch=main init -q . && git add -A -f . && "
                    "git -c commit.gpgsign=false commit -q --no-verify -m initial",
              env=GIT_IDENTITY)
    if r.returncode != 0:
        raise RuntimeError(f"git init/commit failed: {r.stdout}{r.stderr}")


def install(name: str, repo_id: str) -> tuple[bool, str]:
    """Run the repo's offline install command in /work as dev."""
    cfg = _cfg(repo_id)
    t0 = time.time()
    r = exec_(name, cfg["install"], timeout=int(cfg.get("install_timeout", 1800)))
    log = f"$ {cfg['install']}\n{r.stdout}{r.stderr}\n[exit {r.returncode} in {time.time() - t0:.0f}s]\n"
    return r.returncode == 0, log


def write_files(name: str, files: dict[str, bytes]) -> None:
    """Write files (paths relative to /work) owned by dev, creating parent directories."""
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w", format=tarfile.PAX_FORMAT) as tf:
        dirs: set[str] = set()
        now = int(time.time())
        for rel, data in files.items():
            rel = rel.lstrip("/")
            if ".." in Path(rel).parts:
                raise ValueError(f"bad path {rel}")
            if isinstance(data, str):
                data = data.encode()
            parts = Path(rel).parts[:-1]
            for i in range(1, len(parts) + 1):
                d = "/".join(parts[:i])
                if d not in dirs:
                    dirs.add(d)
                    ti = tarfile.TarInfo(d)
                    ti.type, ti.mode, ti.uid, ti.gid, ti.mtime = tarfile.DIRTYPE, 0o755, 1000, 1000, now
                    tf.addfile(ti)
            ti = tarfile.TarInfo(rel)
            ti.size, ti.mode, ti.uid, ti.gid, ti.mtime = len(data), 0o644, 1000, 1000, now
            tf.addfile(ti, io.BytesIO(data))
    r = subprocess.run(["docker", "exec", "-i", "-u", "dev", name, "tar", "-x", "--no-overwrite-dir",
                        "-C", WORKDIR], input=buf.getvalue(), capture_output=True)
    if r.returncode != 0:
        raise RuntimeError(f"write_files failed: {r.stderr.decode(errors='replace')}")


def read_file(name: str, path: str) -> bytes | None:
    r = exec_(name, ["cat", path if path.startswith("/") else f"{WORKDIR}/{path}"], text=False)
    return r.stdout if r.returncode == 0 else None


# --------------------------------------------------------------------------- tests

@dataclass
class TestRun:
    results: dict[str, str] = field(default_factory=dict)
    log: str = ""
    returncode: int = 0
    junit_found: bool = False
    timed_out: bool = False


_SEVERITY = {"passed": 0, "skipped": 1, "failed": 2, "error": 3}


def _merge(out: dict[str, str], tid: str, status: str) -> None:
    # A test reported twice (e.g. call failure + teardown error) keeps the worst status.
    if tid not in out or _SEVERITY[status] > _SEVERITY[out[tid]]:
        out[tid] = status


def _case_status(tc: ET.Element) -> str:
    tags = {c.tag for c in tc}
    if "error" in tags:
        return "error"
    if "failure" in tags:
        return "failed"
    if "skipped" in tags:
        return "skipped"
    return "passed"


def _pytest_dotted(path: str) -> str:
    return re.sub(r"\.py$", "", path).replace("/", ".")


def parse_junit(xml_text: str, flavor: str = "pytest", targets: list[str] | None = None) -> dict[str, str]:
    """JUnit XML -> {normalised id: status}. See module docstring for the id format."""
    root = ET.fromstring(xml_text)
    out: dict[str, str] = {}
    target_files = sorted({t.split("::", 1)[0] for t in (targets or [])}, key=len, reverse=True)
    for tc in root.iter("testcase"):
        cls = tc.get("classname") or ""
        nm = tc.get("name") or ""
        fattr = tc.get("file")
        status = _case_status(tc)
        if flavor == "vitest":
            f = fattr or cls
            if nm == f or (not cls and not nm):
                _merge(out, f, "error")  # file failed to load
            else:
                _merge(out, f"{f}::{nm}", status)
            continue
        # pytest: classname = dotted(file) [+ "." + classes]; collection error has classname "".
        if not cls:
            f = fattr or next((t for t in target_files if _pytest_dotted(t) == nm), None) \
                or nm.replace(".", "/") + ".py"
            _merge(out, f, "error" if status in ("error", "failed") else status)
            continue
        f = None
        candidates = ([fattr] if fattr else []) + target_files
        for c in candidates:
            d = _pytest_dotted(c)
            if cls == d or cls.startswith(d + "."):
                f = c
                rest = cls[len(d) + 1:] if cls != d else ""
                break
        if f is None:  # unknown file: best effort, assume module == classname (no classes)
            f, rest = cls.replace(".", "/") + ".py", ""
        tid = f + ("::" + rest.replace(".", "::") if rest else "") + "::" + nm
        _merge(out, tid, status)
    return out


def run_tests_detailed(name: str, repo_id: str, targets: list[str], timeout: int = 900) -> TestRun:
    cfg = _cfg(repo_id)
    junit = f"/tmp/forge-junit-{uuid.uuid4().hex[:12]}.xml"
    cmd = cfg["test_cmd"].replace("{junit}", junit).replace(
        "{targets}", " ".join(shlex.quote(t) for t in targets))
    t0 = time.time()
    r = exec_(name, cmd, timeout=timeout)
    run = TestRun(returncode=r.returncode, timed_out=r.returncode in (124, 137) and time.time() - t0 >= timeout - 1)
    run.log = f"$ {cmd}\n{r.stdout}{r.stderr}\n[exit {r.returncode} in {time.time() - t0:.0f}s]\n"
    raw = read_file(name, junit)
    exec_(name, ["rm", "-f", junit])
    if raw:
        run.junit_found = True
        try:
            run.results = parse_junit(raw.decode("utf-8", "replace"), cfg["junit_flavor"], targets)
        except ET.ParseError as e:
            run.junit_found = False
            run.log += f"\n[junit parse error: {e}]\n"
    return run


def run_tests(name: str, repo_id: str, targets: list[str], timeout: int = 900) -> dict[str, str]:
    """test_id -> passed|failed|error|skipped. Requested ids missing from the result were not run."""
    return run_tests_detailed(name, repo_id, targets, timeout).results


# --------------------------------------------------------------------------- lint

def _rel(path: str) -> str:
    return path[len(WORKDIR) + 1:] if path.startswith(WORKDIR + "/") else path


def lint(name: str, repo_id: str, files: list[str]) -> list[dict]:
    """[{file, line, code, message}] from the repo's linter (ruff JSON or biome JSON)."""
    cfg = _cfg(repo_id)
    if not files or not cfg.get("lint_cmd"):
        return []
    cmd = cfg["lint_cmd"].replace("{files}", " ".join(shlex.quote(f) for f in files))
    r = exec_(name, cmd, timeout=600)
    text = r.stdout.strip()
    if not text:
        return []
    start = min([i for i in (text.find("["), text.find("{")) if i >= 0], default=0)
    try:
        data = json.loads(text[start:])
    except json.JSONDecodeError:
        raise RuntimeError(f"lint output is not JSON (exit {r.returncode}): {text[:500]} {r.stderr[:500]}")
    out: list[dict] = []
    if isinstance(data, list):  # ruff
        for d in data:
            loc = d.get("location") or {}
            out.append({"file": _rel(d.get("filename", "")), "line": loc.get("row"),
                        "code": d.get("code") or "syntax-error", "message": d.get("message", "")})
    elif isinstance(data, dict) and "diagnostics" in data:  # biome --reporter=json
        for d in data["diagnostics"]:
            loc = d.get("location") or {}
            p = loc.get("path")
            f = p.get("file") if isinstance(p, dict) else p
            line = (loc.get("start") or {}).get("line") if isinstance(loc.get("start"), dict) else None
            if line is None and f and isinstance(loc.get("span"), list):
                src = read_file(name, f)
                if src is not None:
                    line = src[: loc["span"][0]].count(b"\n") + 1
            msg = d.get("description") or d.get("message") or ""
            if isinstance(msg, list):
                msg = "".join(m.get("content", "") if isinstance(m, dict) else str(m) for m in msg)
            out.append({"file": _rel(f or ""), "line": line, "code": d.get("category", ""),
                        "message": msg})
    return out


# --------------------------------------------------------------------------- CLI

def git_show(repo_id: str, commit: str, path: str) -> bytes:
    return subprocess.run(["git", "-C", _cfg(repo_id)["mirror"], "show", f"{commit}:{path}"],
                          capture_output=True, check=True).stdout


def _smoke(argv: list[str]) -> int:
    import argparse
    ap = argparse.ArgumentParser(prog="repo_env.py smoke")
    ap.add_argument("repo_id")
    ap.add_argument("commit")
    ap.add_argument("test_files", nargs="+")
    ap.add_argument("--overlay-from", help="write the test files as of this commit over the tree")
    ap.add_argument("--network", default="none")
    ap.add_argument("--keep", action="store_true", help="do not remove the container")
    ap.add_argument("--timeout", type=int, default=900)
    a = ap.parse_args(argv)
    name = f"smoke-{a.repo_id}-{uuid.uuid4().hex[:8]}"
    t0 = time.time()
    start_container(a.repo_id, name, network=a.network)
    try:
        materialize(name, a.repo_id, a.commit)
        print(f"[materialized {a.commit[:10]} in {time.time() - t0:.0f}s]", flush=True)
        if a.overlay_from:
            write_files(name, {p: git_show(a.repo_id, a.overlay_from, p) for p in a.test_files})
            print(f"[overlaid test files from {a.overlay_from[:10]}]", flush=True)
        ok, log = install(name, a.repo_id)
        print(log[-3000:], flush=True)
        if not ok:
            print("INSTALL FAILED")
            return 2
        run = run_tests_detailed(name, a.repo_id, a.test_files, a.timeout)
        print(run.log[-4000:])
        counts: dict[str, int] = {}
        for tid, st in sorted(run.results.items()):
            counts[st] = counts.get(st, 0) + 1
            if st in ("failed", "error"):
                print(f"  {st:7} {tid}")
        print(f"RESULT {a.repo_id} {a.commit[:10]} junit={run.junit_found} rc={run.returncode} "
              f"total={len(run.results)} {json.dumps(counts, sort_keys=True)} "
              f"elapsed={time.time() - t0:.0f}s")
        return 0
    finally:
        if not a.keep:
            remove(name)
        else:
            print(f"[kept container {name}]")


def main(argv: list[str]) -> int:
    if len(argv) >= 1 and argv[0] == "smoke":
        return _smoke(argv[1:])
    print(__doc__)
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
