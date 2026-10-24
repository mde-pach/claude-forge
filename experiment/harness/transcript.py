"""Parse a Claude Code stream-json transcript (with --include-hook-events) into trial metrics:
turns, cost, result, how files were edited, and what hooks did (Forge's are recognised by
their `forge:` message prefix and their script names)."""
import json
import re

EDIT_TOOLS = {"Edit", "MultiEdit", "Write", "NotebookEdit"}
# Shell commands that write files (heuristic, applied identically to both arms).
SHELL_WRITE = re.compile(
    r"\bsed\s+(-[a-zA-Z]*\s+)*-i|\bperl\s+-[a-zA-Z]*i|\btee\b|(^|[^0-9&>])>{1,2}\s*(?!/dev/null|&)[\"']?[\w./~$-]"
    r"|<<-?\s*['\"]?\w+|\bgit\s+(apply|checkout\s+--|restore)\b|\bpatch\s+-|\bopen\([^)]*,\s*['\"][wa]|\.write_text\("
    r"|writeFileSync|\bcp\s|\bmv\s|\btruncate\b|\bdd\s")
FORGE_SCRIPT = re.compile(r"forge|guard\.sh|post-edit\.sh|stop\.sh|turn-start\.sh|session-start\.sh|typecheck\.sh|bash-snapshot\.sh|review\.sh|notify\.sh")


def parse(path):
    info = {"has_result": False, "num_turns": 0, "cost_usd_agent": None, "result_subtype": None, "is_error": None,
            "result_text": None, "terminal_reason": None, "last_text": "", "api_errors": 0, "rate_limit": None,
            "tool_calls": {}}
    edits = {"edit_tool_calls": 0, "shell_edit_calls": 0, "bash_calls": 0, "edit_files": [], "shell_edit_commands": []}
    hooks = {"active": False, "events": {}, "forge_messages": 0, "blocks": 0, "stop_blocks": 0, "feedback": 0,
             "nonzero_exit": 0, "by_hook": {}, "samples": []}
    try:
        lines = open(path, errors="replace").read().splitlines()
    except OSError:
        lines = []
    for line in lines:
        try:
            d = json.loads(line)
        except ValueError:
            continue
        t, st = d.get("type"), d.get("subtype")
        if t == "assistant":
            for c in d.get("message", {}).get("content", []) or []:
                if c.get("type") == "text":
                    info["last_text"] = c.get("text", "")[-2000:]
                if c.get("type") != "tool_use":
                    continue
                name = c.get("name")
                info["tool_calls"][name] = info["tool_calls"].get(name, 0) + 1
                inp = c.get("input") or {}
                if name in EDIT_TOOLS:
                    edits["edit_tool_calls"] += 1
                    fp = inp.get("file_path") or inp.get("notebook_path")
                    if fp and fp not in edits["edit_files"]:
                        edits["edit_files"].append(fp)
                elif name == "Bash":
                    edits["bash_calls"] += 1
                    cmd = inp.get("command", "")
                    if SHELL_WRITE.search(cmd):
                        edits["shell_edit_calls"] += 1
                        if len(edits["shell_edit_commands"]) < 50:
                            edits["shell_edit_commands"].append(cmd[:300])
            if d.get("error") or (d.get("message", {}).get("model") == "<synthetic>"):
                info["api_errors"] += 1
        elif t == "system" and st in ("hook_started", "hook_response"):
            ev = d.get("hook_event") or "?"
            hn = d.get("hook_name") or ev
            if st == "hook_started":
                hooks["events"][ev] = hooks["events"].get(ev, 0) + 1
                continue
            out = d.get("output") or ((d.get("stdout") or "") + (d.get("stderr") or ""))
            rec = hooks["by_hook"].setdefault(hn, {"runs": 0, "nonzero": 0, "forge_msgs": 0})
            rec["runs"] += 1
            is_forge = "forge:" in out or bool(FORGE_SCRIPT.search(d.get("command", "") or ""))
            if "forge" in out.lower() or is_forge:
                hooks["active"] = True
            n_forge = out.count("forge:")
            hooks["forge_messages"] += n_forge
            rec["forge_msgs"] += n_forge
            code = d.get("exit_code")
            blocked = code == 2 or '"decision":"block"' in out.replace(" ", "") or '"permissionDecision":"deny"' in out.replace(" ", "")
            if code not in (0, None):
                hooks["nonzero_exit"] += 1
                rec["nonzero"] += 1
            if blocked:
                hooks["blocks"] += 1
                if ev == "Stop":
                    hooks["stop_blocks"] += 1
            if n_forge and not blocked:
                hooks["feedback"] += 1
            if (n_forge or blocked) and len(hooks["samples"]) < 40:
                hooks["samples"].append({"hook": hn, "exit": code, "out": out[:600]})
        elif t == "rate_limit_event":
            info["rate_limit"] = d.get("rate_limit_info")
        elif t == "result":
            info.update(has_result=True, num_turns=d.get("num_turns") or 0, cost_usd_agent=d.get("total_cost_usd"),
                        result_subtype=d.get("subtype"), is_error=d.get("is_error"),
                        result_text=(d.get("result") or "")[:2000] if isinstance(d.get("result"), str) else None,
                        terminal_reason=d.get("terminal_reason"),
                        api_error_text=str(d.get("api_error_status") or ""))
    edits["total"] = edits["edit_tool_calls"] + edits["shell_edit_calls"]
    edits["edit_tool_share"] = (edits["edit_tool_calls"] / edits["total"]) if edits["total"] else None
    info["edits"] = edits
    info["hooks"] = hooks
    return info


if __name__ == "__main__":
    import sys
    r = parse(sys.argv[1])
    print(json.dumps(r, indent=1)[:5000])
