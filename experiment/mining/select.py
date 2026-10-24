"""Deterministic candidate selection (DESIGN.md §4 steps 1-3). Fixed before any trial.

usage: python3 select.py <repo_id> <mirror> <window: main|pilot>  -> writes ../dataset/candidates/<repo_id>.<window>.json
"""
import json
import os
import re
import subprocess
import sys

WINDOWS = {"main": ("2026-07-01T00:00:00Z", "2026-10-15T00:00:00Z"),
           "pilot": ("2026-05-01T00:00:00Z", "2026-07-01T00:00:00Z")}
TEST_RE = re.compile(r"(^|/)(tests?|__tests__|testing)/|(^|/)test_[^/]*\.py$|_test\.py$|(^|/)conftest\.py$|\.(test|spec)\.(m?[jt]sx?|cts)$")
SRC_RE = re.compile(r"\.(py|ts|tsx|mts|cts|js|mjs|cjs|jsx)$")
NON_CODE_DIR = re.compile(r"(^|/)(docs?|examples?|\.github|scripts|benchmarks?)/")
DEP_FILES = re.compile(r"(^|/)(uv\.lock|pnpm-lock\.yaml|bun\.lockb?|package-lock\.json|yarn\.lock|pyproject\.toml|package\.json|pnpm-workspace\.yaml|requirements[^/]*\.txt)$")
BOT = re.compile(r"bot|\[bot\]|github-actions|renovate|dependabot|stainless|copilot|claude", re.I)
TYPE_RE = [("fix", re.compile(r"^(fix|bugfix|hotfix)(\(.*?\))?!?:|\bfix(es|ed)?\b", re.I)),
           ("feat", re.compile(r"^(feat|feature)(\(.*?\))?!?:|^add\b|\badd(s|ed)? support\b", re.I)),
           ("refactor", re.compile(r"^(refactor|perf|chore)(\(.*?\))?!?:", re.I))]


def git(mirror, *args):
    return subprocess.run(["git", "-C", mirror, *args], capture_output=True, text=True, check=True).stdout


def task_type(subject):
    for name, rx in TYPE_RE:
        if rx.search(subject):
            return name
    return "other"


def main():
    repo_id, mirror, window = sys.argv[1:4]
    since, until = WINDOWS[window]
    log = git(mirror, "log", "--first-parent", "--no-merges", f"--since={since}", f"--until={until}",
              "--date=iso-strict", "--format=@@%H\t%P\t%cd\t%ae\t%an\t%s", "--numstat", "HEAD")
    out, drops = [], {}
    for block in log.split("@@")[1:]:
        lines = block.strip("\n").splitlines()
        sha, parents, date, ae, an, subject = lines[0].split("\t", 5)
        files = [l.split("\t") for l in lines[1:] if l.count("\t") == 2]
        paths = [f[2] for f in files]

        def drop(reason):
            drops[reason] = drops.get(reason, 0) + 1

        if len(parents.split()) != 1:
            drop("merge"); continue
        if BOT.search(ae + " " + an):
            drop("bot_author"); continue
        if subject.lower().startswith("revert"):
            drop("revert"); continue
        tests = [p for p in paths if TEST_RE.search(p) and SRC_RE.search(p)]
        src = [f for f in files if SRC_RE.search(f[2]) and not TEST_RE.search(f[2]) and not NON_CODE_DIR.search(f[2])]
        if not tests:
            drop("no_test_change"); continue
        if not src:
            drop("no_source_change"); continue
        if any(DEP_FILES.search(p) for p in paths):
            drop("dependency_manifest_or_lock_changed"); continue
        if any(a == "-" for a, _, _ in src):
            drop("binary_source"); continue
        n_lines = sum(int(a) + int(b) for a, b, _ in src)
        if not (3 <= n_lines <= 300):
            drop("source_lines_out_of_range"); continue
        if len(src) > 8:
            drop("too_many_source_files"); continue
        out.append({"repo": repo_id, "commit": sha, "parent": parents, "date": date, "subject": subject,
                    "type": task_type(subject), "test_files": tests, "source_files": [f[2] for f in src],
                    "source_lines": n_lines, "all_files": paths})
    out.sort(key=lambda c: c["date"])  # oldest first
    os.makedirs(os.path.join(os.path.dirname(__file__), "..", "dataset", "candidates"), exist_ok=True)
    dest = os.path.join(os.path.dirname(__file__), "..", "dataset", "candidates", f"{repo_id}.{window}.json")
    with open(dest, "w") as f:
        json.dump({"repo": repo_id, "window": window, "range": [since, until], "drops": drops, "candidates": out}, f, indent=1)
    print(repo_id, window, len(out), drops)


if __name__ == "__main__":
    main()
