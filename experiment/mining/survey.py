"""Survey candidate repos: count commits in a window that touch both tests and source.
Used only to choose repositories (before any trial). Output: survey.json"""
import json, subprocess, sys, re, os
from concurrent.futures import ThreadPoolExecutor
REPOS = sys.argv[1].split(",")
SINCE, UNTIL = "2026-07-01", "2026-10-15"
ROOT = "/work/repos"
TEST_RE = re.compile(r"(^|/)(tests?|__tests__|testing)/|(^|/)test_[^/]*\.py$|_test\.py$|\.(test|spec)\.(m?[jt]sx?)$")
SRC_RE = re.compile(r"\.(py|ts|tsx|mts|js|mjs)$")
def run(*a, cwd=None):
    return subprocess.run(a, cwd=cwd, capture_output=True, text=True).stdout
def survey(r):
    d = f"{ROOT}/{r.replace('/','__')}"
    if not os.path.exists(d):
        subprocess.run(["git","clone","-q","--no-checkout","--shallow-since=2026-04-15",f"https://github.com/{r}",d])
    if not os.path.exists(d): return r,{"commits":0,"candidates":0,"root":[],"note":"no commits since shallow date"}
    out = run("git","log","--first-parent","--no-merges",f"--since={SINCE}",f"--until={UNTIL}","--format=@@%H","--numstat","HEAD",cwd=d)
    n_total=n_ok=0
    for block in out.split("@@")[1:]:
        lines=block.strip().splitlines(); n_total+=1
        files=[l.split("\t") for l in lines[1:] if l.count("\t")==2]
        tests=[f for f in files if TEST_RE.search(f[2]) and SRC_RE.search(f[2])]
        src=[f for f in files if SRC_RE.search(f[2]) and not TEST_RE.search(f[2])]
        lines_src=sum(int(a)+int(b) for a,b,_ in src if a!='-')
        if tests and src and 3<=lines_src<=300 and len(src)<=8: n_ok+=1
    tools=run("git","ls-tree","--name-only","HEAD",cwd=d)
    return r,{"commits":n_total,"candidates":n_ok,"root":tools.split()}
with ThreadPoolExecutor(8) as ex:
    res=dict(ex.map(survey,REPOS))
print(json.dumps(res,indent=1))
