"""One trial: prepare the container, (forge arm) apply Forge, verify parity, run Claude Code,
collect artifacts, score in a fresh container. See DESIGN.md §2, §6, §7, §9.

usage (normally called by orchestrate.py):
  python3 harness/run_trial.py <task_dir> <arm> <rep> <run_dir> [--attempt N]
"""
import argparse
import fcntl
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import repo_env  # noqa: E402
import classify  # noqa: E402
import transcript as tparse  # noqa: E402

PINS = json.load(open(os.path.join(HERE, "..", "config", "pins.json")))
CLAUDE_BIN = "/opt/claude-code/bin/claude"
AUTH_SRC = "/home/claude/.claude/remote/.oauth_token"
AUTH_DIR = "/work/auth/remote"  # per-run copy readable by uid 1000, refreshed before each trial
CA = "/root/.ccr/ca-bundle.crt"
FORGE_PKG = "/work/forge-pkg.tar"
TIMEOUT_S = PINS["trial_timeout_s"]
SCRATCH = "/work/runs"

# Paths Forge is documented to own (DESIGN §7). Anything else that differs between arms fails parity.
FORGE_OWNED = [re.compile(p) for p in [
    r"^/work/\.forge(/|$)", r"^/work/\.claude(/|$)", r"^/work/\.git/hooks/pre-commit$",
    r"^/home/dev/\.claude/settings\.json$", r"^/home/dev/\.claude/skills/forge-user(/|$)",
    r"^/home/dev/\.claude/forge[^/]*$",
]]
# Paths that only exist once Forge is applied: none may exist before apply, in either arm.
FORGE_MARKERS = [re.compile(p) for p in [
    r"^/work/\.forge(/|$)", r"^/work/\.claude/skills/forge-project(/|$)", r"^/work/\.claude/rules/forge-",
    r"^/home/dev/\.claude/skills/forge-user(/|$)", r"^/home/dev/\.claude/settings\.json$",
]]
# Side effects of running Forge's documented apply (its tool audits run the project's tools / pinned bunx
# and uvx tools). Allowed, but listed in parity.json.
FORGE_SIDE_EFFECTS = [re.compile(p) for p in [
    r"(^|/)__pycache__/", r"(^|/)\.ruff_cache(/|$)", r"(^|/)\.pytest_cache(/|$)", r"(^|/)node_modules/\.cache(/|$)",
    r"^/home/dev/\.cache(/|$)", r"^/home/dev/\.bun(/|$)", r"^/home/dev/\.npm(/|$)", r"^/home/dev/\.local/share/uv(/|$)",
    r"(^|/)\.basedpyright(/|$)", r"(^|/)tsconfig\.tsbuildinfo$", r"^/home/dev/\.claude\.json$",
]]


def sh(cmd, **kw):
    return subprocess.run(cmd, shell=isinstance(cmd, str), capture_output=True, text=True, **kw)


def root_exec(name, cmd, timeout=600, input_=None):
    return subprocess.run(["docker", "exec", "-i", "-u", "root", name, "sh", "-c", cmd], input=input_,
                          capture_output=True, text=True, timeout=timeout)


def dev_exec(name, cmd, env, timeout=600, workdir="/work"):
    args = ["docker", "exec", "-u", "dev", "-w", workdir]
    for k, v in env.items():
        args += ["-e", f"{k}={v}"]
    return subprocess.run(args + [name, "sh", "-c", cmd], capture_output=True, text=True, timeout=timeout)


def fingerprint(name):
    r = subprocess.run(["docker", "exec", "-u", "root", name, "python3", "/opt/harness/fingerprint.py"],
                       capture_output=True, text=True, timeout=900)
    if r.returncode != 0:
        raise RuntimeError("fingerprint failed: " + r.stderr[-2000:])
    return json.loads(r.stdout)


def digest(obj):
    return hashlib.sha256(json.dumps(obj, sort_keys=True).encode()).hexdigest()


