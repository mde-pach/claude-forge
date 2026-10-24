"""Prepare the same task N times (no agent) and diff the pre-agent fingerprints: the parity check
is only meaningful if preparation is deterministic."""
import json, os, shutil, subprocess, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import repo_env, run_trial as rt
td, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 3
task = json.load(open(os.path.join(td, "task.json")))
repo = repo_env.load_repos()[task["repo"]]
fps = []
for i in range(n):
    name = f"det-{task['task_id'][:30]}-{i}"
    logs = f"/work/runs/{name}/logs"; os.makedirs(logs, exist_ok=True); os.chmod(logs, 0o777)
    try:
        rt.prepare(name, task, repo, "baseline", logs)
        fps.append(rt.fingerprint(name))
    finally:
        subprocess.run(["docker", "rm", "-f", name], capture_output=True); shutil.rmtree(f"/work/runs/{name}", ignore_errors=True)
base = fps[0]
for i, fp in enumerate(fps[1:], 1):
    diff = sorted(k for k in set(base) | set(fp) if base.get(k) != fp.get(k))
    print(json.dumps({"task": task["task_id"], "run": i, "n_diff": len(diff), "paths": diff[:20]}))
