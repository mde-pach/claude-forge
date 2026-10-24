"""Build the task dataset (DESIGN.md §4-5). Stages per candidate, cached in dataset/work/<task_id>/:
  1. validate.json   : execution validation (3 runs parent+hidden, 3 runs reference+hidden), F2P/P2P
  2. instruction.md  : fresh claude -p (no tools) from commit message + diff
  3. audit.json      : fresh, isolated claude -p (Read/Grep/Glob on the repo at C) applying the fixed rule
  4. revalidate.json : audited tests re-run 3x on parent (kept F2P must fail) and reference (all kept must pass)
Then `freeze` takes the first N accepted per repo (date order) into dataset/tasks/ + manifest.

usage:
  python3 dataset/build.py process <repo_id> [--window main|pilot] [--max-candidates K] [--jobs J]
  python3 dataset/build.py freeze --version v1 [--per-repo 10] [--window main] [--repos a,b,...]
"""
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from concurrent.futures import ThreadPoolExecutor

HERE = os.path.dirname(os.path.abspath(__file__))
EXP = os.path.dirname(HERE)
sys.path.insert(0, os.path.join(EXP, "harness"))
import repo_env  # noqa: E402
import classify  # noqa: E402

PINS = json.load(open(os.path.join(EXP, "config", "pins.json")))
WORK = os.path.join(HERE, "work")
TEST_PATH = re.compile(r"(^|/)(tests?|__tests__|testing)/|(^|/)test_[^/]*\.py$|_test\.py$|(^|/)conftest\.py$|\.(test|spec)\.(m?[jt]sx?|cts)$")
TEST_MODULE = re.compile(r"(^|/)test_[^/]*\.py$|_test\.py$|\.(test|spec)\.(m?[jt]sx?|cts)$")
N_RUNS = 3
PREAMBLE = ("You are working in the git repository in the current directory. Make the change described below. "
            "Work autonomously: nobody will answer questions.\n\n")
AUDIT_SCHEMA = {
    "type": "object", "required": ["verdicts", "edits"],
    "properties": {
        "verdicts": {"type": "array", "items": {"type": "object", "required": ["id", "action", "reason"], "properties": {
            "id": {"type": "string"}, "action": {"type": "string", "enum": ["keep", "drop", "relax"]},
            "reason": {"type": "string"}}}},
        "edits": {"type": "array", "items": {"type": "object", "required": ["file", "old", "new"], "properties": {
            "file": {"type": "string"}, "old": {"type": "string"}, "new": {"type": "string"}}}}}}


def git(mirror, *a, binary=False):
    r = subprocess.run(["git", "-C", mirror, *a], capture_output=True, check=True)
    return r.stdout if binary else r.stdout.decode("utf8", "replace")


def tid(repo_id, sha):
    return f"{repo_id}__{sha[:10]}"


def jdump(obj, path):
    tmp = path + ".tmp"
    json.dump(obj, open(tmp, "w"), indent=1)
    os.replace(tmp, path)


def task_files(cand, mirror):
    """Hidden test files (all changed test-path files at C), deletions, targets, diffs."""
    parent, commit = cand["parent"], cand["commit"]
    status = git(mirror, "diff", "--name-status", "--no-renames", parent, commit).splitlines()
    hidden, deleted = [], []
    for line in status:
        st, path = line.split("\t", 1)
        if TEST_PATH.search(path):
            (deleted if st == "D" else hidden).append(path)
    targets = [p for p in hidden if TEST_MODULE.search(p)]
    test_diff = git(mirror, "diff", "--no-renames", parent, commit, "--", *hidden, *deleted) if (hidden or deleted) else ""
    excl = [f":(exclude){p}" for p in hidden + deleted]
    ref_diff = git(mirror, "diff", "--binary", "--no-renames", parent, commit, "--", ".", *excl)
    src_diff = git(mirror, "diff", "--no-renames", parent, commit, "--", *cand["source_files"])
    return hidden, deleted, targets, test_diff, ref_diff, src_diff


def hidden_contents(mirror, commit, hidden):
    return {p: git(mirror, "show", f"{commit}:{p}", binary=True) for p in hidden}