def agent_env(repo):
    # PATH comes from the container (image PATH with the repo's path_prepend, set by repo_env; D5)
    env = {
        "HOME": "/home/dev", "USER": "dev", "LANG": "C.UTF-8", "TERM": "dumb",
        "HTTPS_PROXY": PINS["trial_proxy"], "https_proxy": PINS["trial_proxy"],
        "NO_PROXY": "localhost,127.0.0.1", "no_proxy": "localhost,127.0.0.1",
        "NODE_EXTRA_CA_CERTS": "/etc/ssl/ccr-ca.crt", "SSL_CERT_FILE": "/etc/ssl/ccr-ca.crt",
        "DISABLE_AUTOUPDATER": "1",
    }
    env.update(repo.get("env", {}) or {})
    return env


def claude_argv(model):
    return [CLAUDE_BIN.replace("/opt/claude-code/bin/claude", "claude"), "-p", "--model", model,
            "--permission-mode", "bypassPermissions", "--output-format", "stream-json", "--verbose",
            "--include-hook-events", "--debug-file", "/logs/debug.log",
            "--max-budget-usd", str(PINS["max_budget_usd"])]


def parity_ref_check(run_dir, task_id, fp_digest, extra, source=None):
    """First prepared trial of a task sets the reference; every later one must match."""
    d = os.path.join(run_dir, "_parity")
    os.makedirs(d, exist_ok=True)
    p = os.path.join(d, f"{task_id}.json")
    with open(p + ".lock", "w") as lk:
        fcntl.flock(lk, fcntl.LOCK_EX)
        if not os.path.exists(p):
            json.dump({"fingerprint": fp_digest, "source": source, **extra}, open(p, "w"), indent=1)
            if source:
                shutil.copy(source, os.path.join(d, f"{task_id}.full.json"))
            return True, "reference set"
        ref = json.load(open(p))
        if ref["fingerprint"] != fp_digest:
            return False, "pre-agent fingerprint differs from the task reference"
        for k, v in extra.items():
            if ref.get(k) != v:
                return False, f"{k} differs from the task reference"
        return True, "matches reference"


def refresh_auth():
    os.makedirs(AUTH_DIR, exist_ok=True)
    tmp = os.path.join(AUTH_DIR, ".oauth_token.tmp")
    shutil.copy(AUTH_SRC, tmp)
    os.chown(tmp, 1000, 1000)
    os.chmod(tmp, 0o400)
    os.replace(tmp, os.path.join(AUTH_DIR, ".oauth_token"))
    os.chown(AUTH_DIR, 1000, 1000)
    os.chmod(AUTH_DIR, 0o700)


def prepare(name, task, repo, arm, logs_dir):
    refresh_auth()
    mounts = [(CLAUDE_BIN, CLAUDE_BIN, "ro"), (AUTH_DIR, "/home/claude/.claude/remote", "ro"), (CA, "/etc/ssl/ccr-ca.crt", "ro"),
              (logs_dir, "/logs", "rw"), (HERE, "/opt/harness", "ro")]
    subprocess.run(["docker", "rm", "-f", name], capture_output=True)
    repo_env.start_container(task["repo"], name, network="trialnet", mounts=mounts, env={}, security_extra=True)
    root_exec(name, "chown dev:dev /logs && install -m 0755 /opt/harness/claude_wrapper.py /usr/local/bin/claude")
    repo_env.materialize(name, task["repo"], task["parent"])
    ok, log = repo_env.install(name, task["repo"])
    open(os.path.join(logs_dir, "install.log"), "w").write(log)
    if not ok:
        raise classify.Infra("dependency install failed")
    # trust the workspace + skip onboarding (both arms; DECISIONS D8)
    cfg = {"hasCompletedOnboarding": True, "bypassPermissionsModeAccepted": True,
           "projects": {"/work": {"hasTrustDialogAccepted": True, "hasCompletedProjectOnboarding": True}}}
    root_exec(name, "cat > /home/dev/.claude.json && chown dev:dev /home/dev/.claude.json", input_=json.dumps(cfg))


