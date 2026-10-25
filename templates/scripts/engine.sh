# Check engine (static part). Expects: IFS = newline, FORGE_TMP, FORGE_NCHECKS,
# forge_check_meta_<n>, forge_scope_<n>, forge_when_<n>, forge_check_run_<n>,
# forge_check_changed_<n>, plus tool() and first_lines() from lib.sh.

# forge_change_set: the files a run is about, for $mode (--changed, --since,
# --turn, --all; $FORGE_ARG is the ref or the snapshot directory). Writes to $FORGE_TMP:
#   changed   changed paths, one per line   existing  those that exist now
#   deleted   those gone since the base     inbase    those that exist in the base
#   base/<p>  each one as it is in the base
#   added     "path<TAB>line" per added line, "path<TAB>*" per new file (empty with --all)
# The base is HEAD (--changed, --all), git merge-base <ref> HEAD (--since) or the
# tree at the start of the turn (--turn: FORGE_SNAP, plus FORGE_BASE_REV for files
# that were clean then).
forge_change_set() {
  FORGE_SNAP=''
  case $mode in
    --turn)
      FORGE_SNAP=$FORGE_ARG
      FORGE_BASE_REV=$(cat "$FORGE_SNAP/head" 2>/dev/null) ;;
    --since)
      FORGE_BASE_REV=$(git merge-base "$FORGE_ARG" HEAD 2>/dev/null) || {
        printf 'forge:forge/since no merge base for %s and HEAD (fetch it: git fetch origin <branch>, or check out with fetch-depth: 0)\n' "$FORGE_ARG"
        return 1
      } ;;
    *) FORGE_BASE_REV=$(git rev-parse -q --verify HEAD 2>/dev/null) ;;
  esac
  case $mode in
    --turn)
      { status_paths; cut -f 1 "$FORGE_SNAP/dirty" 2>/dev/null
        [ -n "$FORGE_BASE_REV" ] && git diff --name-only --no-renames "$FORGE_BASE_REV" HEAD 2>/dev/null
      } | sort -u | while IFS= read -r _cs_f; do
        [ -n "$_cs_f" ] && [ "$(now_hash "$_cs_f")" != "$(base_hash "$_cs_f")" ] && printf '%s\n' "$_cs_f"
      done ;;
    --since)
      { git diff --name-only --no-renames "$FORGE_BASE_REV" 2>/dev/null; git ls-files --others --exclude-standard 2>/dev/null; } | sort -u ;;
    *) status_paths | sort -u ;;
  esac > "$FORGE_TMP/changed"
  : > "$FORGE_TMP/existing"; : > "$FORGE_TMP/deleted"; : > "$FORGE_TMP/inbase"; : > "$FORGE_TMP/added"
  mkdir -p "$FORGE_TMP/base"
  while IFS= read -r _cs_f; do
    if base_file "$_cs_f" "$FORGE_TMP/base/$_cs_f"; then printf '%s\n' "$_cs_f" >> "$FORGE_TMP/inbase"; _cs_b=1; else _cs_b=0; fi
    if [ -f "$_cs_f" ]; then
      printf '%s\n' "$_cs_f" >> "$FORGE_TMP/existing"
      [ "$mode" = --all ] || added_lines "$_cs_f" "$FORGE_TMP/base/$_cs_f" >> "$FORGE_TMP/added"
    elif [ "$_cs_b" = 1 ]; then
      printf '%s\n' "$_cs_f" >> "$FORGE_TMP/deleted"
    fi
  done < "$FORGE_TMP/changed"
  return 0
}

# forge_sg_report <json>: ast-grep --json=stream matches as
# "forge:<rule> path:line:column <description>" (from 1); unless $mode is --all, only
# matches that overlap a line of $FORGE_TMP/added (a match spanning an edited line counts).
forge_sg_report() {
  awk -v all="$([ "$mode" = --all ] && printf 1)" '
    function str(s, key,   i, out, c) {
      i = index(s, "\"" key "\":\""); if (!i) return ""
      s = substr(s, i + length(key) + 4); out = ""
      while (length(s)) {
        c = substr(s, 1, 1)
        if (c == "\"") break
        if (c == "\\") { c = substr(s, 2, 1); if (c == "n") c = " "; s = substr(s, 2) }
        out = out c; s = substr(s, 2)
      }
      return out
    }
    function num(s, key,   i) { i = index(s, key); return i ? substr(s, i + length(key)) + 0 : -1 }
    FILENAME == ARGV[1] { if ($2 == "*") whole[$1] = 1; else ad[$1 "\t" $2] = 1; next }
    {
      r = substr($0, index($0, "\"range\":{"))
      sl = num(r, "\"start\":{\"line\":") + 1; sc = num(r, "\"column\":") + 1
      el = num(substr(r, index(r, "\"end\":{\"line\":")), "\"end\":{\"line\":") + 1
      f = str($0, "file"); m = str($0, "message")
      keep = (all != "" || (f in whole))
      for (l = sl; !keep && l <= el; l++) if ((f "\t" l) in ad) keep = 1
      if (!keep) next
      id = m; sub(/^forge:/, "", id); sub(/ .*$/, "", id)
      d = m; sub(/^[^ ]* ?/, "", d)
      printf "forge:%s %s:%d:%d %s\n", id, f, sl, sc, d
    }' "$FORGE_TMP/added" "$1"
}

# forge_grep_counts <dir> <file-list>: "rule<TAB>file<TAB>count" (matching lines)
# for each grep rule that counts (no-increase, no-decrease), on the listed files under <dir>.
forge_grep_counts() {
  _gc_i=1
  while [ "$_gc_i" -le "$FORGE_NGREPS" ]; do
    forge_grep_meta_"$_gc_i"
    if [ "$g_mode" != forbid ]; then
      while IFS= read -r _gc_f; do
        [ -f "$1/$_gc_f" ] && forge_grep_scope_"$_gc_i" "$_gc_f" || continue
        printf '%s\t%s\t%s\n' "$g_id" "$_gc_f" "$(grep -c "$g_flags" -- "$g_pattern" "$1/$_gc_f")"
      done < "$2"
    fi
    _gc_i=$((_gc_i + 1))
  done
}

# forge_grep_forbid <file-list>: "forge:<rule> path:line <description>" for each
# match of a forbid grep rule; unless $mode is --all, only on lines of $FORGE_TMP/added.
forge_grep_forbid() {
  _gf_i=1
  while [ "$_gf_i" -le "$FORGE_NGREPS" ]; do
    forge_grep_meta_"$_gf_i"
    if [ "$g_mode" = forbid ]; then
      while IFS= read -r _gf_f; do
        [ -f "$_gf_f" ] && forge_grep_scope_"$_gf_i" "$_gf_f" || continue
        grep -n "$g_flags" -- "$g_pattern" "$_gf_f" 2>/dev/null | awk -F: -v f="$_gf_f" -v id="$g_id" -v d="$g_desc" -v all="$([ "$mode" = --all ] && printf 1)" '
          FILENAME == ARGV[1] { split($0, a, "\t"); if (a[2] == "*") whole[a[1]] = 1; else ad[a[1] "\t" a[2]] = 1; next }
          all != "" || (f in whole) || ((f "\t" $1) in ad) { printf "forge:%s %s:%s %s\n", id, f, $1, d }' "$FORGE_TMP/added" -
      done < "$1"
    fi
    _gf_i=$((_gf_i + 1))
  done
}

# forge_ratchet <runtime> <script>: the config ratchet. Runs <script> (embedded in
# check.sh as forge_script_<script>) with the project's runtime on the changed files
# and their base copies; each printed line is a loosened setting, unless forge.yaml
# records it as accepted.
forge_ratchet() {
  _fr_fn=forge_script_$(printf '%s' "$2" | tr -c 'A-Za-z0-9_\n' '_')
  "$_fr_fn" > "$FORGE_TMP/$2" 2>/dev/null || { printf 'forge:%s skipped: %s is not in check.sh (run forge apply)\n' "$c_id" "$2"; return 0; }
  set -f
  # shellcheck disable=SC2046
  "$1" "$FORGE_TMP/$2" "$FORGE_TMP/base" $(cat "$FORGE_TMP/existing") > "$FORGE_TMP/ratchet" 2>&1
  _fr_rc=$?
  set +f
  case $_fr_rc in
    0) return 0 ;;
    1) ;;
    *) printf 'forge:%s %s\n' "$c_id" "$(grep '^skipped:' "$FORGE_TMP/ratchet" | head -n 1 | grep . || printf 'skipped: %s exited with %s' "$1" "$_fr_rc")"; return 0 ;;
  esac
  while IFS= read -r _fr_l; do
    [ -f forge.yaml ] && grep -qF -- "$_fr_l" forge.yaml && continue
    printf '%s\n' "$_fr_l"
  done < "$FORGE_TMP/ratchet" > "$FORGE_TMP/ratchet.open"
  [ -s "$FORGE_TMP/ratchet.open" ] || return 0
  cat "$FORGE_TMP/ratchet.open"
  printf 'Restore the setting, or have the user accept the change in forge.yaml: accept: [{rule: %s, change: "<line above>", reason: "..."}]\n' "$c_id"
  return 1
}

# Test blocks, for forge_test_edits: pytest test functions (with their decorators)
# and classes; Vitest/Playwright test and it calls, and the describe calls around them.
FORGE_TEST_BLOCKS='id: forge-test
language: Python
rule:
  any:
    - kind: decorated_definition
      has: {field: definition, kind: function_definition, has: {field: name, regex: "^test"}}
    - all:
        - kind: function_definition
        - has: {field: name, regex: "^test"}
        - not: {inside: {kind: decorated_definition, stopBy: neighbor}}
