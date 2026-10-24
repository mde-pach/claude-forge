"""Rank eligible repos by distinct non-bot authors of candidate commits in the main window."""
import json, re, subprocess, sys
sys.path.insert(0, '.')
TEST_RE = re.compile(r"(^|/)(tests?|__tests__|testing)/|(^|/)test_[^/]*\.py$|_test\.py$|\.(test|spec)\.(m?[jt]sx?)$")
SRC_RE = re.compile(r"\.(py|ts|tsx|mts|js|mjs)$")
BOT = re.compile(r"bot|\[bot\]|github-actions|renovate|dependabot|stainless|copilot|claude", re.I)
out = {}
for r in sys.argv[1].split(","):
    d = "/work/repos/" + r.replace("/", "__")
    log = subprocess.run(["git","log","--first-parent","--no-merges","--since=2026-07-01","--until=2026-10-15","--format=@@%H\t%ae\t%an","--numstat","HEAD"],cwd=d,capture_output=True,text=True).stdout
    authors=set(); n=0
    for block in log.split("@@")[1:]:
        lines=block.strip().splitlines(); h,ae,an=lines[0].split("\t")
        files=[l.split("\t") for l in lines[1:] if l.count("\t")==2]
        tests=[f for f in files if TEST_RE.search(f[2]) and SRC_RE.search(f[2])]
        src=[f for f in files if SRC_RE.search(f[2]) and not TEST_RE.search(f[2])]
        ls=sum(int(a)+int(b) for a,b,_ in src if a!='-')
        if tests and src and 3<=ls<=300 and len(src)<=8 and not BOT.search(ae+an):
            n+=1; authors.add(ae.lower())
    out[r]={"human_candidates":n,"distinct_authors":len(authors)}
print(json.dumps(out,indent=1))