def apply_forge(name, env, logs_dir):
    with open(FORGE_PKG, "rb") as f:
        r = subprocess.run(["docker", "exec", "-i", "-u", "root", name, "sh", "-c",
                            "mkdir -p /tmp/forge && tar -xf - -C /tmp/forge && chown -R dev:dev /tmp/forge"],
                           stdin=f, capture_output=True, text=True)
    if r.returncode:
        raise classify.Infra("forge copy failed: " + r.stderr)
    out = []
    for cmd in ["node /tmp/forge/dist/forge.js apply --user --yes", "node /tmp/forge/dist/forge.js apply --yes",
                "node /tmp/forge/dist/forge.js status --json; node /tmp/forge/dist/forge.js status --user --json"]:
        r = dev_exec(name, cmd, env, timeout=900)
        out.append(f"$ {cmd}\n[exit {r.returncode}]\n{r.stdout}\n{r.stderr}\n")
        if r.returncode and "status" not in cmd:
            open(os.path.join(logs_dir, "forge_apply.log"), "w").write("\n".join(out))
            raise classify.Infra(f"forge apply failed ({cmd})")
    root_exec(name, "rm -rf /tmp/forge")
    # Getting started: `git add .forge .claude && git commit -m "Add Forge"` (fixed identity/date, like the initial commit)
    r = dev_exec(name, "git add .forge .claude && git -c user.name=dev -c user.email=dev@example.com commit -q -m 'Add Forge'",
                 {**env, "GIT_AUTHOR_DATE": "2000-01-01T00:00:00Z", "GIT_COMMITTER_DATE": "2000-01-01T00:00:00Z"})
    out.append(f"$ git add .forge .claude && git commit -m 'Add Forge'\n[exit {r.returncode}]\n{r.stdout}{r.stderr}")
    open(os.path.join(logs_dir, "forge_apply.log"), "w").write("\n".join(out))
    if r.returncode:
        raise classify.Infra("committing the Forge setup failed")


def classify_paths(diff_paths):
    owned, side, other = [], [], []
    for p in diff_paths:
        if any(rx.search(p) for rx in FORGE_OWNED):
            owned.append(p)
        elif any(rx.search(p) for rx in FORGE_SIDE_EFFECTS):
            side.append(p)
        else:
            other.append(p)
    return owned, side, other


def fp_diff(a, b):
    return sorted(k for k in set(a) | set(b) if a.get(k) != b.get(k))


def collect_diff(name, logs_dir):
    """Agent's final change against the starting (root) commit, ignoring Forge-owned paths (gitignore respected).
    Runs as dev: git refuses a repository owned by another user."""
    cmd = ("cd /work && export GIT_INDEX_FILE=/tmp/score.idx && rm -f $GIT_INDEX_FILE && "
           "git add -A -- . ':(exclude).forge' ':(exclude).claude' && "
           "git diff --cached --binary $(git rev-list --max-parents=0 HEAD | tail -1) -- . ':(exclude).forge' ':(exclude).claude'")
    r = subprocess.run(["docker", "exec", "-u", "dev", name, "sh", "-c", cmd], capture_output=True, text=True, timeout=300)
    open(os.path.join(logs_dir, "collect_diff.log"), "w").write(f"exit {r.returncode}\n{r.stderr}")
    if r.returncode != 0:
        raise classify.Infra("collecting the agent diff failed: " + r.stderr[-500:])
    return r.stdout


