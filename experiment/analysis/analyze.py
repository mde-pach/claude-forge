#!/usr/bin/env python3
"""Pre-registered analysis of the Forge experiment (DESIGN.md sections 6, 8 and 10).

Usage:
    python3 analysis/analyze.py <results_dir> <dataset_dir> --out <out_dir>

Writes <out_dir>/summary.json (all numbers) and <out_dir>/tables.md (human-readable).
Python 3 standard library only, so the analysis is reproducible on any host.

Analysis sets
-------------
* primary:     trials of *complete replicates* only (DESIGN.md section 8: replicate k is complete
               when every dataset task has a scored trial in both arms at rep k). Truncation
               therefore cannot favour an arm.
* sensitivity: every scored trial, whatever its replicate.

Unit of analysis is the task: per-task resolved rate = mean over its scored trials in that arm;
per-task difference d_i = rate_forge - rate_baseline.

Methods
-------
* Per-arm resolved rates: trial-level pooled mean and task-level mean of per-task rates; 95%
  cluster bootstrap CI resampling tasks (10,000 draws, seed 20261024, percentile intervals).
* Paired difference: mean d_i, 95% task bootstrap CI, two-sided sign-flip permutation test (exact
  when <= 20 nonzero d_i, else 100,000 random flips). TOST equivalence at +/-10 pp via the 90%
  bootstrap CI (Schuirmann 1987: equivalence at alpha = .05 iff the 90% CI lies inside the
  margins).
* McNemar exact test on trial pairs matched by (task, rep).
* Mantel-Haenszel odds ratio stratified by task, Robins-Breslow-Greenland variance.
  (A GLMM is skipped: statsmodels is not available on the analysis host.)
* Breakdowns by repository and task type, with a label-permutation heterogeneity test.
* Secondary metrics: per-arm median and IQR over trials; paired difference = median over tasks of
  (per-task mean forge - per-task mean baseline), 95% task bootstrap CI.
* Validity: infra/parity error rates per arm (Fisher exact and Pearson chi-square), timeouts,
  ceiling/floor tasks, Forge-active rate, pin consistency.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from collections import Counter, defaultdict

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import stats as S  # noqa: E402
from common import ARMS, load_dataset, load_trials, resolved_of, status_of, total_cost  # noqa: E402

TOST_MARGIN = 0.10


# ============================================================ metric extraction

def _outcome_field(name):
    def f(rec):
        o = rec.get("outcome") or {}
        v = o.get(name)
        return None if v is None else float(v)
    return f


def _meta_field(name):
    def f(rec):
        v = rec["meta"].get(name)
        return None if v is None else float(v)
    return f


def _edit_share(rec):
    e = rec.get("edits") or {}
    et, sh = e.get("edit_tool_calls"), e.get("shell_edit_calls")
    if et is None or sh is None or (et + sh) == 0:
        return None
    return et / (et + sh)


SECONDARY = {
    "cost_usd": lambda r: total_cost(r["meta"]),
    "cost_usd_reviewer": _meta_field("cost_usd_reviewer"),
    "num_turns": _meta_field("num_turns"),
    "duration_s": _meta_field("duration_s"),
    "f2p_frac": _outcome_field("f2p_frac"),
    "p2p_regressions": _outcome_field("p2p_regressions"),
    "lint_new": _outcome_field("lint_new"),
    "diff_lines": _outcome_field("diff_lines"),
    "edit_tool_share": _edit_share,
}


# ============================================================ helpers

def fmt(x, nd=3):
    if x is None:
        return "–"
    if isinstance(x, float):
        return f"{x:.{nd}f}"
    return str(x)


def pct(x):
    return "–" if x is None else f"{100 * x:.1f}%"


def ci_str(t, as_pct=True, nd=3):
    pt, lo, hi = t
    f = pct if as_pct else (lambda v: fmt(v, nd))
    return f"{f(pt)} [{f(lo)}, {f(hi)}]"


def per_task_outcomes(trials, task_ids):
    """{task: {arm: [0/1, ...]}} using scored trials only."""
    out = {t: {a: [] for a in ARMS} for t in task_ids}
    for (t, a, _k), rec in trials.items():
        if t in out:
            r = resolved_of(rec)
            if r is not None:
                out[t][a].append(1 if r else 0)
    return out


# ============================================================ core analysis

def arm_rates(per_task, arm):
    units = [v[arm] for v in per_task.values() if v[arm]]

    def trial_level(us):
        n = sum(len(u) for u in us)
        return sum(sum(u) for u in us) / n if n else None

    def task_level(us):
        return sum(sum(u) / len(u) for u in us) / len(us) if us else None

    return {
        "n_tasks": len(units),
        "n_trials": sum(len(u) for u in units),
        "n_resolved": sum(sum(u) for u in units),
        "trial_level": dict(zip(("est", "lo", "hi"), S.bootstrap_ci(units, trial_level))),
        "task_level": dict(zip(("est", "lo", "hi"), S.bootstrap_ci(units, task_level))),
    }


def paired_diffs(per_task):
    """[(task, rate_baseline, rate_forge, d)] for tasks with >= 1 scored trial in both arms."""
    rows = []
    for t, v in sorted(per_task.items()):
        if v["baseline"] and v["forge"]:
            rb = sum(v["baseline"]) / len(v["baseline"])
            rf = sum(v["forge"]) / len(v["forge"])
            rows.append((t, rb, rf, rf - rb))
    return rows


def paired_block(diffs, *, with_tests=True):
    ds = [d for *_, d in diffs]
    mean_ = lambda xs: sum(xs) / len(xs) if xs else None  # noqa: E731
    out = {"n_tasks": len(ds)}
    pt, lo, hi = S.bootstrap_ci(ds, mean_)
    out["mean_diff"] = {"est": pt, "lo": lo, "hi": hi}
    out["forge_better"] = sum(d > 1e-12 for d in ds)
    out["forge_worse"] = sum(d < -1e-12 for d in ds)
    out["equal"] = sum(abs(d) <= 1e-12 for d in ds)
    if with_tests:
        out["sign_flip"] = S.sign_flip_test(ds)
        _, lo90, hi90 = S.bootstrap_ci(ds, mean_, level=0.90)
        out["tost"] = {"margin": TOST_MARGIN, "ci90_lo": lo90, "ci90_hi": hi90,
                       "equivalent": (lo90 is not None and lo90 > -TOST_MARGIN and hi90 < TOST_MARGIN)}
    return out


def mcnemar_block(trials, task_ids, reps=None):
    b = c = both = neither = 0
    for t in task_ids:
        ks = {k for (tt, a, k) in trials if tt == t}
        for k in ks:
            if reps is not None and k not in reps:
                continue
            rb = trials.get((t, "baseline", k))
            rf = trials.get((t, "forge", k))
            if not rb or not rf:
                continue
            xb, xf = resolved_of(rb), resolved_of(rf)
            if xb is None or xf is None:
                continue
            if xf and not xb:
                b += 1
            elif xb and not xf:
                c += 1
            elif xb and xf:
                both += 1
            else:
                neither += 1
    out = S.mcnemar_exact(b, c)
    out.update({"both_resolved": both, "neither_resolved": neither, "n_pairs": b + c + both + neither})
    return out


def mh_block(per_task):
    tables = []
    for v in per_task.values():
        if v["baseline"] and v["forge"]:
            a = sum(v["forge"]); b = len(v["forge"]) - a
            c = sum(v["baseline"]); d = len(v["baseline"]) - c
            tables.append((a, b, c, d))
    return S.mantel_haenszel_or(tables)


def breakdown(per_task, tasks, field):
    diffs = paired_diffs(per_task)
    groups = defaultdict(list)
    for row in diffs:
        groups[tasks[row[0]].get(field, "unknown")].append(row)
    out = {}
    mean_ = lambda xs: sum(xs) / len(xs) if xs else None  # noqa: E731
    for g, rows in sorted(groups.items()):
        ds = [r[3] for r in rows]
        pt, lo, hi = S.bootstrap_ci(ds, mean_)
        out[g] = {
            "n_tasks": len(rows),
            "baseline_rate": mean_([r[1] for r in rows]),
            "forge_rate": mean_([r[2] for r in rows]),
            "mean_diff": {"est": pt, "lo": lo, "hi": hi},
            "forge_better": sum(d > 1e-12 for d in ds),
            "forge_worse": sum(d < -1e-12 for d in ds),
        }
    het = S.label_permutation_heterogeneity({g: [r[3] for r in rows] for g, rows in groups.items()})
    return {"groups": out, "heterogeneity": het}


def secondary_block(trials, task_ids_set, reps=None):
    out = {}
    for name, fn in SECONDARY.items():
        per_arm_vals = {a: [] for a in ARMS}
        per_task = defaultdict(lambda: {a: [] for a in ARMS})
        for (t, a, k), rec in trials.items():
            if t not in task_ids_set or status_of(rec) != "scored":
                continue
            if reps is not None and k not in reps:
                continue
            v = fn(rec)
            if v is None:
                continue
            per_arm_vals[a].append(v)
            per_task[t][a].append(v)
        entry = {}
        for a in ARMS:
            q1, med, q3 = S.quartiles(per_arm_vals[a])
            entry[a] = {"n": len(per_arm_vals[a]), "median": med, "q1": q1, "q3": q3,
                        "mean": S.mean(per_arm_vals[a])}
        ds = [S.mean(v["forge"]) - S.mean(v["baseline"])
              for v in per_task.values() if v["forge"] and v["baseline"]]
        pt, lo, hi = S.bootstrap_ci(ds, S.median)
        entry["paired_median_diff"] = {"n_tasks": len(ds), "est": pt, "lo": lo, "hi": hi}
        out[name] = entry
    return out


def hook_block(trials, task_ids_set, reps=None):
    """Forge hook activity from forge/hook_summary.json. All numeric top-level fields (and fields
    of a nested `counts` object) are summarised per trial: total, mean, median, share > 0."""
    out = {}
    for arm in ARMS:
        vals = defaultdict(list)
        n_summ = n_active = n_trials = 0
        for (t, a, k), rec in trials.items():
            if a != arm or t not in task_ids_set or status_of(rec) != "scored":
                continue
            if reps is not None and k not in reps:
                continue
            n_trials += 1
            h = rec.get("hook")
            if not isinstance(h, dict):
                continue
            n_summ += 1
            n_active += bool(h.get("active"))
            flat = dict(h)
            if isinstance(h.get("counts"), dict):
                flat.update(h["counts"])
            for key, v in flat.items():
                if isinstance(v, (int, float)) and not isinstance(v, bool):
                    vals[key].append(float(v))
        out[arm] = {
            "n_trials": n_trials, "n_with_summary": n_summ, "n_active": n_active,
            "active_rate": n_active / n_trials if n_trials else None,
            "fields": {key: {"total": sum(v), "mean": S.mean(v), "median": S.median(v),
                             "share_nonzero": sum(x > 0 for x in v) / len(v)}
                       for key, v in sorted(vals.items())},
        }
    return out


def analyse_set(label, trials, tasks, task_ids, reps):
    sub = {k: v for k, v in trials.items() if k[0] in task_ids and (reps is None or k[2] in reps)}
    per_task = per_task_outcomes(sub, task_ids)
    diffs = paired_diffs(per_task)
    ceiling = [t for t, rb, rf, _ in diffs if rb == 1 and rf == 1]
    floor = [t for t, rb, rf, _ in diffs if rb == 0 and rf == 0]
    return {
        "label": label,
        "replicates": sorted(reps) if reps is not None else "all",
        "rates": {a: arm_rates(per_task, a) for a in ARMS},
        "paired": paired_block(diffs),
        "mcnemar": mcnemar_block(sub, task_ids),
        "mantel_haenszel": mh_block(per_task),
        "by_repo": breakdown(per_task, tasks, "repo"),
        "by_type": breakdown(per_task, tasks, "type"),
        "secondary": secondary_block(sub, set(task_ids)),
        "hooks": hook_block(sub, set(task_ids)),
        "ceiling_tasks": ceiling,
        "floor_tasks": floor,
        "per_task": [{"task": t, "baseline": rb, "forge": rf, "diff": d,
                      "n_baseline": len(per_task[t]["baseline"]), "n_forge": len(per_task[t]["forge"])}
                     for t, rb, rf, d in diffs],
    }


# ============================================================ counts & validity

def counts_block(trials, tasks):
    task_ids = set(tasks)
    by_status = {a: Counter() for a in ARMS}
    attempts = {a: Counter() for a in ARMS}
    attempt_hist = {a: Counter() for a in ARMS}
    reps_seen = set()
    for (t, a, k), rec in trials.items():
        if t not in task_ids:
            continue
        reps_seen.add(k)
        by_status[a][status_of(rec)] += 1
        attempts[a]["total"] += rec["attempts"]
        attempt_hist[a][rec["attempts"]] += 1
    complete = []
    rep_cov = {}
    for k in sorted(reps_seen):
        n_ok = sum(1 for t in task_ids
                   if all((t, a, k) in trials and resolved_of(trials[(t, a, k)]) is not None
                          for a in ARMS))
        rep_cov[k] = {"tasks_complete": n_ok, "tasks_total": len(task_ids)}
        if n_ok == len(task_ids) and task_ids:
            complete.append(k)
    unknown = sorted({t for (t, _, _) in trials if t not in task_ids})
    return {
        "n_tasks_dataset": len(task_ids),
        "trials_by_status": {a: dict(by_status[a]) for a in ARMS},
        "attempts_total": {a: attempts[a]["total"] for a in ARMS},
        "attempts_histogram": {a: {str(k): v for k, v in sorted(attempt_hist[a].items())} for a in ARMS},
        "replicates_seen": sorted(reps_seen),
        "replicate_coverage": {str(k): v for k, v in rep_cov.items()},
        "complete_replicates": complete,
        "results_tasks_not_in_dataset": unknown,
    }


def validity_block(trials, tasks):
    task_ids = set(tasks)
    failed_att = Counter(); ok_att = Counter(); parity = Counter(); infra_final = Counter()
    timeouts = Counter(); scored = Counter()
    pins = {a: defaultdict(Counter) for a in ARMS}
    for (t, a, k), rec in trials.items():
        if t not in task_ids:
            continue
        st = status_of(rec)
        n_fail = rec["attempts"] - (1 if st == "scored" else 0)
        failed_att[a] += n_fail
        ok_att[a] += 1 if st == "scored" else 0
        parity[a] += sum(1 for s in rec["attempt_records"] if s == "parity_error")
        if st in ("infra_error", "parity_error"):
            infra_final[a] += 1
        if st == "scored":
            scored[a] += 1
            timeouts[a] += bool(rec["meta"].get("timed_out"))
        for f in ("model", "claude_version", "forge_commit"):
            pins[a][f][str(rec["meta"].get(f))] += 1
    fb, ff = failed_att["baseline"], failed_att["forge"]
    ob, of = ok_att["baseline"], ok_att["forge"]
    tb, tf = timeouts["baseline"], timeouts["forge"]
    sb, sf = scored["baseline"], scored["forge"]
    return {
        "infra_attempts": {
            "baseline": {"failed_attempts": fb, "scored_attempts": ob,
                         "rate": fb / (fb + ob) if fb + ob else None},
            "forge": {"failed_attempts": ff, "scored_attempts": of,
                      "rate": ff / (ff + of) if ff + of else None},
            "fisher_p": S.fisher_exact(ff, of, fb, ob),
            "chi2": S.chi2_2x2(ff, of, fb, ob),
            "note": "attempt level: failed = infra_error or parity_error attempts (attempts - 1 for "
                    "a scored trial, all attempts for an exhausted one)",
        },
        "trials_exhausted_retries": dict(infra_final),
        "parity_error_attempts_recorded": dict(parity),
        "timeouts": {
            "baseline": {"n": tb, "of_scored": sb, "rate": tb / sb if sb else None},
            "forge": {"n": tf, "of_scored": sf, "rate": tf / sf if sf else None},
            "fisher_p": S.fisher_exact(tf, sf - tf, tb, sb - tb),
        },
        "pins": {a: {f: dict(c) for f, c in pins[a].items()} for a in ARMS},
    }


# ============================================================ markdown

def md_set(r):
    L = []
    L.append(f"### Resolved rate (replicates: {r['replicates']})\n")
    L.append("| Arm | tasks | trials | resolved | trial-level [95% CI] | task-level [95% CI] |")
    L.append("|---|---|---|---|---|---|")
    for a in ARMS:
        x = r["rates"][a]
        tl, kl = x["trial_level"], x["task_level"]
        L.append(f"| {a} | {x['n_tasks']} | {x['n_trials']} | {x['n_resolved']} | "
                 f"{ci_str((tl['est'], tl['lo'], tl['hi']))} | {ci_str((kl['est'], kl['lo'], kl['hi']))} |")
    p = r["paired"]
    md, tost, sf = p["mean_diff"], p["tost"], p["sign_flip"]
    L.append("\n**Paired difference forge − baseline (per-task rates)**\n")
    L.append(f"- tasks: {p['n_tasks']}; forge better {p['forge_better']}, worse {p['forge_worse']}, "
             f"equal {p['equal']}")
    L.append(f"- mean difference: {ci_str((md['est'], md['lo'], md['hi']))} (95% task bootstrap)")
    L.append(f"- sign-flip permutation p = {fmt(sf['p'], 4)} ({sf['method']}, {sf['n_nonzero']} nonzero)")
    L.append(f"- TOST ±{int(100 * tost['margin'])} pp: 90% CI [{pct(tost['ci90_lo'])}, {pct(tost['ci90_hi'])}] "
             f"→ {'equivalent' if tost['equivalent'] else 'equivalence NOT shown'}")
    mc = r["mcnemar"]
    L.append(f"- McNemar (pairs by task × rep): {mc['n_pairs']} pairs, forge-only {mc['b_forge_only']}, "
             f"baseline-only {mc['c_baseline_only']}, exact p = {fmt(mc['p'], 4)}")
    mh = r["mantel_haenszel"]
    L.append(f"- Mantel–Haenszel OR (strata = tasks, RBG variance): {fmt(mh['or'])} "
             f"[{fmt(mh['lo'])}, {fmt(mh['hi'])}], informative strata {mh['strata_informative']}")
    for key, title in (("by_repo", "repository"), ("by_type", "task type")):
        b = r[key]
        L.append(f"\n**By {title}** (heterogeneity permutation p = {fmt(b['heterogeneity']['p'], 4)})\n")
        L.append("| group | tasks | baseline | forge | diff [95% CI] | better/worse |")
        L.append("|---|---|---|---|---|---|")
        for g, x in b["groups"].items():
            m = x["mean_diff"]
            L.append(f"| {g} | {x['n_tasks']} | {pct(x['baseline_rate'])} | {pct(x['forge_rate'])} | "
                     f"{ci_str((m['est'], m['lo'], m['hi']))} | {x['forge_better']}/{x['forge_worse']} |")
    L.append("\n**Secondary metrics** (per-arm median [IQR] over trials; paired = median over tasks of "
             "per-task mean difference, 95% bootstrap CI)\n")
    L.append("| metric | baseline | forge | paired median diff [95% CI] |")
    L.append("|---|---|---|---|")
    for name, e in r["secondary"].items():
        cells = []
        for a in ARMS:
            x = e[a]
            cells.append(f"{fmt(x['median'], 2)} [{fmt(x['q1'], 2)}–{fmt(x['q3'], 2)}] (n={x['n']})")
        pm = e["paired_median_diff"]
        L.append(f"| {name} | {cells[0]} | {cells[1]} | {fmt(pm['est'], 3)} [{fmt(pm['lo'], 3)}, "
                 f"{fmt(pm['hi'], 3)}] (tasks={pm['n_tasks']}) |")
    h = r["hooks"]
    L.append("\n**Forge hook activity** (forge/hook_summary.json)\n")
    for a in ARMS:
        x = h[a]
        L.append(f"- {a}: active {x['n_active']}/{x['n_trials']} ({pct(x['active_rate'])}), "
                 f"summaries {x['n_with_summary']}")
        for k, f in x["fields"].items():
            L.append(f"  - {k}: total {fmt(f['total'], 0)}, mean {fmt(f['mean'], 2)}, "
                     f"median {fmt(f['median'], 1)}, nonzero {pct(f['share_nonzero'])}")
    L.append(f"\nCeiling tasks (both arms 100%): {len(r['ceiling_tasks'])}; "
             f"floor tasks (both 0%): {len(r['floor_tasks'])}")
    return "\n".join(L)


def render_md(summary):
    c, v = summary["counts"], summary["validity"]
    L = ["# Forge experiment — analysis", "",
         f"Results: `{summary['inputs']['results_dir']}`  ", f"Dataset: `{summary['inputs']['dataset_dir']}` "
         f"({c['n_tasks_dataset']} tasks)", "", "## Counts", "",
         "| Arm | scored | infra_error | parity_error | attempts used |", "|---|---|---|---|---|"]
    for a in ARMS:
        s = c["trials_by_status"][a]
        L.append(f"| {a} | {s.get('scored', 0)} | {s.get('infra_error', 0)} | {s.get('parity_error', 0)} | "
                 f"{c['attempts_total'][a]} |")
    L.append("")
    L.append("Replicate coverage (tasks with a scored trial in both arms): " + ", ".join(
        f"r{k}: {x['tasks_complete']}/{x['tasks_total']}" for k, x in c["replicate_coverage"].items()))
    L.append(f"\n**Complete replicates (primary analysis set): {c['complete_replicates'] or 'none'}**")
    if c["results_tasks_not_in_dataset"]:
        L.append(f"\nWarning: results for tasks not in the dataset (ignored): {c['results_tasks_not_in_dataset']}")
    L.append("\n## Primary analysis (complete replicates only)\n")
    L.append(md_set(summary["primary"]) if summary["primary"] else "_No complete replicate yet._")
    L.append("\n## Sensitivity analysis (all scored trials)\n")
    L.append(md_set(summary["sensitivity"]))
    ia, to = v["infra_attempts"], v["timeouts"]
    L += ["\n## Validity checks\n",
          f"- Infra/parity failed attempts: baseline {ia['baseline']['failed_attempts']} "
          f"({pct(ia['baseline']['rate'])}), forge {ia['forge']['failed_attempts']} ({pct(ia['forge']['rate'])}); "
          f"Fisher p = {fmt(ia['fisher_p'], 4)}, χ² = {fmt(ia['chi2']['chi2'], 2)} (p = {fmt(ia['chi2']['p'], 4)})",
          f"- Trials that exhausted retries: {v['trials_exhausted_retries'] or 0}",
          f"- Parity-error attempts recorded: {v['parity_error_attempts_recorded'] or 0}",
          f"- Timeouts (scored): baseline {to['baseline']['n']}/{to['baseline']['of_scored']}, "
          f"forge {to['forge']['n']}/{to['forge']['of_scored']}; Fisher p = {fmt(to['fisher_p'], 4)}"]
    prim = summary["primary"] or summary["sensitivity"]
    h = prim["hooks"]
    L.append(f"- Forge active in forge arm: {h['forge']['n_active']}/{h['forge']['n_trials']} "
             f"({pct(h['forge']['active_rate'])}); active in baseline (should be 0): {h['baseline']['n_active']}")
    L.append(f"- Ceiling / floor tasks ({prim['label']}): {len(prim['ceiling_tasks'])} / {len(prim['floor_tasks'])}")
    L.append(f"- Repo heterogeneity ({prim['label']}): permutation p = "
             f"{fmt(prim['by_repo']['heterogeneity']['p'], 4)}")
    for a in ARMS:
        L.append(f"- Pins ({a}): " + "; ".join(f"{f}={dict(cnt)}" for f, cnt in v["pins"][a].items()))
    return "\n".join(L) + "\n"


# ============================================================ main

def run(results_dir, dataset_dir, out_dir):
    tasks = load_dataset(dataset_dir)
    trials, running = load_trials(results_dir)
    task_ids = sorted(tasks)
    counts = counts_block(trials, tasks)
    complete = set(counts["complete_replicates"])
    summary = {
        "inputs": {"results_dir": os.path.abspath(results_dir), "dataset_dir": os.path.abspath(dataset_dir),
                   "seed": S.SEED, "n_boot": S.N_BOOT, "n_perm": S.N_PERM, "tost_margin": TOST_MARGIN},
        "counts": counts,
        "running_trials": len(running),
        "primary": analyse_set("primary (complete replicates)", trials, tasks, task_ids, complete)
        if complete else None,
        "sensitivity": analyse_set("sensitivity (all scored trials)", trials, tasks, task_ids, None),
        "validity": validity_block(trials, tasks),
    }
    os.makedirs(out_dir, exist_ok=True)
    with open(os.path.join(out_dir, "summary.json"), "w", encoding="utf-8") as f:
        json.dump(summary, f, indent=2, sort_keys=False)
    with open(os.path.join(out_dir, "tables.md"), "w", encoding="utf-8") as f:
        f.write(render_md(summary))
    return summary


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("results_dir")
    ap.add_argument("dataset_dir")
    ap.add_argument("--out", required=True)
    args = ap.parse_args(argv)
    s = run(args.results_dir, args.dataset_dir, args.out)
    prim = s["primary"] or s["sensitivity"]
    md = prim["paired"]["mean_diff"]
    print(f"[{prim['label']}] diff forge-baseline = {md['est']:+.3f} [{md['lo']:+.3f}, {md['hi']:+.3f}], "
          f"sign-flip p = {prim['paired']['sign_flip']['p']:.4f}; wrote {args.out}/summary.json, tables.md")


if __name__ == "__main__":
    main()
