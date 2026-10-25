"""Render the live progress page (status/index.html) from status/state.json and the raw results.

usage: python3 status/render.py [results_run_dir dataset_dir]
"""
import datetime
import glob
import html
import json
import os
import statistics as st
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
state = json.load(open(os.path.join(HERE, "state.json")))
state["updated"] = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%d %H:%M UTC")
e = html.escape
ARMS = ("baseline", "forge")
REPOS = ["pydantic-ai", "fastmcp", "logfire", "better-auth", "zod", "inngest-js"]
REPS = 3
RUN_LABEL = os.environ.get("RUN_LABEL", "Results so far")


def load(run_dir, ds_dir):
    """Every task in the frozen manifest, with every trial's status/outcome/cost/time/hooks."""
    man = json.load(open(os.path.join(ds_dir, "manifest.v1.json")))
    tasks = {}
    for t in man["tasks"]:
        tj = json.load(open(os.path.join(ds_dir, "tasks", t["task_id"], "task.json")))
        tasks[t["task_id"]] = {"repo": tj["repo"], "subject": tj["subject"], "type": tj["type"], "trials": {}}
    for p in glob.glob(os.path.join(run_dir, "*", "*", "r*", "meta.json")):
        m = json.load(open(p))
        d = os.path.dirname(p)
        if m["task_id"] not in tasks:
            continue
        rec = {"status": m.get("status"), "cost": m.get("cost_usd"), "agent_s": m.get("duration_s"),
               "wall_s": (m["finished"] - m["started"]) if m.get("finished") else None, "turns": m.get("num_turns")}
        if os.path.exists(os.path.join(d, "outcome.json")):
            o = json.load(open(os.path.join(d, "outcome.json")))
            rec.update(resolved=o.get("resolved"), f2p=o.get("f2p_frac"))
        hp = os.path.join(d, "forge", "hook_summary.json")
        if os.path.exists(hp):
            h = json.load(open(hp))
            rec.update(stop_blocks=h.get("stop_blocks", 0), feedback=h.get("feedback", 0), blocks=h.get("blocks", 0))
        tasks[m["task_id"]]["trials"][(m["arm"], m["rep"])] = rec
    # running = directory exists without meta.json
    for d in glob.glob(os.path.join(run_dir, "*", "*", "r*")):
        parts = d.rstrip("/").split("/")
        tid, arm, rep = parts[-3], parts[-2], parts[-1]
        if tid in tasks and not os.path.exists(os.path.join(d, "meta.json")):
            tasks[tid]["trials"].setdefault((arm, int(rep[1:])), {"status": "running"})
    return tasks


def scored(tasks, arm, repo=None):
    return [r for t in tasks.values() if repo in (None, t["repo"])
            for (a, _), r in t["trials"].items() if a == arm and r.get("status") == "scored"]


def med(xs):
    xs = [x for x in xs if x is not None]
    return st.median(xs) if xs else None


def pct(x):
    return "–" if x is None else f"{x * 100:.0f}%"


def bar_path(x, y, w, h, r=4):
    """Bar anchored at the baseline (left), 4px rounded data end."""
    if w <= 0:
        return f"M{x},{y}h0v{h}h0z"
    r = min(r, w, h / 2)
    return f"M{x},{y}h{w - r:.1f}a{r},{r} 0 0 1 {r},{r}v{h - 2 * r}a{r},{r} 0 0 1 -{r},{r}h-{w - r:.1f}z"


def chart_rates(tasks):
    """Horizontal grouped bars: resolved rate per repo (and overall) per arm, direct-labelled."""
    rows = [("All repositories", None)] + [(r, r) for r in REPOS]
    W, label_w, bar_h, gap, group_gap = 640, 132, 14, 2, 18
    plot_w = W - label_w - 70
    H = 30 + len(rows) * (2 * bar_h + gap + group_gap)
    out = [f'<svg class="chart" viewBox="0 0 {W} {H}" role="img" aria-label="Resolved rate by repository and arm">']
    for i in range(0, 101, 25):
        x = label_w + plot_w * i / 100
        out.append(f'<line x1="{x:.1f}" y1="18" x2="{x:.1f}" y2="{H - 6}" class="grid"/>'
                   f'<text x="{x:.1f}" y="12" class="axis" text-anchor="middle">{i}%</text>')
    y = 26
    for name, repo in rows:
        out.append(f'<text x="{label_w - 10}" y="{y + bar_h + 4}" class="rowlab{" strong" if repo is None else ""}" '
                   f'text-anchor="end">{e(name)}</text>')
        for k, arm in enumerate(ARMS):
            rs = scored(tasks, arm, repo)
            n, ok = len(rs), sum(1 for r in rs if r.get("resolved"))
            rate = ok / n if n else 0
            by = y + k * (bar_h + gap)
            w = max(plot_w * rate, 0)
            tip = f"{name} · {arm}: {ok} of {n} scored trials resolved ({pct(rate if n else None)})"
            out.append(f'<g class="hit" data-tip="{e(tip)}"><rect x="{label_w}" y="{by}" width="{plot_w}" height="{bar_h}" class="hitbox"/>'
                       f'<path d="{bar_path(label_w, by, w, bar_h)}" class="m-{arm}"/></g>'
                       f'<text x="{label_w + w + 6:.1f}" y="{by + bar_h - 3}" class="val">{pct(rate) if n else "–"}'
                       f'<tspan class="muted"> {ok}/{n}</tspan></text>')
        y += 2 * bar_h + gap + group_gap
    out.append("</svg>")
    return "".join(out)


