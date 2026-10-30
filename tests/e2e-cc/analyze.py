"""Checks the transcripts run.sh collected: analyze.py <out-dir> <scenario>...

Reads <scenario>.jsonl (claude -p stream-json with --include-hook-events), <scenario>.debug
(Claude Code's debug log: hook durations), <scenario>.diff/.log (the repo after the session) and
rules.md (the rules files the agent reads). Prints one PASS/FAIL line per check with its evidence,
a hook latency table, and exits 1 when a check fails. Standard library only.
"""

from __future__ import annotations

import json
import re
import statistics
import sys
from dataclasses import dataclass, field
from pathlib import Path

# forge_core::internals, plus the cache and state roots (crates/forge-home).
INTERNALS = re.compile(
    r"(?:^|[^\w.-])(\.forge)(?:$|[^\w-])"
    r"|\b(check\.sh\b|forge\.yaml\b|forge\.lock\b|forge (?:apply|diff|check|status|hook)\b"
    r"|claude-forge\b|FORGE_[A-Z0-9_]+|forge-state\b|models\.tsv\b|accept:)"
    r"|(\.cache/forge\b|\.local/state/forge\b)"
)
# A tool call into Forge: its cache/state, its commands, its config and the files it generates.
INTERNAL_CALL = re.compile(
    r"\.cache/forge\b|\.local/state/forge\b|\bclaude-forge\b|(?:^|[;&|(\s])forge\s+\w"
    r"|\bforge\.(?:lock|yaml)\b|\.claude/settings(?:\.local)?\.json|\.git/hooks/|(?:^|/)\.forge(?:/|\b)"
)
INTERNAL_REASON = "Internal files of Forge, this project's quality tooling"
# Scenarios that ask the agent to touch internals on purpose (excluded from the zero-calls count).
PROMPTED_INTERNALS = {"internals"}


@dataclass
class Hook:
    event: str
    name: str
    output: dict
    raw: str
    outcome: str
    tool: "Tool | None"


@dataclass
class Tool:
    id: str
    name: str
    input: dict
    result: str = ""
    is_error: bool = False
    hooks: list[Hook] = field(default_factory=list)

    def text(self) -> str:
        return self.input.get("command") or json.dumps(self.input)


@dataclass
class Session:
    name: str
    hooks: list[Hook] = field(default_factory=list)
    tools: list[Tool] = field(default_factory=list)
    texts: list[str] = field(default_factory=list)
    result: dict = field(default_factory=dict)

    def decisions(self, tool: Tool) -> list[tuple[str, str]]:
        out = []
        for h in tool.hooks:
            hso = h.output.get("hookSpecificOutput") or {}
            if hso.get("permissionDecision"):
                out.append((hso["permissionDecision"], hso.get("permissionDecisionReason", "")))
        return out

    def agent_texts(self) -> list[tuple[str, str]]:
        """What Forge put in front of the agent: hook context, reasons, and tool output."""
        out = []
        for h in self.hooks:
            o, hso = h.output, h.output.get("hookSpecificOutput") or {}
            for k in ("additionalContext", "permissionDecisionReason"):
                if hso.get(k):
                    out.append((f"{h.event} {k}", hso[k]))
            if o.get("decision") == "block" and o.get("reason"):
                out.append((f"{h.event} block reason", o["reason"]))
            if h.raw and not h.output and h.outcome != "success":
                out.append((f"{h.event} stderr", h.raw))
        for t in self.tools:
            if t.result:
                out.append((f"{t.name} result", t.result))
        return out


def load(d: Path, name: str) -> Session:
    s = Session(name)
    by_id: dict[str, Tool] = {}
    last: Tool | None = None
    started: dict[str, Tool | None] = {}
    for line in (d / f"{name}.jsonl").read_text().splitlines():
        try:
            e = json.loads(line)
        except ValueError:
            continue
        typ = e.get("type")
        if typ == "system" and e.get("subtype") == "hook_started":
            # a tool hook belongs to the latest call of its tool (calls may run in parallel)
            ev, _, tname = e.get("hook_name", "").partition(":")
            if ev in ("PreToolUse", "PostToolUse", "PostToolUseFailure"):
                started[e.get("hook_id")] = next((t for t in reversed(s.tools) if t.name == tname), last)
        elif typ == "system" and e.get("subtype") == "hook_response":
            raw = (e.get("stdout") or e.get("output") or "").strip()
            try:
                out = json.loads(raw) if raw.startswith("{") else {}
            except ValueError:
                out = {}
            ev = e.get("hook_event", "")
            tool = started.get(e.get("hook_id")) if ev in ("PreToolUse", "PostToolUse", "PostToolUseFailure") else None
            h = Hook(ev, e.get("hook_name", ""), out, raw or (e.get("stderr") or ""), e.get("outcome", ""), tool)
            s.hooks.append(h)
            if tool:
                tool.hooks.append(h)
        elif typ == "assistant":
            for c in e["message"].get("content", []):
                if c.get("type") == "tool_use":
                    last = Tool(c["id"], c["name"], c.get("input") or {})
                    by_id[last.id] = last
                    s.tools.append(last)
                elif c.get("type") == "text":
                    s.texts.append(c["text"])
        elif typ == "user":
            content = e["message"].get("content")
            for c in content if isinstance(content, list) else []:
                if c.get("type") == "tool_result" and c.get("tool_use_id") in by_id:
                    t = by_id[c["tool_use_id"]]
                    r = c.get("content")
                    if isinstance(r, list):
                        r = "\n".join(x.get("text", "") for x in r if isinstance(x, dict))
                    t.result, t.is_error = r or "", bool(c.get("is_error"))
        elif typ == "result":
            s.result = e
    return s


