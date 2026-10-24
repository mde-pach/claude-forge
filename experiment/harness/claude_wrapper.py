#!/usr/bin/env python3
"""Installed as /usr/local/bin/claude in every trial container (both arms, identical).
Logs every invocation of Claude Code to /logs/claude_calls.jsonl and, for calls with
`--output-format json` (Forge's ask-reviewer uses it), captures the result object so its
cost is counted. Everything else is passed through untouched."""
import json
import os
import subprocess
import sys
import time

REAL = "/opt/claude-code/bin/claude"
LOG = "/logs/claude_calls.jsonl"


def log(rec):
    try:
        with open(LOG, "a") as f:
            f.write(json.dumps(rec) + "\n")
    except OSError:
        pass


argv = sys.argv[1:]
rec = {"ts": time.time(), "pid": os.getpid(), "argv": argv[:64],
       "forge_reviewer": os.environ.get("FORGE_REVIEWER") == "1", "cwd": os.getcwd()}
json_out = "--output-format" in argv and argv[argv.index("--output-format") + 1:argv.index("--output-format") + 2] == ["json"]
if not json_out:
    log(rec)
    os.execv(REAL, [REAL] + argv)
t0 = time.time()
p = subprocess.run([REAL] + argv, stdin=sys.stdin, stdout=subprocess.PIPE)
rec["duration_s"] = time.time() - t0
rec["exit"] = p.returncode
try:
    res = json.loads(p.stdout.decode() or "null")
    if isinstance(res, dict):
        rec["result"] = {k: res.get(k) for k in ("total_cost_usd", "num_turns", "is_error", "subtype",
                                                 "duration_ms", "structured_output", "result", "usage")}
except ValueError:
    rec["stdout_head"] = p.stdout[:2000].decode("utf8", "replace")
log(rec)
sys.stdout.buffer.write(p.stdout)
sys.exit(p.returncode)