def score(task, task_dir, diff_text, logs_dir, name):
    """Fresh container, offline: parent tree + install + agent diff + hidden tests -> run kept tests."""
    subprocess.run(["docker", "rm", "-f", name], capture_output=True)
    repo_env.start_container(task["repo"], name, network="none", mounts=[(HERE, "/opt/harness", "ro")], env={})
    try:
        repo_env.materialize(name, task["repo"], task["parent"])
        ok, log = repo_env.install(name, task["repo"])
        if not ok:
            raise classify.Infra("scoring install failed")
        changed = [l[6:] for l in diff_text.splitlines() if l.startswith("+++ b/")]
        src_changed = [p for p in changed if re.search(r"\.(py|ts|tsx|mts|cts|js|mjs|cjs)$", p)]
        lint_before = repo_env.lint(name, task["repo"], [p for p in src_changed]) if src_changed else []
        applied = True
        if diff_text.strip():
            r = subprocess.run(["docker", "exec", "-i", "-u", "dev", "-w", "/work", name, "git", "apply", "--binary",
                                "--whitespace=nowarn", "-"], input=diff_text, capture_output=True, text=True)
            applied = r.returncode == 0
            open(os.path.join(logs_dir, "score_apply.log"), "w").write(r.stdout + r.stderr)
        lint_after = repo_env.lint(name, task["repo"], src_changed) if (src_changed and applied) else []
        hidden_root = os.path.join(task_dir, "hidden")
        files = {}
        for d, _, fs in os.walk(hidden_root):
            for f in fs:
                p = os.path.join(d, f)
                files[os.path.relpath(p, hidden_root)] = open(p, "rb").read()
        repo_env.write_files(name, files)
        for rel in task.get("hidden_deleted", []):
            repo_env.exec_(name, f"rm -f '/work/{rel}'")
        targets = task["test_targets"]
        res = repo_env.run_tests(name, task["repo"], targets, timeout=1200) if applied else {}
        f2p = {t: res.get(t, "missing") for t in task["f2p"]}
        p2p = {t: res.get(t, "missing") for t in task["p2p"]}
        if applied and not res:
            # nothing parsed at all: the runner could not run (environment) or the agent broke collection.
            # Distinguish by running the same targets on the untouched reference state? No: we never look at
            # the reference here. An empty result with a clean apply is scored as a failure (agent broke
            # collection, e.g. syntax error), and logged for review in validity checks.
            pass
        resolved = applied and all(s == "passed" for s in f2p.values()) and all(s == "passed" for s in p2p.values())
        key = lambda x: (x.get("file"), x.get("code"), x.get("message"))  # noqa: E731
        before = {}
        for x in lint_before:
            before[key(x)] = before.get(key(x), 0) + 1
        lint_new = 0
        for x in lint_after:
            k = key(x)
            if before.get(k, 0) > 0:
                before[k] -= 1
            else:
                lint_new += 1
        n_diff = sum(1 for l in diff_text.splitlines() if (l.startswith("+") or l.startswith("-"))
                     and not l.startswith("+++") and not l.startswith("---"))
        return {"resolved": bool(resolved), "patch_applied": applied, "f2p": f2p, "p2p": p2p,
                "f2p_frac": (sum(s == "passed" for s in f2p.values()) / len(f2p)) if f2p else 0.0,
                "p2p_regressions": sum(s != "passed" for s in p2p.values()),
                "lint_new": lint_new, "diff_lines": n_diff, "changed_files": changed, "all_results": res}
    finally:
        repo_env.remove(name)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("task_dir")
    ap.add_argument("arm", choices=["baseline", "forge"])
    ap.add_argument("rep", type=int)
    ap.add_argument("run_dir")
    ap.add_argument("--attempt", type=int, default=1)
    a = ap.parse_args()
    task = json.load(open(os.path.join(a.task_dir, "task.json")))
    instruction = open(os.path.join(a.task_dir, "instruction.md")).read()
    repo = repo_env.load_repos()[task["repo"]]
    out = os.path.join(a.run_dir, task["task_id"], a.arm, f"r{a.rep}")
    if os.path.exists(out):
        shutil.rmtree(out)
    os.makedirs(os.path.join(out, "forge"), exist_ok=True)
    tag = re.sub(r"[^a-zA-Z0-9_.-]", "-", f"{task['task_id']}-{a.arm}-r{a.rep}-a{a.attempt}")[:100]
    logs_dir = os.path.join(SCRATCH, tag, "logs")
    shutil.rmtree(os.path.join(SCRATCH, tag), ignore_errors=True)
    os.makedirs(logs_dir)
    os.chmod(logs_dir, 0o777)
    name, sname = "trial-" + tag, "score-" + tag
    meta = {"task_id": task["task_id"], "repo": task["repo"], "arm": a.arm, "rep": a.rep, "attempt": a.attempt,
            "status": None, "infra_reason": None, "model": PINS["model"], "claude_version": PINS["claude_version"],
            "forge_commit": PINS["forge_commit"], "started": time.time()}
    env = agent_env(repo)
    argv = claude_argv(PINS["model"])
    try:
        try:
            prepare(name, task, repo, a.arm, logs_dir)
            fp_pre = fingerprint(name)
            json.dump(fp_pre, open(os.path.join(out, "fingerprint_pre.json"), "w"))
            insp = json.loads(sh(["docker", "inspect", name]).stdout)[0]
            hc = insp["HostConfig"]
            extra = {"image": insp["Image"], "argv": argv, "env": digest(env),
                     "container_env": sorted(insp["Config"].get("Env") or []),
                     "claude_sha256": PINS["claude_sha256"],
                     "limits": [hc.get("NanoCpus"), hc.get("Memory"), sorted(hc.get("SecurityOpt") or []),
                                sorted(hc.get("CapAdd") or []), insp["HostConfig"].get("NetworkMode"),
                                sorted(m["Destination"] + ":" + str(m.get("RW")) for m in insp.get("Mounts", []))]}
            owned_pre = [p for p in fp_pre if any(rx.search(p) for rx in FORGE_MARKERS)]
            parity = {"pre_digest": digest(fp_pre), "extra": extra}
            if owned_pre:
                raise classify.Parity(f"Forge-owned paths present before apply: {owned_pre[:5]}")
            ok, why = parity_ref_check(a.run_dir, task["task_id"], digest(fp_pre), extra, source=os.path.join(out, "fingerprint_pre.json"))
            parity["reference"] = why
            if not ok:
                raise classify.Parity(why)
            if a.arm == "forge":
                apply_forge(name, env, logs_dir)
                fp_post = fingerprint(name)
                owned, side, other = classify_paths(fp_diff(fp_pre, fp_post))
                parity.update({"forge_owned_changed": owned, "forge_side_effects": side, "unexpected": other})
                if other:
                    raise classify.Parity(f"forge apply changed non-Forge paths: {other[:10]}")
                if not any(p.startswith("/work/.forge/") for p in owned) or \
                        not any(p.startswith("/home/dev/.claude/") for p in owned):
                    raise classify.Parity("Forge files missing after apply")
                lock = root_exec(name, "cat /work/.forge/forge.lock.json").stdout
                parity["forge_lock_version"] = json.loads(lock).get("forge", json.loads(lock).get("version")) \
                    if lock.strip().startswith("{") else None
            parity["ok"] = True
            json.dump(parity, open(os.path.join(out, "parity.json"), "w"), indent=1)
        except classify.Parity as e:
            meta.update(status="parity_error", infra_reason=str(e))
            json.dump(locals().get("parity", {}), open(os.path.join(out, "parity.json"), "w"), indent=1)
            return
        except classify.Infra as e:
            meta.update(status="infra_error", infra_reason=str(e))
            return

        # ---- the agent ----
        t0 = time.time()
        tpath = os.path.join(out, "transcript.jsonl")
        dargs = ["docker", "exec", "-i", "-u", "dev", "-w", "/work"]
        for k, v in env.items():
            dargs += ["-e", f"{k}={v}"]
        timed_out = False
        with open(tpath, "w") as tf, open(os.path.join(out, "agent_stderr.log"), "w") as ef:
            p = subprocess.Popen(dargs + [name] + argv, stdin=subprocess.PIPE, stdout=tf, stderr=ef, text=True)
            p.stdin.write(instruction)
            p.stdin.close()
            try:
                p.wait(timeout=TIMEOUT_S)
            except subprocess.TimeoutExpired:
                timed_out = True
                root_exec(name, "pkill -TERM -f /opt/claude-code/bin/claude; sleep 5; pkill -KILL -f /opt/claude-code/bin/claude")
                p.kill()
                p.wait()
        meta["duration_s"] = time.time() - t0
        meta["exit_code"] = p.returncode
        meta["timed_out"] = timed_out
        # background processes the agent left are stopped before collection (same for both arms)
        root_exec(name, "for p in $(pgrep -u dev); do c=$(tr '\\0' ' ' < /proc/$p/cmdline 2>/dev/null); "
                        "case \"$c\" in *docker-init*|'sleep infinity'*) ;; *) kill -KILL $p 2>/dev/null;; esac; done; true")
        tinfo = tparse.parse(tpath)
        meta.update({k: tinfo[k] for k in ("num_turns", "cost_usd_agent", "result_subtype", "is_error", "result_text")})
        json.dump(tinfo["edits"], open(os.path.join(out, "edits.json"), "w"), indent=1)
        json.dump(tinfo["hooks"], open(os.path.join(out, "forge", "hook_summary.json"), "w"), indent=1)
        infra = classify.agent_infra(tinfo, timed_out, open(os.path.join(out, "agent_stderr.log")).read())
        # copy logs + forge state
        root_exec(name, "cp -r /work/.forge/state /logs/forge_state 2>/dev/null; cp /home/dev/.claude/forge-*.log /logs/ 2>/dev/null; chmod -R a+rX /logs")
        diff_text = collect_diff(name, logs_dir)
        open(os.path.join(out, "final.diff"), "w").write(diff_text)
        repo_env.remove(name)
        for f in os.listdir(logs_dir):
            src = os.path.join(logs_dir, f)
            if os.path.islink(src):
                continue  # e.g. Claude Code's debug `latest` symlink
            dst = os.path.join(out, "forge" if f.startswith("forge") else "", f)
            (shutil.copytree if os.path.isdir(src) else shutil.copy)(src, dst)
        calls = os.path.join(out, "claude_calls.jsonl")
        rev_cost = 0.0
        if os.path.exists(calls):
            for line in open(calls):
                c = json.loads(line)
                if c.get("forge_reviewer") or (c.get("result") and c["result"].get("total_cost_usd")):
                    rev_cost += (c.get("result") or {}).get("total_cost_usd") or 0.0
        meta["cost_usd_reviewer"] = rev_cost
        meta["cost_usd"] = (meta.get("cost_usd_agent") or 0.0) + rev_cost
        if infra:
            meta.update(status="infra_error", infra_reason=infra)
            return
        # ---- scoring ----
        try:
            outcome = score(task, a.task_dir, diff_text, os.path.join(out), sname)
        except classify.Infra as e:
            meta.update(status="infra_error", infra_reason="scoring: " + str(e))
            return
        json.dump(outcome, open(os.path.join(out, "outcome.json"), "w"), indent=1)
        meta["status"] = "scored"
    except Exception as e:  # noqa: BLE001 - any harness exception is infrastructure, never a result
        meta.update(status="infra_error", infra_reason=f"harness exception: {e!r}"[:2000])
    finally:
        meta["finished"] = time.time()
        json.dump(meta, open(os.path.join(out, "meta.json"), "w"), indent=1)
        for n in (name, sname):
            subprocess.run(["docker", "rm", "-f", n], capture_output=True)
        if meta.get("status") != "scored" and os.path.isdir(logs_dir):
            shutil.copytree(logs_dir, os.path.join(out, "_logs"), dirs_exist_ok=True, symlinks=True)
        shutil.rmtree(os.path.join(SCRATCH, tag), ignore_errors=True)
        print(json.dumps({k: meta.get(k) for k in ("task_id", "arm", "rep", "status", "infra_reason", "cost_usd",
                                                    "duration_s")}))


if __name__ == "__main__":
    main()