failures: list[str] = []


def check(ok: bool, what: str, evidence: str = "") -> bool:
    print(f"  {'PASS' if ok else 'FAIL'} {what}" + (f"\n       {evidence}" if evidence else ""))
    if not ok:
        failures.append(what)
    return ok


def short(t: str, n: int = 160) -> str:
    t = " ".join(t.split())
    return t if len(t) <= n else t[: n - 1] + "…"


def find_tool(s: Session, pred) -> Tool | None:
    return next((t for t in s.tools if pred(t)), None)


def scenario_hooks(s: Session, d: Path) -> None:
    fired = {h.event for h in s.hooks if h.outcome == "success"}
    for ev in ("SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "Stop"):
        check(ev in fired, f"hook {ev} fired", f"{sum(h.event == ev for h in s.hooks)} response(s)")
    ctx = next((h for h in s.hooks if h.event == "SessionStart"), None)
    c = ctx and (ctx.output.get("hookSpecificOutput") or {}).get("additionalContext", "")
    check(bool(c), "SessionStart gives the agent context", short(c or "(none)"))
    errs = [h for h in s.hooks if h.outcome not in ("success", "")]
    check(not errs, "no hook failed", "; ".join(f"{h.name}: {short(h.raw)}" for h in errs))
    commit = find_tool(s, lambda t: t.name == "Bash" and "chmod 777" in t.text())
    if check(commit is not None, "the agent ran the commit naming chmod 777"):
        dec = s.decisions(commit)
        check(not dec, "commit -m 'remove chmod 777 from docs' is not denied or asked",
              f"decisions={dec} result={short(commit.result, 100)}")
    log = (d / f"{s.name}.log").read_text() if (d / f"{s.name}.log").exists() else ""
    check("remove chmod 777 from docs" in log, "the commit landed", short(log.splitlines()[0] if log else ""))


def scenario_pip(s: Session, d: Path) -> None:
    dry = find_tool(s, lambda t: t.name == "Bash" and "--dry-run" in t.text())
    if check(dry is not None, "the agent ran pip install --dry-run"):
        check(not s.decisions(dry), "pip install --dry-run -r requirements.txt is neither denied nor asked",
              f"decisions={s.decisions(dry)}")
    add = find_tool(s, lambda t: t.name == "Bash" and re.search(r"pip3? install\s+requests", t.text()))
    if check(add is not None, "the agent ran pip install requests"):
        dec = s.decisions(add)
        deny = next((r for k, r in dec if k == "deny"), "")
        check(bool(deny) and "uv add" in deny, "pip install requests is denied with the uv way",
              f"reason: {short(deny)}")
        check(bool(deny) and deny.split(":")[0][:40] in add.result,
              "the deny reason reached the agent", f"tool result: {short(add.result)}")
    alt = [t.text() for t in s.tools if t.name == "Bash" and "pip" in t.text() and t not in (dry, add)]
    print(f"  info: other pip calls: {alt or 'none'}")


def scenario_internals(s: Session, d: Path) -> None:
    for label, pred in (
        ("Bash cat of the cache", lambda t: t.name == "Bash" and ".cache/forge" in t.text()),
        ("Read of the state", lambda t: t.name == "Read" and ".local/state/forge" in t.text()),
    ):
        t = find_tool(s, pred)
        if check(t is not None, f"the agent tried the {label}"):
            dec = s.decisions(t)
            deny = next((r for k, r in dec if k == "deny"), "")
            check(deny.startswith(INTERNAL_REASON), f"{label} is denied with the neutral reason",
                  f"reason: {short(deny)}")
            check(INTERNALS.search(deny) is None, f"{label}: the reason names no internals")
            check(INTERNAL_REASON in t.result, f"{label}: the reason reached the agent", short(t.result))


def scenario_stopgate(s: Session, d: Path) -> None:
    stops = [h for h in s.hooks if h.event == "Stop"]
    blocks = [h for h in stops if h.output.get("decision") == "block"]
    post = [h for h in s.hooks if h.event == "PostToolUse"
            and ((h.output.get("hookSpecificOutput") or {}).get("additionalContext") or h.output.get("reason"))]
    feedback = [short(h.output.get("reason", ""), 200) for h in blocks] + [
        short(h.output.get("reason") or h.output["hookSpecificOutput"]["additionalContext"], 200) for h in post]
    check(bool(blocks) or bool(post), "Forge reported the stub (post-edit or Stop)", f"{len(stops)} Stop run(s); {len(blocks)} blocked; "
          f"post-edit findings: {len(post)}")
    for f in feedback:
        print(f"       forge said: {f}")
    check(bool(stops) and stops[-1].output.get("decision") != "block", "the last Stop passed")
    diff = (d / f"{s.name}.diff").read_text() if (d / f"{s.name}.diff").exists() else ""
    added = "\n".join(l for l in diff.splitlines() if l.startswith("+"))
    check("def fuel_required" in added and "NotImplementedError" not in added,
          "the agent replaced the stub with an implementation",
          short(" | ".join(l for l in added.splitlines() if "fuel_required" in l or "return" in l), 200))


def scenario_realistic(s: Session, d: Path) -> None:
    print(f"  info: {len(s.tools)} tool calls, {s.result.get('num_turns')} turns, "
          f"{s.result.get('duration_ms', 0) / 1000:.0f}s, ${s.result.get('total_cost_usd', 0):.2f}")
    for label, text in s.agent_texts():
        if not label.endswith("result"):
            print(f"       forge said ({label}): {short(text, 220)}")
    check(s.result.get("subtype") == "success", "the session finished", s.result.get("subtype", ""))


SCENARIOS = {
    "hooks": scenario_hooks,
    "pip": scenario_pip,
    "internals": scenario_internals,
    "stopgate": scenario_stopgate,
    "realistic": scenario_realistic,
}

HOOK_LINE = re.compile(r"Hooks: (\w+)(?::\S*)? \[.*?hook ([\w-]+)'?\] finished with status (\d+) \((\d+)ms\)")


def latencies(d: Path, names: list[str]) -> dict[str, list[int]]:
    out: dict[str, list[int]] = {}
    for n in names:
        p = d / f"{n}.debug"
        if p.exists():
            for m in HOOK_LINE.finditer(p.read_text(errors="replace")):
                out.setdefault(m.group(2), []).append(int(m.group(4)))
    return out


def main() -> int:
    d = Path(sys.argv[1])
    names = sys.argv[2:]
    sessions = {}
    for n in names:
        print(f"[{n}]")
        if not (d / f"{n}.jsonl").exists():
            check(False, f"{n}: transcript exists")
            continue
        s = sessions[n] = load(d, n)
        r = s.result
        print(f"  info: {len(s.tools)} tool calls, result: {short(r.get('result') or r.get('subtype') or '', 120)}")
        SCENARIOS[n](s, d)

    print("[all transcripts]")
    calls = [(n, t.name, short(t.text(), 140)) for n, s in sessions.items() if n not in PROMPTED_INTERNALS
             for t in s.tools if INTERNAL_CALL.search(json.dumps(t.input))]
    check(not calls, "zero tool calls into Forge internals (unprompted sessions)",
          "; ".join(f"{n}: {k} {c}" for n, k, c in calls))
    named = []
    for n, s in sessions.items():
        for label, text in s.agent_texts():
            # tool output is the project's and the commands' (an `ls` lists forge.lock); what
            # Forge prints into it (git hook output) is checked as notes below
            if label.endswith("result"):
                continue
            for m in INTERNALS.finditer(text):
                named.append(f"{n}/{label}: {m.group(0).strip()!r} in {short(text, 120)!r}")
    rules = d / "rules.md"
    if rules.exists():
        for m in INTERNALS.finditer(rules.read_text()):
            named.append(f"rules files: {m.group(0).strip()!r}")
    check(not named, "no hook output or rules file names internals", "\n       ".join(named))
    person = [f"{n}/{label}: {short(text, 160)!r}" for n, s in sessions.items() for label, text in s.agent_texts()
              if label.endswith("result") and re.search(r"(?m)^Notes:|\b[a-z-]+/[a-z0-9-]+: (?:skipped|ok|unsatisfied)", text)]
    check(not person, "no person-facing notes or rule ids in what the agent sees", "\n       ".join(person))

    print("[hook latency, ms, from the debug log; wall time incl. sh and process start]")
    for hook, v in sorted(latencies(d, names).items()):
        print(f"  {hook:28} n={len(v):3}  median={statistics.median(v):6.0f}  max={max(v):6}")
    print(f"{len(failures)} check(s) failed" if failures else "all checks passed")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
