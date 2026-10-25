"""Export one compact JSON per task (all its trials) for the progress page's trial view:
status/data/<task_id>.json. Only files whose content changed are rewritten.

usage: python3 status/export_trials.py <results_run_dir> <dataset_dir>
"""
import glob
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "data")
TXT, OUTPUT, DIFF_MAX, STEPS_MAX = 700, 900, 40000, 400


def cut(s, n):
    s = s if isinstance(s, str) else json.dumps(s)
    return s if len(s) <= n else s[:n] + f" … [{len(s) - n} more chars]"


def tool_summary(name, inp):
    if name == "Bash":
        return inp.get("command", "")
    if name in ("Read", "Write", "Edit", "MultiEdit", "NotebookEdit"):
        s = inp.get("file_path") or inp.get("notebook_path") or ""
        if name == "Edit":
            s += f"\n- {cut(inp.get('old_string', ''), 300)}\n+ {cut(inp.get('new_string', ''), 300)}"
        if name == "Write":
            s += f"\n{cut(inp.get('content', ''), 400)}"
        return s
    if name in ("Grep", "Glob"):
        return f"{inp.get('pattern', '')}  {inp.get('path', '') or ''}"
    return json.dumps(inp)


def result_text(c):
    v = c.get("content")
    if isinstance(v, list):
        v = "\n".join(x.get("text", "") for x in v if isinstance(x, dict))
    return v or ""


def timeline(path):
    steps, turn, by_id = [], 0, {}
    if not os.path.exists(path):
        return steps
    for line in open(path, errors="replace"):
        try:
            d = json.loads(line)
        except ValueError:
            continue
        t, sub = d.get("type"), d.get("subtype")
        if t == "assistant":
            turn += 1
            for c in d.get("message", {}).get("content", []) or []:
                if c.get("type") == "text" and c.get("text", "").strip():
                    steps.append({"k": "text", "turn": turn, "s": cut(c["text"].strip(), TXT)})
                elif c.get("type") == "tool_use":
                    st = {"k": "tool", "turn": turn, "tool": c.get("name"),
                          "s": cut(tool_summary(c.get("name"), c.get("input") or {}), TXT)}
                    by_id[c.get("id")] = st
                    steps.append(st)
        elif t == "user":
            content = d.get("message", {}).get("content")
            for c in content if isinstance(content, list) else []:
                if c.get("type") == "tool_result" and c.get("tool_use_id") in by_id:
                    st = by_id[c["tool_use_id"]]
                    st["out"] = cut(result_text(c), OUTPUT)
                    st["err"] = bool(c.get("is_error"))
        elif t == "system" and sub == "hook_response":
            out = d.get("output") or ((d.get("stdout") or "") + (d.get("stderr") or ""))
            if "forge" in out or d.get("exit_code") not in (0, None) or '"decision"' in out:
                kind = "block" if ('"decision":"block"' in out.replace(" ", "") or d.get("exit_code") == 2) else "hook"
                steps.append({"k": kind, "turn": turn, "hook": d.get("hook_name"), "exit": d.get("exit_code"),
                              "s": cut(out.strip(), OUTPUT)})
        elif t == "result":
            steps.append({"k": "result", "turn": turn, "s": cut(d.get("result") or "", TXT),
                          "sub": d.get("subtype"), "cost": d.get("total_cost_usd")})
    if len(steps) > STEPS_MAX:
        steps = steps[:STEPS_MAX] + [{"k": "text", "turn": turn, "s": f"… {len(steps) - STEPS_MAX} more steps not shown"}]
    return steps


def trial(d):
    m = json.load(open(os.path.join(d, "meta.json")))
    rec = {k: m.get(k) for k in ("status", "infra_reason", "attempt", "num_turns", "cost_usd", "cost_usd_reviewer",
                                  "duration_s", "timed_out", "result_subtype")}
    rec["wall_s"] = (m["finished"] - m["started"]) if m.get("finished") else None
    op = os.path.join(d, "outcome.json")
    if os.path.exists(op):
        o = json.load(open(op))
        rec.update({k: o.get(k) for k in ("resolved", "patch_applied", "f2p", "p2p", "f2p_frac", "p2p_regressions",
                                         "lint_new", "diff_lines", "changed_files")})
    for name, key in (("edits.json", "edits"), (os.path.join("forge", "hook_summary.json"), "hooks")):
        p = os.path.join(d, name)
        if os.path.exists(p):
            x = json.load(open(p))
            rec[key] = {k: v for k, v in x.items() if k not in ("samples", "shell_edit_commands", "edit_files")}
    dp = os.path.join(d, "final.diff")
    if os.path.exists(dp):
        rec["diff"] = cut(open(dp, errors="replace").read(), DIFF_MAX)
    rec["steps"] = timeline(os.path.join(d, "transcript.jsonl"))
    return rec


def main():
    run_dir, ds = sys.argv[1], sys.argv[2]
    os.makedirs(OUT, exist_ok=True)
    man = json.load(open(os.path.join(ds, "manifest.v1.json")))
    changed = 0
    for t in man["tasks"]:
        tid = t["task_id"]
        tdir = os.path.join(ds, "tasks", tid)
        tj = json.load(open(os.path.join(tdir, "task.json")))
        doc = {"task_id": tid, "repo": tj["repo"], "subject": tj["subject"], "type": tj["type"], "commit": tj["commit"],
               "instruction": open(os.path.join(tdir, "instruction.md")).read(),
               "test_files": tj["test_targets"], "n_f2p": len(tj["f2p"]), "n_p2p": len(tj["p2p"]),
               "audit_dropped": len(tj["audit"]["dropped"]), "trials": {}}
        for md in sorted(glob.glob(os.path.join(run_dir, tid, "*", "r*", "meta.json"))):
            d = os.path.dirname(md)
            doc["trials"][f"{d.split('/')[-2]}/{d.split('/')[-1]}"] = trial(d)
        p = os.path.join(OUT, f"{tid}.json")
        text = json.dumps(doc, separators=(",", ":"))
        if not os.path.exists(p) or open(p).read() != text:
            open(p, "w").write(text)
            changed += 1
    print(json.dumps({"changed": changed, "files": len(man["tasks"])}))


if __name__ == "__main__":
    main()