def run_state(name, repo_id, targets, runs):
    out = []
    for _ in range(runs):
        tr = repo_env.run_tests_detailed(name, repo_id, targets, timeout=1500)
        bad = classify.env_failure(tr.log)
        if bad:
            raise RuntimeError(f"infrastructure failure during tests: {bad}")  # never a task decision (rule 5)
        out.append(tr.results)
    return out


def apply_patch(name, patch):
    r = subprocess.run(["docker", "exec", "-i", "-u", "dev", "-w", "/work", name, "git", "apply", "--binary",
                        "--whitespace=nowarn", "-"], input=patch.encode() if isinstance(patch, str) else patch,
                       capture_output=True)
    return r.returncode == 0, r.stderr.decode("utf8", "replace")


def classify_runs(parent_runs, ref_runs):
    ids = set()
    for r in parent_runs + ref_runs:
        ids |= set(r)
    f2p, p2p, flaky = [], [], []
    for t in sorted(ids):
        ps = [r.get(t, "missing") for r in parent_runs]
        rs = [r.get(t, "missing") for r in ref_runs]
        if all(s in ("failed", "error", "missing") for s in ps) and all(s == "passed" for s in rs):
            f2p.append(t)
        elif all(s == "passed" for s in ps + rs):
            p2p.append(t)
        elif len(set(ps)) > 1 or len(set(rs)) > 1:
            flaky.append(t)
    return f2p, p2p, flaky


def execute(repo_id, cand, hidden_files, deleted, targets, ref_diff, label):
    """Start offline container at parent, install, write hidden tests, run N; apply ref, run N."""
    name = f"val-{label}-{cand['commit'][:10]}-{os.getpid()}"
    repo_env.start_container(repo_id, name, network="none", mounts=[], env={})
    try:
        repo_env.materialize(name, repo_id, cand["parent"])
        ok, log = repo_env.install(name, repo_id)
        if not ok:
            bad = classify.env_failure(log)
            if bad:
                raise RuntimeError(f"infrastructure failure during install: {bad}")
            return {"error": "install_failed", "log": log[-3000:]}
        repo_env.write_files(name, hidden_files)
        for p in deleted:
            repo_env.exec_(name, f"rm -f '/work/{p}'")
        t0 = time.time()
        parent_runs = run_state(name, repo_id, targets, N_RUNS)
        ok, err = apply_patch(name, ref_diff)
        if not ok:
            alive = subprocess.run(["docker", "inspect", "-f", "{{.State.Running}}", name], capture_output=True, text=True).stdout.strip()
            if alive != "true":
                raise RuntimeError("infrastructure failure: validation container is gone")
            return {"error": "reference_patch_failed", "log": err[-3000:]}
        ref_runs = run_state(name, repo_id, targets, N_RUNS)
        return {"parent_runs": parent_runs, "ref_runs": ref_runs, "seconds": time.time() - t0}
    finally:
        repo_env.remove(name)


def clean_env():
    home = tempfile.mkdtemp(prefix="dsagent-home-")
    return home, {"PATH": "/opt/node22/bin:/usr/local/bin:/usr/bin:/bin", "HOME": home,
                  "HTTPS_PROXY": os.environ["HTTPS_PROXY"], "NODE_EXTRA_CA_CERTS": "/root/.ccr/ca-bundle.crt",
                  "SSL_CERT_FILE": "/root/.ccr/ca-bundle.crt"}


def claude_json(prompt, cwd, tools, schema=None, max_turns=40):
    home, env = clean_env()
    args = ["claude", "-p", "--model", PINS["dataset_model"], "--setting-sources", "", "--strict-mcp-config",
            "--no-session-persistence", "--output-format", "json", "--max-turns", str(max_turns),
            "--settings", '{"disableAllHooks":true}']
    if tools:
        args += ["--tools", tools, "--allowedTools", tools, "--permission-mode", "dontAsk"]
    else:
        args += ["--tools", ""]
    if schema:
        args += ["--json-schema", json.dumps(schema)]
    for attempt in range(4):
        r = subprocess.run(args, input=prompt, cwd=cwd, env=env, capture_output=True, text=True, timeout=1800)
        try:
            res = json.loads(r.stdout)
        except ValueError:
            res = None
        if res and not res.get("is_error"):
            shutil.rmtree(home, ignore_errors=True)
            return res
        time.sleep(20 * (attempt + 1))  # infrastructure retry (API/network); never a task decision
    shutil.rmtree(home, ignore_errors=True)
    raise RuntimeError(f"claude failed: {r.stdout[-1000:]} {r.stderr[-1000:]}")