def chart_time(tasks):
    """Per repo and arm: median wall time per trial split into agent time and setup+scoring."""
    W, label_w, bar_h, gap, group_gap = 640, 132, 14, 2, 18
    plot_w = W - label_w - 110
    data = []
    for r in REPOS:
        for arm in ARMS:
            rs = [x for x in scored(tasks, arm, r) if x.get("wall_s") and x.get("agent_s") is not None]
            a, w = med([x["agent_s"] / 60 for x in rs]), med([x["wall_s"] / 60 for x in rs])
            data.append((r, arm, a, w, len(rs)))
    vmax = max([d[3] or 0 for d in data] + [1])
    step = 10 if vmax > 20 else 5
    top = (int(vmax / step) + 1) * step
    H = 30 + len(REPOS) * (2 * bar_h + gap + group_gap)
    out = [f'<svg class="chart" viewBox="0 0 {W} {H}" role="img" aria-label="Median minutes per trial">']
    for i in range(0, top + 1, step):
        x = label_w + plot_w * i / top
        out.append(f'<line x1="{x:.1f}" y1="18" x2="{x:.1f}" y2="{H - 6}" class="grid"/>'
                   f'<text x="{x:.1f}" y="12" class="axis" text-anchor="middle">{i} min</text>')
    y = 26
    for idx, r in enumerate(REPOS):
        out.append(f'<text x="{label_w - 10}" y="{y + bar_h + 4}" class="rowlab" text-anchor="end">{e(r)}</text>')
        for k, arm in enumerate(ARMS):
            _, _, a, w, n = data[2 * idx + k]
            by = y + k * (bar_h + gap)
            if not n:
                out.append(f'<text x="{label_w + 4}" y="{by + bar_h - 3}" class="val muted">no scored trials yet</text>')
                continue
            aw, ww = plot_w * a / top, plot_w * w / top
            tip = (f"{r} · {arm}: median {w:.1f} min per trial, of which the agent {a:.1f} min; "
                   f"setup and scoring {w - a:.1f} min ({n} trials)")
            out.append(f'<g class="hit" data-tip="{e(tip)}"><rect x="{label_w}" y="{by}" width="{plot_w}" height="{bar_h}" class="hitbox"/>'
                       f'<rect x="{label_w}" y="{by}" width="{max(aw - 1, 0):.1f}" height="{bar_h}" class="m-{arm}"/>'
                       f'<path d="{bar_path(label_w + aw + 1, by, max(ww - aw - 1, 0), bar_h)}" class="m-setup"/></g>'
                       f'<text x="{label_w + ww + 6:.1f}" y="{by + bar_h - 3}" class="val">{w:.0f} min'
                       f'<tspan class="muted"> · agent {a:.0f}</tspan></text>')
        y += 2 * bar_h + gap + group_gap
    out.append("</svg>")
    return "".join(out)


def grid(tasks):
    """Every trial as a cell: rows = tasks grouped by repo, columns = arm × replicate."""
    out = ['<div class="tw"><table class="grid"><thead><tr><th class="tl">Task</th>'
           '<th colspan="3" class="armh"><span class="key k-baseline"></span>Baseline</th>'
           '<th colspan="3" class="armh"><span class="key k-forge"></span>Forge</th></tr>'
           '<tr><th></th>' + "".join(f'<th class="rep">r{k}</th>' for _ in ARMS for k in range(1, REPS + 1)) +
           '</tr></thead><tbody>']
    for repo in REPOS:
        ts = sorted((tid, t) for tid, t in tasks.items() if t["repo"] == repo)
        out.append(f'<tr class="grp"><td colspan="7">{e(repo)}</td></tr>')
        for tid, t in ts:
            cells = []
            for arm in ARMS:
                for k in range(1, REPS + 1):
                    r = t["trials"].get((arm, k), {})
                    s = r.get("status")
                    if s == "scored":
                        cls, sym = (f"ok-{arm}", "✓") if r.get("resolved") else ("fail", "✗")
                        tip = (f"{tid} · {arm} r{k}: {'resolved' if r.get('resolved') else 'not resolved'}"
                               f" · {r.get('turns') or '?'} turns · ${r.get('cost') or 0:.2f}"
                               f" · agent {(r.get('agent_s') or 0) / 60:.1f} min")
                        if arm == "forge":
                            tip += f" · Forge Stop blocks {r.get('stop_blocks', 0)}"
                    elif s == "running":
                        cls, sym, tip = "run", "…", f"{tid} · {arm} r{k}: running"
                    elif s in ("infra_error", "parity_error"):
                        cls, sym, tip = "run", "↻", f"{tid} · {arm} r{k}: retrying after {s}"
                    else:
                        cls, sym, tip = "todo", "", f"{tid} · {arm} r{k}: not run yet"
                    attrs = (f' tabindex="0" role="button" data-task="{e(tid)}" data-key="{arm}/r{k}"'
                             if s in ("scored", "infra_error", "parity_error") else "")
                    cells.append(f'<td class="c {cls}" data-tip="{e(tip)}"{attrs}>{sym}</td>')
            out.append(f'<tr><td class="tl" title="{e(t["subject"])}"><span class="tid">{e(tid.split("__")[1][:7])}</span> '
                       f'<span class="subj">{e(t["subject"][:64])}</span></td>{"".join(cells)}</tr>')
    out.append("</tbody></table></div>")
    return "".join(out)


