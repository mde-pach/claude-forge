# Harness interfaces (contract between components)

All code is Python 3.11+ on the host (stdlib + PyYAML only) and drives Docker through the `docker` CLI.
Paths are relative to `experiment/`.

## Host layout
- `/work/repos/<owner>__<name>`: git mirrors, shallow since 2026-04-15. They are host-only and never mounted into an agent container.
- `/work/cache/{uv,pnpm,npm,bun,pip}`: shared package caches. They are filled with network access during warm-up and mounted **read-only** at `/cache/...` in validation and trial containers, which run offline.
- `/work/runs/`: scratch space per trial, deleted after results are copied.

## harness/repos/<repo_id>.yaml (one file per repo; images in harness/images/<repo_id>.Dockerfile, FROM forge-exp/base:v1)
```yaml
pydantic-ai:
  url: https://github.com/pydantic/pydantic-ai
  mirror: /work/repos/pydantic__pydantic-ai
  lang: python            # python | typescript
  image: forge-exp/pydantic-ai:v1
  install: "<sh run in /work as user dev, offline, from /cache>"
  path_prepend: "/work/.venv/bin"      # D5 dev-shell PATH
  test_file_regex: "..."               # which changed files are tests
  test_cmd: "<sh; {junit} = output path, {targets} = space-separated test files or ids>"
  lint_cmd: "<sh; {files}; prints JSON findings>"   # secondary metric only
  env: {KEY: VALUE}                    # extra env, same in both arms
```

## harness/repo_env.py (module API)
```python
load_repos() -> dict[str, dict]
start_container(repo_id, name, *, network="none"|"trialnet"|"bridge", mounts=[(host, ctr, "ro"|"rw")],
                env={}, security_extra=False) -> str          # returns container name; runs `sleep infinity`; user dev uid 1000
materialize(name, repo_id, commit) -> None    # git archive <commit> from mirror -> /work; fresh `git init`; single commit
                                              # author "dev <dev@example.com>", date 2000-01-01T00:00:00Z, message "initial"
install(name, repo_id) -> (ok: bool, log: str)            # offline
write_files(name, files: dict[str, bytes]) -> None        # paths relative to /work, owned by dev
run_tests(name, repo_id, targets: list[str], timeout=900) -> dict[str, str]
          # test_id -> "passed"|"failed"|"error"|"skipped"; ids normalised as "<file>::<name>" (pytest node id style;
          # for vitest: "<file>::<describe> > <test>")
lint(name, repo_id, files) -> list[dict]                  # [{file, line, code, message}]
exec_(name, cmd, user="dev", timeout=None, env=None) -> CompletedProcess
remove(name) -> None
```

## dataset/ (frozen manifest)
- `dataset/manifest.v1.json`: `{version, created, rules_sha256, pins:{...}, tasks:[{task_id, sha256}]}`
- `dataset/tasks/<task_id>/task.json`:
  ```json
  {"task_id":"pydantic-ai__1a2b3c4","repo":"pydantic-ai","commit":"<sha>","parent":"<sha>","date":"...",
   "subject":"...","type":"fix|feat|refactor|other","source_files":[...],"test_files":[...],
   "f2p":[ids],"p2p":[ids],"audit":{"kept":[...],"dropped":[{"id":..,"reason":..}]}}
  ```
- `dataset/tasks/<task_id>/instruction.md`
- `dataset/tasks/<task_id>/hidden/<repo-relative test paths>`: the audited test files, copied over the workspace at scoring
- `dataset/tasks/<task_id>/reference.patch`: the upstream non-test diff, used only for validation

## results/raw/<run_id>/<task_id>/<arm>/r<k>/
`meta.json`, `outcome.json`, `transcript.jsonl`, `debug.log`, `final.diff`, `edits.json`, `claude_calls.jsonl`, `forge/`, `parity.json`

`meta.json`:
```json
{"task_id","arm","rep","attempt","status":"scored|infra_error|parity_error","infra_reason":null,
 "cost_usd","cost_usd_agent","cost_usd_reviewer","num_turns","duration_s","exit_code","timed_out":false,
 "model","claude_version","forge_commit"}
```
`outcome.json`: `{"resolved":bool,"f2p":{"id":"status"},"p2p":{...},"f2p_frac":float,"p2p_regressions":int,"lint_new":int,"diff_lines":int}`
