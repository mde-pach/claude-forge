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

# forge_once: for advisory checks. At Stop (--turn) a check that calls it after a
# finding blocks once per turn: the first call records the finding in the turn
# snapshot and fails; a later call in the same turn prints "accepted" and passes.
# In other modes (pre-commit, CI) it always fails.
forge_once() {
  [ "$mode" = --turn ] && [ -n "$FORGE_SNAP" ] || return 1
  _fo_m="$FORGE_SNAP/once.$(printf '%s' "$c_id" | tr -c 'A-Za-z0-9_.-' '_')"
  if [ -f "$_fo_m" ]; then
    printf 'forge:%s accepted: reported once this turn\n' "$c_id"
    return 0
  fi
  : > "$_fo_m" 2>/dev/null
  return 1
}

FORGE_JS_LOCKS='pnpm-lock.yaml package-lock.json npm-shrinkwrap.json yarn.lock bun.lock bun.lockb'

# forge_pkg_deps <package.json>: "section<TAB>name<TAB>spec" for each entry of
# dependencies, devDependencies and optionalDependencies (what lock files record;
# pnpm's lock does not record peerDependencies), sorted.
forge_pkg_deps() {
  [ -f "$1" ] || return 0
  awk '
    { buf = buf $0 "\n" }
    END {
      n = length(buf); depth = 0; top = ""; key = ""; i = 1
      while (i <= n) {
        c = substr(buf, i, 1)
        if (c == "\"") {
          j = i + 1; s = ""
          while (j <= n) {
            d = substr(buf, j, 1)
            if (d == "\\") { s = s substr(buf, j, 2); j += 2; continue }
            if (d == "\"") break
            s = s d; j++
          }
          i = j + 1; k = i
          while (k <= n && substr(buf, k, 1) ~ /[ \t\r\n]/) k++
          if (substr(buf, k, 1) == ":") { key = s; if (depth == 1) top = s; i = k + 1; continue }
          if (depth == 2 && top ~ /^(dependencies|devDependencies|optionalDependencies)$/) print top "\t" key "\t" s
          continue
        }
        if (c == "{" || c == "[") depth++
        else if (c == "}" || c == "]") depth--
        i++
      }
    }' "$1" | sort
}

# forge_lockfile_updated: a package.json whose dependencies changed since the base
# needs a changed lock file, when the project has one (next to it or at the root).
# Blocks once per turn (forge_once): the package manager may not be able to run here.
forge_lockfile_updated() {
  grep -qE '(^|/)(pnpm-lock\.yaml|package-lock\.json|npm-shrinkwrap\.json|yarn\.lock|bun\.lockb?)$' "$FORGE_TMP/changed" && return 0
  _lu_rc=0
  while IFS= read -r _lu_f; do
    case $_lu_f in package.json|*/package.json) ;; *) continue ;; esac
    case /$_lu_f in */node_modules/*) continue ;; esac
    _lu_lock=''
    _lu_ifs=$IFS; IFS=' '
    for _lu_l in $FORGE_JS_LOCKS; do
      if [ -f "$_lu_l" ] || [ -f "$(dirname "$_lu_f")/$_lu_l" ]; then _lu_lock=$_lu_l; break; fi
    done
    IFS=$_lu_ifs
    [ -n "$_lu_lock" ] || continue
    forge_pkg_deps "$FORGE_TMP/base/$_lu_f" > "$FORGE_TMP/deps.then"
    forge_pkg_deps "$_lu_f" > "$FORGE_TMP/deps.now"
    cmp -s "$FORGE_TMP/deps.then" "$FORGE_TMP/deps.now" && continue
    _lu_what=$(awk -F '\t' '
      FILENAME == ARGV[1] { then[$1 " " $2] = $3; next }
      { now[$1 " " $2] = $3 }
      END {
        for (k in now) if (!(k in then)) print k " " now[k] " added"; else if (then[k] != now[k]) print k " " then[k] " -> " now[k]
        for (k in then) if (!(k in now)) print k " removed"
      }' "$FORGE_TMP/deps.then" "$FORGE_TMP/deps.now" | sort | head -n 5 | awk 'NR > 1 { printf "; " } { printf "%s", $0 }')
    printf '%s: %s, but %s did not change\n' "$_lu_f" "$_lu_what" "$_lu_lock"
    _lu_rc=1
  done < "$FORGE_TMP/existing"
  [ "$_lu_rc" = 0 ] && return 0
  printf 'Run the package manager (bun install, pnpm install, npm install, yarn) to update the lock file; it is not edited by hand. If it cannot run here, tell the user.\n'
  forge_once
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