def forge_activity(tasks):
    rows = []
    for r in REPOS:
        rs = scored(tasks, "forge", r)
        if not rs:
            rows.append(f"<tr><td>{e(r)}</td><td class='n'>0</td><td class='n'>–</td><td class='n'>–</td><td class='n'>–</td></tr>")
            continue
        sb = [x.get("stop_blocks", 0) for x in rs]
        fb = [x.get("feedback", 0) for x in rs]
        handed = sum(1 for x in sb if x >= 3)
        rows.append(f"<tr><td>{e(r)}</td><td class='n'>{len(rs)}</td><td class='n'>{st.mean(sb):.1f}</td>"
                    f"<td class='n'>{st.mean(fb):.1f}</td><td class='n'>{handed}</td></tr>")
    return ("<div class='tw'><table class='plain'><thead><tr><th>Repo</th><th class='n'>Forge trials</th>"
            "<th class='n'>Stop blocks per trial</th><th class='n'>Feedback messages per trial</th>"
            "<th class='n'>Handed over after 3 blocks</th></tr></thead><tbody>" + "".join(rows) + "</tbody></table></div>")


LABEL = {"done": "Done", "running": "In progress", "todo": "Not started", "blocked": "Blocked"}
phases = "\n".join(
    f'<li class="ph {p["state"]}"><span class="pill">{LABEL[p["state"]]}</span>'
    f'<span class="phn">{e(p["name"])}</span><span class="note">{e(p.get("note", ""))}</span></li>'
    for p in state["phases"])
pins = "".join(f"<div><dt>{e(k)}</dt><dd>{e(v)}</dd></div>" for k, v in state["pins"].items())
log = "".join(f"<li>{e(x)}</li>" for x in reversed(state["log"][-14:]))

results_html = '<p class="muted">The main run has not started.</p>'
if len(sys.argv) >= 3 and os.path.exists(os.path.join(sys.argv[2], "manifest.v1.json")):
    tasks = load(sys.argv[1], sys.argv[2])
    total = len(tasks) * 2 * REPS
    done = sum(1 for t in tasks.values() for r in t["trials"].values() if r.get("status") == "scored")
    running = sum(1 for t in tasks.values() for r in t["trials"].values() if r.get("status") == "running")
    tiles = [f'<div class="tile"><div class="tl2">Trials scored</div><div class="big">{done}<span class="of"> / {total}</span></div>'
             f'<div class="bar"><span style="width:{100 * done / total:.1f}%"></span></div>'
             f'<div class="sub">{running} running now</div></div>']
    for arm in ARMS:
        rs = scored(tasks, arm)
        ok = sum(1 for r in rs if r.get("resolved"))
        tiles.append(f'<div class="tile"><div class="tl2"><span class="key k-{arm}"></span>{arm.capitalize()} resolved</div>'
                     f'<div class="big">{pct(ok / len(rs) if rs else None)}</div>'
                     f'<div class="sub">{ok} of {len(rs)} scored · median ${med([r.get("cost") for r in rs]) or 0:.2f} '
                     f'and {(med([r.get("agent_s") for r in rs]) or 0) / 60:.1f} agent-min per trial</div></div>')
    better = worse = same = 0
    for t in tasks.values():
        rates = {}
        for arm in ARMS:
            v = [r.get("resolved") for (a, _), r in t["trials"].items() if a == arm and r.get("status") == "scored"]
            if v:
                rates[arm] = sum(map(bool, v)) / len(v)
        if len(rates) == 2:
            d = rates["forge"] - rates["baseline"]
            better += d > 0
            worse += d < 0
            same += d == 0
    tiles.append(f'<div class="tile"><div class="tl2">Tasks compared so far</div><div class="big">{better + worse + same}</div>'
                 f'<div class="sub">Forge better on {better}, worse on {worse}, equal on {same}</div></div>')
    results_html = f"""
<div class="tiles">{''.join(tiles)}</div>
<p class="caveat">These are interim numbers, not the result. The pre-registered analysis uses only complete replicates, with confidence intervals, once the run ends.</p>
<div class="two">
  <figure><figcaption><h3>Resolved rate by repository</h3><p>Share of scored trials whose hidden tests all pass.</p></figcaption>{chart_rates(tasks)}</figure>
  <figure><figcaption><h3>Where a trial's time goes</h3><p>Median minutes per trial: the agent working (in colour) against setting up the container and scoring in a fresh one (in grey).</p></figcaption>
  {chart_time(tasks)}
  <div class="legend"><span><span class="key k-baseline"></span>Baseline agent</span><span><span class="key k-forge"></span>Forge agent</span><span><span class="key k-setup"></span>Setup + scoring</span></div></figure>
</div>
<section class="why"><h3>Why the run is slow</h3>
<ul>
<li><b>Each trial pays for a full project setup twice.</b> The agent gets a freshly installed container, and its diff is scored in a second fresh one, so it cannot tamper with the tests or the environment. That is 1–3 minutes for the Python repos and zod, and 15–25 minutes for better-auth, whose tests need the whole monorepo built.</li>
<li><b>better-auth dominates.</b> It is 1 trial in 6 but takes about half of all machine time.</li>
<li><b>Forge makes its arm slower by design.</b> Its Stop hook runs the project's checks and related tests before the agent may finish, and sends it back to work (up to 3 times) until they pass. Agent time in the Forge arm is several times the baseline's on zod, pydantic-ai and better-auth.</li>
<li><b>The machine is small.</b> It has 4 CPUs and about 11 GB of free disk, and each workspace takes 1.5–3.5 GB, so only 3 trials run at once and the CPUs are saturated.</li>
</ul></section>
<figure><figcaption><h3>Every trial</h3><p>One row per task, one cell per trial. A filled cell (✓) is resolved, ✗ is not resolved, … is running, and an empty cell is not run yet. <b>Click a cell</b> to see its turns, tests, diff and what Forge did.</p></figcaption>{grid(tasks)}</figure>
<figure><figcaption><h3>What Forge did in its arm</h3><p>Averages per scored Forge trial, from the hook events in the transcripts.</p></figcaption>{forge_activity(tasks)}</figure>
"""

