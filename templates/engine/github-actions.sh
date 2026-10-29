# @module github-actions
# GitHub Actions: zizmor, actionlint, and CI jobs that disappear.

# GitHub Actions workflows: pinned zizmor (through uvx), offline audits only, inline
# ignore comments and ignore rules not honoured (disabling goes through forge.yaml).
FORGE_ZIZMOR_VERSION="@@ZIZMOR_VERSION@@"

# forge_pinned_zizmor <args>: Forge's pinned zizmor through uvx (forge_pinned).
forge_pinned_zizmor() { uvx -q "zizmor@$FORGE_ZIZMOR_VERSION" "$@"; }

# forge_zizmor <audit>...: zizmor on the in-scope files (forge_scope_files: the change, or
# every tracked file of the rule's scope: workflows, actions, dependabot.yml, a pre-commit
# config, as the rule lists them); prints "path:line audit message" for the findings of the
# named audits.
forge_zizmor() {
  set -f
  # shellcheck disable=SC2046
  set -- "$@" -- $(forge_scope_files)
  set +f
  _fz_audits=''
  while [ "$1" != -- ]; do _fz_audits="$_fz_audits|$1"; shift; done
  shift
  [ "$#" -gt 0 ] || return 0
  command -v uvx >/dev/null 2>&1 || { forge_for_agent || printf 'forge:%s skipped: uvx is needed to run zizmor\n' "$c_id"; return 0; }
  forge_do zizmor lint "$@" > "$FORGE_TMP/zizmor" 2> "$FORGE_TMP/zizmor.err"
  _fz_rc=$?
  if [ "$_fz_rc" -ne 0 ] && [ "$_fz_rc" -lt 10 ]; then
    forge_for_agent || printf 'forge:%s skipped: zizmor could not run (%s)\n' "$c_id" "$(grep -v '^[[:space:]]*$' "$FORGE_TMP/zizmor.err" | tail -n 1)"
    return 0
  fi
  awk -v audits="${_fz_audits#|}" '
    BEGIN { n = split(audits, a, "|"); for (i = 1; i <= n; i++) want[a[i]] = 1 }
    /^::(error|warning|notice) / {
      meta = $0; sub(/^::[a-z]+ /, "", meta); msg = meta; sub(/^[^:]*::/, "", msg); sub(/::.*$/, "", meta)
      file = meta; sub(/^.*file=/, "", file); sub(/,.*$/, "", file)
      line = meta; sub(/^.*line=/, "", line); sub(/,.*$/, "", line)
      title = meta; sub(/^.*title=/, "", title); sub(/,.*$/, "", title)
      sub(/^[^:]*:[0-9]+: /, "", msg)
      if (title in want) { print file ":" line " " title ": " msg; bad = 1 }
    }
    END { exit bad }' "$FORGE_TMP/zizmor"
}

# ---- GitHub Actions: actionlint, removed jobs

# forge_actionlint: the pinned actionlint (§13.5) on the in-scope workflows (forge_scope_files),
# "path:line:column: kind: message" per error. Its shellcheck and pyflakes integrations are off
# (results do not depend on what is installed), and it runs with a config of Forge's own that
# copies only the declarative self-hosted-runner and config-variables blocks of the project's
# .github/actionlint.y*ml: the project's paths: … ignore: entries do not apply.
forge_actionlint() {
  forge_scope_files > "$FORGE_TMP/al.files"
  [ -s "$FORGE_TMP/al.files" ] || return 0
  forge_release actionlint _al_bin || return 0
  awk '
    /^[^[:space:]#]/ { keep = ($0 ~ /^(self-hosted-runner|config-variables)[[:space:]]*:/) }
    keep' .github/actionlint.yaml .github/actionlint.yml 2>/dev/null > "$FORGE_TMP/actionlint.yaml"
  set -f
  # shellcheck disable=SC2046
  forge_do actionlint lint "$FORGE_TMP/actionlint.yaml" $(cat "$FORGE_TMP/al.files") > "$FORGE_TMP/al.out" 2>&1
  _al_rc=$?
  set +f
  case $_al_rc in
    0) return 0 ;;
    1) cat "$FORGE_TMP/al.out"; return 1 ;;
    *) forge_for_agent || printf 'forge:%s skipped: actionlint could not run (%s)\n' "$c_id" "$(grep -v '^[[:space:]]*$' "$FORGE_TMP/al.out" | tail -n 1)"; return 0 ;;
  esac
}

# forge_workflow_jobs <file>: "on" when the workflow runs on push or pull_request (on: as a
# scalar, a list or a map), then the ids of its jobs (the keys under the top-level jobs:).
forge_workflow_jobs() {
  awk '
    function trig(s) { if (s ~ /(^|[^A-Za-z_])(push|pull_request)([^A-Za-z_]|$)/) on = 1 }
    { sub(/\r$/, "") }
    /^[^[:space:]#]/ {
      top = $0; sub(/[[:space:]]*:.*$/, "", top); gsub(/["\047]/, "", top)
      sect = top; ind = ""
      if (top == "on" || top == "true") { v = $0; sub(/^[^:]*:/, "", v); sub(/#.*$/, "", v); trig(v); sect = "on" }
      next
    }
    /^[[:space:]]*(#|$)/ { next }
    sect == "on" { l = $0; sub(/#.*$/, "", l); if (l ~ /^[[:space:]]+-/ ) trig(l); else { k = l; sub(/^[[:space:]]+/, "", k); sub(/[[:space:]]*:.*$/, "", k); gsub(/["\047]/, "", k); if (k == "push" || k == "pull_request") on = 1 } ; next }
    sect == "jobs" {
      match($0, /^[[:space:]]+/); w = RLENGTH
      if (ind == "") ind = w
      if (w == ind) { k = $0; sub(/^[[:space:]]+/, "", k); sub(/[[:space:]]*:.*$/, "", k); gsub(/["\047]/, "", k); jobs[++n] = k }
    }
    END { if (on) print "on"; for (i = 1; i <= n; i++) print jobs[i] }' "$1" 2>/dev/null
}

# forge_ci_jobs: for each workflow of the change that existed in the base and ran on push or
# pull_request there, each job of the base that is gone now (the whole file deleted included):
# "path:0 job <id> was removed …", unless forge.yaml accepts that line.
forge_ci_jobs() {
  while IFS= read -r _cj_f; do
    case $_cj_f in .github/workflows/*.yml|.github/workflows/*.yaml) ;; *) continue ;; esac
    [ -f "$FORGE_TMP/base/$_cj_f" ] || continue
    forge_workflow_jobs "$FORGE_TMP/base/$_cj_f" > "$FORGE_TMP/jobs.then"
    [ "$(head -n 1 "$FORGE_TMP/jobs.then")" = on ] || continue
    if [ -f "$_cj_f" ]; then forge_workflow_jobs "$_cj_f" > "$FORGE_TMP/jobs.now"; else : > "$FORGE_TMP/jobs.now"; fi
    sed 1d "$FORGE_TMP/jobs.then" | while IFS= read -r _cj_j; do
      grep -qxF -- "$_cj_j" "$FORGE_TMP/jobs.now" && continue
      printf '%s:0 job %s was removed from a workflow that runs on push or pull_request; restore it rather than switching a check off\n' "$_cj_f" "$_cj_j"
    done
  done < "$FORGE_TMP/changed" | forge_unaccepted
}