def write_instruction(repo_id, cand, mirror, src_diff, test_diff):
    msg = git(mirror, "log", "-1", "--format=%B", cand["commit"]).strip()
    tpl = open(os.path.join(HERE, "prompts", "instruction_writer.md")).read()
    prompt = tpl.format(repo=repo_id, message=msg, src_diff=src_diff[:60000], test_diff=test_diff[:60000])
    res = claude_json(prompt, cwd=tempfile.gettempdir(), tools=None, max_turns=2)
    return res["result"].strip(), res.get("total_cost_usd")


def audit(repo_id, cand, mirror, instruction, hidden, test_diff, ref_diff, f2p, p2p):
    checkout = tempfile.mkdtemp(prefix="audit-repo-")
    try:
        tar = git(mirror, "archive", cand["commit"], binary=True)
        subprocess.run(["tar", "-x", "-C", checkout], input=tar, check=True)
        tpl = open(os.path.join(HERE, "prompts", "auditor.md")).read()
        p2p_txt = "\n".join(f"- {t}" for t in p2p[:400]) + (f"\n- ... and {len(p2p) - 400} more" if len(p2p) > 400 else "")
        prompt = tpl.format(repo=repo_id, instruction=instruction, test_files="\n".join(f"- {p}" for p in hidden),
                            test_diff=test_diff[:80000], ref_diff=ref_diff[:60000],
                            f2p="\n".join(f"- {t}" for t in f2p), p2p=p2p_txt or "(none)")
        res = claude_json(prompt, cwd=checkout, tools="Read,Grep,Glob", schema=AUDIT_SCHEMA, max_turns=60)
        out = res.get("structured_output")
        if out is None:
            out = json.loads(res["result"])
        return out, res.get("total_cost_usd")
    finally:
        shutil.rmtree(checkout, ignore_errors=True)


def apply_audit(hidden_files, audit_out, f2p, p2p):
    files = dict(hidden_files)
    applied, rejected = [], []
    for e in audit_out.get("edits", []):
        f = e["file"]
        if f not in files:
            rejected.append({**e, "why": "unknown file"}); continue
        text = files[f].decode("utf8")
        if text.count(e["old"]) != 1:
            rejected.append({**e, "why": f"old occurs {text.count(e['old'])} times"}); continue
        files[f] = text.replace(e["old"], e["new"]).encode()
        applied.append(e)
    dropped = {v["id"] for v in audit_out.get("verdicts", []) if v["action"] == "drop"}
    # an edit that could not be applied means the relaxed test is not as the auditor intended: drop it (when in doubt, drop)
    relax_ids = {v["id"] for v in audit_out.get("verdicts", []) if v["action"] == "relax"}
    if rejected:
        dropped |= relax_ids
    kf2p = [t for t in f2p if t not in dropped]
    kp2p = [t for t in p2p if t not in dropped]
    return files, kf2p, kp2p, applied, rejected


