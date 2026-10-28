### gitlab-ci

Detected by `.gitlab-ci.yml`. Rules apply to `.gitlab-ci.yml` and `.gitlab/ci/**/*.y*ml`; ast-grep rules use `scope.language: yaml`.

| Rule | Targets |
|---|---|
| `gitlab-ci/ci-ratchet` | `grep` no-increase: `allow_failure: true`, `when: manual`, a script line ending in `\|\| true` / `\|\| exit 0` / `\|\| :` |
| `gitlab-ci/no-debug-trace` | `grep` forbid: `CI_DEBUG_TRACE` or `CI_DEBUG_SERVICES` set to `true` (masked secrets go to the job log) |
| `gitlab-ci/pinned-images` | `ast-grep` forbid: an `image:` value, `image: name:`, a `services:` entry or its `name:` with `:latest` or no tag or digest; `$VAR` images are not checked |
| `gitlab-ci/rules-not-only-except` | `ast-grep` forbid: `only:` / `except:` in a job (a top-level key other than `variables`, `workflow`, `default`, `include`, `stages`) |
| `gitlab-ci/pinned-includes` | `ast-grep` forbid: under `include`, a `project:` entry without `ref:` or with `ref:` `main`, `master`, `HEAD`, `develop`; a `component:` ending in `@~latest`, `@main` or `@master` |
