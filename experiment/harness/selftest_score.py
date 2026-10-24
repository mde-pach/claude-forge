"""Scorer self-test: the reference patch must be resolved, an empty change must not (per task)."""
import json, os, sys, tempfile
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import run_trial as rt
for td in sys.argv[1:]:
    task = json.load(open(os.path.join(td, "task.json")))
    ref = open(os.path.join(td, "reference.patch")).read()
    out = tempfile.mkdtemp()
    a = rt.score(task, td, ref, out, "selftest-ref-" + task["task_id"][:40])
    b = rt.score(task, td, "", out, "selftest-empty-" + task["task_id"][:40])
    print(json.dumps({"task": task["task_id"], "ref_resolved": a["resolved"], "ref_f2p": a["f2p_frac"],
                      "ref_p2p_regr": a["p2p_regressions"], "empty_resolved": b["resolved"], "empty_f2p": b["f2p_frac"]}))