def process_one(repo_id, cand, window):
    repo = repo_env.load_repos()[repo_id]
    mirror = repo["mirror"]
    t = tid(repo_id, cand["commit"])
    wd = os.path.join(WORK, t)
    os.makedirs(wd, exist_ok=True)
    state_p = os.path.join(wd, "state.json")
    state = json.load(open(state_p)) if os.path.exists(state_p) else {"task_id": t, "window": window, **cand}
    if state.get("final"):
        return state
    hidden, deleted, targets, test_diff, ref_diff, src_diff = task_files(cand, mirror)
    state.update(hidden=hidden, deleted=deleted, targets=targets)
    if not targets:
        state.update(final="rejected", reason="no_test_module_changed")
        jdump(state, state_p); return state
    hfiles = hidden_contents(mirror, cand["commit"], hidden)
    # 1. execution validation
    vp = os.path.join(wd, "validate.json")
    if not os.path.exists(vp):
        v = execute(repo_id, cand, hfiles, deleted, targets, ref_diff, "v")
        jdump(v, vp)
    v = json.load(open(vp))
    if v.get("error"):
        state.update(final="rejected", reason=v["error"])
        jdump(state, state_p); return state
    f2p, p2p, flaky = classify_runs(v["parent_runs"], v["ref_runs"])
    state.update(f2p=f2p, p2p=p2p, flaky=flaky)
    if not any(r for r in v["ref_runs"]):
        state.update(final="rejected", reason="tests_did_not_run")
        jdump(state, state_p); return state
    if not f2p:
        state.update(final="rejected", reason="no_f2p_test")
        jdump(state, state_p); return state
    # 2. instruction
    ip = os.path.join(wd, "instruction.md")
    if not os.path.exists(ip):
        text, cost = write_instruction(repo_id, cand, mirror, src_diff, test_diff)
        open(ip, "w").write(text + "\n")
        state["cost_instruction"] = cost
    instruction = open(ip).read()
    # 3. audit
    ap = os.path.join(wd, "audit.json")
    if not os.path.exists(ap):
        out, cost = audit(repo_id, cand, mirror, instruction, hidden, test_diff, ref_diff, f2p, p2p)
        state["cost_audit"] = cost
        jdump(out, ap)
    audit_out = json.load(open(ap))
    files, kf2p, kp2p, applied, rejected = apply_audit(hfiles, audit_out, f2p, p2p)
    state.update(kept_f2p=kf2p, kept_p2p=kp2p, edits_applied=len(applied), edits_rejected=rejected,
                 dropped=[v_ for v_ in audit_out.get("verdicts", []) if v_["action"] == "drop"])
    if not kf2p:
        state.update(final="rejected", reason="audit_left_no_f2p")
        jdump(state, state_p); return state
    # 4. re-validation of audited tests
    rp = os.path.join(wd, "revalidate.json")
    if not os.path.exists(rp):
        jdump(execute(repo_id, cand, files, deleted, targets, ref_diff, "r"), rp)
    rv = json.load(open(rp))
    if rv.get("error"):
        state.update(final="rejected", reason="revalidate_" + rv["error"])
        jdump(state, state_p); return state
    bad_f2p = [x for x in kf2p if any(r.get(x, "missing") == "passed" for r in rv["parent_runs"])
               or any(r.get(x, "missing") != "passed" for r in rv["ref_runs"])]
    bad_p2p = [x for x in kp2p if any(r.get(x, "missing") != "passed" for r in rv["parent_runs"] + rv["ref_runs"])]
    state.update(revalidate_bad_f2p=bad_f2p, revalidate_bad_p2p=bad_p2p)
    if bad_f2p or bad_p2p:
        state.update(final="rejected", reason="revalidation_failed")
        jdump(state, state_p); return state
    hdir = os.path.join(wd, "hidden")
    shutil.rmtree(hdir, ignore_errors=True)
    for p, b in files.items():
        os.makedirs(os.path.dirname(os.path.join(hdir, p)), exist_ok=True)
        open(os.path.join(hdir, p), "wb").write(b)
    open(os.path.join(wd, "reference.patch"), "w").write(ref_diff)
    state.update(final="accepted", reason=None)
    jdump(state, state_p)
    return state


def cmd_process(a):
    cands = json.load(open(os.path.join(HERE, "candidates", f"{a.repo}.{a.window}.json")))["candidates"]
    cands = cands[a.start:a.start + a.max_candidates]

    def run(c):
        try:
            s = process_one(a.repo, c, a.window)
            print(json.dumps({"task": s["task_id"], "final": s.get("final"), "reason": s.get("reason"),
                              "f2p": len(s.get("f2p", [])), "kept_f2p": len(s.get("kept_f2p", []))}), flush=True)
        except Exception as e:  # noqa: BLE001 - infrastructure: leave unfinished, retried on the next run
            print(json.dumps({"task": tid(a.repo, c["commit"]), "infra_error": repr(e)[:500]}), flush=True)
    with ThreadPoolExecutor(a.jobs) as ex:
        list(ex.map(run, cands))


def sha_tree(d):
    h = hashlib.sha256()
    for root, _, fs in sorted(os.walk(d)):
        for f in sorted(fs):
            p = os.path.join(root, f)
            h.update(os.path.relpath(p, d).encode() + b"\0" + open(p, "rb").read() + b"\0")
    return h.hexdigest()