page = rf"""<title>Forge Trial Ledger</title>
<meta name="claude:viewport-fit-sides" content="cover">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Fraunces:opsz,wght@9..144,500;9..144,650&family=IBM+Plex+Sans:wght@400;500;600&family=IBM+Plex+Mono:wght@400;500&display=swap">
<style>
/* Layout: lab-ledger column; headline numbers, then charts, then every trial, then the process. */
:root {{
  --bg: #f6f7f5; --panel: #ffffff; --fg: #1d2421; --muted: #5d6963; --line: #d9dfdb; --grid: #e7ebe8;
  --accent: #2f6b5a; --base: #3d5fc4; --forge: #b8561c; --setup: #b9c1bd; --fail: #7d8782;
  --ok: #2f7d4f; --run: #a57a12; --todo: #8a948f;
  --display: "Fraunces", Georgia, serif; --body: "IBM Plex Sans", system-ui, sans-serif; --mono: "IBM Plex Mono", ui-monospace, monospace;
}}
@media (prefers-color-scheme: dark) {{ :root:not([data-theme="light"]) {{
  --bg: #141917; --panel: #1b2220; --fg: #e3e8e5; --muted: #9aa6a0; --line: #2d3733; --grid: #252e2b;
  --accent: #6cbca2; --base: #6d8ef0; --forge: #d2712f; --setup: #4a5551; --fail: #8c9792;
  --ok: #6cc28f; --run: #e0b54a; --todo: #77827d; color-scheme: dark; }} }}
:root[data-theme="dark"] {{
  --bg: #141917; --panel: #1b2220; --fg: #e3e8e5; --muted: #9aa6a0; --line: #2d3733; --grid: #252e2b;
  --accent: #6cbca2; --base: #6d8ef0; --forge: #d2712f; --setup: #4a5551; --fail: #8c9792;
  --ok: #6cc28f; --run: #e0b54a; --todo: #77827d; color-scheme: dark; }}
body {{ background: var(--bg); color: var(--fg); font: 15px/1.55 var(--body); }}
.wrap {{ max-width: 1040px; margin: 0 auto; padding-block: 28px 56px;
  padding-left: max(16px, env(safe-area-inset-left, 0px)); padding-right: max(16px, env(safe-area-inset-right, 0px));
  display: grid; gap: 30px; }}
header {{ display: grid; gap: 6px; }}
.eyebrow {{ font: 500 12px var(--mono); letter-spacing: .08em; text-transform: uppercase; color: var(--accent); }}
h1 {{ font: 650 clamp(28px, 5vw, 40px)/1.1 var(--display); margin: 0; text-wrap: balance; }}
h2 {{ font: 600 22px/1.2 var(--display); margin: 0 0 12px; }}
h3 {{ font: 600 16px/1.3 var(--body); margin: 0; }}
figcaption p, .muted, .note, .caveat {{ color: var(--muted); font-size: 13.5px; margin: 2px 0 10px; }}
.lede {{ font-size: 17px; max-width: 70ch; margin: 0; }}
.stamp {{ font: 12px var(--mono); color: var(--muted); }}
section, figure {{ min-width: 0; margin: 0; }}
section > figure + figure, .two + .why, .why + figure {{ margin-top: 26px; }}
.tiles {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(210px, 1fr)); gap: 12px; }}
.tile {{ background: var(--panel); border: 1px solid var(--line); padding: 14px 16px; display: grid; gap: 4px; align-content: start; }}
.tl2 {{ font: 500 12px var(--mono); text-transform: uppercase; letter-spacing: .05em; color: var(--muted); display: flex; align-items: center; gap: 6px; }}
.big {{ font: 600 34px/1.1 var(--display); font-variant-numeric: tabular-nums; }}
.of {{ font-size: 18px; color: var(--muted); }}
.sub {{ font-size: 13px; color: var(--muted); }}
.bar {{ height: 6px; background: var(--grid); border-radius: 3px; overflow: hidden; }}
.bar span {{ display: block; height: 100%; background: var(--accent); }}
.caveat {{ margin: 10px 0 18px; }}
.two {{ display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 28px; }}
@media (max-width: 860px) {{ .two {{ grid-template-columns: 1fr; }} }}
svg.chart {{ width: 100%; height: auto; display: block; overflow: visible; }}
.grid {{ stroke: var(--grid); stroke-width: 1; }}
.axis {{ fill: var(--muted); font: 11px var(--mono); }}
.rowlab {{ fill: var(--fg); font: 13px var(--body); }}
.rowlab.strong {{ font-weight: 600; }}
.val {{ fill: var(--fg); font: 500 12px var(--mono); font-variant-numeric: tabular-nums; }}
tspan.muted {{ fill: var(--muted); font-weight: 400; }}
.m-baseline {{ fill: var(--base); }} .m-forge {{ fill: var(--forge); }} .m-setup {{ fill: var(--setup); }}
.hitbox {{ fill: transparent; }}
.hit:hover .hitbox {{ fill: var(--grid); }}
.legend {{ display: flex; flex-wrap: wrap; gap: 16px; font-size: 13px; color: var(--muted); margin-top: 6px; }}
.legend > span {{ display: inline-flex; align-items: center; gap: 6px; }}
.key {{ display: inline-block; width: 10px; height: 10px; border-radius: 2px; flex: none; }}
.k-baseline {{ background: var(--base); }} .k-forge {{ background: var(--forge); }} .k-setup {{ background: var(--setup); }}
.why ul {{ margin: 8px 0 0; padding-left: 18px; display: grid; gap: 6px; max-width: 80ch; }}
.tw {{ overflow-x: auto; }}
table {{ border-collapse: collapse; font-size: 13.5px; }}
table.plain {{ width: 100%; }}
table.plain th, table.plain td {{ text-align: left; padding: 6px 12px 6px 0; border-bottom: 1px solid var(--line); }}
th {{ font: 500 11.5px var(--mono); color: var(--muted); text-transform: uppercase; letter-spacing: .05em; }}
td.n, th.n {{ text-align: right; font-family: var(--mono); font-variant-numeric: tabular-nums; }}
table.grid {{ width: 100%; }}
table.grid td, table.grid th {{ padding: 1px; }}
table.grid th.armh {{ text-align: center; padding-bottom: 2px; white-space: nowrap; }}
table.grid th.armh .key {{ margin-right: 6px; vertical-align: -1px; }}
table.grid th.rep {{ text-align: center; font-size: 10.5px; }}
table.grid td.tl, table.grid th.tl {{ text-align: left; padding-right: 12px; white-space: nowrap; max-width: 0; width: 100%; overflow: hidden; text-overflow: ellipsis; }}
.tid {{ font: 12px var(--mono); color: var(--muted); }}
.subj {{ font-size: 13px; }}
tr.grp td {{ font: 600 12px var(--mono); text-transform: uppercase; letter-spacing: .06em; color: var(--accent); padding-top: 12px; border-bottom: 1px solid var(--line); }}
td.c {{ width: 26px; min-width: 26px; height: 22px; text-align: center; font: 600 12px var(--mono); border-radius: 4px; border: 2px solid var(--bg); }}
td.ok-baseline {{ background: var(--base); color: var(--panel); }}
td.ok-forge {{ background: var(--forge); color: var(--panel); }}
td.fail {{ color: var(--fail); box-shadow: inset 0 0 0 1.5px var(--fail); }}
td.run {{ color: var(--run); box-shadow: inset 0 0 0 1.5px var(--run); }}
td.todo {{ box-shadow: inset 0 0 0 1px var(--line); }}
td.c:hover {{ outline: 2px solid var(--fg); outline-offset: -2px; }}
.tip {{ position: fixed; z-index: 20; pointer-events: none; background: var(--panel); color: var(--fg); border: 1px solid var(--line);
  padding: 6px 9px; font-size: 12.5px; max-width: 320px; box-shadow: 0 4px 14px rgba(0,0,0,.12); border-radius: 4px; }}
.process {{ display: grid; grid-template-columns: minmax(0, 1.4fr) minmax(0, 1fr); gap: 28px; }}
@media (max-width: 760px) {{ .process {{ grid-template-columns: 1fr; }} }}
ol.phases {{ list-style: none; margin: 0; padding: 0; display: grid; gap: 2px; }}
.ph {{ display: grid; grid-template-columns: 104px minmax(0, 1fr); column-gap: 12px; padding: 7px 0; border-bottom: 1px solid var(--line); font-size: 14px; }}
.ph .note {{ grid-column: 2; font-size: 12.5px; margin: 0; }}
.pill {{ grid-row: span 2; align-self: start; font: 500 10.5px var(--mono); text-transform: uppercase; letter-spacing: .05em;
  padding: 3px 6px; border-radius: 3px; border: 1px solid currentColor; text-align: center; }}
.done .pill {{ color: var(--ok); }} .running .pill {{ color: var(--run); }} .todo .pill {{ color: var(--todo); }} .blocked .pill {{ color: var(--forge); }}
dl {{ margin: 0; display: grid; gap: 8px; }}
dl div {{ display: grid; grid-template-columns: 130px minmax(0, 1fr); gap: 8px; }}
dt {{ color: var(--muted); font-size: 13px; }} dd {{ margin: 0; font: 13px var(--mono); overflow-wrap: anywhere; }}
ul.log {{ margin: 12px 0 0; padding-left: 18px; display: grid; gap: 4px; font-size: 13.5px; }}
[hidden] {{ display: none !important; }}
td.c[role=button] {{ cursor: pointer; }}
td.c:focus-visible {{ outline: 2px solid var(--accent); outline-offset: -2px; }}
.drawer {{ position: fixed; z-index: 30; top: 0; right: 0; bottom: 0; width: min(760px, 100vw); background: var(--bg);
  border-left: 1px solid var(--line); box-shadow: -8px 0 30px rgba(0,0,0,.18); display: flex; flex-direction: column;
  padding-top: env(safe-area-inset-top, 0px); padding-bottom: env(safe-area-inset-bottom, 0px); padding-right: env(safe-area-inset-right, 0px); }}
.dh {{ padding: 16px 18px 10px; border-bottom: 1px solid var(--line); display: grid; gap: 8px; }}
.dh .top {{ display: flex; gap: 12px; align-items: start; justify-content: space-between; }}
.dh h3 {{ font: 600 18px/1.3 var(--display); }}
.dh .meta {{ font: 12px var(--mono); color: var(--muted); }}
.close {{ font: 600 13px var(--body); background: var(--panel); color: var(--fg); border: 1px solid var(--line); border-radius: 4px; padding: 5px 10px; cursor: pointer; }}
.switch {{ display: flex; flex-wrap: wrap; gap: 6px; }}
.switch button, .tabs button {{ font: 500 12.5px var(--body); background: var(--panel); color: var(--fg); border: 1px solid var(--line);
  border-radius: 4px; padding: 4px 9px; cursor: pointer; display: inline-flex; align-items: center; gap: 6px; }}
.switch button[aria-pressed=true], .tabs button[aria-selected=true] {{ border-color: var(--fg); box-shadow: inset 0 0 0 1px var(--fg); }}
.switch button:disabled {{ opacity: .45; cursor: default; }}
.stats {{ display: grid; grid-template-columns: repeat(auto-fill, minmax(118px, 1fr)); gap: 6px 14px; padding: 12px 18px; border-bottom: 1px solid var(--line); }}
.stats div {{ display: grid; }}
.stats b {{ font: 600 16px var(--mono); font-variant-numeric: tabular-nums; }}
.stats span {{ font-size: 11.5px; color: var(--muted); text-transform: uppercase; letter-spacing: .04em; }}
.verdict {{ font: 600 13px var(--mono); padding: 2px 8px; border-radius: 3px; display: inline-block; }}
.verdict.ok {{ background: var(--ok); color: var(--panel); }} .verdict.no {{ box-shadow: inset 0 0 0 1.5px var(--fail); color: var(--fail); }}
.tabs {{ display: flex; gap: 6px; padding: 10px 18px 0; }}
.dbody {{ overflow-y: auto; padding: 12px 18px 40px; flex: 1; min-height: 0; }}
.step {{ display: grid; grid-template-columns: 74px minmax(0, 1fr); gap: 10px; padding: 7px 0; border-bottom: 1px solid var(--grid); }}
.badge {{ font: 600 10.5px var(--mono); text-transform: uppercase; letter-spacing: .04em; border-radius: 3px; padding: 2px 5px; align-self: start; text-align: center; border: 1px solid var(--line); color: var(--muted); }}
.b-tool {{ color: var(--fg); }} .b-block {{ color: var(--panel); background: var(--forge); border-color: var(--forge); }}
.b-hook {{ color: var(--forge); border-color: var(--forge); }} .b-result {{ color: var(--panel); background: var(--accent); border-color: var(--accent); }}
.turnno {{ font: 11px var(--mono); color: var(--muted); display: block; margin-top: 3px; text-align: center; }}
pre.s {{ margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; font: 12.5px/1.45 var(--mono); }}
.step details {{ margin-top: 4px; }}
.step summary {{ cursor: pointer; font-size: 12px; color: var(--muted); }}
.step summary.err {{ color: var(--forge); }}
pre.o {{ margin: 4px 0 0; padding: 8px; background: var(--panel); border: 1px solid var(--line); white-space: pre-wrap; overflow-wrap: anywhere; font: 11.5px/1.45 var(--mono); max-height: 320px; overflow: auto; }}
.trow {{ display: grid; grid-template-columns: 64px minmax(0, 1fr); gap: 8px; padding: 3px 0; font: 12px var(--mono); overflow-wrap: anywhere; }}
.st-passed {{ color: var(--ok); }} .st-failed, .st-error, .st-missing {{ color: var(--forge); font-weight: 600; }}
.diff .add {{ color: var(--ok); }} .diff .del {{ color: var(--forge); }} .diff .hd {{ color: var(--accent); font-weight: 600; }}
.instr {{ white-space: pre-wrap; font-size: 14px; max-width: 80ch; }}
@media (prefers-reduced-motion: reduce) {{ * {{ transition: none !important; }} }}
</style>
<div class="wrap">
<header>
  <div class="eyebrow">Controlled experiment · baseline vs Forge</div>
  <h1>Does Forge make Claude Code better at real tasks?</h1>
  <p class="lede">{e(state["headline"])}</p>
  <div class="stamp">Updated {state["updated"]}</div>
</header>
<section><h2>{e(RUN_LABEL)}</h2>{results_html}</section>
<section class="process">
  <div><h2>Progress</h2><ol class="phases">{phases}</ol></div>
  <div><h2>Pinned setup</h2><dl>{pins}</dl><h2 style="margin-top:24px">Log</h2><ul class="log">{log}</ul></div>
</section>
</div>
<div class="tip" id="tip" hidden></div>
<aside class="drawer" id="drawer" hidden aria-label="Trial details">
  <div class="dh"><div class="top"><div><h3 id="d-title"></h3><div class="meta" id="d-meta"></div></div>
  <button class="close" id="d-close" type="button">Close</button></div><div class="switch" id="d-switch"></div></div>
  <div class="stats" id="d-stats"></div>
  <div class="tabs" role="tablist" id="d-tabs"></div>
  <div class="dbody" id="d-body"></div>
</aside>
<script>
(function () {{
  var drawer = document.getElementById('drawer'), cache = {{}}, cur = null;
  var TABS = [['turns', 'Turns'], ['tests', 'Tests'], ['diff', 'Diff'], ['task', 'Task']];
  function el(tag, cls, text) {{ var n = document.createElement(tag); if (cls) n.className = cls; if (text != null) n.textContent = text; return n; }}
  function fmt(x, d) {{ return x == null ? '–' : x.toFixed(d); }}
  function open(task, key) {{
    cur = {{ task: task, key: key, tab: (cur && cur.tab) || 'turns' }};
    drawer.hidden = false;
    document.getElementById('d-title').textContent = 'Loading…';
    (cache[task] ? Promise.resolve(cache[task]) : fetch('data/' + task + '.json').then(function (r) {{
      if (!r.ok) throw new Error(r.status); return r.json(); }})).then(function (doc) {{ cache[task] = doc; render(doc); }})
      .catch(function () {{ document.getElementById('d-title').textContent = 'The details for this task could not be loaded. Reload the page and try again.'; }});
  }}
  function render(doc) {{
    var t = doc.trials[cur.key] || {{}};
    document.getElementById('d-title').textContent = doc.subject;
    document.getElementById('d-meta').textContent = doc.task_id + ' · ' + doc.repo + ' · ' + doc.type + ' · upstream ' + doc.commit.slice(0, 10);
    var sw = document.getElementById('d-switch'); sw.textContent = '';
    ['baseline', 'forge'].forEach(function (arm) {{ [1, 2, 3].forEach(function (k) {{
      var key = arm + '/r' + k, tr = doc.trials[key], b = el('button');
      b.type = 'button'; b.setAttribute('aria-pressed', key === cur.key);
      var dot = el('span', 'key k-' + arm); b.appendChild(dot);
      b.appendChild(document.createTextNode((arm === 'forge' ? 'Forge' : 'Baseline') + ' r' + k +
        (tr ? (tr.status !== 'scored' ? ' ↻' : tr.resolved ? ' ✓' : ' ✗') : '')));
      if (!tr) b.disabled = true; else b.onclick = function () {{ cur.key = key; render(doc); }};
      sw.appendChild(b); }}); }});
    var stats = document.getElementById('d-stats'); stats.textContent = '';
    function stat(label, val, node) {{ var d = el('div'); d.appendChild(node || el('b', null, val)); d.appendChild(el('span', null, label)); stats.appendChild(d); }}
    if (t.status === 'scored') {{
      stat('outcome', null, el('b', 'verdict ' + (t.resolved ? 'ok' : 'no'), t.resolved ? 'Resolved' : 'Not resolved'));
      var f = t.f2p || {{}}, fk = Object.keys(f), fp = fk.filter(function (k) {{ return f[k] === 'passed'; }}).length;
      stat('target tests pass', fp + ' / ' + fk.length);
      stat('regressions', String(t.p2p_regressions != null ? t.p2p_regressions : '–'));
    }} else {{ stat('status', t.status || '–'); }}
    stat('turns', String(t.num_turns != null ? t.num_turns : '–'));
    stat('cost', t.cost_usd != null ? '$' + fmt(t.cost_usd, 2) : '–');
    stat('agent time', t.duration_s != null ? fmt(t.duration_s / 60, 1) + ' min' : '–');
    stat('whole trial', t.wall_s != null ? fmt(t.wall_s / 60, 1) + ' min' : '–');
    stat('lines changed', String(t.diff_lines != null ? t.diff_lines : '–'));
    stat('new lint errors', String(t.lint_new != null ? t.lint_new : '–'));
    if (t.edits) stat('edits: tool / shell', t.edits.edit_tool_calls + ' / ' + t.edits.shell_edit_calls);
    if (cur.key.indexOf('forge') === 0 && t.hooks) {{ stat('Forge stop blocks', String(t.hooks.stop_blocks)); stat('Forge feedback', String(t.hooks.feedback)); }}
    var tabs = document.getElementById('d-tabs'); tabs.textContent = '';
    TABS.forEach(function (x) {{ var b = el('button', null, x[1]); b.type = 'button'; b.setAttribute('role', 'tab');
      b.setAttribute('aria-selected', cur.tab === x[0]); b.onclick = function () {{ cur.tab = x[0]; render(doc); }}; tabs.appendChild(b); }});
    var body = document.getElementById('d-body'); body.textContent = ''; body.scrollTop = 0;
    if (cur.tab === 'turns') turns(body, t);
    else if (cur.tab === 'tests') tests(body, t, doc);
    else if (cur.tab === 'diff') diff(body, t);
    else {{ body.appendChild(el('p', 'muted', 'The instruction both arms received, word for word. Hidden test files: ' + doc.test_files.join(', ') +
      '. ' + doc.n_f2p + ' target tests and ' + doc.n_p2p + ' existing tests are scored; the audit dropped ' + doc.audit_dropped + '.'));
      body.appendChild(el('div', 'instr', doc.instruction)); }}
  }}
  function turns(body, t) {{
    if (t.infra_reason) body.appendChild(el('p', 'muted', 'Infrastructure: ' + t.infra_reason));
    var steps = t.steps || [];
    if (!steps.length) {{ body.appendChild(el('p', 'muted', 'No transcript for this attempt.')); return; }}
    body.appendChild(el('p', 'muted', 'Every message, tool call and result, plus each hook that said something (Forge feedback and blocks). Click "output" to expand a result.'));
    steps.forEach(function (s) {{
      var row = el('div', 'step'), left = el('div');
      var label = s.k === 'tool' ? s.tool : s.k === 'text' ? 'says' : s.k === 'block' ? 'blocked' : s.k === 'hook' ? 'hook' : 'final';
      left.appendChild(el('div', 'badge b-' + s.k, label)); left.appendChild(el('span', 'turnno', 'turn ' + s.turn));
      var right = el('div'); right.style.minWidth = '0';
      if (s.k === 'hook' || s.k === 'block') right.appendChild(el('div', 'muted', (s.hook || '') + (s.exit != null ? ' · exit ' + s.exit : '')));
      right.appendChild(el('pre', 's', s.s));
      if (s.out != null) {{ var det = el('details'), sm = el('summary', s.err ? 'err' : null, s.err ? 'output (error)' : 'output');
        det.appendChild(sm); det.appendChild(el('pre', 'o', s.out || '(empty)')); right.appendChild(det); }}
      row.appendChild(left); row.appendChild(right); body.appendChild(row);
    }});
  }}
  function tests(body, t, doc) {{
    if (!t.f2p) {{ body.appendChild(el('p', 'muted', 'This attempt was not scored.')); return; }}
    function list(title, obj, onlyBad) {{
      var keys = Object.keys(obj), bad = keys.filter(function (k) {{ return obj[k] !== 'passed'; }});
      body.appendChild(el('h3', null, title + ' (' + (keys.length - bad.length) + ' of ' + keys.length + ' pass)'));
      var show = onlyBad ? bad : keys;
      if (onlyBad) body.appendChild(el('p', 'muted', bad.length ? 'Only the existing tests that broke are listed.' : 'No existing test broke.'));
      show.forEach(function (k) {{ var r = el('div', 'trow'); r.appendChild(el('span', 'st-' + obj[k], obj[k])); r.appendChild(el('span', null, k)); body.appendChild(r); }});
    }}
    body.appendChild(el('p', 'muted', 'Target tests failed before the upstream change and pass after it. Existing tests passed before and must still pass. Run on the agent’s final diff in a fresh container.'));
    list('Target tests', t.f2p, false);
    var sp = el('div'); sp.style.height = '14px'; body.appendChild(sp);
    list('Existing tests', t.p2p || {{}}, true);
  }}
  function diff(body, t) {{
    if (!t.diff) {{ body.appendChild(el('p', 'muted', 'The agent changed no files.')); return; }}
    body.appendChild(el('p', 'muted', 'The agent’s final change against the starting commit (Forge’s own files excluded). Files: ' + (t.changed_files || []).join(', ')));
    var pre = el('pre', 'o diff'); pre.style.maxHeight = 'none';
    t.diff.split('\n').forEach(function (l) {{ var c = /^\+\+\+|^---|^diff |^@@/.test(l) ? 'hd' : l[0] === '+' ? 'add' : l[0] === '-' ? 'del' : null;
      pre.appendChild(el('span', c, l + '\n')); }});
    body.appendChild(pre);
  }}
  document.addEventListener('click', function (ev) {{ var c = ev.target.closest && ev.target.closest('td.c[data-task]'); if (c) open(c.dataset.task, c.dataset.key); }});
  document.addEventListener('keydown', function (ev) {{
    if (ev.key === 'Escape') drawer.hidden = true;
    var c = ev.target.closest && ev.target.closest('td.c[data-task]');
    if (c && (ev.key === 'Enter' || ev.key === ' ')) {{ ev.preventDefault(); open(c.dataset.task, c.dataset.key); }} }});
  document.getElementById('d-close').onclick = function () {{ drawer.hidden = true; }};
}})();
</script>

<script>
(function () {{
  var tip = document.getElementById('tip');
  document.addEventListener('mousemove', function (ev) {{
    var t = ev.target.closest && ev.target.closest('[data-tip]');
    if (!t) {{ tip.hidden = true; return; }}
    tip.textContent = t.getAttribute('data-tip'); tip.hidden = false;
    var x = Math.min(ev.clientX + 14, window.innerWidth - tip.offsetWidth - 8);
    var y = ev.clientY + 16 + tip.offsetHeight > window.innerHeight ? ev.clientY - tip.offsetHeight - 10 : ev.clientY + 16;
    tip.style.left = x + 'px'; tip.style.top = y + 'px';
  }});
}})();
</script>
"""
open(os.path.join(HERE, "index.html"), "w").write(page)
print("rendered", os.path.join(HERE, "index.html"))