---
id: forge-test-scope
language: Python
rule: {kind: class_definition}
---
id: forge-test
language: Tsx
rule: {pattern: "$F($NAME, $$$REST)"}
constraints:
  F: {all: [{regex: "^(test|it)([.(]|$)"}, {not: {regex: "^test[.](describe|step|use|extend|info|beforeEach|afterEach|beforeAll|afterAll)"}}]}
  NAME: {any: [{kind: string}, {kind: template_string}]}
---
id: forge-test-scope
language: Tsx
rule: {pattern: "$F($NAME, $$$REST)"}
constraints:
  F: {regex: "^(describe|suite|test[.]describe)([.(]|$)"}
  NAME: {any: [{kind: string}, {kind: template_string}]}'

# forge_test_blocks <dir> <file-list>: "file<TAB>key<TAB>body" per test of the listed
# files under <dir>; key = enclosing classes or describe names, then the test name
# (" #2" for a repeated one); body = the test's text without whitespace.
forge_test_blocks() {
  set -f
  # shellcheck disable=SC2046
  ( cd "$1" && astgrep scan -c "$FORGE_ROOT/.forge/ast-grep/sgconfig.yml" --inline-rules "$FORGE_TEST_BLOCKS" --filter '^forge-test' --json=stream $(cat "$2") ) > "$FORGE_TMP/blocks.json"
  _tb_rc=$?
  set +f
  [ "$_tb_rc" -le 1 ] || return 1
  awk '
    function str(s, key,   i, out, c) {
      i = index(s, "\"" key "\":\""); if (!i) return ""
      s = substr(s, i + length(key) + 4); out = ""
      while (length(s)) {
        c = substr(s, 1, 1)
        if (c == "\"") break
        if (c == "\\") { c = substr(s, 2, 1); if (c == "n" || c == "t" || c == "r") c = " "; s = substr(s, 2) }
        out = out c; s = substr(s, 2)
      }
      return out
    }
    {
      rid = str($0, "ruleId"); if (rid !~ /^forge-test/) next
      r = substr($0, index($0, "\"byteOffset\":{\"start\":") + 22)
      n++; F[n] = str($0, "file"); R[n] = rid; S[n] = r + 0; E[n] = substr(r, index(r, "\"end\":") + 6) + 0
      t = str($0, "text"); k = index($0, "\"NAME\":{"); nm = k ? str(substr($0, k), "text") : ""
      if (nm == "" && match(t, /(def|class)[ ]+[A-Za-z_][A-Za-z0-9_]*/)) { nm = substr(t, RSTART, RLENGTH); sub(/^(def|class)[ ]+/, "", nm) }
      if (nm ~ /^["\047`]/) nm = substr(nm, 2, length(nm) - 2)
      N[n] = nm; gsub(/[[:space:]]/, "", t); T[n] = t
    }
    END {
      for (a = 1; a <= n; a++) {
        if (R[a] != "forge-test") continue
        m = 0
        for (b = 1; b <= n; b++)
          if (R[b] == "forge-test-scope" && F[b] == F[a] && S[b] <= S[a] && E[a] <= E[b]) {
            m++; cs[m] = S[b]; cn[m] = N[b]
            for (j = m; j > 1 && cs[j - 1] > cs[j]; j--) { x = cs[j]; cs[j] = cs[j - 1]; cs[j - 1] = x; x = cn[j]; cn[j] = cn[j - 1]; cn[j - 1] = x }
          }
        key = ""
        for (j = 1; j <= m; j++) key = key cn[j] " > "
        key = key N[a]
        if (++seen[F[a] "\t" key] > 1) key = key " #" seen[F[a] "\t" key]
        print F[a] "\t" key "\t" T[a]
      }
    }' "$FORGE_TMP/blocks.json"
}

# forge_test_edits: Stop only (--turn). Each test of the in-scope files ($FORGE_FILES)
# that existed at the start of the turn must be unchanged (whitespace aside) and
# still present; new tests are fine.
forge_test_edits() {
  [ "$mode" = --turn ] || return 0
  printf '%s\n' "$FORGE_FILES" | grep -xF -f "$FORGE_TMP/inbase" > "$FORGE_TMP/tests.files"
  [ -s "$FORGE_TMP/tests.files" ] || return 0
  if ! forge_test_blocks "$FORGE_TMP/base" "$FORGE_TMP/tests.files" > "$FORGE_TMP/tests.then" \
    || ! forge_test_blocks "$FORGE_ROOT" "$FORGE_TMP/tests.files" > "$FORGE_TMP/tests.now"; then
    printf 'forge:%s skipped: ast-grep unavailable\n' "$c_id"
    return 0
  fi
  awk -F '\t' '
    FILENAME == ARGV[1] { then[$1 "\t" $2] = $3; next }
    { now[$1 "\t" $2] = $3 }
    END {
      for (k in then) {
        split(k, p, "\t")
        if (!(k in now)) print p[1] ": test \"" p[2] "\" was removed"
        else if (now[k] != then[k]) print p[1] ": test \"" p[2] "\" was changed"
      }
    }' "$FORGE_TMP/tests.then" "$FORGE_TMP/tests.now" | sort > "$FORGE_TMP/tests.edits"
  [ -s "$FORGE_TMP/tests.edits" ] || return 0
  cat "$FORGE_TMP/tests.edits"
  printf 'Tests that existed before this turn are not edited to make the code pass. Restore them; if one looks wrong, stop and ask the user.\n'
  return 1
}

# forge_toml_get <file> <section> <key>: the value of <key> in [<section>] ("" for
# the top level), quotes and comments removed; nothing when absent. One-line values only.
forge_toml_get() {
  awk -v want="$2" -v key="$3" '
    /^[[:space:]]*\[/ { s = $0; sub(/^[[:space:]]*\[[[:space:]]*/, "", s); sub(/[[:space:]]*\].*$/, "", s); next }
    s == want {
      line = $0; sub(/^[[:space:]]*/, "", line)
      if (substr(line, 1, length(key)) != key) next
      rest = substr(line, length(key) + 1)
      if (rest !~ /^[[:space:]]*=/) next
      sub(/^[[:space:]]*=[[:space:]]*/, "", rest)
      if (rest ~ /^"/) { sub(/^"/, "", rest); sub(/".*$/, "", rest) }
      else if (rest ~ /^\047/) { sub(/^\047/, "", rest); sub(/\047.*$/, "", rest) }
      else sub(/[[:space:]]*(#.*)?$/, "", rest)
      print rest; exit
    }' "$1" 2>/dev/null
}

# forge_duration_seconds <text>: a relative duration ("7 days", "1 week", "72 hours",
# P7D, P1W, PT72H) in seconds; nothing when it is not one.
forge_duration_seconds() {
  printf '%s\n' "$1" | awk '{
    v = tolower($0)
    if (v ~ /^p[0-9]+w$/) print substr(v, 2) * 604800
    else if (v ~ /^p[0-9]+d$/) print substr(v, 2) * 86400
    else if (v ~ /^pt[0-9]+h$/) print substr(v, 3) * 3600
    else if (v ~ /^[0-9]+[[:space:]]*(weeks?|days?|hours?)$/) {
      n = v + 0; u = v; sub(/^[0-9]+[[:space:]]*/, "", u)
      print n * (u ~ /^w/ ? 604800 : u ~ /^d/ ? 86400 : 3600)
    }
  }'
}

# forge_version_ge <a> <b>: succeeds when version a >= b (numeric dot-separated parts).
forge_version_ge() {
  awk -v a="$1" -v b="$2" 'BEGIN {
    na = split(a, x, "."); nb = split(b, y, ".")
    for (i = 1; i <= (na > nb ? na : nb); i++) { p = x[i] + 0; q = y[i] + 0; if (p > q) exit 0; if (p < q) exit 1 }
    exit 0 }'
}

# forge_bun_cooldown: bunfig.toml [install] minimumReleaseAge is at least 3 days (259200 s).
forge_bun_cooldown() {
  _bc_v=$(forge_toml_get bunfig.toml install minimumReleaseAge)
  case $_bc_v in ''|*[!0-9]*) ;; *) [ "$_bc_v" -ge 259200 ] && return 0 ;; esac
  printf 'bunfig.toml: [install] minimumReleaseAge is %s; set it to at least 259200 (3 days, in seconds) so a freshly published (possibly compromised) version is not installed. Packages you must update sooner go in minimumReleaseAgeExcludes.\n' "${_bc_v:-unset}"
  return 1
}

# forge_uv_cooldown: [tool.uv] (or uv.toml) exclude-newer is a relative duration of at
# least 3 days, and required-version asks for uv >= 0.9.17 (older uv ignores relative values).
forge_uv_cooldown() {
  if [ -f uv.toml ]; then _uc_f=uv.toml; _uc_s=''; else _uc_f=pyproject.toml; _uc_s=tool.uv; fi
  _uc_rc=0
  _uc_v=$(forge_toml_get "$_uc_f" "$_uc_s" exclude-newer)
  _uc_n=$(forge_duration_seconds "$_uc_v")
  if [ -z "$_uc_n" ] || [ "$_uc_n" -lt 259200 ]; then
    printf '%s: [%s] exclude-newer is %s; set a relative duration of at least 3 days, e.g. exclude-newer = "7 days", so a freshly published (possibly compromised) version is not resolved.\n' "$_uc_f" "${_uc_s:-top level}" "${_uc_v:-unset}"
    _uc_rc=1
  fi
  _uc_r=$(forge_toml_get "$_uc_f" "$_uc_s" required-version)
  _uc_min=$(printf '%s' "$_uc_r" | sed -n -E 's/^[[:space:]]*(>=|==|~=|>)[[:space:]]*([0-9][0-9.]*).*$/\2/p')
  if [ -z "$_uc_min" ] || ! forge_version_ge "$_uc_min" 0.9.17; then
    printf '%s: [%s] required-version is %s; set required-version = ">=0.9.17" (older uv ignores a relative exclude-newer).\n' "$_uc_f" "${_uc_s:-top level}" "${_uc_r:-unset}"
    _uc_rc=1
  fi
  return "$_uc_rc"
}

# forge_trusted_deps: no package.json gains trustedDependencies entries (each one
# lets that package run install scripts); compared with the base, with bun.
forge_trusted_deps() {
  _td_rc=0
  while IFS= read -r _td_f; do
    case $_td_f in package.json|*/package.json) ;; *) continue ;; esac
    _td_new=$(bun -e '
      const fs = require("fs");
      const read = (p) => { try { const v = JSON.parse(fs.readFileSync(p, "utf8")).trustedDependencies; return Array.isArray(v) ? v : []; } catch { return []; } };
      const before = new Set(read(process.argv[1]));
      console.log(read(process.argv[2]).filter((d) => !before.has(d)).join(" "));' "$FORGE_TMP/base/$_td_f" "$_td_f") || continue
    if [ -n "$_td_new" ]; then
      printf '%s: trustedDependencies adds %s (its install scripts would run); the user adds trusted packages, not Claude\n' "$_td_f" "$_td_new"
      _td_rc=1
    fi
  done < "$FORGE_TMP/existing"
  return "$_td_rc"
}

# GitHub Actions workflows: pinned zizmor (through uvx), offline audits only, inline
# ignore comments and ignore rules not honoured (disabling goes through forge.yaml).
FORGE_ZIZMOR_VERSION="@@ZIZMOR_VERSION@@"

# forge_zizmor <audit>...: zizmor on $FORGE_FILES (else every tracked workflow and
# action); prints "path:line audit message" for the findings of the named audits.
forge_zizmor() {
  if [ -n "$FORGE_FILES" ]; then set -f; set -- "$@" -- $FORGE_FILES; set +f
  else
    set -f
    # shellcheck disable=SC2046
    set -- "$@" -- $(git ls-files -- '.github/workflows/*.yml' '.github/workflows/*.yaml' '.github/actions/*action.yml' '.github/actions/*action.yaml' 'action.yml' 'action.yaml')
    set +f
  fi
  _fz_audits=''
  while [ "$1" != -- ]; do _fz_audits="$_fz_audits|$1"; shift; done
  shift
  [ "$#" -gt 0 ] || return 0
  command -v uvx >/dev/null 2>&1 || { printf 'forge:%s skipped: uvx is needed to run zizmor\n' "$c_id"; return 0; }
  uvx -q "zizmor@$FORGE_ZIZMOR_VERSION" --offline --no-progress --no-ignores --format github "$@" > "$FORGE_TMP/zizmor" 2> "$FORGE_TMP/zizmor.err"
  _fz_rc=$?
  if [ "$_fz_rc" -ne 0 ] && [ "$_fz_rc" -lt 10 ]; then
    printf 'forge:%s skipped: zizmor could not run (%s)\n' "$c_id" "$(grep -v '^[[:space:]]*$' "$FORGE_TMP/zizmor.err" | tail -n 1)"
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

# Dockerfiles: pinned hadolint (hadolint-bin through uvx; its binary must match the
# upstream release's sha256), run once per check.sh run and shared by the rules.
FORGE_HADOLINT_VERSION="@@HADOLINT_VERSION@@"
FORGE_HADOLINT_SHA256='@@HADOLINT_SHAS@@'

# forge_dockerfiles: the Dockerfiles / Containerfiles of $FORGE_FILES, else every tracked one.
forge_dockerfiles() {
  if [ -n "$FORGE_FILES" ]; then printf '%s\n' "$FORGE_FILES"; else git ls-files 2>/dev/null; fi \
    | grep -E '(^|/)(Dockerfile(\.[^/]*)?|[^/]*\.Dockerfile|Containerfile)$' | grep -v '\.dockerignore$'
}

# forge_hadolint <code>...: "path:line CODE: message" for the named hadolint codes.
forge_hadolint() {
  forge_dockerfiles > "$FORGE_TMP/dockerfiles"
  [ -s "$FORGE_TMP/dockerfiles" ] || return 0
  if ! cmp -s "$FORGE_TMP/dockerfiles" "$FORGE_TMP/hadolint.files" 2>/dev/null; then
    cp "$FORGE_TMP/dockerfiles" "$FORGE_TMP/hadolint.files"
    : > "$FORGE_TMP/hadolint.out"
    if ! command -v uvx >/dev/null 2>&1; then
      printf 'skipped: uvx is needed to run hadolint\n' > "$FORGE_TMP/hadolint.skip"
    elif ! _fh_bin=$(uvx -q --from "hadolint-bin==$FORGE_HADOLINT_VERSION" python -c 'import shutil; print(shutil.which("hadolint") or "")' 2>/dev/null) || [ -z "$_fh_bin" ]; then
      printf 'skipped: hadolint could not be installed (offline?)\n' > "$FORGE_TMP/hadolint.skip"
    else
      _fh_plat=$(uname -s)-$(uname -m)
      _fh_want=$(printf '%s\n' "$FORGE_HADOLINT_SHA256" | awk -v p="$_fh_plat" '$1 == p { print $2 }')
      _fh_have=$(forge_sha256 < "$_fh_bin")
      if [ -z "$_fh_want" ]; then
        printf 'skipped: no upstream hadolint checksum for %s\n' "$_fh_plat" > "$FORGE_TMP/hadolint.skip"
      elif [ "$_fh_have" != "$_fh_want" ]; then
        printf 'skipped: the hadolint-bin %s binary does not match the upstream release (sha256 %s)\n' "$FORGE_HADOLINT_VERSION" "$_fh_have" > "$FORGE_TMP/hadolint.skip"
      else
        rm -f "$FORGE_TMP/hadolint.skip"
        printf 'ignored: []\n' > "$FORGE_TMP/hadolint.yaml"
        set -f
        # shellcheck disable=SC2046
        "$_fh_bin" -c "$FORGE_TMP/hadolint.yaml" --disable-ignore-pragma --no-fail --no-color -f tty $(cat "$FORGE_TMP/dockerfiles") > "$FORGE_TMP/hadolint.out" 2>&1
        set +f
      fi
    fi
  fi
  if [ -f "$FORGE_TMP/hadolint.skip" ]; then printf 'forge:%s %s\n' "$c_id" "$(cat "$FORGE_TMP/hadolint.skip")"; return 0; fi
  _fh_codes=' '
  for _fh_c in "$@"; do _fh_codes="$_fh_codes$_fh_c "; done
  awk -v codes="$_fh_codes" '
    match($0, /^[^[:space:]]+:[0-9]+ [A-Z]+[0-9]+ [a-z]+: /) {
      split($0, w, " "); if (index(codes, " " w[2] " ") == 0) next
      msg = $0; sub(/^[^[:space:]]+ [^[:space:]]+ [a-z]+: /, "", msg)
      print w[1] " " w[2] ": " msg; bad = 1
    }
    END { exit bad }' "$FORGE_TMP/hadolint.out"
}

# forge_dockerfile_records <file>: one "line<TAB>instruction" per instruction,
# continuation lines joined, comments dropped.
forge_dockerfile_records() {
  awk '
    function flush() { if (cur != "") print start "\t" cur; cur = "" }
    {
      line = $0; sub(/\r$/, "", line)
      if (cur == "") { if (line ~ /^[[:space:]]*(#|$)/) next; start = NR }
      else if (line ~ /^[[:space:]]*#/) next
      if (match(line, /\\[[:space:]]*$/)) { cur = cur substr(line, 1, RSTART - 1) " "; next }
      cur = cur line; flush()
    }
    END { flush() }' "$1"
}

# forge_docker_nonroot: the final stage of each Dockerfile runs as a non-root USER,
# set in that stage or inherited from the named stage it builds on; images that run
# non-root by default (cgr.dev/, dhi.io/, *nonroot*, *unprivileged*, *rootless*) are exempt.
forge_docker_nonroot() {
  _dn_rc=0
  forge_dockerfiles > "$FORGE_TMP/dockerfiles"
  while IFS= read -r _dn_f; do
    [ -f "$_dn_f" ] || continue
    forge_dockerfile_records "$_dn_f" | awk -F '\t' -v f="$_dn_f" '
      {
        args = $2; sub(/^[[:space:]]*[A-Za-z]+[[:space:]]*/, "", args); instr = toupper($2); sub(/[[:space:]].*$/, "", instr)
        if (instr == "FROM") {
          n++; split(args, w, /[[:space:]]+/); i = 1; while (w[i] ~ /^--/) i++
          img[n] = w[i]; name[n] = ""; user[n] = ""
          if (tolower(w[i + 1]) == "as") name[n] = tolower(w[i + 2])
        } else if (instr == "USER" && n) { split(args, w, /[[:space:]]+/); user[n] = w[1] }
      }
      END {
        if (!n) exit 0
        s = n; u = user[s]; b = img[s]
        for (guard = 0; u == "" && guard < 50; guard++) {
          k = 0; for (j = 1; j < s; j++) if (name[j] != "" && name[j] == tolower(b)) k = j
          if (!k) break
          s = k; u = user[k]; b = img[k]
        }
        if (u == "") {
          if (b ~ /^(cgr\.dev|dhi\.io)\// || tolower(b) ~ /nonroot|unprivileged|rootless/) exit 0
          print f ":0 the final stage has no USER (it runs as root); add a non-root USER"; exit 1
        }
        if (u ~ /^(root|0)(:.*)?$/) { print f ":0 the final stage runs as USER " u "; use a non-root user"; exit 1 }
      }' || _dn_rc=1
  done < "$FORGE_TMP/dockerfiles"
  return "$_dn_rc"
}

# forge_docker_secrets: no ARG or ENV named like a secret (PASSWORD, SECRET, TOKEN,
# API_KEY, PRIVATE_KEY, CREDENTIALS); *_FILE names are fine.
forge_docker_secrets() {
  _ds_rc=0
  forge_dockerfiles > "$FORGE_TMP/dockerfiles"
  while IFS= read -r _ds_f; do
    [ -f "$_ds_f" ] || continue
    forge_dockerfile_records "$_ds_f" | awk -F '\t' -v f="$_ds_f" '
      {
        instr = toupper($2); sub(/[[:space:]].*$/, "", instr)
        if (instr != "ARG" && instr != "ENV") next
        args = $2; sub(/^[[:space:]]*[A-Za-z]+[[:space:]]*/, "", args)
        n = split(args, w, /[[:space:]]+/)
        for (i = 1; i <= n; i++) {
          name = w[i]; sub(/=.*$/, "", name)
          if (instr == "ENV" && i > 1 && w[1] !~ /=/) break
          if (toupper(name) ~ /(PASSWORD|SECRET|TOKEN|API_?KEY|PRIVATE_KEY|CREDENTIALS)/ && toupper(name) !~ /_FILE$/) {
            print f ":" $1 " " instr " " name " puts a secret in the image or its history; use RUN --mount=type=secret,id=..."; bad = 1
          }
          if (instr == "ARG") break
        }
      }
      END { exit bad }' || _ds_rc=1
  done < "$FORGE_TMP/dockerfiles"
  return "$_ds_rc"
}

# forge_docker_locked: RUN installs from the lock file (npm ci, bun install
# --frozen-lockfile, uv sync --locked or --frozen, pnpm install --frozen-lockfile,
# yarn --immutable or --frozen-lockfile) and never adds packages (bun add, uv add).
forge_docker_locked() {
  _dl_rc=0
  forge_dockerfiles > "$FORGE_TMP/dockerfiles"
  while IFS= read -r _dl_f; do
    [ -f "$_dl_f" ] || continue
    forge_dockerfile_records "$_dl_f" | awk -F '\t' -v f="$_dl_f" '
      function why(c,   t) {
        t = " " c " "
        if (c ~ /^npm (install|i|add)( |$)/ && t !~ / (-g|--global) /) return "use npm ci"
        if (c ~ /^bun (install|i)( |$)/ && t !~ / --frozen-lockfile /) return "add --frozen-lockfile"
        if (c ~ /^bun add( |$)/) return "add dependencies to package.json and bun.lock, not in the image"
        if (c ~ /^uv sync( |$)/ && t !~ / (--locked|--frozen) /) return "add --locked (or --frozen)"
        if (c ~ /^uv add( |$)/) return "add dependencies to pyproject.toml and uv.lock, not in the image"
        if (c ~ /^pnpm (install|i)( |$)/ && t !~ / --frozen-lockfile /) return "add --frozen-lockfile"
        if (c ~ /^yarn( install)?( -[^ ]*)*$/ && t !~ / (--immutable|--frozen-lockfile) /) return "add --immutable (Yarn 2+) or --frozen-lockfile"
        return ""
      }
      {
        instr = toupper($2); sub(/[[:space:]].*$/, "", instr)
        if (instr != "RUN") next
        args = $2; sub(/^[[:space:]]*[A-Za-z]+[[:space:]]*/, "", args)
        while (args ~ /^--[^[:space:]]+[[:space:]]/) sub(/^--[^[:space:]]+[[:space:]]+/, "", args)
        n = split(args, cmds, /&&|\|\||;/)
        for (i = 1; i <= n; i++) {
          c = cmds[i]; gsub(/[[:space:]]+/, " ", c); sub(/^ /, "", c); sub(/ $/, "", c)
          while (c ~ /^([A-Za-z_][A-Za-z0-9_]*=[^ ]* |sudo )/) sub(/^[^ ]+ /, "", c)
          r = why(c)
          if (r != "") { print f ":" $1 " " c ": " r; bad = 1 }
        }
      }
      END { exit bad }' || _dl_rc=1
  done < "$FORGE_TMP/dockerfiles"
  return "$_dl_rc"
}

# forge_docker_ignore: each Dockerfile has a .dockerignore (<Dockerfile>.dockerignore,
# the one in its directory, or the one at the repository root) that excludes .env files.
forge_docker_ignore() {
  _di_rc=0
  forge_dockerfiles > "$FORGE_TMP/dockerfiles"
  while IFS= read -r _di_f; do
    [ -f "$_di_f" ] || continue
    _di_d=$(dirname "$_di_f")
    _di_ig=''
    [ "$_di_d" = . ] && _di_d='' || _di_d="$_di_d/"
    for _di_c in "$_di_f.dockerignore" "$_di_d.dockerignore" .dockerignore; do
      [ -f "$_di_c" ] && { _di_ig=$_di_c; break; }
    done
    if [ -z "$_di_ig" ]; then
      printf '%s:0 has no .dockerignore (next to it or at the root); add one that excludes .env files\n' "$_di_f"; _di_rc=1
    elif ! grep -qE '^[[:space:]]*((\*\*/)?\.env(\*|\.\*)?|\*\.env|\*|\*\*)[[:space:]]*$' "$_di_ig"; then
      printf '%s:0 %s does not exclude .env files; add .env* to it\n' "$_di_f" "$_di_ig"; _di_rc=1
    fi
  done < "$FORGE_TMP/dockerfiles"
  return "$_di_rc"
}

# forge_exec_tool <name> <args>: runs the project's own <name>; 127 when absent.
forge_exec_tool() {
  _fx_cmd=$(tool "$1")
  [ -n "$_fx_cmd" ] || { printf 'forge: %s not installed\n' "$1" >&2; return 127; }
  shift
  _fx_ifs=$IFS
  IFS=' '
  # shellcheck disable=SC2086
  set -- $_fx_cmd "$@"
  IFS=$_fx_ifs
  "$@"
}

# forge_has_tool <name>: system runtimes (uv, bun) via PATH, others via tool().
forge_has_tool() {
  case $1 in
    uv|uvx|bun|docker) command -v "$1" >/dev/null 2>&1 ;;
    *) [ -n "$(tool "$1")" ] ;;
  esac
}

# forge_missing_tool: prints the first tool of $c_tools that is absent.
forge_missing_tool() {
  _fm_ifs=$IFS
  IFS=' '
  for _fm_t in $c_tools; do
    if ! forge_has_tool "$_fm_t"; then IFS=$_fm_ifs; printf '%s' "$_fm_t"; return 0; fi
  done
  IFS=$_fm_ifs
  return 1
}

# forge_filter <predicate>: lines of stdin for which <predicate> <line> succeeds.
forge_filter() {
  while read -r _ff_line; do
    [ -n "$_ff_line" ] && "$1" "$_ff_line" && printf '%s\n' "$_ff_line"
  done
  return 0
}

# forge_db_url: DATABASE_URL from the environment, then .env, then alembic.ini.
forge_db_url() {
  if [ -n "${DATABASE_URL:-}" ]; then printf '%s' "$DATABASE_URL"; return 0; fi
  if [ -f .env ]; then
    _fd_url=$(sed -n 's/^[[:space:]]*\(export[[:space:]][[:space:]]*\)\{0,1\}DATABASE_URL[[:space:]]*=[[:space:]]*//p' .env \
      | tail -n 1 | sed -e 's/[[:space:]]*$//' -e 's/^"\(.*\)"$/\1/' -e "s/^'\(.*\)'\$/\1/")
    if [ -n "$_fd_url" ]; then printf '%s' "$_fd_url"; return 0; fi
  fi
  if [ -f alembic.ini ]; then
    sed -n 's/^[[:space:]]*sqlalchemy\.url[[:space:]]*=[[:space:]]*//p' alembic.ini | head -n 1 | sed 's/[[:space:]]*$//'
  fi
}

# forge_local_db: succeeds when the configured database is on this machine
# (localhost, 127.0.0.1, ::1, empty host or a Unix socket).
forge_local_db() {
  _fl_url=$(forge_db_url)
  [ -n "$_fl_url" ] || return 1
  case $_fl_url in *host=/*|*host=%2[Ff]*) return 0 ;; esac
  _fl_host=$(printf '%s' "$_fl_url" | sed -e 's|^[A-Za-z][A-Za-z0-9+.-]*://||' -e 's|[/?].*$||' -e 's|^.*@||')
  case $_fl_host in
    \[*) _fl_host=$(printf '%s' "$_fl_host" | sed 's|^\[\([^]]*\)\].*$|\1|') ;;
    *) _fl_host=$(printf '%s' "$_fl_host" | sed 's|:.*$||') ;;
  esac
  case $_fl_host in ''|localhost|127.0.0.1|::1|%2[Ff]*) return 0 ;; esac
  return 1
}

# Migration linting with Squawk: only the changes that break the running version of
# the application during a deploy fail (renames, type changes, new required columns,
# DROP DATABASE); Squawk's other advice is not enforced.
FORGE_SQUAWK_VERSION="@@SQUAWK_VERSION@@"
FORGE_SQUAWK_BLOCKING='renaming-column|renaming-table|changing-column-type|adding-required-field|adding-not-nullable-field|ban-drop-database'

# forge_squawk <args>: the pinned Squawk through bunx or uvx; gcc-style output in
# $FORGE_TMP/squawk. Exit status 0 when Squawk ran (with or without findings).
forge_squawk() {
  if command -v bunx >/dev/null 2>&1; then
    bunx -p "squawk-cli@$FORGE_SQUAWK_VERSION" squawk --reporter gcc "$@" > "$FORGE_TMP/squawk" 2>&1
  elif command -v uvx >/dev/null 2>&1; then
    uvx -q --from "squawk-cli==$FORGE_SQUAWK_VERSION" squawk --reporter gcc "$@" > "$FORGE_TMP/squawk" 2>&1
  else
    printf 'bunx or uvx is needed\n' > "$FORGE_TMP/squawk"
    return 2
  fi
  _sq_rc=$?
  # Squawk exits 1 whenever it warns, which is almost always
  [ "$_sq_rc" -eq 0 ] || grep -qE ':[0-9]+:[0-9]+: (warning|error): ' "$FORGE_TMP/squawk"
}

# forge_squawk_blocking: prints the blocking findings of the last forge_squawk run
# (Squawk counts lines and columns from 0; printed from 1); fails if any.
forge_squawk_blocking() {
  grep -E ": (warning|error): ($FORGE_SQUAWK_BLOCKING) " "$FORGE_TMP/squawk" \
    | sed -E 's/^(.*):([0-9]+):([0-9]+): /\1 \2 \3 /' \
    | awk '{ f = $1; l = $2 + 1; c = $3 + 1; $1 = $2 = $3 = ""; sub(/^ +/, ""); print f ":" l ":" c ": " $0 }' > "$FORGE_TMP/squawk.found"
  cat "$FORGE_TMP/squawk.found"
  [ ! -s "$FORGE_TMP/squawk.found" ]
}

# forge_squawk_skipped: reports, as a forge: line of the current check, that Squawk could not run.
forge_squawk_skipped() {
  printf 'forge:%s skipped: squawk could not run (%s)\n' "$c_id" "$(grep -v '^[[:space:]]*$' "$FORGE_TMP/squawk" | tail -n 1)"
}

# forge_squawk_files <sql files>: lints migration SQL files.
forge_squawk_files() {
  [ "$#" -gt 0 ] || return 0
  forge_squawk "$@" || { forge_squawk_skipped; return 0; }
  forge_squawk_blocking
}

# forge_alembic_files: Alembic revision ids on stdin (as printed by `alembic heads`)
# -> the revision files under alembic/versions that define them.
forge_alembic_files() {
  awk '{ print $1 }' | while read -r _fa_id; do
    [ -n "$_fa_id" ] && find alembic/versions -name '*.py' -exec grep -lE "^revision[^=]*=[[:space:]]*['\"]$_fa_id['\"]" {} + 2>/dev/null
  done
}

# forge_squawk_alembic <revision files>: lints the SQL each revision's upgrade emits
# (`alembic upgrade <down_revision>:<revision> --sql`, offline: no database needed).
# Merge revisions are skipped; when Alembic cannot render SQL, the revision is reported skipped.
forge_squawk_alembic() {
  _fs_rc=0
  for _fs_f in "$@"; do
    _fs_rev=$(sed -n "s/^revision[^=]*=[[:space:]]*['\"]\([^'\"]*\)['\"].*/\1/p" "$_fs_f" | head -n 1)
    _fs_down=$(sed -n 's/^down_revision[^=]*=[[:space:]]*//p' "$_fs_f" | head -n 1)
    [ -n "$_fs_rev" ] || continue
    case $_fs_down in
      \(*|\[*) continue ;;
      None*|'') _fs_range=$_fs_rev ;;
      *) _fs_range="$(printf '%s' "$_fs_down" | sed "s/^['\"]\([^'\"]*\)['\"].*/\1/"):$_fs_rev" ;;
    esac
    if ! alembic upgrade "$_fs_range" --sql > "$FORGE_TMP/alembic.sql" 2> "$FORGE_TMP/alembic.err"; then
      printf 'forge:%s skipped: alembic upgrade %s --sql failed for %s (%s)\n' "$c_id" "$_fs_range" "$_fs_f" "$(grep -v '^[[:space:]]*$' "$FORGE_TMP/alembic.err" | tail -n 1)"
      continue
    fi
    if ! forge_squawk --stdin-filepath "$_fs_f" < "$FORGE_TMP/alembic.sql"; then forge_squawk_skipped; return 0; fi
    if ! forge_squawk_blocking > "$FORGE_TMP/squawk.block"; then
      printf 'SQL of alembic upgrade %s --sql (line numbers are in that SQL):\n' "$_fs_range"
      cat "$FORGE_TMP/squawk.block"
      _fs_rc=1
    fi
  done
  return "$_fs_rc"
}

# ---- How applications use SQLAlchemy and Alembic (python/sqlalchemy/alembic rules).
# Each prints "path:line: message" (lint findings, attributed to added lines).

# forge_alembic_targets: the revision files to examine: the in-scope changed ones
# ($FORGE_FILES), else (--all) the head revisions' (no file names them as a
# down_revision), so revisions written before a rule was adopted do not fail CI.
forge_alembic_targets() {
  if [ -n "$FORGE_FILES" ]; then
    printf '%s\n' "$FORGE_FILES" | grep -E '^alembic/versions/.*\.py$'
    return 0
  fi
  find alembic/versions -name '*.py' 2>/dev/null | sort > "$FORGE_TMP/alembic.revs"
  [ -s "$FORGE_TMP/alembic.revs" ] || return 0
  # shellcheck disable=SC2046
  awk '
    function ids(s,   out) {
      out = ""
      while (match(s, /["\047][^"\047]+["\047]/)) { out = out " " substr(s, RSTART + 1, RLENGTH - 2); s = substr(s, RSTART + RLENGTH) }
      return out
    }
    /^revision[^=]*=/ { r = $0; sub(/^[^=]*=/, "", r); split(ids(r), a, " "); if (a[1] != "") rev[FILENAME] = a[1] }
    /^down_revision[^=]*=/ { d = $0; sub(/^[^=]*=/, "", d); n = split(ids(d), a, " "); for (i = 1; i <= n; i++) down[a[i]] = 1 }
    END { for (f in rev) if (!(rev[f] in down)) print f }' $(cat "$FORGE_TMP/alembic.revs") | sort
}

# forge_alembic_sqlite: the project also runs its migrations on SQLite: alembic/env.py
# sets render_as_batch=True, or alembic.ini's sqlalchemy.url is a sqlite URL.
forge_alembic_sqlite() {
  grep -qE '^[^#]*render_as_batch[[:space:]]*=[[:space:]]*True' alembic/env.py 2>/dev/null \
    || grep -qE '^[[:space:]]*sqlalchemy\.url[[:space:]]*=[[:space:]]*sqlite' alembic.ini 2>/dev/null
}

# forge_alembic_batch: on such a project, operations SQLite cannot run as ALTER TABLE
# (alter_column, drop_column, constraints) go through op.batch_alter_table(), which
# recreates the table there. Operations under an `if` on the dialect are left alone.
forge_alembic_batch() {
  forge_alembic_sqlite || return 0
  _ab_rc=0
  forge_alembic_targets > "$FORGE_TMP/alembic.targets"
  while IFS= read -r _ab_f; do
    [ -f "$_ab_f" ] || continue
    awk -v f="$_ab_f" '
      { line = $0; sub(/\r$/, "", line) }
      line ~ /^[ \t]*(#|$)/ { next }
      {
        ind = match(line, /[^ \t]/) - 1
        if (skip != "") {
          if (ind > skip + 0) next
          if (ind == skip + 0 && line ~ /^[ \t]*(elif|else)[ \t:]/) next
          skip = ""
        }
        if (line ~ /^[ \t]*(if|elif)[ \t].*(dialect|sqlite|postgres|mysql)/) { skip = ind ""; next }
        if (match(line, /(^|[^A-Za-z0-9_.])op\.(alter_column|drop_column|create_foreign_key|drop_constraint|create_unique_constraint|create_check_constraint|create_primary_key)\(/)) {
          op = substr(line, RSTART, RLENGTH - 1); sub(/^[^o]*op\./, "", op)
          print f ":" NR ": op." op "() outside op.batch_alter_table(): SQLite cannot ALTER a table this way, and this project runs its migrations on SQLite too (render_as_batch=True or a sqlite URL). Use: with op.batch_alter_table(\"<table>\") as batch_op: batch_op." op "(...)"
          bad = 1
        }
      }
      END { exit bad }' "$_ab_f" || _ab_rc=1
  done < "$FORGE_TMP/alembic.targets"
  return "$_ab_rc"
}

# forge_alembic_fk_index: in each revision's upgrade(), a column that gets a foreign key
# (sa.Column(..., sa.ForeignKey(...)), sa.ForeignKeyConstraint([...]),
# op.create_foreign_key(..., [...], ...)) also leads an index: op.create_index,
# index=True, a primary key or unique constraint, or CREATE INDEX. PostgreSQL does not
# index referencing columns; skipped when the database URL is MySQL (which does).
forge_alembic_fk_index() {
  case $(forge_db_url) in mysql*|mariadb*) return 0 ;; esac
  _ai_rc=0
  forge_alembic_targets > "$FORGE_TMP/alembic.targets"
  while IFS= read -r _ai_f; do
    [ -f "$_ai_f" ] || continue
    awk -v f="$_ai_f" '
      # the position of the bracket closing the one at p (strings skipped)
      function close_of(s, p,   d, c, q, i, n) {
        d = 0; q = ""; n = length(s)
        for (i = p; i <= n; i++) {
          c = substr(s, i, 1)
          if (q != "") { if (c == "\\") i++; else if (c == q) q = ""; continue }
          if (c == "\"" || c == "\047") q = c
          else if (c == "(" || c == "[") d++
          else if (c == ")" || c == "]") { d--; if (d == 0) return i }
        }
        return n
      }
      function strs(t, all,   out) {
        out = ""
        while (match(t, /["\047][^"\047]*["\047]/)) {
          out = out (out == "" ? "" : SUBSEP) substr(t, RSTART + 1, RLENGTH - 2); t = substr(t, RSTART + RLENGTH)
          if (!all) break
        }
        return out
      }
      function first_list(t,   p) { p = index(t, "["); if (!p) return ""; t = substr(t, p); return substr(t, 1, close_of(t, 1)) }
      function line_of(p,   k) { for (k = nl; k > 1 && start[k] > p; k--) ; return first + k - 1 }
      function cover(list,   n, a, i) { n = split(list, a, SUBSEP); for (i = 1; i <= n; i++) cov[tolower(a[i])] = 1 }
      { line = $0; sub(/\r$/, "", line) }
      /^def[ \t]+upgrade[ \t]*\(/ { on = 1; first = NR + 1; next }
      on && line ~ /^[^ \t#]/ { on = 0 }
      on {
        if (line ~ /^[ \t]*#/) line = ""
        nl++; start[nl] = length(s) + 1; s = s line "\n"
      }
      END {
        rest = s; off = 0
        while (match(rest, /(^|[^A-Za-z0-9_])(Column|ForeignKeyConstraint|create_foreign_key|create_index|PrimaryKeyConstraint|UniqueConstraint|Index)\(/)) {
          name = substr(rest, RSTART, RLENGTH - 1); sub(/^[^A-Za-z]/, "", name)
          po = off + RSTART + RLENGTH - 1
          pc = close_of(s, po); args = substr(s, po + 1, pc - po - 1)
          if (name == "Column") {
            if (args ~ /^[ \t\n]*["\047]/) {
              col = tolower(strs(args, 0))
              if (args ~ /(^|[^A-Za-z0-9_])ForeignKey\(/ && !(col in fk)) { fk[col] = line_of(po); order[++nfk] = col }
              if (args ~ /(index|primary_key|unique)[ \t]*=[ \t]*True/) cov[col] = 1
            }
          } else if (name == "ForeignKeyConstraint" || name == "create_foreign_key") {
            col = tolower(strs(first_list(args), 0))
            if (col != "" && !(col in fk)) { fk[col] = line_of(po); order[++nfk] = col }
          } else if (name == "create_index") cover(strs(first_list(args), 0))
          else cover(strs(args, 1))
          off = po; rest = substr(s, po + 1)
        }
        u = toupper(s)
        while (match(u, /CREATE[ \t\n]+(UNIQUE[ \t\n]+)?INDEX[^(]*\([ \t\n]*/)) {
          t = substr(u, RSTART + RLENGTH); sub(/[^A-Z0-9_"].*$/, "", t); gsub(/"/, "", t)
          cov[tolower(t)] = 1; u = substr(u, RSTART + RLENGTH)
        }
        for (i = 1; i <= nfk; i++) {
          col = order[i]
          if (col in cov) continue
          print f ":" fk[col] ": foreign key column \"" col "\" gets no index in this revision; PostgreSQL does not index referencing columns, so joins on it and deletes of the referenced rows scan the table. Add op.create_index(..., [\"" col "\"]) (index=True on the model mapped_column, then autogenerate)."
          bad = 1
        }
        exit bad
      }' "$_ai_f" || _ai_rc=1
  done < "$FORGE_TMP/alembic.targets"
  return "$_ai_rc"
}

# forge_alembic_tested: some test file mentions Alembic (pytest-alembic, alembic.command, ...).
forge_alembic_tested() {
  git ls-files -co --exclude-standard -- '*test_*.py' '*_test.py' '*conftest.py' 2>/dev/null | while IFS= read -r _at_f; do
    [ -f "$_at_f" ] && grep -lE '(pytest_alembic|alembic_runner|from alembic|import alembic|alembic\.(command|config))' "$_at_f"
  done | grep -q .
}

# forge_alembic_data_downgrade: a revision whose upgrade() changes rows (UPDATE, INSERT,
# DELETE through execute(), sa.update/insert/delete, op.bulk_insert, an ORM Session) has a
# downgrade() that does something (reverts, or raises to say it cannot), unless the
# project tests its migrations. A downgrade that only passes reports success and leaves the data.
forge_alembic_data_downgrade() {
  forge_alembic_targets > "$FORGE_TMP/alembic.targets"
  : > "$FORGE_TMP/alembic.data"
  while IFS= read -r _ad_f; do
    [ -f "$_ad_f" ] || continue
    awk -v f="$_ad_f" '
      { line = $0; sub(/\r$/, "", line) }
      /^def[ \t]+upgrade[ \t]*\(/ { sec = "up"; next }
      /^def[ \t]+downgrade[ \t]*\(/ { sec = "down"; dl = NR; next }
      line ~ /^[^ \t#]/ { sec = ""; next }
      sec == "up" && line !~ /^[ \t]*#/ { up = up line "\n" }
      sec == "down" {
        t = line; sub(/^[ \t]+/, "", t); sub(/[ \t]+$/, "", t)
        if (indoc) { if (index(t, q)) indoc = 0; next }
        if (t == "" || t ~ /^#/ || t == "pass" || t == "...") next
        if (t ~ /^("""|\047\047\047)/) { q = substr(t, 1, 3); if (!index(substr(t, 4), q)) indoc = 1; next }
        if (t ~ /^["\047]/) next
        real = 1
      }
      END {
        u = toupper(up)
        data = (up ~ /execute\(/ && (u ~ /(^|[^A-Z_])UPDATE[ \t\n]+[^ \t\n]+[ \t\n]+SET[ \t\n]/ || u ~ /INSERT[ \t\n]+INTO[ \t\n]/ || u ~ /DELETE[ \t\n]+FROM[ \t\n]/)) \
          || up ~ /(^|[^A-Za-z0-9_.])(sa\.)?(update|insert|delete)\([A-Za-z_]/ || up ~ /\.(update|insert|delete)\(\)/ \
          || up ~ /op\.bulk_insert\(/ || up ~ /(^|[^A-Za-z0-9_])Session\(/
        if (data && !real) print f ":" (dl ? dl : 1) ": upgrade() changes data but downgrade() does nothing, so alembic downgrade reports success and leaves the rows changed. Write the inverse, raise an error that says the revision cannot be reverted, or test the migrations (pytest-alembic)."
      }' "$_ad_f" >> "$FORGE_TMP/alembic.data"
  done < "$FORGE_TMP/alembic.targets"
  [ -s "$FORGE_TMP/alembic.data" ] || return 0
  forge_alembic_tested && return 0
  cat "$FORGE_TMP/alembic.data"
  return 1
}

# forge_mentions <file> <name>: <name> appears as a word in <file>, outside comment lines.
forge_mentions() {
  grep -v '^[[:space:]]*#' "$1" 2>/dev/null | grep -qE "(^|[^A-Za-z0-9_])$2([^A-Za-z0-9_]|\$)"
}

# forge_models_registered: a module of a models package that maps a table (a class with
# __tablename__) is imported by the package's __init__.py when that file is the registry
# (it imports other modules of the package that map tables), or by alembic/env.py:
# Base.metadata, which autogenerate and alembic check compare, knows only imported models.
forge_models_registered() {
  if [ -n "$FORGE_FILES" ]; then printf '%s\n' "$FORGE_FILES"
  else git ls-files -co --exclude-standard -- 'models/*.py' '*/models/*.py' 2>/dev/null; fi > "$FORGE_TMP/models.files"
  _mr_rc=0
  while IFS= read -r _mr_f; do
    case ${_mr_f##*/} in __init__.py|conftest.py|test_*) continue ;; *.py) ;; *) continue ;; esac
    [ -f "$_mr_f" ] || continue
    _mr_d=$(dirname "$_mr_f")
    _mr_m=$(basename "$_mr_f" .py)
    [ -f "$_mr_d/__init__.py" ] || continue
    awk '
      /^class[ \t]+[A-Za-z_]/ { c = $0; sub(/^class[ \t]+/, "", c); sub(/[^A-Za-z0-9_].*$/, "", c); cl = NR; next }
      /^[^ \t#\r]/ { c = "" }
      c != "" && /^[ \t]+__tablename__[ \t]*[:=]/ { print cl "\t" c; c = "" }' "$_mr_f" > "$FORGE_TMP/models.classes"
    [ -s "$FORGE_TMP/models.classes" ] || continue
    forge_mentions "$_mr_d/__init__.py" "$_mr_m" && continue
    forge_mentions alembic/env.py "$_mr_m" && continue
    # the registry: __init__.py imports another module of the package that maps a table
    _mr_reg=''
    for _mr_x in $(awk '
      /^[ \t]*from[ \t]+\.[ \t]+import[ \t]/ { s = $0; sub(/^.*import[ \t]+/, "", s); gsub(/[(),]/, " ", s); n = split(s, a, /[ \t]+/); for (i = 1; i <= n; i++) if (a[i] != "as") print a[i]; next }
      /^[ \t]*from[ \t]+[A-Za-z0-9_.]+[ \t]+import/ { s = $0; sub(/^[ \t]*from[ \t]+/, "", s); sub(/[ \t].*$/, "", s); sub(/^.*\./, "", s); print s; next }
      /^[ \t]*import[ \t]+[A-Za-z0-9_.]+/ { s = $0; sub(/^[ \t]*import[ \t]+/, "", s); sub(/[ \t,].*$/, "", s); sub(/^.*\./, "", s); print s }' "$_mr_d/__init__.py"); do
      [ "$_mr_x" != "$_mr_m" ] && [ -f "$_mr_d/$_mr_x.py" ] && grep -q '__tablename__' "$_mr_d/$_mr_x.py" && { _mr_reg=1; break; }
    done
    [ -n "$_mr_reg" ] || continue
    while IFS='	' read -r _mr_l _mr_c; do
      printf '%s:%s: model %s is not imported by %s/__init__.py, which imports the other models: Base.metadata will not have its table, so autogenerate writes no migration for it and alembic check passes. Import it there.\n' "$_mr_f" "$_mr_l" "$_mr_c" "$_mr_d"
    done < "$FORGE_TMP/models.classes"
    _mr_rc=1
  done < "$FORGE_TMP/models.files"
  return "$_mr_rc"
}

# ---- Settings kept in sync with .env templates (python/env-example, typescript/env-example).

# forge_env_fields_py <file>: "line<TAB>VAR<TAB>required<TAB>class" for each field of a
# pydantic-settings BaseSettings subclass (env_prefix, alias, case_sensitive applied;
# required = no default). Fields with AliasChoices / AliasPath are left out.
forge_env_fields_py() {
  awk '
    function cls_of(l,   c) { c = l; sub(/^class[ \t]+/, "", c); sub(/[^A-Za-z0-9_].*$/, "", c); return c }
    { line = $0; sub(/\r$/, "", line) }
    line ~ /^class[ \t]/ { cls = (line ~ /\(.*BaseSettings.*\)[ \t]*:/) ? cls_of(line) : ""; body = ""; next }
    line ~ /^[^ \t#]/ { cls = ""; next }
    cls == "" || line ~ /^[ \t]*(#|$)/ { next }
    {
      ind = match(line, /[^ \t]/) - 1
      if (body == "") body = ind
    }
    FNR == NR {
      if (match(line, /env_prefix[ \t]*=[ \t]*["\047][^"\047]*["\047]/)) { p = substr(line, RSTART, RLENGTH); sub(/^[^"\047]*["\047]/, "", p); sub(/["\047]$/, "", p); pre[cls] = p }
      if (line ~ /case_sensitive[ \t]*=[ \t]*True/) cs[cls] = 1
      next
    }
    ind == body && line ~ /^[ \t]*[A-Za-z_][A-Za-z0-9_]*[ \t]*:/ {
      name = line; sub(/^[ \t]*/, "", name); sub(/[ \t]*:.*$/, "", name)
      rest = line; sub(/^[^:]*:/, "", rest)
      if (name == "model_config" || rest ~ /ClassVar|AliasChoices|AliasPath/) next
      key = pre[cls] name
      if (match(rest, /(validation_alias|alias)[ \t]*=[ \t]*["\047][^"\047]+["\047]/)) { key = substr(rest, RSTART, RLENGTH); sub(/^[^"\047]*["\047]/, "", key); sub(/["\047]$/, "", key) }
      if (!cs[cls]) key = toupper(key)
      req = 1
      if (match(rest, /[ \t]=([ \t]|$)/)) {
        v = substr(rest, RSTART + RLENGTH); sub(/^[ \t]+/, "", v)
        req = 0
        if (v ~ /^([A-Za-z_]+\.)?Field\(/) {
          sub(/^[^(]*\([ \t]*/, "", v)
          if (v ~ /^(\.\.\.|\))/) req = 1
          else if (v ~ /^[a-z_]+[ \t]*=/ && v !~ /default(_factory)?[ \t]*=/) req = 1
        }
      }
      print FNR "\t" key "\t" req "\t" cls
    }' "$1" "$1"
}

# forge_env_fields_ts <file>: the same for the keys of a Zod env schema (t3-env
# createEnv, or z.object(...) parsed from process.env): UPPER_CASE keys whose value is a
# z. chain on the same line; required = no .default(), .optional() or .catch().
forge_env_fields_ts() {
  awk '
    match($0, /^[ \t]*["\047]?[A-Z][A-Z0-9_]*["\047]?[ \t]*:[ \t]*z\./) {
      k = $0; sub(/^[ \t]*["\047]?/, "", k); sub(/[^A-Z0-9_].*$/, "", k)
      print FNR "\t" k "\t" ($0 ~ /\.(default|optional|catch)\(/ ? 0 : 1) "\tenv"
    }' "$1"
}

# forge_env_example <py|ts>: Stop and pre-commit, not --all. A settings field or env schema
# key added in the change appears in the .env template of the nearest directory that has
# one (.env.example, .env.template, .env.sample, .env.dist, .env.<name>.example): always
# when it has no default, and also with a default when the template already lists at
# least half of that class's variables. A template that lists none of them is not about it.
forge_env_example() {
  [ "$mode" = --all ] && return 0
  _ee_rc=0
  printf '%s\n' "$FORGE_FILES" > "$FORGE_TMP/env.files"
  while IFS= read -r _ee_f; do
    [ -f "$_ee_f" ] || continue
    case $1 in
      py) grep -q 'BaseSettings' "$_ee_f" || continue ;;
      ts) grep -qE 'createEnv\(|process\.env' "$_ee_f" && grep -q 'z\.' "$_ee_f" || continue ;;
    esac
    _ee_d=$(dirname "$_ee_f")
    while :; do
      _ee_t=$(ls -A "$_ee_d" 2>/dev/null | grep -E '^\.env(\.[A-Za-z0-9_-]+)?\.(example|template|sample|dist)$')
      [ -n "$_ee_t" ] || [ "$_ee_d" = . ] && break
      _ee_d=$(dirname "$_ee_d")
    done
    [ -n "$_ee_t" ] || continue
    _ee_names=''
    : > "$FORGE_TMP/env.keys"
    for _ee_x in $_ee_t; do
      [ "$_ee_d" = . ] && _ee_p=$_ee_x || _ee_p="$_ee_d/$_ee_x"
      _ee_names="${_ee_names:+$_ee_names, }$_ee_p"
      sed -n -E 's/^[[:space:]]*(export[[:space:]]+)?#?[[:space:]]*([A-Za-z_][A-Za-z0-9_]*)[[:space:]]*=.*/\2/p' "$_ee_p" >> "$FORGE_TMP/env.keys"
    done
    "forge_env_fields_$1" "$_ee_f" > "$FORGE_TMP/env.fields"
    awk -F '\t' -v f="$_ee_f" -v t="$_ee_names" '
      function found(k,   h) { if (k in have) return 1; for (h in have) if (index(h, k "_") == 1) return 1; return 0 }
      FILENAME == ARGV[1] { have[toupper($0)] = 1; next }
      { n++; ln[n] = $1; key[n] = $2; rq[n] = $3; g[n] = $4; tot[$4]++; if (found(toupper($2))) got[$4]++ }
      END {
        for (j = 1; j <= n; j++) {
          if (!got[g[j]] || found(toupper(key[j]))) continue
          if (rq[j] || got[g[j]] * 2 >= tot[g[j]]) {
            print f ":" ln[j] ": " key[j] " is not in " t "; add it there (a placeholder value is fine) so the next person to set up the project knows it exists"
            bad = 1
          }
        }
        exit bad
      }' "$FORGE_TMP/env.keys" "$FORGE_TMP/env.fields" || _ee_rc=1
  done < "$FORGE_TMP/env.files"
  return "$_ee_rc"
}

# forge_related_py_tests <files>: changed test files plus tests/**/test_<module>.py
# for each changed module, one per line.
forge_related_py_tests() {
  for _fr_f in "$@"; do
    case $_fr_f in
      *.py) ;;
      *) continue ;;
    esac
    _fr_b=${_fr_f##*/}
    case $_fr_b in
      test_*.py|*_test.py) [ -f "$_fr_f" ] && printf '%s\n' "$_fr_f" ;;
      __init__.py|conftest.py) ;;
      *) [ -d tests ] && find tests -type f -name "test_${_fr_b%.py}.py" 2>/dev/null ;;
    esac
  done | sort -u
}

# forge_exec_check <fn> <timeout>: runs <fn> with output in $FORGE_TMP/out.
forge_exec_check() {
  set -f
  if [ -n "$2" ]; then
    "$1" > "$FORGE_TMP/out" 2>&1 &
    _fe_pid=$!
    ( sleep "$2"; printf 'forge: timed out after %ss\n' "$2" >> "$FORGE_TMP/out"; forge_kill_tree "$_fe_pid" ) >/dev/null 2>&1 &
    _fe_watch=$!
    wait "$_fe_pid" 2>/dev/null
    _fe_rc=$?
    forge_kill_tree "$_fe_watch"
  else
    ( "$1" ) > "$FORGE_TMP/out" 2>&1 < /dev/null
    _fe_rc=$?
  fi
  set +f
  return "$_fe_rc"
}

# forge_diagnostics <output> <file|line> [print]: sorts the error diagnostics of a
# failed run by what they name (tsc "path(l,c)", "path:l:c" from Python and JS
# linters, "--> path:l:c"; "Would reformat: path" and Biome's "path format" name a
# file only; absolute paths under the repository are made relative; warning-only
# lines are ignored). A diagnostic counts when it names a file of $FORGE_TMP/changed
# ("file"), or an added line of $FORGE_TMP/added ("line"; one naming only a file, or
# line 0, counts when that file changed). Prints "changed" when one counts, "other" when
# they name only other files or lines, "none" when no line names a file. With
# "print", prints the lines that count instead. ANSI colour sequences are ignored, so a
# tool that colours its output (tsc with "pretty": true) is attributed the same way.
forge_diagnostics() {
  awk -v r1="$(pwd)" -v r2="$(pwd -P)" -v level="$2" -v mode="$3" '
    BEGIN { ansi = sprintf("%c", 27) "\\[[0-9;]*[A-Za-z]" }
    FILENAME == ARGV[1] { ch[$0] = 1; next }
    FILENAME == ARGV[2] { if ($2 == "*") whole[$1] = 1; else ad[$1 "\t" $2] = 1; next }
    {
      line = $0
      gsub(ansi, "", line)
      if (line ~ /[Ww]arning/ && line !~ /[Ee]rror/) next
      sub(/^[[:space:]]*(-->[[:space:]]*)?/, "", line)
      ln = ""
      if (match(line, /^[^[:space:]:(]+([(][0-9]+,[0-9]+[)]|:[0-9]+(:[0-9]+)?)/)) {
        p = substr(line, 1, RLENGTH)
        loc = p; sub(/^[^[:space:]:(]+[(:]/, "", loc); ln = loc + 0
        sub(/([(][0-9]+,[0-9]+[)]|:[0-9]+(:[0-9]+)?)$/, "", p)
      } else if (match(line, /^Would reformat: [^[:space:]]+/)) p = substr(line, 18, RLENGTH - 17)
      else if (match(line, /^[^[:space:]:(]+ format /)) { p = line; sub(/ format .*$/, "", p) }
      else next
      if (index(p, r1 "/") == 1) p = substr(p, length(r1) + 2)
      else if (index(p, r2 "/") == 1) p = substr(p, length(r2) + 2)
      sub(/^\.\//, "", p)
      named = 1
      if (level == "line") counts = (p in whole) || (ln == "" || ln == 0 ? (p in ch) : ((p "\t" ln) in ad))
      else counts = (p in ch)
      if (counts) { hit = 1; if (mode == "print") print $0 }
    }
    END { if (mode != "print") print (hit ? "changed" : named ? "other" : "none") }' "$FORGE_TMP/changed" "$FORGE_TMP/added" "$1"
}

# forge_attribute_level <stage> <whole-project run: 1|0>: how a failed check's
# diagnostics are attributed outside --all: lint by added line, typecheck and a
# whole-project format run by changed file; anything else is not attributed.
forge_attribute_level() {
  case $1 in
    lint) printf 'line' ;;
    typecheck) printf 'file' ;;
    format) [ "$2" = 1 ] && printf 'file' ;;
  esac
}

# forge_run_check <n>: one check under check.sh ($mode and forge_change_set's files).
# --changed and --turn run the {files} form on the changed files when there is one;
# --since and --all run the whole-project form (read-only, as CI should).
forge_run_check() {
  forge_check_meta_"$1"
  FORGE_FILES=''
  if [ "$mode" != --all ]; then
    if [ "$c_when" = 1 ]; then
      [ -n "$(forge_filter "forge_when_$1" < "$FORGE_TMP/changed")" ] || return 0
    fi
    FORGE_FILES=$(forge_filter "forge_scope_$1" < "$FORGE_TMP/existing")
    [ "$c_when" = 1 ] || [ -n "$FORGE_FILES" ] || return 0
  fi
  if _fc_missing=$(forge_missing_tool); then
    printf 'forge:%s skipped: %s not installed\n' "$c_id" "$_fc_missing"
    return 0
  fi
  if [ "$c_localdb" = 1 ] && ! forge_local_db; then
    printf 'forge:%s skipped: no local database\n' "$c_id"
    return 0
  fi
  _fc_fn=forge_check_run_$1
  _fc_whole=1
  case $mode in --changed|--turn)
    if [ "$c_changed" = 1 ] && [ -n "$FORGE_FILES" ]; then _fc_fn=forge_check_changed_$1; _fc_whole=0; fi ;;
  esac
  if forge_exec_check "$_fc_fn" "$c_timeout"; then
    grep '^forge:' "$FORGE_TMP/out"
    return 0
  fi
  # outside --all, only errors in the change count (§10.1 step 5)
  _fc_level=$(forge_attribute_level "$c_stage" "$_fc_whole")
  if [ "$mode" != --all ] && [ -n "$_fc_level" ]; then
    case $(forge_diagnostics "$FORGE_TMP/out" "$_fc_level") in
      other)
        if [ "$_fc_level" = line ]; then
          printf 'forge:%s ok for the added lines (findings remain on lines this change did not add: sh .forge/check.sh --all)\n' "$c_id"
        else
          printf 'forge:%s ok for the changed files (errors remain in unchanged files: sh .forge/check.sh --all)\n' "$c_id"
        fi
        return 0 ;;
      changed)
        printf 'forge:%s failed\n' "$c_id"
        forge_diagnostics "$FORGE_TMP/out" "$_fc_level" print | first_lines 15
        return 1 ;;
    esac
  fi
  printf 'forge:%s failed\n' "$c_id"
  first_lines 15 < "$FORGE_TMP/out"
  return 1
}

# forge_run_stage <stage>: every check of <stage>; fails if one fails.
forge_run_stage() {
  _fs_i=1
  _fs_rc=0
  while [ "$_fs_i" -le "$FORGE_NCHECKS" ]; do
    forge_check_meta_"$_fs_i"
    if [ "$c_stage" = "$1" ]; then
      forge_run_check "$_fs_i" || _fs_rc=1
    fi
    _fs_i=$((_fs_i + 1))
  done
  [ "$_fs_rc" -eq 0 ] || fail=1
  return "$_fs_rc"
}

# forge_file_stage <stage> <file>: the {files} form of each in-scope check of
# <stage>, on one file (PostToolUse). Prints failures; lint findings only on the
# lines of $FORGE_TMP/added; absent tools are skipped.
forge_file_stage() {
  _fp_i=1
  _fp_rc=0
  FORGE_FILES=$2
  while [ "$_fp_i" -le "$FORGE_NCHECKS" ]; do
    forge_check_meta_"$_fp_i"
    if [ "$c_stage" = "$1" ] && [ "$c_files" = 1 ] && forge_scope_"$_fp_i" "$2" && ! forge_missing_tool >/dev/null; then
      if ! forge_exec_check "forge_check_changed_$_fp_i" "$c_timeout"; then
        _fp_level=$(forge_attribute_level "$1" 0)
        _fp_what=none
        [ -n "$_fp_level" ] && _fp_what=$(forge_diagnostics "$FORGE_TMP/out" "$_fp_level")
        case $_fp_what in
          other) ;;
          changed)
            printf 'forge:%s\n' "$c_id"
            forge_diagnostics "$FORGE_TMP/out" "$_fp_level" print | first_lines 15
            _fp_rc=1 ;;
          *)
            printf 'forge:%s\n' "$c_id"
            first_lines 15 < "$FORGE_TMP/out"
            _fp_rc=1 ;;
        esac
      fi
    fi
    _fp_i=$((_fp_i + 1))
  done
  return "$_fp_rc"
}

# forge_file_fixes <file>: the safe fixes (fix_run) of each in-scope lint check, on
# one file. When they changed it, prints "forge: auto-fixed <codes> in <file>" (codes
# from ruff's --show-fixes summary), so Claude knows its view of the file is stale.
forge_file_fixes() {
  _fx_i=1
  _fx_codes=''
  _fx_before=$(git hash-object -- "$1" 2>/dev/null)
  FORGE_FILES=$1
  while [ "$_fx_i" -le "$FORGE_NCHECKS" ]; do
    forge_check_meta_"$_fx_i"
    if [ "$c_stage" = lint ] && [ "$c_fix" = 1 ] && forge_scope_"$_fx_i" "$1" && ! forge_missing_tool >/dev/null; then
      forge_exec_check "forge_check_fix_$_fx_i" "$c_timeout"
      _fx_codes="$_fx_codes $(awk '/^Fixed [0-9]+ error/ { on = 1; next } on && /^ +[0-9]+ / && match($0, /[A-Z]+[0-9]+ \(/) { printf "%s ", substr($0, RSTART, RLENGTH - 2) } /^$/ { on = 0 }' "$FORGE_TMP/out")"
    fi
    _fx_i=$((_fx_i + 1))
  done
  [ "$(git hash-object -- "$1" 2>/dev/null)" != "$_fx_before" ] || return 0
  _fx_codes=$(printf '%s' "$_fx_codes" | tr ' ' '\n' | awk 'NF && !seen[$0]++' | tr '\n' ' ' | sed 's/ $//; s/ /, /g')
  printf 'forge: auto-fixed %s in %s; re-read it before editing it again\n' "${_fx_codes:-safe lint fixes}" "$1"
}

# forge_typecheck <file>: the full run of each typecheck check whose scope covers <file>;
# only errors in files of $FORGE_TMP/changed count ($FORGE_TMP/added must exist).
forge_typecheck() {
  _ft_i=1
  _ft_rc=0
  while [ "$_ft_i" -le "$FORGE_NCHECKS" ]; do
    forge_check_meta_"$_ft_i"
    if [ "$c_stage" = typecheck ] && forge_scope_"$_ft_i" "$1" && ! forge_missing_tool >/dev/null; then
      if ! forge_exec_check "forge_check_run_$_ft_i" "$c_timeout"; then
        # only errors in changed files wake Claude
        case $(forge_diagnostics "$FORGE_TMP/out" file) in
          other) ;;
          changed)
            printf 'forge:%s\n' "$c_id"
            forge_diagnostics "$FORGE_TMP/out" file print | first_lines 19
            _ft_rc=1 ;;
          *)
            printf 'forge:%s\n' "$c_id"
            first_lines 19 < "$FORGE_TMP/out"
            _ft_rc=1 ;;
        esac
      fi
    fi
    _ft_i=$((_ft_i + 1))
  done
  return "$_ft_rc"
}

# forge_missing_tools: the tools the checks need that are not here, comma-separated
# (also what forge status prints: one source of truth).
forge_missing_tools() {
  _mt=''
  for _mt_t in $(forge_required_tools); do
    forge_has_tool "$_mt_t" || _mt="${_mt:+$_mt, }$_mt_t"
  done
  if ! command -v bunx >/dev/null 2>&1 && ! command -v uvx >/dev/null 2>&1; then
    _mt="${_mt:+$_mt, }bunx or uvx (ast-grep)"
  fi
  printf '%s' "$_mt"
}

# forge_required_tools: every tool the checks use, one per line.
forge_required_tools() {
  _fq_i=1
  while [ "$_fq_i" -le "$FORGE_NCHECKS" ]; do
    forge_check_meta_"$_fq_i"
    printf '%s\n' "$c_tools" | tr ' ' '\n'
    _fq_i=$((_fq_i + 1))
  done | awk 'NF && !seen[$0]++'
}