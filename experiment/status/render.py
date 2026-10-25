"""Render the live progress page (status/index.html) from status/state.json and,
when a run exists, the live trial status from analysis/status.py.

usage: python3 status/render.py [results_run_dir dataset_dir]
"""
import html
import json
import os
import subprocess
import sys
import datetime

HERE = os.path.dirname(os.path.abspath(__file__))
state = json.load(open(os.path.join(HERE, "state.json")))
state["updated"] = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%d %H:%M UTC")

live = None
if len(sys.argv) >= 3 and os.path.exists(os.path.join(HERE, "..", "analysis", "status.py")):
    try:
        out = subprocess.run([sys.executable, os.path.join(HERE, "..", "analysis", "status.py"), sys.argv[1], sys.argv[2]],
                             capture_output=True, text=True, timeout=120).stdout
        live = json.loads(out)
    except Exception:  # noqa: BLE001 - the page renders without live data
        live = None
summary = None
sp = os.path.join(HERE, "summary.json")
if os.path.exists(sp):
    summary = json.load(open(sp))

e = html.escape
LABEL = {"done": "Done", "running": "In progress", "todo": "Not started", "blocked": "Blocked"}

phases = "\n".join(
    f'<li class="ph {p["state"]}"><span class="pill">{LABEL[p["state"]]}</span>'
    f'<span class="phn">{e(p["name"])}</span><span class="note">{e(p.get("note", ""))}</span></li>'
    for p in state["phases"])
pins = "".join(f"<div><dt>{e(k)}</dt><dd>{e(v)}</dd></div>" for k, v in state["pins"].items())
rows = "".join(
    f'<tr><td>{e(r["repo"])}</td><td>{e(r["lang"])}</td><td class="n">{r["candidates"]}</td>'
    f'<td class="n">{"—" if r.get("accepted") is None else r["accepted"]}</td></tr>'
    for r in state["dataset"]["repos"])
log = "".join(f"<li>{e(x)}</li>" for x in reversed(state["log"][-14:]))

live_html = '<p class="muted">The run has not started.</p>'
if live:
    arms = live.get("arms", {})
    cards = ""
    for arm in ("baseline", "forge"):
        a = arms.get(arm, {})
        rate = a.get("resolved_rate")
        cards += (f'<div class="arm {arm}"><h3>{arm}</h3>'
                  f'<div class="big">{"—" if rate is None else f"{rate*100:.1f}%"}</div>'
                  f'<div class="sub">resolved, {a.get("scored", 0)} scored of {a.get("total", "?")}</div>'
                  f'<div class="sub">infra errors {a.get("infra_errors", 0)} · cost ${a.get("cost_usd", 0):.2f}</div></div>')
    last = "".join(
        f'<tr><td>{e(str(t.get("task")))}</td><td>{e(str(t.get("arm")))}</td><td class="n">{t.get("rep")}</td>'
        f'<td>{e(str(t.get("status")))}</td><td>{"✓" if t.get("resolved") else ("✗" if t.get("resolved") is False else "")}</td>'
        f'<td class="n">{"" if t.get("cost") is None else f"${t["cost"]:.2f}"}</td>'
        f'<td class="n">{"" if t.get("duration_s") is None else f"{t["duration_s"]/60:.1f}m"}</td></tr>'
        for t in live.get("last", []))
    live_html = (f'<div class="arms">{cards}</div><p class="muted">Running now: {live.get("running", 0)}. '
                 f'Interim rates; the analysis uses completed replicates only.</p>'
                 f'<div class="tw"><table><thead><tr><th>Task</th><th>Arm</th><th>Rep</th><th>Status</th><th>Resolved</th>'
                 f'<th>Cost</th><th>Time</th></tr></thead><tbody>{last}</tbody></table></div>')
res_html = ""
if summary:
    res_html = f'<section><h2>Results</h2><div class="prose">{summary.get("html", "")}</div></section>'