def cmd_freeze(a):
    out_dir = os.path.join(HERE, "tasks" if a.window == "main" else f"tasks_{a.window}")
    manifest = {"version": a.version, "created": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                "window": a.window, "per_repo": a.per_repo, "pins": PINS, "tasks": [], "rejections": {},
                "audit_totals": {}, "rules_sha256": sha_tree(os.path.join(HERE, "prompts")) + ":" +
                hashlib.sha256(open(__file__, "rb").read() + open(os.path.join(EXP, "mining", "select.py"), "rb").read()).hexdigest()}
    repos = a.repos.split(",")
    tot = {"f2p_before": 0, "f2p_kept": 0, "p2p_before": 0, "p2p_kept": 0, "edits": 0, "edits_rejected": 0, "tasks_audited": 0}
    for repo_id in repos:
        cands = json.load(open(os.path.join(HERE, "candidates", f"{repo_id}.{a.window}.json")))["candidates"]
        accepted, rej = 0, {}
        for c in cands:
            if accepted >= a.per_repo:
                break
            t = tid(repo_id, c["commit"])
            sp = os.path.join(WORK, t, "state.json")
            if not os.path.exists(sp) or not json.load(open(sp)).get("final"):
                raise SystemExit(f"{t} not processed yet: process candidates in order before freezing")
            s = json.load(open(sp))
            if "kept_f2p" in s:
                tot["tasks_audited"] += 1
                tot["f2p_before"] += len(s["f2p"]); tot["f2p_kept"] += len(s["kept_f2p"])
                tot["p2p_before"] += len(s["p2p"]); tot["p2p_kept"] += len(s["kept_p2p"])
                tot["edits"] += s.get("edits_applied", 0); tot["edits_rejected"] += len(s.get("edits_rejected", []))
            if s["final"] != "accepted":
                rej[s["reason"]] = rej.get(s["reason"], 0) + 1
                continue
            accepted += 1
            dst = os.path.join(out_dir, t)
            shutil.rmtree(dst, ignore_errors=True)
            os.makedirs(dst)
            shutil.copytree(os.path.join(WORK, t, "hidden"), os.path.join(dst, "hidden"))
            shutil.copy(os.path.join(WORK, t, "reference.patch"), dst)
            shutil.copy(os.path.join(WORK, t, "audit.json"), dst)
            open(os.path.join(dst, "instruction.md"), "w").write(PREAMBLE + open(os.path.join(WORK, t, "instruction.md")).read())
            task = {"task_id": t, "repo": repo_id, "commit": c["commit"], "parent": c["parent"], "date": c["date"],
                    "subject": c["subject"], "type": c["type"], "source_files": c["source_files"],
                    "test_files": s["hidden"], "hidden_deleted": s["deleted"], "test_targets": s["targets"],
                    "f2p": s["kept_f2p"], "p2p": s["kept_p2p"],
                    "audit": {"dropped": s["dropped"], "edits_applied": s["edits_applied"],
                              "f2p_before": len(s["f2p"]), "p2p_before": len(s["p2p"]), "flaky_excluded": s["flaky"]}}
            jdump(task, os.path.join(dst, "task.json"))
            manifest["tasks"].append({"task_id": t, "repo": repo_id, "sha256": sha_tree(dst)})
        manifest["rejections"][repo_id] = rej
    manifest["audit_totals"] = tot
    jdump(manifest, os.path.join(HERE, f"manifest.{a.version}.json"))
    print(json.dumps({"tasks": len(manifest["tasks"]), "rejections": manifest["rejections"], "audit": tot}, indent=1))


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("process"); p.add_argument("repo"); p.add_argument("--window", default="main")
    p.add_argument("--start", type=int, default=0); p.add_argument("--max-candidates", type=int, default=20)
    p.add_argument("--jobs", type=int, default=2)
    f = sub.add_parser("freeze"); f.add_argument("--version", required=True); f.add_argument("--per-repo", type=int, default=10)
    f.add_argument("--window", default="main")
    f.add_argument("--repos", default="pydantic-ai,fastmcp,logfire,better-auth,zod,inngest-js")
    a = ap.parse_args()
    {"process": cmd_process, "freeze": cmd_freeze}[a.cmd](a)


if __name__ == "__main__":
    main()
