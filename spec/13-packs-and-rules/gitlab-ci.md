<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `gitlab-ci`

### gitlab-ci

GitLab CI/CD pipelines (.gitlab-ci.yml and the files it includes from .gitlab/ci/).

Detected by: a file matching `.gitlab-ci.yml`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `gitlab-ci/ci-ratchet` | Pipeline changes do not switch checks off: a changed CI file may not have more allow_failure: true, when: manual, or script lines ending in \|\| true / \|\| exit 0 / \|\| : than it had. Fix the failing job instead; switching a check off is the user's decision: ask them. | any: `.gitlab-ci.yml`, `.gitlab/ci/**/*.yml`, `.gitlab/ci/**/*.yaml` | grep no-increase `allow_failure:[[:space:]]*true\|when:[[:space:]]*manual\|\\|\\|[[:space:]]*(true\|exit[[:space:]]+0\|:)([[:space:];)"']\|$)` |
| `gitlab-ci/no-debug-trace` | No committed CI_DEBUG_TRACE or CI_DEBUG_SERVICES set to true: debug tracing prints every variable, masked secrets included, into the job log. Set it for one manual run instead. | any: `.gitlab-ci.yml`, `.gitlab/ci/**/*.yml`, `.gitlab/ci/**/*.yaml` | grep forbid `(CI_DEBUG_TRACE\|CI_DEBUG_SERVICES)["']?[[:space:]]*:[[:space:]]*["']?(true\|1)` |
| `gitlab-ci/pinned-images` | image: and services: entries name a version tag (or a digest), never latest or nothing. | yaml: `.gitlab-ci.yml`, `.gitlab/ci/**/*.yml`, `.gitlab/ci/**/*.yaml` | ast-grep forbid |
| `gitlab-ci/pinned-includes` | include: project: entries name a ref: that is not main, master, HEAD or develop, and CI/CD component: references do not end in @~latest, @main or @master: an unpinned include runs whatever the other project pushes. | yaml: `.gitlab-ci.yml`, `.gitlab/ci/**/*.yml`, `.gitlab/ci/**/*.yaml` | ast-grep forbid |
| `gitlab-ci/rules-not-only-except` | Jobs use rules:, not only: / except:, which GitLab no longer develops. | yaml: `.gitlab-ci.yml`, `.gitlab/ci/**/*.yml`, `.gitlab/ci/**/*.yaml` | ast-grep forbid |