page = f"""<title>Forge Trial Ledger</title>
<meta name="claude:viewport-fit-sides" content="cover">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Fraunces:opsz,wght@9..144,500;9..144,650&family=IBM+Plex+Sans:wght@400;500;600&family=IBM+Plex+Mono:wght@400;500&display=swap">
<style>
/* Layout: one lab-ledger column; summary (headline, phases) before detail (tables, log). */
:root {{
  --bg: #f6f7f5; --panel: #ffffff; --fg: #1d2421; --muted: #5d6963; --line: #d9dfdb;
  --accent: #2f6b5a; --base: #4a5a8c; --forge: #b5602b;
  --ok: #2f7d4f; --run: #a57a12; --todo: #8a948f;
  --display: "Fraunces", Georgia, serif; --body: "IBM Plex Sans", system-ui, sans-serif; --mono: "IBM Plex Mono", ui-monospace, monospace;
}}
@media (prefers-color-scheme: dark) {{ :root:not([data-theme="light"]) {{
  --bg: #141917; --panel: #1b2220; --fg: #e3e8e5; --muted: #9aa6a0; --line: #2d3733;
  --accent: #6cbca2; --base: #93a6e6; --forge: #e59a68; --ok: #6cc28f; --run: #e0b54a; --todo: #77827d; color-scheme: dark; }} }}
:root[data-theme="dark"] {{
  --bg: #141917; --panel: #1b2220; --fg: #e3e8e5; --muted: #9aa6a0; --line: #2d3733;
  --accent: #6cbca2; --base: #93a6e6; --forge: #e59a68; --ok: #6cc28f; --run: #e0b54a; --todo: #77827d; color-scheme: dark; }}
body {{ background: var(--bg); color: var(--fg); font: 15px/1.55 var(--body); }}
.wrap {{ max-width: 980px; margin: 0 auto; padding-block: 28px 56px;
  padding-left: max(16px, env(safe-area-inset-left, 0px)); padding-right: max(16px, env(safe-area-inset-right, 0px));
  display: grid; gap: 28px; }}
header {{ display: grid; gap: 6px; }}
.eyebrow {{ font: 500 12px var(--mono); letter-spacing: .08em; text-transform: uppercase; color: var(--accent); }}
h1 {{ font: 650 clamp(28px, 5vw, 40px)/1.1 var(--display); margin: 0; text-wrap: balance; }}
h2 {{ font: 600 20px/1.2 var(--display); margin: 0 0 12px; }}
h3 {{ font: 600 13px var(--mono); text-transform: uppercase; letter-spacing: .06em; margin: 0; }}
.lede {{ font-size: 17px; max-width: 65ch; margin: 0; }}
.muted, .note {{ color: var(--muted); }}
.stamp {{ font: 12px var(--mono); color: var(--muted); }}
section {{ min-width: 0; }}
.grid2 {{ display: grid; grid-template-columns: minmax(0, 1.4fr) minmax(0, 1fr); gap: 28px; }}
@media (max-width: 760px) {{ .grid2 {{ grid-template-columns: 1fr; }} }}
ol.phases {{ list-style: none; margin: 0; padding: 0; display: grid; gap: 2px; }}
.ph {{ display: grid; grid-template-columns: 104px minmax(0, 1fr); column-gap: 12px; padding: 8px 0; border-bottom: 1px solid var(--line); }}
.ph .note {{ grid-column: 2; font-size: 13px; }}
.pill {{ grid-row: span 2; align-self: start; font: 500 11px var(--mono); text-transform: uppercase; letter-spacing: .05em;
  padding: 3px 7px; border-radius: 3px; border: 1px solid currentColor; text-align: center; }}
.done .pill {{ color: var(--ok); }} .running .pill {{ color: var(--run); }} .todo .pill {{ color: var(--todo); }} .blocked .pill {{ color: var(--forge); }}
.todo .phn {{ color: var(--muted); }}
dl {{ margin: 0; display: grid; gap: 8px; }}
dl div {{ display: grid; grid-template-columns: 130px minmax(0, 1fr); gap: 8px; }}
dt {{ color: var(--muted); font-size: 13px; }} dd {{ margin: 0; font: 13px var(--mono); overflow-wrap: anywhere; }}
.tw {{ overflow-x: auto; }}
table {{ border-collapse: collapse; width: 100%; font-size: 14px; }}
th, td {{ text-align: left; padding: 6px 10px 6px 0; border-bottom: 1px solid var(--line); }}
th {{ font: 500 12px var(--mono); color: var(--muted); text-transform: uppercase; letter-spacing: .05em; }}
td.n, th.n {{ text-align: right; font-family: var(--mono); font-variant-numeric: tabular-nums; }}
.arms {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 12px; margin-bottom: 10px; }}
.arm {{ background: var(--panel); border: 1px solid var(--line); border-top: 3px solid var(--base); padding: 14px 16px; display: grid; gap: 2px; }}
.arm.forge {{ border-top-color: var(--forge); }}
.big {{ font: 600 34px var(--display); font-variant-numeric: tabular-nums; }}
.sub {{ font-size: 13px; color: var(--muted); }}
ul.log {{ margin: 0; padding-left: 18px; display: grid; gap: 4px; font-size: 14px; }}
.prose {{ max-width: 75ch; }}
</style>
<div class="wrap">
<header>
  <div class="eyebrow">Controlled experiment · baseline vs Forge</div>
  <h1>Does Forge make Claude Code better at real tasks?</h1>
  <p class="lede">{e(state["headline"])}</p>
  <div class="stamp">Updated {state["updated"]}</div>
</header>
<div class="grid2">
  <section><h2>Progress</h2><ol class="phases">{phases}</ol></section>
  <section><h2>Pinned setup</h2><dl>{pins}</dl>
    <h2 style="margin-top:24px">Dataset</h2>
    <div class="tw"><table><thead><tr><th>Repo</th><th>Lang</th><th class="n">Candidates</th><th class="n">Accepted</th></tr></thead><tbody>{rows}</tbody></table></div>
  </section>
</div>
<section><h2>Trials</h2>{live_html}</section>
{res_html}
<section><h2>Log</h2><ul class="log">{log}</ul></section>
</div>
"""
open(os.path.join(HERE, "index.html"), "w").write(page)
print("rendered", os.path.join(HERE, "index.html"))
