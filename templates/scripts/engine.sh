# Check engine (static part). Expects: IFS = newline, FORGE_TMP, FORGE_NCHECKS,
# forge_check_meta_<n>, forge_scope_<n>, forge_when_<n>, forge_check_run_<n>,
# forge_check_changed_<n>, plus tool() and first_lines() from lib.sh.

# forge_change_set: the files a run is about, for $mode (--changed, --since,
# --turn, --all; $FORGE_ARG is the ref or the snapshot directory). Writes to $FORGE_TMP:
#   changed   changed paths, one per line   existing  those that exist now
#   deleted   those gone since the base     inbase    those that exist in the base
#   base/<p>  each one as it is in the base
#   added     "path<TAB>line" per added line, "path<TAB>*" per new file (empty with --all);
#             lines only formatting changed are left out (forge_added_lines), unless
#             FORGE_PLAIN_ADDED=1 (typecheck.sh, which attributes by file only)
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
        [ -n "$FORGE_BASE_REV" ] && git diff --name-only -z --no-renames "$FORGE_BASE_REV" HEAD 2>/dev/null | tr '\000' '\n'
      } | sort -u | while IFS= read -r _cs_f; do
        [ -n "$_cs_f" ] && [ "$(now_hash "$_cs_f")" != "$(base_hash "$_cs_f")" ] && printf '%s\n' "$_cs_f"
      done ;;
    --since)
      { git diff --name-only -z --no-renames "$FORGE_BASE_REV" 2>/dev/null; git ls-files -z --others --exclude-standard 2>/dev/null; } | tr '\000' '\n' | sort -u ;;
    *) status_paths | sort -u ;;
  esac | grep -v '^\.forge/' > "$FORGE_TMP/changed"
  # Forge's own files (.forge/: check.sh, its helper scripts) are not the project's code:
  # integrity covers them (check.sh step 1), the project's rules do not
  : > "$FORGE_TMP/existing"; : > "$FORGE_TMP/deleted"; : > "$FORGE_TMP/inbase"; : > "$FORGE_TMP/added"
  mkdir -p "$FORGE_TMP/base"
  # "<0|1><TAB><path>": 0 when the path is not in FORGE_BASE_REV's tree at all (one ls-tree
  # for the whole change, instead of a git cat-file per new file in base_file)
  : > "$FORGE_TMP/inrev"
  [ -z "${FORGE_BASE_REV:-}" ] || git ls-tree -r -t -z --full-tree --name-only "$FORGE_BASE_REV" 2>/dev/null | tr '\000' '\n' > "$FORGE_TMP/inrev"
  awk 'FILENAME == ARGV[1] { t[$0] = 1; next } { print (($0 in t) ? 1 : 0) "\t" $0 }' "$FORGE_TMP/inrev" "$FORGE_TMP/changed" > "$FORGE_TMP/changed.inrev"
  while IFS= read -r _cs_l; do
    _bf_inrev=${_cs_l%%"	"*}
    _cs_f=${_cs_l#*"	"}
    if base_file "$_cs_f" "$FORGE_TMP/base/$_cs_f"; then printf '%s\n' "$_cs_f" >> "$FORGE_TMP/inbase"; _cs_b=1; else _cs_b=0; fi
    if [ -f "$_cs_f" ]; then
      printf '%s\n' "$_cs_f" >> "$FORGE_TMP/existing"
      [ "$mode" = --all ] || forge_added_lines "$_cs_f" "$FORGE_TMP/base/$_cs_f" >> "$FORGE_TMP/added"
    elif [ "$_cs_b" = 1 ]; then
      printf '%s\n' "$_cs_f" >> "$FORGE_TMP/deleted"
    fi
  done < "$FORGE_TMP/changed.inrev"
  unset _bf_inrev
  return 0
}

# forge_format_copy <path> <in> <out>: writes <in> to <out> formatted as <path> would be,
# by the format_stdin filter of each check whose scope covers <path>, in check order.
# Returns 2 when no filter applies, 1 when a tool is absent or a filter fails or prints nothing.
forge_format_copy() {
  _fy_i=1
  _fy_rc=2
  FORGE_FILE=$1
  cp "$2" "$3" 2>/dev/null || return 1
  while [ "$_fy_i" -le "$FORGE_NCHECKS" ]; do
    forge_check_meta_"$_fy_i"
    if [ "$c_fmt" = 1 ] && forge_scope_"$_fy_i" "$1"; then
      forge_missing_tool >/dev/null && return 1
      if ( forge_check_fmt_"$_fy_i" ) < "$3" > "$3.fmt" 2>/dev/null && { [ -s "$3.fmt" ] || [ ! -s "$3" ]; }; then
        mv "$3.fmt" "$3"
        _fy_rc=0
      else
        rm -f "$3.fmt"
        return 1
      fi
    fi
    _fy_i=$((_fy_i + 1))
  done
  return "$_fy_rc"
}

# forge_format_base <path> <base-copy> <out>: forge_format_copy of a base copy. With a
# turn snapshot (FORGE_SNAP) the result is kept for the turn in <snapshot>/formatted/
# (<snapshot>/unformattable/ when the formatter failed), since the base does not change
# during a turn: post-edit and Stop then format each base once.
forge_format_base() {
  if [ -z "${FORGE_SNAP:-}" ] || [ ! -d "$FORGE_SNAP" ]; then forge_format_copy "$@"; return; fi
  if [ -f "$FORGE_SNAP/formatted/$1" ]; then cp "$FORGE_SNAP/formatted/$1" "$3"; return; fi
  [ -f "$FORGE_SNAP/unformattable/$1" ] && return 1
  forge_format_copy "$@"
  _fb_rc=$?
  case $_fb_rc in
    0) mkdir -p "$(dirname "$FORGE_SNAP/formatted/$1")" && cp "$3" "$FORGE_SNAP/formatted/$1" ;;
    1) mkdir -p "$(dirname "$FORGE_SNAP/unformattable/$1")" && : > "$FORGE_SNAP/unformattable/$1" ;;
  esac
  return "$_fb_rc"
}

# forge_added_lines <path> <base-copy>: added_lines, less the lines that only formatting
# changed: a line counts when it is added both against the base copy and against the
# base copy formatted like <path> (forge_format_copy), so a whole-file format of an
# unformatted file does not make its untouched lines Claude's. Without a formatter
# (none in scope, tool absent, it fails on the base), the plain added_lines.
# The two diffs may align repeated lines differently (a duplicated line is added as
# the first copy in one and the second in the other), so a line added against the
# base also counts when its text is that of a line added against the formatted base
# not matched by number (each such line counted once).
forge_added_lines() {
  # a file new since the base: every line is added
  if [ ! -f "$2" ]; then printf '%s\t*\n' "$1"; return 0; fi
  added_lines "$1" "$2" > "$FORGE_TMP/al.raw"
  if [ ! -f "$2" ] || [ ! -s "$FORGE_TMP/al.raw" ] || [ "${FORGE_PLAIN_ADDED:-0}" = 1 ] || ! forge_format_base "$1" "$2" "$FORGE_TMP/al.base" || cmp -s "$2" "$FORGE_TMP/al.base"; then
    cat "$FORGE_TMP/al.raw"
  else
    added_lines "$1" "$FORGE_TMP/al.base" > "$FORGE_TMP/al.fmt"
    awk -F '\t' '
      FILENAME == ARGV[1] { fmt[$2] = 1; next }
      FILENAME == ARGV[2] { raw[++n] = $0; num[n] = $2; next }
      { text[FNR] = $0 }
      END {
        for (l in fmt) if (l in text) left[text[l]]++
        for (i = 1; i <= n; i++) if (num[i] in fmt) { keep[i] = 1; left[text[num[i]]]-- }
        for (i = 1; i <= n; i++) if (!(i in keep) && left[text[num[i]]] > 0) { keep[i] = 1; left[text[num[i]]]-- }
        for (i = 1; i <= n; i++) if (i in keep) print raw[i]
      }' "$FORGE_TMP/al.fmt" "$FORGE_TMP/al.raw" "./$1"
  fi
  rm -f "$FORGE_TMP/al.raw" "$FORGE_TMP/al.fmt" "$FORGE_TMP/al.base"
  return 0
}

# forge_sg_report <json> [json]: ast-grep --json=stream matches as
# "forge:<rule> path:line:column <description>" (from 1); unless $mode is --all, only
# matches that overlap a line of $FORGE_TMP/added (a match spanning an edited line counts).
# With "json", those matches as they are (for forge_types_filter).
forge_sg_report() {
  awk -v all="$([ "$mode" = --all ] && printf 1)" -v json="${2:-}" '
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
      if (json != "") { print; next }
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

# forge_ratchet <runtime> <script>: the config ratchet. Runs .forge/helpers/<script>
# with the project's runtime on the changed files and their base copies; each printed
# line is a loosened setting, unless forge.yaml records it as accepted.
forge_ratchet() {
  [ -f ".forge/helpers/$2" ] || { printf 'forge:%s skipped: .forge/helpers/%s is missing (run forge apply)\n' "$c_id" "$2"; return 0; }
  set -f
  # shellcheck disable=SC2046
  "$1" ".forge/helpers/$2" "$FORGE_TMP/base" $(cat "$FORGE_TMP/existing") > "$FORGE_TMP/ratchet" 2>&1
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

# forge_helper <runtime> <script> [file...]: runs .forge/helpers/<script> with the
# project's runtime, as `<script> <FORGE_TMP> [file...]`
# (FORGE_TMP holds changed, existing, inbase and base/ from forge_change_set). Exit 0:
# nothing found; 1: findings printed ("path:line message"; a line also present in
# forge.yaml is accepted); 3: cannot run, its "skipped: ..." line is printed.
forge_helper() {
  [ -f ".forge/helpers/$2" ] || { printf 'forge:%s skipped: .forge/helpers/%s is missing (run forge apply)\n' "$c_id" "$2"; return 0; }
  _fh_rt=$1; _fh_s=$2; shift 2
  "$_fh_rt" ".forge/helpers/$_fh_s" "$FORGE_TMP" "$@" > "$FORGE_TMP/helper" 2>&1
  case $? in
    0) return 0 ;;
    1) ;;
    *) printf 'forge:%s %s\n' "$c_id" "$(grep '^skipped:' "$FORGE_TMP/helper" | head -n 1 | grep . || printf 'skipped: %s could not run' "$_fh_s")"; return 0 ;;
  esac
  _fh_rc=0
  while IFS= read -r _fh_l; do
    [ -f forge.yaml ] && grep -qF -- "$_fh_l" forge.yaml && continue
    printf '%s\n' "$_fh_l"; _fh_rc=1
  done < "$FORGE_TMP/helper"
  return "$_fh_rc"
}

# Test blocks, for forge_test_edits: pytest test functions (with their decorators)
# and classes; Vitest/Playwright test and it calls (with their .each tables), and the
# describe calls around them; test support code (forge-test-support): top-level
# functions and classes not named test* / Test* (fixtures, fakes, mocks, helpers),
# top-level declarations and vi.mock calls.
FORGE_TEST_BLOCKS='id: forge-test-support
language: Python
rule:
  any:
    - all:
        - any: [{kind: function_definition}, {kind: class_definition}]
        - inside: {kind: module, stopBy: neighbor}
        - not: {has: {field: name, regex: "^(test|Test)"}}
    - all:
        - kind: decorated_definition
        - inside: {kind: module, stopBy: neighbor}
        - not: {has: {field: definition, has: {field: name, regex: "^(test|Test)"}}}
---
id: forge-test-support
language: Tsx
rule:
  any:
    - all:
        - any: [{kind: function_declaration}, {kind: class_declaration}, {kind: lexical_declaration}, {kind: variable_declaration}]
        - any: [{inside: {kind: program, stopBy: neighbor}}, {inside: {kind: export_statement, stopBy: neighbor, inside: {kind: program, stopBy: neighbor}}}]
    - {kind: expression_statement, regex: "^vi[.](do)?[mM]ock[(]", inside: {kind: program, stopBy: neighbor}}

---
id: forge-test
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

# forge_test_blocks <dir> <file-list>: "file<TAB>key<TAB>body<TAB>print" per test of
# the listed files under <dir>; key = enclosing classes or describe names, then the
# test name (" #2" for a repeated one); body = the test's text without whitespace;
# print = its fingerprint: the whole test (decorators, .each table, setup, input data,
# control flow and assertions) without whitespace, semicolons and trailing commas, with
# ' read as ", so formatting is not a change. Test support code is listed too, keyed
# "support:<name>", with the same fingerprint.
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
      if (rid == "forge-test-support") {
        if (match(t, /^vi[.](do)?[mM]ock[(][^,)]*/)) nm = substr(t, RSTART, RLENGTH) ")"
        else if (match(t, /(def|class|function\*?|const|let|var)[ ]+[A-Za-z_$][A-Za-z0-9_$]*/)) { nm = substr(t, RSTART, RLENGTH); sub(/^[^ ]+[ ]+/, "", nm) }
        else nm = substr(t, 1, 40)
      }
      N[n] = nm; gsub(/[[:space:]]/, "", t); T[n] = t
      p = t; gsub(/;/, "", p); gsub(sprintf("%c", 39), "\"", p); gsub(/,[)]/, ")", p); gsub(/,[]]/, "]", p); gsub(/,[}]/, "}", p); P[n] = p
    }
    END {
      for (a = 1; a <= n; a++) {
        if (R[a] == "forge-test-support") {
          key = "support:" N[a]
          if (++seen[F[a] "\t" key] > 1) key = key " #" seen[F[a] "\t" key]
          print F[a] "\t" key "\t" T[a] "\t" P[a]
          continue
        }
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
        print F[a] "\t" key "\t" T[a] "\t" P[a]
      }
    }' "$FORGE_TMP/blocks.json"
}

# forge_test_edits: Stop only (--turn). Each test of the in-scope files ($FORGE_FILES)
# that existed at the start of the turn must keep its whole text (its fingerprint:
# formatting aside) and still be present, unless an Edit or Write changing it was
# approved during the turn (<snapshot>/tests-approved, written by test-edit.sh); new
# tests are fine. Test support code (fakes, fixtures, helpers)
# changed or removed is reported once per turn and finding (forge_once_new): it
# encodes the interface callers rely on.
forge_test_edits() {
  [ "$mode" = --turn ] || return 0
  printf '%s\n' "$FORGE_FILES" | grep -xF -f "$FORGE_TMP/inbase" > "$FORGE_TMP/tests.files"
  [ -s "$FORGE_TMP/tests.files" ] || return 0
  if ! forge_test_blocks "$FORGE_TMP/base" "$FORGE_TMP/tests.files" > "$FORGE_TMP/tests.then" \
    || ! forge_test_blocks "$FORGE_ROOT" "$FORGE_TMP/tests.files" > "$FORGE_TMP/tests.now"; then
    printf 'forge:%s skipped: ast-grep unavailable\n' "$c_id"
    return 0
  fi
  _te_ok=$FORGE_ARG/tests-approved
  [ -f "$_te_ok" ] || _te_ok=/dev/null
  awk -F '\t' '
    FILENAME == ARGV[1] { ok[$1 "\t" $2] = 1; next }
    FILENAME == ARGV[2] { then[$1 "\t" $2] = $4; next }
    { now[$1 "\t" $2] = $4 }
    END {
      for (k in then) {
        split(k, p, "\t")
        if (p[2] ~ /^support:/) {
          if (!(k in now)) print "S\t" p[1] "\t" substr(p[2], 9) "\tremoved"
          else if (now[k] != then[k]) print "S\t" p[1] "\t" substr(p[2], 9) "\tchanged"
          continue
        }
        if (k in ok) continue
        if (!(k in now)) print "T\t" p[1] ": test \"" p[2] "\" was removed"
        else if (now[k] != then[k]) print "T\t" p[1] ": test \"" p[2] "\" was changed"
      }
    }' "$_te_ok" "$FORGE_TMP/tests.then" "$FORGE_TMP/tests.now" | sort > "$FORGE_TMP/tests.edits"
  _te_rc=0
  if grep -q '^T' "$FORGE_TMP/tests.edits"; then
    sed -n 's/^T	//p' "$FORGE_TMP/tests.edits"
    printf 'These tests existed before this turn and were changed without approval. Restore them. If the task requires the change (it changes the behaviour they assert), make it with Edit or Write: Forge then asks for approval (the user, or a reviewer given the request).\n'
    _te_rc=1
  fi
  # test support code: each finding reported once per turn (forge_once_new), then accepted
  grep '^S' "$FORGE_TMP/tests.edits" | while IFS='	' read -r _te_k _te_f _te_n _te_how; do
    printf '%s: test support `%s` was %s\n' "$_te_f" "$_te_n" "$_te_how"
  done > "$FORGE_TMP/support.lines"
  if [ -s "$FORGE_TMP/support.lines" ] && forge_once_new "$FORGE_TMP/support.lines"; then
    printf 'Test doubles, fixtures and helpers encode the interface the code'"'"'s callers rely on. Keep that interface in production (e.g. a property alias for a moved attribute) unless the task removes it; if it does, carry on: this is reported once.\n'
    # alone, it is advice (§10.7): the next Stop accepts it
    [ "$_te_rc" = 1 ] || : > "$(forge_advised_mark)"
    _te_rc=1
  fi
  return "$_te_rc"
}

# ---- findings reported once per turn (§10.7)

# forge_once_new <file>: Stop (--turn) only. The lines of <file> that check $c_id has
# not reported yet in this turn (<snapshot>/reported, "<rule-id><TAB><line>"), which
# are recorded as reported; fails when there is none. Each line is one finding.
forge_once_new() {
  _on_seen=$FORGE_SNAP/reported
  while IFS= read -r _on_l; do
    [ -n "$_on_l" ] || continue
    grep -qxF "$c_id	$_on_l" "$_on_seen" 2>/dev/null || printf '%s\n' "$_on_l"
  done < "$1" > "$FORGE_TMP/once.new"
  [ -s "$FORGE_TMP/once.new" ] || return 1
  awk -v id="$c_id" '{ print id "\t" $0 }' "$FORGE_TMP/once.new" >> "$_on_seen"
  cat "$FORGE_TMP/once.new"
}

# forge_once <helper> [args]: runs <helper>; what it prints is its findings, one per
# line. At Stop (--turn) they are advice reported once per finding: output with a line
# not reported yet in this turn is printed in full and fails as advice (it does not stop
# the later steps, and check.sh prints it after the failing checks); output with
# nothing new passes with "forge:<id> accepted: reported once this turn". A new prompt
# (a new snapshot) re-arms it. In other modes (pre-commit, CI) any output fails. A
# "forge:<id> skipped: …" line (the helper could not run) is printed, not a finding.
forge_once() {
  "$@" > "$FORGE_TMP/once.all" 2>/dev/null
  grep -F "forge:$c_id skipped: " "$FORGE_TMP/once.all"
  grep -vF "forge:$c_id skipped: " "$FORGE_TMP/once.all" > "$FORGE_TMP/once.out"
  [ -s "$FORGE_TMP/once.out" ] || return 0
  if [ "$mode" != --turn ] || [ -z "$FORGE_SNAP" ]; then cat "$FORGE_TMP/once.out"; return 1; fi
  if forge_once_new "$FORGE_TMP/once.out" > /dev/null; then
    : > "$(forge_advised_mark)"
    cat "$FORGE_TMP/once.out"
    return 1
  fi
  printf 'forge:%s accepted: reported once this turn\n' "$c_id"
  return 0
}

# forge_advise <helper> [args]: forge_once at Stop only; pre-commit and CI never run
# advisory checks (Claude acts on the advice or says why it does not apply).
forge_advise() {
  [ "$mode" = --turn ] && [ -n "$FORGE_SNAP" ] || return 0
  forge_once "$@"
}

# Test files of the packs' conventions (pytest, Vitest, Playwright), as a POSIX ERE.
FORGE_TEST_PATH_ERE='(^|/)(tests?|__tests__|e2e)/|(^|/)(test_[^/]*|[^/]*_test|conftest)\.py$|\.(test|spec)\.[^/]*$'

# forge_diff_lines <path>: "-<text>" / "+<text>" for each line removed or added
# since the base ($FORGE_TMP/base/<path>; a file new or deleted in the change
# counts fully).
forge_diff_lines() {
  _fd_old=$FORGE_TMP/base/$1; [ -f "$_fd_old" ] || _fd_old=/dev/null
  _fd_new=$1; [ -f "$_fd_new" ] || _fd_new=/dev/null
  git diff --no-index --no-ext-diff --no-textconv --no-color -U0 -- "$_fd_old" "$_fd_new" 2>/dev/null \
    | awk '/^@@/ { h = 1; next } h && /^[-+]/'
}

# forge_code_changed <path>: succeeds when a removed or added line of <path> is
# code: not blank, not only a comment (#, //, /* … */ lines).
forge_code_changed() {
  _cc_py=''
  case $1 in *.py|*.pyi) _cc_py=1 ;; esac
  forge_diff_lines "$1" | awk -v py="$_cc_py" '
    { t = substr($0, 2); gsub(/^[ \t]+|[ \t\r]+$/, "", t)
      if (t == "") next
      if (py && t ~ /^#/) next
      if (!py && (t ~ /^\/\// || t ~ /^\/\*/ || t ~ /^\*/)) next
      found = 1; exit }
    END { exit !found }'
}

# forge_untested_change [<test-path-ERE>]: when the turn changed code (not only
# comments or blank lines) in the in-scope files ($FORGE_FILES) and no file
# matching <test-path-ERE> (default FORGE_TEST_PATH_ERE) was added or changed, says so.
forge_untested_change() {
  grep -Eq -- "${1:-$FORGE_TEST_PATH_ERE}" "$FORGE_TMP/existing" && return 0
  _fu_n=0; _fu_list=''
  for _fu_f in $FORGE_FILES; do
    forge_code_changed "$_fu_f" || continue
    _fu_n=$((_fu_n + 1))
    [ "$_fu_n" -le 3 ] && _fu_list="${_fu_list:+$_fu_list, }$_fu_f"
  done
  [ "$_fu_n" -gt 0 ] || return 0
  [ "$_fu_n" -gt 3 ] && _fu_list="$_fu_list and $((_fu_n - 3)) more"
  printf 'You changed behaviour in %s without adding or updating a test; add a test that fails without your change, or say why none is needed.\n' "$_fu_list"
}

# forge_changed_ids: the public identifiers whose definition the change added,
# removed or edited, one per line, from the removed and added lines of the
# in-scope files ($FORGE_FILES and the deleted ones it names): Python def, class,
# annotated names (parameters, fields) and module-level assignments; TypeScript
# and JavaScript exported declarations, optional properties (name?:), typed
# members (name: T;) and methods with a modifier. A definition whose line only
# moved does not count. Private (_x), short (< 4) and common names are dropped.
forge_changed_ids() {
  { printf '%s\n' "$FORGE_FILES"; cat "$FORGE_TMP/deleted"; } | while IFS= read -r _ci_f; do
    printf '%s\n' "$_ci_f" | grep -Eq -- "$FORGE_TEST_PATH_ERE" && continue
    case $_ci_f in *.py|*.pyi) _ci_py=1 ;; *.ts|*.tsx|*.mts|*.cts|*.js|*.jsx|*.mjs|*.cjs) _ci_py=0 ;; *) continue ;; esac
    forge_diff_lines "$_ci_f" | sed "s/^/$_ci_py/"
  done | awk '
    function keep(id) {
      if (length(id) < 4 || id ~ /^_/ || (id in stop)) return
      ids[id] = 1
    }
    function def(id, sign, text) { keep(id); if (id in ids) { if (sign == "-") M[id SUBSEP text] = 1; else P[id SUBSEP text] = 1 } }
    function ident(s) { sub(/^[^A-Za-z_$]*/, "", s); match(s, /^[A-Za-z_$][A-Za-z0-9_$]*/); return substr(s, 1, RLENGTH) }
    BEGIN {
      n = split("self cls this args kwargs props options opts config settings params param value values data name type kind keys item items result results error errors message path file files input output default index count size text body request response context event callback handler children model state status client server string number boolean object array true false none null undefined return async await const function class export import from init main test start stop update create delete call load save open close read write parse format info debug warn logger user node tree root parent child source target mode level headers method query schema field fields label title description content length list dict tuple bool float bytes json None True False Exception ValueError TypeError KeyError RuntimeError else elif while with pass yield raise assert except finally lambda global nonlocal case break continue Args Returns Raises Yields Example Examples Note Notes Attributes", w, " ")
      for (i = 1; i <= n; i++) stop[w[i]] = 1
    }
    {
      py = substr($0, 1, 1); sign = substr($0, 2, 1); line = substr($0, 3)
      t = line; gsub(/^[ \t]+|[ \t\r]+$/, "", t)
      if (t == "") next
      if (py == 1) {
        if (t ~ /^#/) next
        if (t ~ /^(async[ \t]+)?def[ \t]/) {
          s = t; sub(/^(async[ \t]+)?def[ \t]+/, "", s); def(ident(s), sign, t)
          if (i = index(s, "(")) {
            s = substr(s, i + 1); m = split(s, a, ",")
            for (j = 1; j <= m; j++) { p = a[j]; sub(/^[ \t*]+/, "", p); if (p ~ /^[A-Za-z_][A-Za-z0-9_]*[ \t]*[:=,)]?/) def(ident(p), sign, t) }
          }
        } else if (t ~ /^class[ \t]/) { s = t; sub(/^class[ \t]+/, "", s); def(ident(s), sign, t) }
        else if (t ~ /^[A-Za-z_][A-Za-z0-9_]*[ \t]*:[ \t]*[^ \t=]/) {
          r = t; sub(/^[A-Za-z_][A-Za-z0-9_]*[ \t]*:[ \t]*/, "", r)
          if (r ~ /=/ || r ~ /^[A-Za-z_][A-Za-z0-9_.]*(\[.*)?([ \t]*\|[ \t]*[^ \t]+)*[ \t]*,?$/) def(ident(t), sign, t)
        }
        else if (line ~ /^[A-Za-z_][A-Za-z0-9_]*[ \t]*=[^=]/) def(ident(line), sign, t)
      } else {
        if (t ~ /^(\/\/|\/\*|\*)/) next
        if (t ~ /^export[ \t]/) {
          s = t; sub(/^export[ \t]+((default|declare|abstract|async)[ \t]+)*/, "", s)
          if (s ~ /^(function\*?|class|const|let|var|type|interface|enum|namespace)[ \t]/) { sub(/^[a-z]+\*?[ \t]+/, "", s); def(ident(s), sign, t) }
        } else if (t ~ /^(readonly[ \t]+)?[A-Za-z_$][A-Za-z0-9_$]*\?[ \t]*:/ || t ~ /^(readonly[ \t]+)?[A-Za-z_$][A-Za-z0-9_$]*[ \t]*:.*;$/) {
          s = t; sub(/^readonly[ \t]+/, "", s); def(ident(s), sign, t)
        } else if (t ~ /^((public|static|async|get|set|override)[ \t]+)+[A-Za-z_$][A-Za-z0-9_$]*[ \t]*[(<]/) {
          s = t; sub(/^((public|static|async|get|set|override)[ \t]+)+/, "", s); def(ident(s), sign, t)
        }
      }
    }
    END {
      for (k in M) if (!(k in P)) { split(k, x, SUBSEP); out[x[1]] = 1 }
      for (k in P) if (!(k in M)) { split(k, x, SUBSEP); out[x[1]] = 1 }
      for (id in out) print id
    }' | sort
}

# forge_adr_accepted <file>: the ADR's status is accepted: MADR front matter
# `status: accepted`, or a Status heading whose first non-empty line starts with Accepted.
forge_adr_accepted() {
  awk '
    NR == 1 && /^---[ \t]*$/ { fm = 1; next }
    fm && /^---[ \t]*$/ { fm = 0; next }
    fm { if (tolower($0) ~ /^status:[ \t]*["\047]?accepted["\047]?[ \t]*$/) ok = 1; next }
    /^#+[ \t]+[Ss][Tt][Aa][Tt][Uu][Ss][ \t]*$/ { st = 1; next }
    st && /^#/ { st = 0 }
    st && NF { w = tolower($1); gsub(/[^a-z]/, "", w); if (w == "accepted") ok = 1; st = 0 }
    END { exit !ok }' "$1"
}

# forge_adr_body <file>: the ADR without its status (the front matter status line, the
# content of the Status section).
forge_adr_body() {
  awk '
    NR == 1 && /^---[ \t]*$/ { fm = 1; print; next }
    fm && /^---[ \t]*$/ { fm = 0; print; next }
    fm && tolower($0) ~ /^status:/ { next }
    fm { print; next }
    /^#+[ \t]+[Ss][Tt][Aa][Tt][Uu][Ss][ \t]*$/ { st = 1; print; next }
    st && /^#/ { st = 0 }
    st { next }
    { print }' "$1"
}

# forge_adr_edits: the changed ADRs ($FORGE_FILES: file role @adr) that were accepted in
# the base and whose change touches more than their status; one line each.
forge_adr_edits() {
  for _ae_f in $FORGE_FILES; do
    case $_ae_f in *.md|*.mdx|*.markdown|*.rst) ;; *) continue ;; esac
    [ -f "$FORGE_TMP/base/$_ae_f" ] && [ -f "$_ae_f" ] || continue
    forge_adr_accepted "$FORGE_TMP/base/$_ae_f" || continue
    forge_adr_body "$FORGE_TMP/base/$_ae_f" > "$FORGE_TMP/adr.base"
    forge_adr_body "$_ae_f" > "$FORGE_TMP/adr.now"
    cmp -s "$FORGE_TMP/adr.base" "$FORGE_TMP/adr.now" && continue
    printf '%s: an accepted ADR was rewritten; an ADR is a record of a decision: only its status may change (superseded, deprecated), and the new decision goes in a new ADR that supersedes it\n' "$_ae_f"
  done
  return 0
}

# forge_doc_files: documentation in the repository (tracked or not ignored):
# Markdown, MDX and reStructuredText under a docs/ or doc/ directory, and README files.
FORGE_DOC_ERE='(^|/)(docs?/(.*/)?[^/]*\.(md|mdx|rst)|README[^/]*)$'

# forge_docs_mentions: when no documentation file changed in the change, the
# lines of the documentation that mention an identifier the change defined
# differently (forge_changed_ids): up to 5 "path:line identifier" lines. A name
# with an underscore or a capital inside counts anywhere; a plain word only in a
# code span or a fenced block.
forge_docs_mentions() {
  grep -Eq -- "$FORGE_DOC_ERE" "$FORGE_TMP/changed" && return 0
  forge_changed_ids > "$FORGE_TMP/doc.ids"
  [ -s "$FORGE_TMP/doc.ids" ] || return 0
  git ls-files --cached --others --exclude-standard 2>/dev/null | grep -E -- "$FORGE_DOC_ERE" | while IFS= read -r _dm_f; do
    [ -f "$_dm_f" ] && grep -qF -f "$FORGE_TMP/doc.ids" "$_dm_f" 2>/dev/null && printf '%s\n' "$_dm_f"
  done > "$FORGE_TMP/doc.files"
  [ -s "$FORGE_TMP/doc.files" ] || return 0
  # shellcheck disable=SC2046
  awk '
    function word(s, id,   i, b, a) {
      while (i = index(s, id)) {
        b = substr(s, i - 1, 1); a = substr(s, i + length(id), 1)
        if ((i == 1 || b !~ /[A-Za-z0-9_$]/) && a !~ /[A-Za-z0-9_$]/) return 1
        s = substr(s, i + length(id))
      }
      return 0
    }
    function spans(s,   n, p, i, out) { n = split(s, p, "`"); out = ""; for (i = 2; i <= n; i += 2) out = out " " p[i]; return out }
    FILENAME == ARGV[1] { ids[++nid] = $0; plain[nid] = ($0 !~ /_/ && $0 !~ /.[A-Z]/); next }
    FNR == 1 { fence = 0 }
    /^[ \t]*(```|~~~)/ { fence = !fence; next }
    {
      code = fence ? $0 : spans($0)
      for (j = 1; j <= nid; j++)
        if (word(plain[j] ? code : $0, ids[j])) { print FILENAME ":" FNR " " ids[j]; break }
    }' "$FORGE_TMP/doc.ids" $(cat "$FORGE_TMP/doc.files") > "$FORGE_TMP/doc.hits"
  [ -s "$FORGE_TMP/doc.hits" ] || return 0
  printf 'These docs mention identifiers you changed; check they still describe the behaviour:\n'
  awk 'NR <= 5 { print "  - " $0 } END { if (NR > 5) print "  (and " NR - 5 " more)" }' "$FORGE_TMP/doc.hits"
}

# forge_py_public <file>: the public names of a Python module, one per line: the
# entries of __all__ when it has one, else its top-level def and class names
# without a leading underscore.
forge_py_public() {
  awk '
    /^__all__[ \t]*(:[^=]*)?\+?=/ { all = 1; inall = 1 }
    inall {
      s = $0
      while (match(s, /["\047][A-Za-z_][A-Za-z0-9_]*["\047]/)) { a[substr(s, RSTART + 1, RLENGTH - 2)] = 1; s = substr(s, RSTART + RLENGTH) }
      if ($0 ~ /[\])]/) inall = 0
      next
    }
    /^(async[ \t]+)?def[ \t]+[A-Za-z]/ || /^class[ \t]+[A-Za-z]/ {
      s = $0; sub(/^(async[ \t]+)?(def|class)[ \t]+/, "", s); match(s, /^[A-Za-z0-9_]+/); d[substr(s, 1, RLENGTH)] = 1
    }
    END { if (all) { for (k in a) print k } else for (k in d) print k }' "$1" 2>/dev/null | sort -u
}

# forge_py_module <path>: the module path inside its top-level package
# ("experimental/query_client" for logfire/experimental/query_client.py and for
# logfire-api/logfire_api/experimental/query_client.pyi): directories with an
# __init__.py or __init__.pyi count as packages, the topmost one is dropped.
forge_py_module() {
  _pm_d=$(dirname "$1"); _pm_m=$(basename "$1"); _pm_m=${_pm_m%.py}; _pm_m=${_pm_m%.pyi}
  _pm_top=''
  while [ "$_pm_d" != . ] && [ "$_pm_d" != / ] && { [ -f "$_pm_d/__init__.py" ] || [ -f "$_pm_d/__init__.pyi" ]; }; do
    [ -n "$_pm_top" ] && _pm_m="$_pm_top/$_pm_m"
    _pm_top=$(basename "$_pm_d"); _pm_d=$(dirname "$_pm_d")
  done
  printf '%s\n' "$_pm_m"
}

# forge_stub_sync: for each changed module ($FORGE_FILES) that gained public names
# (forge_py_public, compared with the base), each .pyi stub in the repository with
# the same module path (forge_py_module) must mention every new name.
forge_stub_sync() {
  git ls-files --cached --others --exclude-standard -- '*.pyi' 2>/dev/null | while IFS= read -r _ss_s; do
    [ -f "$_ss_s" ] && printf '%s\t%s\n' "$(forge_py_module "$_ss_s")" "$_ss_s"
  done > "$FORGE_TMP/stubs"
  [ -s "$FORGE_TMP/stubs" ] || return 0
  for _ss_f in $FORGE_FILES; do
    case $_ss_f in *.py) ;; *) continue ;; esac
    _ss_mod=$(forge_py_module "$_ss_f")
    awk -F '\t' -v m="$_ss_mod" '$1 == m { print $2 }' "$FORGE_TMP/stubs" > "$FORGE_TMP/stubs.for"
    [ -s "$FORGE_TMP/stubs.for" ] || continue
    forge_py_public "$_ss_f" > "$FORGE_TMP/names.now"
    : > "$FORGE_TMP/names.then"
    if [ -f "$FORGE_TMP/base/$_ss_f" ]; then
      { forge_py_public "$FORGE_TMP/base/$_ss_f"
        awk '/^(async[ \t]+)?(def|class)[ \t]+[A-Za-z_]/ { s = $0; sub(/^(async[ \t]+)?(def|class)[ \t]+/, "", s); match(s, /^[A-Za-z0-9_]+/); print substr(s, 1, RLENGTH) }' "$FORGE_TMP/base/$_ss_f"
      } | sort -u > "$FORGE_TMP/names.then"
    fi
    grep -vxF -f "$FORGE_TMP/names.then" "$FORGE_TMP/names.now" > "$FORGE_TMP/names.new"
    [ -s "$FORGE_TMP/names.new" ] || continue
    while IFS= read -r _ss_s; do
      _ss_miss=''
      while IFS= read -r _ss_n; do
        awk -v id="$_ss_n" '
          { s = $0
            while (i = index(s, id)) {
              b = substr(s, i - 1, 1); a = substr(s, i + length(id), 1)
              if ((i == 1 || b !~ /[A-Za-z0-9_]/) && a !~ /[A-Za-z0-9_]/) { f = 1; exit }
              s = substr(s, i + length(id))
            } }
          END { exit !f }' "$_ss_s" || _ss_miss="${_ss_miss:+$_ss_miss, }$_ss_n"
      done < "$FORGE_TMP/names.new"
      [ -n "$_ss_miss" ] && printf 'The stub %s does not have %s, new in %s; add it there too.\n' "$_ss_s" "$_ss_miss" "$_ss_f"
    done < "$FORGE_TMP/stubs.for"
  done
}

# Output values, for forge_snapshot_stale: string and template literals in expressions
# (not import / export sources, type annotations, interfaces or type aliases), and the
# keys of object literals (pair, shorthand property).
FORGE_SNAP_VALUES='id: forge-snap-str
language: Tsx
rule:
  any: [{kind: string}, {kind: template_string}]
  not:
    inside:
      stopBy: end
      any: [{kind: import_statement}, {kind: export_statement, has: {field: source, kind: string}}, {kind: type_annotation}, {kind: literal_type}, {kind: interface_declaration}, {kind: type_alias_declaration}, {kind: type_arguments}]
---
id: forge-snap-key
language: Tsx
rule:
  any: [{kind: pair}, {kind: shorthand_property_identifier}]
  inside: {kind: object, stopBy: neighbor}'

# forge_snapshot_stale: Stop only (--turn). A changed source file ($FORGE_FILES) whose
# added lines hold a string literal in an expression or an object literal key, imported directly (relative import) by a
# test that compares output with snapshots or golden files (toMatchSnapshot,
# toMatchFileSnapshot, toMatchInlineSnapshot, testdata/, golden), while no .snap,
# __snapshots__, testdata or golden file changed in the turn. A test that uses only
# inline snapshots counts as updated when it changed. Run as advice (forge_advise).
forge_snapshot_stale() {
  [ "$mode" = --turn ] || return 0
  grep -qE '\.snap$|(^|/)(__snapshots__|testdata|golden|__golden__)/|\.golden(\.|$)' "$FORGE_TMP/changed" && return 0
  : > "$FORGE_TMP/snap.src"
  : > "$FORGE_TMP/snap.cand"
  for _ss_f in $FORGE_FILES; do
    [ -f "$_ss_f" ] && awk -F '\t' -v f="$_ss_f" '$1 == f { found = 1; exit } END { exit !found }' "$FORGE_TMP/added" \
      && printf '%s\n' "$_ss_f" >> "$FORGE_TMP/snap.cand"
  done
  [ -s "$FORGE_TMP/snap.cand" ] || return 0
  # output values on added lines: string literals in expressions and object literal keys
  # (not imports, type annotations, interfaces or type aliases)
  set -f
  # shellcheck disable=SC2046
  astgrep scan -c "$FORGE_ROOT/.forge/ast-grep/sgconfig.yml" --inline-rules "$FORGE_SNAP_VALUES" --filter '^forge-snap' --json=stream $(cat "$FORGE_TMP/snap.cand") > "$FORGE_TMP/snap.json"
  _ss_rc=$?
  set +f
  [ "$_ss_rc" -le 1 ] || { printf 'forge:%s skipped: ast-grep unavailable\n' "$c_id"; return 0; }
  awk '
    function str(s, key,   i, out, c) {
      i = index(s, "\"" key "\":\""); if (!i) return ""
      s = substr(s, i + length(key) + 4); out = ""
      while (length(s)) { c = substr(s, 1, 1); if (c == "\"") break; if (c == "\\") { out = out substr(s, 1, 2); s = substr(s, 3); continue } out = out c; s = substr(s, 2) }
      return out
    }
    function num(s, key,   i) { i = index(s, "\"" key "\":{\"line\":"); return i ? substr(s, i + length(key) + 11) + 1 : 0 }
    FILENAME == ARGV[1] { split($0, p, "\t"); if (p[2] == "*") all[p[1]] = 1; else want[p[1] "\t" p[2]] = 1; next }
    {
      f = str($0, "file"); a = num($0, "start"); b = num($0, "end")
      if (str($0, "ruleId") == "forge-snap-key") b = a
      for (l = a; l <= b; l++) if ((f in all) || ((f "\t" l) in want)) { if (!seen[f]++) print f; break }
    }' "$FORGE_TMP/added" "$FORGE_TMP/snap.json" > "$FORGE_TMP/snap.src"
  [ -s "$FORGE_TMP/snap.src" ] || return 0
  git grep --untracked -lE 'toMatch(File|Inline)?Snapshot\(|testdata|golden' -- '*.test.*' '*.spec.*' '*/__tests__/*' > "$FORGE_TMP/snap.tests" 2>/dev/null
  : > "$FORGE_TMP/snap.imports"
  while IFS= read -r _ss_t; do
    [ -f "$_ss_t" ] || continue
    if grep -qxF "$_ss_t" "$FORGE_TMP/changed" && ! grep -qE 'toMatch(File)?Snapshot\(|testdata|golden' "$_ss_t"; then continue; fi
    sed -n -E "s/.*(from|require\(|import\()[[:space:]]*[\"'](\.[^\"']*)[\"'].*/\2/p" "$_ss_t" \
      | awk -v t="$_ss_t" '{ print t "\t" $0 }' >> "$FORGE_TMP/snap.imports"
  done < "$FORGE_TMP/snap.tests"
  awk -F '\t' '
    function stem(p) { sub(/\.(d\.)?[cm]?[jt]sx?$/, "", p); return p }
    function norm(p,   n, a, i, o, k) {
      n = split(p, a, "/"); k = 0
      for (i = 1; i <= n; i++) {
        if (a[i] == "" || a[i] == ".") continue
        if (a[i] == ".." && k > 0) { k--; continue }
        o[++k] = a[i]
      }
      p = ""; for (i = 1; i <= k; i++) p = p (i > 1 ? "/" : "") o[i]
      return p
    }
    FILENAME == ARGV[1] { src[stem($0)] = $0; next }
    {
      d = $1; if (d ~ /\//) sub(/\/[^\/]*$/, "", d); else d = "."
      s = stem(norm(d "/" $2))
      f = (s in src) ? src[s] : ((s "/index") in src) ? src[s "/index"] : ""
      if (f != "" && !seen[f "\t" $1]++) print f ": imported by " $1 ", which compares output with snapshots or golden files"
    }' "$FORGE_TMP/snap.src" "$FORGE_TMP/snap.imports" | sort | head -n 10 > "$FORGE_TMP/snap.out"
  [ -s "$FORGE_TMP/snap.out" ] || return 0
  cat "$FORGE_TMP/snap.out"
  printf 'Output-producing code changed but its snapshots did not: run the snapshot tests with inputs that exercise the new output and update them (snapshot updates still go through no-snapshot-update).\n'
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

# forge_check_active <rule-id>: the rule has a check in this check.sh.
forge_check_active() {
  (
    _ca_i=1
    while [ "$_ca_i" -le "${FORGE_NCHECKS:-0}" ]; do
      forge_check_meta_"$_ca_i"
      [ "$c_id" = "$1" ] && exit 0
      _ca_i=$((_ca_i + 1))
    done
    exit 1
  )
}

# forge_lockfile_updated: a package.json whose dependencies changed since the base
# needs a lock file whose changed lines name each changed dependency (added, removed or
# re-versioned), when the project has a pnpm, npm or Yarn lock file (next to it or at
# the root); a lock change that names none (a new blank line) does not count. bun.lock
# is left to typescript/bun/lock-sync (bun install --frozen-lockfile) when that check is
# active. Run with forge_once: at Stop it is reported once per finding (the package manager
# may not be able to run here); pre-commit and CI always fail on it.
forge_lockfile_updated() {
  _lu_rc=0
  while IFS= read -r _lu_f; do
    case $_lu_f in package.json|*/package.json) ;; *) continue ;; esac
    case /$_lu_f in */node_modules/*) continue ;; esac
    _lu_lock=''
    _lu_ifs=$IFS; IFS=' '
    for _lu_l in $FORGE_JS_LOCKS; do
      if [ -f "$(dirname "$_lu_f")/$_lu_l" ]; then _lu_lock=$(dirname "$_lu_f")/$_lu_l; break; fi
      if [ -f "$_lu_l" ]; then _lu_lock=$_lu_l; break; fi
    done
    IFS=$_lu_ifs
    _lu_lock=${_lu_lock#./}
    case $_lu_lock in
      '') continue ;;
      bun.lock|bun.lockb|*/bun.lock|*/bun.lockb) forge_check_active typescript/bun/lock-sync && continue ;;
    esac
    forge_pkg_deps "$FORGE_TMP/base/$_lu_f" > "$FORGE_TMP/deps.then"
    forge_pkg_deps "$_lu_f" > "$FORGE_TMP/deps.now"
    cmp -s "$FORGE_TMP/deps.then" "$FORGE_TMP/deps.now" && continue
    forge_diff_lines "$_lu_lock" > "$FORGE_TMP/lock.diff"
    awk -F '\t' '
      FILENAME == ARGV[1] { then[$1 " " $2] = $3; name[$1 " " $2] = $2; next }
      FILENAME == ARGV[2] { now[$1 " " $2] = $3; name[$1 " " $2] = $2; next }
      { diff = diff "\n" $0 }
      END {
        for (k in now) if (!(k in then)) w[k] = k " " now[k] " added"; else if (then[k] != now[k]) w[k] = k " " then[k] " -> " now[k]
        for (k in then) if (!(k in now)) w[k] = k " removed"
        for (k in w) if (!index(diff, name[k])) print w[k]
      }' "$FORGE_TMP/deps.then" "$FORGE_TMP/deps.now" "$FORGE_TMP/lock.diff" | sort > "$FORGE_TMP/deps.missing"
    [ -s "$FORGE_TMP/deps.missing" ] || continue
    _lu_what=$(head -n 5 "$FORGE_TMP/deps.missing" | awk 'NR > 1 { printf "; " } { printf "%s", $0 }')
    printf '%s: %s, but %s has no changed line for it\n' "$_lu_f" "$_lu_what" "$_lu_lock"
    _lu_rc=1
  done < "$FORGE_TMP/existing"
  [ "$_lu_rc" = 0 ] && return 0
  printf 'Run the package manager (bun install, pnpm install, npm install, yarn) to update the lock file; it is not edited by hand. If it cannot run here, tell the user.\n'
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

# forge_dockerfiles: the files of $FORGE_FILES in file role @dockerfile (§15.4), else every tracked one.
forge_dockerfiles() {
  if [ -n "$FORGE_FILES" ]; then printf '%s\n' "$FORGE_FILES"; else git ls-files 2>/dev/null; fi \
    | while IFS= read -r _df_f; do forge_role @dockerfile "$_df_f" && printf '%s\n' "$_df_f"; done
  return 0
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

# ---- pinned release binaries (§13.5): tools with no PyPI or npm distribution, run from
# their GitHub release, pinned by version and by each platform's sha256.
# "<name> <version> <platform> <url> <sha256>" per line; platform = `uname -s`-`uname -m`.
FORGE_RELEASES='@@RELEASES@@'

# forge_release_bin <name>: prints the path of the pinned <name> binary. The first use on a
# machine downloads the release asset (curl or wget; in check.sh, outside the Bash
# sandbox), checks its sha256, unpacks the binary (.tar.gz, .zip, or the asset itself) and
# keeps it in ${XDG_CACHE_HOME:-~/.cache}/forge/bin/<name>-<version>-<platform>/<name>.
# Fails, printing why (one line), when the platform has no pinned asset, the download
# fails (offline) or the asset does not match its sha256: the caller reports the check
# skipped, never passed.
forge_release_bin() {
  _rb_plat=$(uname -s)-$(uname -m)
  _rb_row=$(printf '%s\n' "$FORGE_RELEASES" | awk -v n="$1" -v p="$_rb_plat" '$1 == n && $3 == p { print $2 " " $4 " " $5; exit }')
  if [ -z "$_rb_row" ]; then printf 'no pinned %s release binary for %s' "$1" "$_rb_plat"; return 1; fi
  _rb_ver=${_rb_row%% *}; _rb_url=${_rb_row#* }; _rb_sha=${_rb_url#* }; _rb_url=${_rb_url%% *}
  _rb_dir=${XDG_CACHE_HOME:-$HOME/.cache}/forge/bin/$1-$_rb_ver-$_rb_plat
  if [ -f "$_rb_dir/$1" ] && [ -x "$_rb_dir/$1" ]; then printf '%s' "$_rb_dir/$1"; return 0; fi
  mkdir -p "$_rb_dir" 2>/dev/null && _rb_tmp=$(mktemp -d "$_rb_dir/.fetch.XXXXXX" 2>/dev/null) \
    || { printf 'cannot write the cache directory %s' "$_rb_dir"; return 1; }
  if command -v curl >/dev/null 2>&1; then curl -fsSL --retry 2 --connect-timeout 15 -o "$_rb_tmp/asset" "$_rb_url" 2>/dev/null
  elif command -v wget >/dev/null 2>&1; then wget -q -T 15 -O "$_rb_tmp/asset" "$_rb_url" 2>/dev/null
  else rm -rf "$_rb_tmp"; printf 'curl or wget is needed to fetch %s %s' "$1" "$_rb_ver"; return 1
  fi || { rm -rf "$_rb_tmp"; printf 'could not download %s %s (offline?)' "$1" "$_rb_ver"; return 1; }
  _rb_have=$(forge_sha256 < "$_rb_tmp/asset")
  if [ "$_rb_have" != "$_rb_sha" ]; then
    rm -rf "$_rb_tmp"; printf 'the downloaded %s %s does not match its pinned sha256 (got %s)' "$1" "$_rb_ver" "$_rb_have"; return 1
  fi
  case $_rb_url in
    *.tar.gz|*.tgz) ( cd "$_rb_tmp" && tar -xzf asset "$1" ) >/dev/null 2>&1 ;;
    *.zip)
      if ! command -v unzip >/dev/null 2>&1; then rm -rf "$_rb_tmp"; printf 'unzip is needed to unpack %s %s' "$1" "$_rb_ver"; return 1; fi
      ( cd "$_rb_tmp" && unzip -q -o asset "$1" ) >/dev/null 2>&1 ;;
    *) mv "$_rb_tmp/asset" "$_rb_tmp/$1" ;;
  esac
  if [ ! -f "$_rb_tmp/$1" ] || ! chmod 755 "$_rb_tmp/$1" || ! mv -f "$_rb_tmp/$1" "$_rb_dir/$1"; then
    rm -rf "$_rb_tmp"; printf 'could not unpack %s %s' "$1" "$_rb_ver"; return 1
  fi
  rm -rf "$_rb_tmp"
  printf '%s' "$_rb_dir/$1"
}

# forge_release <name> <var>: sets <var> to the pinned binary's path; when it cannot be had,
# prints "forge:<id> skipped: <why>" for the current check and fails.
forge_release() {
  if _rl_bin=$(forge_release_bin "$1"); then eval "$2=\$_rl_bin"; return 0; fi
  printf 'forge:%s skipped: %s\n' "$c_id" "$_rl_bin"
  return 1
}

# forge_scope_files: $FORGE_FILES, else (--all) every tracked file that exists, one per line.
forge_scope_files() {
  if [ -n "$FORGE_FILES" ]; then printf '%s\n' "$FORGE_FILES"
  else git ls-files 2>/dev/null | while IFS= read -r _sf_f; do [ -f "$_sf_f" ] && printf '%s\n' "$_sf_f"; done
  fi
}

# ---- secrets (secrets/*)

# Forge's gitleaks config: gitleaks' default rules without generic-api-key (it decides on
# identifier names and entropy), plus vendor key formats the defaults lack.
FORGE_GITLEAKS_CONFIG='title = "forge"

[extend]
useDefault = true
disabledRules = ["generic-api-key"]

[[rules]]
id = "langsmith-api-key"
description = "LangSmith API key"
regex = "\\blsv2_(?:pt|sk)_[a-f0-9]{32}_[a-f0-9]{10}\\b"
keywords = ["lsv2_"]

[[rules]]
id = "pinecone-api-key"
description = "Pinecone API key"
regex = "\\bpcsk_[A-Za-z0-9_]{40,}\\b"
keywords = ["pcsk_"]'

# forge_gitleaks: "path:line rule: description" for each secret the pinned gitleaks finds in
# the in-scope files (forge_scope_files), scanned as copies with Forge's config only: the
# project's .gitleaks.toml, GITLEAKS_CONFIG, .gitleaksignore and inline gitleaks:allow
# comments do not apply. Secrets are redacted; findings count on added lines (lint).
forge_gitleaks() {
  forge_release gitleaks _gl_bin || return 0
  _gl=$FORGE_TMP/gitleaks
  rm -rf "$_gl"
  mkdir -p "$_gl/scan" "$_gl/ignore" || return 1
  forge_scope_files > "$_gl/files"
  [ -s "$_gl/files" ] || return 0
  tar -cf - -T "$_gl/files" 2>/dev/null | ( cd "$_gl/scan" && tar -xf - 2>/dev/null )
  printf '%s\n' "$FORGE_GITLEAKS_CONFIG" > "$_gl/config.toml"
  printf '{{range .}}{{.File}}:{{.StartLine}} {{.RuleID}}: {{.Description}}\n{{end}}' > "$_gl/report.tmpl"
  if ! ( cd "$_gl/scan" && "$_gl_bin" dir --no-banner --no-color --redact -l error -c "$_gl/config.toml" -i "$_gl/ignore" --ignore-gitleaks-allow \
      --exit-code 0 --report-format template --report-template "$_gl/report.tmpl" --report-path "$_gl/report" . ) > "$_gl/err" 2>&1; then
    printf 'forge:%s skipped: gitleaks could not run (%s)\n' "$c_id" "$(grep -v '^[[:space:]]*$' "$_gl/err" | tail -n 1)"
    return 0
  fi
  sed 's|^\./||' "$_gl/report" | sort -u > "$_gl/found"
  [ -s "$_gl/found" ] || return 0
  cat "$_gl/found"
  return 1
}

# Credential containers: keystores and private keys by their file type, and the auth files
# of pip, netrc and git (secrets/no-credential-files).
FORGE_CREDENTIAL_FILES='(^|/)([^/]+\.(p12|pfx|jks|keystore|key)|id_(rsa|ecdsa|ed25519)|\.pypirc|\.netrc|\.git-credentials)$'

# forge_credential_files: "path:0 …" for each tracked or added credential container file,
# and "path:line …" for each .npmrc line giving _authToken, _auth or _password a literal
# value (a ${VAR} reference is fine); lint attribution keeps those of the change.
forge_credential_files() {
  _cf_rc=0
  { git ls-files 2>/dev/null; cat "$FORGE_TMP/existing"; } | sort -u > "$FORGE_TMP/cred.files"
  while IFS= read -r _cf_f; do
    [ -f "$_cf_f" ] || continue
    if printf '%s\n' "$_cf_f" | grep -qE "$FORGE_CREDENTIAL_FILES"; then
      printf '%s:0 is a credential file; keep it out of the repository (git rm --cached, .gitignore) and load it from a secret store\n' "$_cf_f"; _cf_rc=1
    fi
    case $_cf_f in
      .npmrc|*/.npmrc)
        grep -nE '(_authToken|_auth|_password)[[:space:]]*=[[:space:]]*[^$[:space:]]' "$_cf_f" 2>/dev/null \
          | awk -v f="$_cf_f" -F ':' '{ print f ":" $1 " sets a registry credential literally; use ${NPM_TOKEN}-style environment references"; bad = 1 } END { exit bad }' || _cf_rc=1 ;;
    esac
  done < "$FORGE_TMP/cred.files"
  return "$_cf_rc"
}

# ---- git hygiene (git/*)

# forge_large_files <bytes>: each in-scope file (forge_scope_files) larger than <bytes> that is
# new or grew in the change, and that Git LFS does not store (its filter attribute is not lfs).
forge_large_files() {
  _lg_max=$1
  _lg_rc=0
  forge_scope_files > "$FORGE_TMP/large.files"
  while IFS= read -r _lg_f; do
    [ -f "$_lg_f" ] || continue
    _lg_n=$(wc -c < "$_lg_f" | tr -d ' ')
    [ "$_lg_n" -gt "$_lg_max" ] || continue
    if [ -f "$FORGE_TMP/base/$_lg_f" ] && [ "$(wc -c < "$FORGE_TMP/base/$_lg_f" | tr -d ' ')" -ge "$_lg_n" ]; then continue; fi
    [ "$(git check-attr filter -- "$_lg_f" 2>/dev/null | sed 's/^.*: filter: //')" = lfs ] && continue
    printf '%s:0 is %s KiB, over the %s KiB limit, and Git LFS does not store it: keep it out of the repository or track it with git lfs track\n' "$_lg_f" "$((_lg_n / 1024))" "$((_lg_max / 1024))"
    _lg_rc=1
  done < "$FORGE_TMP/large.files"
  return "$_lg_rc"
}

# forge_generated_edits: each changed in-scope file ($FORGE_FILES) marked generated:
# linguist-generated in .gitattributes (git check-attr), or "@generated" or "Code generated …
# DO NOT EDIT." in its first 15 lines. Run as advice (forge_advise): a regeneration changes it too.
forge_generated_edits() {
  printf '%s\n' "$FORGE_FILES" | while IFS= read -r _ge_f; do
    [ -f "$_ge_f" ] || continue
    if [ "$(git check-attr linguist-generated -- "$_ge_f" 2>/dev/null | sed 's/^.*: linguist-generated: //')" = true ]; then _ge_why='.gitattributes marks it linguist-generated'
    elif head -n 15 "$_ge_f" | grep -qE '@generated|Code generated .* DO NOT EDIT\.'; then _ge_why='its header says it is generated'
    else continue
    fi
    printf '%s is generated (%s): change its source or generator and regenerate it, rather than editing it by hand\n' "$_ge_f" "$_ge_why"
  done
}

# forge_unformatted <list>: the paths of <list> that no format check of this setup covers
# (a formatter owns the others), one per line.
forge_unformatted() {
  ( _uf_i=1
    while [ "$_uf_i" -le "$FORGE_NCHECKS" ]; do
      forge_check_meta_"$_uf_i"
      [ "$c_stage" = format ] && forge_filter "forge_scope_$_uf_i" < "$1"
      _uf_i=$((_uf_i + 1))
    done ) | sort -u > "$FORGE_TMP/formatted"
  grep -vxF -f "$FORGE_TMP/formatted" "$1"
  return 0
}

# forge_editorconfig: when the repository has an .editorconfig, the pinned editorconfig-checker
# on the in-scope files (forge_scope_files) no format check formats, with a config of Forge's
# own (the project's .editorconfig-checker.json excludes and disables do not apply);
# "path:line:column: message" per violation.
forge_editorconfig() {
  [ -f .editorconfig ] || return 0
  forge_scope_files > "$FORGE_TMP/ec.scope"
  forge_unformatted "$FORGE_TMP/ec.scope" > "$FORGE_TMP/ec.files"
  [ -s "$FORGE_TMP/ec.files" ] || return 0
  forge_release editorconfig-checker _ec_bin || return 0
  printf '{}\n' > "$FORGE_TMP/ec.json"
  set -f
  # shellcheck disable=SC2046
  "$_ec_bin" -config "$FORGE_TMP/ec.json" -no-color -format gcc $(cat "$FORGE_TMP/ec.files") > "$FORGE_TMP/ec.out" 2>&1
  _ec_rc=$?
  set +f
  case $_ec_rc in 0) return 0 ;; 1) ;; *) printf 'forge:%s skipped: editorconfig-checker could not run (%s)\n' "$c_id" "$(tail -n 1 "$FORGE_TMP/ec.out")"; return 0 ;; esac
  grep -E '^[^[:space:]:]+:[0-9]+:[0-9]+: ' "$FORGE_TMP/ec.out" | sed 's/: error: /: /'
  return 1
}

# ---- dependency vulnerabilities (supply-chain/vuln-audit)

# forge_osv: for each in-scope lock file of the change ($FORGE_FILES), the pinned osv-scanner
# (one backend for every ecosystem; an empty config of Forge's, so osv-scanner.toml
# [[IgnoredVulns]] do not apply) on it and on its base copy: "path:0 package@version id …" for
# each vulnerability (package, version, advisory) it has now and the base did not. Needs
# api.osv.dev: unreachable, it is reported skipped. Not with --all (no base to compare).
forge_osv() {
  [ "$mode" != --all ] || return 0
  [ -n "$FORGE_FILES" ] || return 0
  forge_release osv-scanner _ov_bin || return 0
  : > "$FORGE_TMP/osv.toml"
  _ov_rc=0
  printf '%s\n' "$FORGE_FILES" > "$FORGE_TMP/osv.files"
  while IFS= read -r _ov_f; do
    case $_ov_f in
      *.lockb) printf 'forge:%s skipped: %s is binary; osv-scanner reads bun.lock (bun install --save-text-lockfile)\n' "$c_id" "$_ov_f"; continue ;;
    esac
    forge_osv_scan "$_ov_f" > "$FORGE_TMP/osv.now" || { printf 'forge:%s skipped: osv-scanner could not scan %s (%s)\n' "$c_id" "$_ov_f" "$(cat "$FORGE_TMP/osv.why")"; continue; }
    : > "$FORGE_TMP/osv.then"
    if [ -f "$FORGE_TMP/base/$_ov_f" ]; then
      ( cd "$FORGE_TMP/base" && forge_osv_scan "$_ov_f" ) > "$FORGE_TMP/osv.then" || { printf 'forge:%s skipped: osv-scanner could not scan the base %s (%s)\n' "$c_id" "$_ov_f" "$(cat "$FORGE_TMP/osv.why")"; continue; }
    fi
    awk -F '\t' -v f="$_ov_f" 'FILENAME == ARGV[1] { old[$1 "\t" $2 "\t" $3] = 1; next }
      !(($1 "\t" $2 "\t" $3) in old) { printf "%s:0 %s@%s has a known vulnerability, %s (fixed in %s); use a fixed version\n", f, $1, $2, $3, ($4 == "" ? "no release yet" : $4); bad = 1 }
      END { exit bad }' "$FORGE_TMP/osv.then" "$FORGE_TMP/osv.now" || _ov_rc=1
  done < "$FORGE_TMP/osv.files"
  return "$_ov_rc"
}

# forge_osv_scan <lock>: "package<TAB>version<TAB>advisory id<TAB>fixed version" for each
# vulnerability osv-scanner reports on <lock> (from the current directory), sorted; fails
# with the reason in $FORGE_TMP/osv.why when it cannot scan (offline: exit 127).
forge_osv_scan() {
  case ${1##*/} in requirements*.txt) set -- "requirements.txt:$1" ;; esac
  "$_ov_bin" scan source --config "$FORGE_TMP/osv.toml" --format markdown -L "$1" > "$FORGE_TMP/osv.md" 2> "$FORGE_TMP/osv.err"
  case $? in
    0|1) ;;
    *) grep -v '^[[:space:]]*$' "$FORGE_TMP/osv.err" | tail -n 1 > "$FORGE_TMP/osv.why"; return 1 ;;
  esac
  # | https://osv.dev/<id>[<br/>…] | CVSS | Ecosystem | Package | Version | Fixed Version | Source |
  awk -F '|' '$2 ~ /https:\/\/osv\.dev\// {
      id = $2; sub(/^[[:space:]]*https:\/\/osv\.dev\//, "", id); sub(/<br\/>.*$/, "", id); sub(/[[:space:]]+$/, "", id)
      for (i = 5; i <= 7; i++) { gsub(/^[[:space:]]+|[[:space:]]+$/, "", $i) }
      fixed = $7; if (fixed == "--") fixed = ""
      print $5 "\t" $6 "\t" id "\t" fixed
    }' "$FORGE_TMP/osv.md" | sort -u
}

# ---- OpenAPI (openapi/no-breaking)

# forge_oasdiff: each OpenAPI document of the repository (forge_openapi_spec) that existed at the
# base, compared by the pinned oasdiff with its base version: "error at <path>, in API … [id]."
# for each breaking change (level ERR). The base side is a copy of the repository's YAML and
# JSON files as they were, so multi-file $refs resolve. A line recorded in forge.yaml (accept:
# change) is accepted. Not with --all (no base to compare).
forge_oasdiff() {
  [ "$mode" != --all ] || return 0
  git ls-files --cached --others --exclude-standard -- '*.yaml' '*.yml' '*.json' 2>/dev/null > "$FORGE_TMP/oas.all"
  forge_filter forge_openapi_spec < "$FORGE_TMP/oas.all" > "$FORGE_TMP/oas.roots"
  [ -s "$FORGE_TMP/oas.roots" ] || return 0
  forge_release oasdiff _oa_bin || return 0
  _oa_base=$FORGE_TMP/oas-base
  rm -rf "$_oa_base"; mkdir -p "$_oa_base"
  tar -cf - -T "$FORGE_TMP/oas.all" 2>/dev/null | ( cd "$_oa_base" && tar -xf - 2>/dev/null )
  while IFS= read -r _oa_f; do
    rm -f "$_oa_base/$_oa_f"
    if [ -f "$FORGE_TMP/base/$_oa_f" ]; then mkdir -p "$_oa_base/$(dirname "$_oa_f")" && cp "$FORGE_TMP/base/$_oa_f" "$_oa_base/$_oa_f"; fi
  done < "$FORGE_TMP/changed"
  : > "$FORGE_TMP/oas.out"
  while IFS= read -r _oa_f; do
    [ -f "$_oa_base/$_oa_f" ] || continue
    "$_oa_bin" breaking "$_oa_base/$_oa_f" "$_oa_f" --fail-on ERR --format singleline > "$FORGE_TMP/oas.one" 2>&1
    case $? in
      0|1) ;;
      *) printf 'forge:%s skipped: oasdiff could not compare %s (%s)\n' "$c_id" "$_oa_f" "$(grep -v '^[[:space:]]*$' "$FORGE_TMP/oas.one" | tail -n 1)"; continue ;;
    esac
    grep '^error at ' "$FORGE_TMP/oas.one" | sed "s|$_oa_base/||g; s/[[:space:]]*\$//" >> "$FORGE_TMP/oas.out"
  done < "$FORGE_TMP/oas.roots"
  _oa_rc=0
  while IFS= read -r _oa_l; do
    [ -f forge.yaml ] && grep -qF -- "$_oa_l" forge.yaml && continue
    printf '%s\n' "$_oa_l"; _oa_rc=1
  done < "$FORGE_TMP/oas.out"
  [ "$_oa_rc" = 0 ] || printf 'Restore what clients rely on, or have the user accept the break in forge.yaml: accept: [{rule: %s, change: "<line above>", reason: "..."}]\n' "$c_id"
  return "$_oa_rc"
}

# ---- Kubernetes (kubernetes/*)

# forge_kube_manifest <path>: a .yaml / .yml whose first document has top-level apiVersion and
# kind (the kubernetes pack's detection, §5.1), outside a Helm chart's templates/.
forge_kube_manifest() {
  case $1 in *.yaml|*.yml) ;; *) return 1 ;; esac
  case $1 in */templates/*|templates/*) [ -f "${1%%templates/*}Chart.yaml" ] && return 1 ;; esac
  [ -f "$1" ] && awk '
    /^(---|\.\.\.)([[:space:]]|$)/ { if (body) exit; next }
    /^[[:space:]]*(#|$)/ || /^%/ { next }
    { body = 1 }
    /^apiVersion:/ { a = 1 } /^kind:/ { k = 1 }
    END { exit !(a && k) }' "$1"
}

# forge_kube_linter [--no-new-ignores] <check>...: the pinned kube-linter with only the named
# checks (a config of Forge's own: the project's .kube-linter.yaml does not apply) on the changed
# manifests (forge_kube_manifest; every one with --all), each linted with the manifests of its
# directory (dangling-service and mismatching-selector need the objects next to it). Prints
# "path:0 check: message (object)" for the changed files. --no-new-ignores: a changed manifest
# also may not gain ignore-check.kube-linter.io annotations (kube-linter cannot be told to
# disregard them).
forge_kube_linter() {
  _kl_ign=0
  [ "$1" = --no-new-ignores ] && { _kl_ign=1; shift; }
  forge_scope_files | forge_filter forge_kube_manifest > "$FORGE_TMP/kl.changed"
  [ -s "$FORGE_TMP/kl.changed" ] || return 0
  _kl_rc=0
  if [ "$_kl_ign" = 1 ]; then
    while IFS= read -r _kl_f; do
      _kl_now=$(grep -c 'ignore-check\.kube-linter\.io/' "$_kl_f")
      _kl_then=0
      [ -f "$FORGE_TMP/base/$_kl_f" ] && _kl_then=$(grep -c 'ignore-check\.kube-linter\.io/' "$FORGE_TMP/base/$_kl_f")
      if [ "$_kl_now" -gt "$_kl_then" ]; then
        printf '%s:0 adds ignore-check.kube-linter.io annotations (%s -> %s); fix the manifest instead of silencing the check\n' "$_kl_f" "$_kl_then" "$_kl_now"; _kl_rc=1
      fi
    done < "$FORGE_TMP/kl.changed"
  fi
  forge_release kube-linter _kl_bin || return "$_kl_rc"
  awk '{ d = $0; if (sub(/\/[^\/]*$/, "", d) == 0) d = "."; print d }' "$FORGE_TMP/kl.changed" | sort -u > "$FORGE_TMP/kl.dirs"
  git ls-files --cached --others --exclude-standard -- '*.yaml' '*.yml' 2>/dev/null \
    | awk 'FILENAME == ARGV[1] { want[$0] = 1; next } { d = $0; if (sub(/\/[^\/]*$/, "", d) == 0) d = "."; if (d in want) print }' "$FORGE_TMP/kl.dirs" - \
    | forge_filter forge_kube_manifest | sort -u > "$FORGE_TMP/kl.files"
  { printf 'checks:\n  doNotAutoAddDefaults: true\n  include:\n'; for _kl_c in "$@"; do printf '    - %s\n' "$_kl_c"; done; } > "$FORGE_TMP/kl.yaml"
  set -f
  # shellcheck disable=SC2046
  "$_kl_bin" lint --config "$FORGE_TMP/kl.yaml" --format plain --with-color=false $(cat "$FORGE_TMP/kl.files") > "$FORGE_TMP/kl.out" 2>&1
  set +f
  awk -v r1="$(pwd)/" -v r2="$(pwd -P)/" '
    BEGIN { ansi = sprintf("%c", 27) "\\[[0-9;]*m" }
    FILENAME == ARGV[1] { want[$0] = 1; next }
    { gsub(ansi, "") }
    match($0, /: \(object: [^)]*\) /) {
      f = substr($0, 1, RSTART - 1); obj = substr($0, RSTART + 11, RLENGTH - 13); msg = substr($0, RSTART + RLENGTH)
      if (index(f, r1) == 1) f = substr(f, length(r1) + 1); else if (index(f, r2) == 1) f = substr(f, length(r2) + 1)
      if (!(f in want)) next
      check = msg; sub(/^.*\(check: /, "", check); sub(/,.*$/, "", check); sub(/ \(check: .*$/, "", msg)
      print f ":0 " check ": " msg " (" obj ")"; bad = 1
    }
    END { exit bad }' "$FORGE_TMP/kl.changed" "$FORGE_TMP/kl.out" || _kl_rc=1
  return "$_kl_rc"
}

# kubeconform's schemas: Kubernetes' own (default) and the CRDs catalog, cached across runs.
FORGE_KUBECONFORM_CRDS='https://raw.githubusercontent.com/datreeio/CRDs-catalog/main/{{.Group}}/{{.ResourceKind}}_{{.ResourceAPIVersion}}.json'

# forge_kubeconform <label> <file>...: the pinned kubeconform in strict mode (no unknown
# field) on the files ("-" for stdin), against the schemas of Kubernetes (its newest) and the
# CRDs catalog. "<label>: <kind> <name>: <message>" per invalid document. A document whose
# apiVersion has no schema is a finding when its group is one of Kubernetes' own (no group, a
# group without a dot, or *.k8s.io: a removed apiVersion), else reported skipped; schemas
# that cannot be downloaded (offline) are reported skipped, never passed.
forge_kubeconform() {
  forge_release kubeconform _kc_bin || return 0
  _kc_label=$1; shift
  _kc_cache=${XDG_CACHE_HOME:-$HOME/.cache}/forge/kubeconform
  mkdir -p "$_kc_cache" 2>/dev/null || _kc_cache=$FORGE_TMP
  "$_kc_bin" -strict -output json -cache "$_kc_cache" -schema-location default -schema-location "$FORGE_KUBECONFORM_CRDS" "$@" > "$FORGE_TMP/kc.json" 2> "$FORGE_TMP/kc.err"
  awk -v id="$c_id" -v label="$_kc_label" '
    function flush() {
      if (status == "statusInvalid") { print label ": " kind " " name ": " msg; bad = 1 }
      else if (status == "statusError") {
        g = version; if (g ~ /\//) sub(/\/[^\/]*$/, "", g); else g = ""
        if (msg ~ /^could not find schema/) {
          if (g == "" || g !~ /\./ || g ~ /\.k8s\.io$/) { print label ": " kind " " name ": apiVersion " version " is not served by current Kubernetes (no schema); use its current apiVersion"; bad = 1 }
          else print "forge:" id " skipped: " label ": no schema for " kind " (" version ")"
        } else if (msg ~ /^failed downloading schema/) print "forge:" id " skipped: " label ": the schema for " kind " could not be downloaded (offline?)"
        else { print label ": " kind " " name ": " msg; bad = 1 }
      }
      status = ""
    }
    function val(line,   v) { v = line; sub(/^[^:]*: "/, "", v); sub(/",?$/, "", v); gsub(/\\"/, "\"", v); return v }
    /^    \{$/ { status = ""; kind = ""; name = ""; version = ""; msg = "" }
    /^      "kind": / { kind = val($0) } /^      "name": / { name = val($0) } /^      "version": / { version = val($0) }
    /^      "status": / { status = val($0) } /^      "msg": / { msg = val($0); sub(/^problem validating schema\. Check JSON formatting: jsonschema validation failed with .* - /, "", msg) }
    /^    \},?$/ { flush() }
    END { exit bad }' "$FORGE_TMP/kc.json"
}

# forge_helm: for each Helm chart with changed files (the nearest Chart.yaml above a changed
# in-scope file; every chart with --all), `helm lint --strict` and its `helm template` output
# validated by forge_kubeconform. A chart whose dependencies are not built (charts/) is
# reported skipped (helm dependency build). Findings are the chart's, not attributed to lines.
forge_helm() {
  forge_scope_files | while IFS= read -r _hm_f; do
    _hm_d=$(dirname "$_hm_f")
    while :; do
      [ -f "$_hm_d/Chart.yaml" ] && { printf '%s\n' "${_hm_d#./}"; break; }
      [ "$_hm_d" = . ] && break
      _hm_d=$(dirname "$_hm_d")
    done
  done | sort -u > "$FORGE_TMP/helm.charts"
  _hm_rc=0
  while IFS= read -r _hm_c; do
    command helm lint --strict "$_hm_c" > "$FORGE_TMP/helm.out" 2>&1
    _hm_lint=$?
    if grep -q 'chart directory is missing these dependencies' "$FORGE_TMP/helm.out"; then
      printf 'forge:%s skipped: %s: its dependencies are not built (helm dependency build %s)\n' "$c_id" "$_hm_c" "$_hm_c"; continue
    fi
    if [ "$_hm_lint" != 0 ]; then
      grep -E '^\[(ERROR|WARNING)\]|^Error' "$FORGE_TMP/helm.out" | grep -v '^Error: [0-9]* chart(s) linted' | sed "s|^|$_hm_c: helm lint: |"
      _hm_rc=1; continue
    fi
    if ! command helm template forge "$_hm_c" > "$FORGE_TMP/helm.yaml" 2> "$FORGE_TMP/helm.err"; then
      printf '%s: helm template: %s\n' "$_hm_c" "$({ grep '^Error' "$FORGE_TMP/helm.err"; grep -v '^[[:space:]]*$' "$FORGE_TMP/helm.err"; } | head -n 1)"; _hm_rc=1; continue
    fi
    forge_kubeconform "$_hm_c" "$FORGE_TMP/helm.yaml" || _hm_rc=1
  done < "$FORGE_TMP/helm.charts"
  return "$_hm_rc"
}

# ---- Terraform / OpenTofu (terraform/*)

# forge_tf_dirs: the directories of the in-scope .tf / .tofu files (forge_scope_files), one per line.
forge_tf_dirs() {
  forge_scope_files | grep -E '\.(tf|tofu)$' | awk '{ d = $0; if (sub(/\/[^\/]*$/, "", d) == 0) d = "."; print d }' | sort -u
}

# forge_tf_validate: `validate` in each directory of a changed .tf / .tofu file (every one with
# --all), with the project's Terraform CLI (forge_terraform): "path:line: Error: summary" per
# error. A directory validate cannot judge before `init` (providers or modules not installed)
# is reported skipped: init downloads providers and configures the backend, which a check does not do.
forge_tf_validate() {
  _tv_rc=0
  forge_tf_dirs > "$FORGE_TMP/tf.dirs"
  while IFS= read -r _tv_d; do
    forge_terraform -chdir="$_tv_d" validate -no-color > "$FORGE_TMP/tf.out" 2>&1 && continue
    if grep -qE '(terraform|tofu) init' "$FORGE_TMP/tf.out"; then
      printf 'forge:%s skipped: %s is not initialised (%s init)\n' "$c_id" "$_tv_d" "$(forge_tf_bin)"; continue
    fi
    _tv_rc=1
    awk -v d="$_tv_d" '
      function flush() { if (err != "") print (loc != "" ? loc ": " : d ": ") "Error: " err; err = ""; loc = "" }
      /^Error: / { flush(); err = substr($0, 8); next }
      err != "" && loc == "" && match($0, /^  on [^ ]+ line [0-9]+/) {
        n = split(substr($0, 6, RLENGTH - 5), w, " "); loc = (d == "." ? "" : d "/") w[1] ":" w[3]; next
      }
      /^Warning: / { flush() }
      END { flush() }' "$FORGE_TMP/tf.out"
  done < "$FORGE_TMP/tf.dirs"
  return "$_tv_rc"
}

# tflint's provider rulesets, pinned: "<provider> <version>" per line (terraform/tflint).
FORGE_TFLINT_RULESETS='@@TFLINT_RULESETS@@'

# forge_tflint: the pinned tflint in each directory of a changed .tf / .tofu file (every one
# with --all), with a config of Forge's own (a project .tflint.hcl cannot disable rules): the
# terraform ruleset's recommended preset, plus the pinned aws / google / azurerm ruleset when a
# .terraform.lock.hcl of the repository lists that provider. "path:line:column: message (rule)".
# Rulesets are installed once (tflint --init, which checks their signatures) under
# ~/.cache/forge/tflint-plugins; when that fails (offline), the check is reported skipped.
forge_tflint() {
  forge_tf_dirs > "$FORGE_TMP/tfl.dirs"
  [ -s "$FORGE_TMP/tfl.dirs" ] || return 0
  forge_release tflint _tl_bin || return 0
  printf 'plugin "terraform" {\n  enabled = true\n  preset  = "recommended"\n}\n' > "$FORGE_TMP/tflint.hcl"
  git ls-files --cached --others --exclude-standard -- '*.terraform.lock.hcl' 2>/dev/null > "$FORGE_TMP/tfl.locks"
  printf '%s\n' "$FORGE_TFLINT_RULESETS" | while IFS=' ' read -r _tl_p _tl_v; do
    [ -n "$_tl_p" ] || continue
    while IFS= read -r _tl_l; do
      if grep -qE "provider \"registry\.(terraform\.io|opentofu\.org)/hashicorp/$_tl_p\"" "$_tl_l" 2>/dev/null; then
        printf 'plugin "%s" {\n  enabled = true\n  version = "%s"\n  source  = "github.com/terraform-linters/tflint-ruleset-%s"\n}\n' "$_tl_p" "$_tl_v" "$_tl_p"
        break
      fi
    done < "$FORGE_TMP/tfl.locks"
  done >> "$FORGE_TMP/tflint.hcl"
  TFLINT_PLUGIN_DIR=${XDG_CACHE_HOME:-$HOME/.cache}/forge/tflint-plugins
  export TFLINT_PLUGIN_DIR
  # a ruleset missing from the cache is installed in a directory of its own, then renamed
  # into place: a concurrent run never loads a half-written plugin
  _tl_missing=0
  sed -n 's/^  source  = "github.com\/\(.*\)"$/\1/p' "$FORGE_TMP/tflint.hcl" > "$FORGE_TMP/tfl.sources"
  while IFS= read -r _tl_s; do
    _tl_v=$(grep -B1 "source  = \"github.com/$_tl_s\"" "$FORGE_TMP/tflint.hcl" | sed -n 's/^  version = "\(.*\)"$/\1/p')
    [ -x "$TFLINT_PLUGIN_DIR/github.com/$_tl_s/$_tl_v/${_tl_s##*/}" ] || _tl_missing=1
  done < "$FORGE_TMP/tfl.sources"
  if [ "$_tl_missing" = 1 ]; then
    mkdir -p "$TFLINT_PLUGIN_DIR"
    _tl_new=$(mktemp -d "$TFLINT_PLUGIN_DIR/.install.XXXXXX") || return 0
    TFLINT_PLUGIN_DIR=$_tl_new "$_tl_bin" --config "$FORGE_TMP/tflint.hcl" --init > "$FORGE_TMP/tfl.init" 2>&1 \
      || { rm -rf "$_tl_new"; printf 'forge:%s skipped: tflint could not install its rulesets (offline?): %s\n' "$c_id" "$(grep -v '^[[:space:]]*$' "$FORGE_TMP/tfl.init" | tail -n 1)"; return 0; }
    ( cd "$_tl_new" && find . -mindepth 4 -maxdepth 4 -type d ) | while IFS= read -r _tl_d; do
      [ -d "$TFLINT_PLUGIN_DIR/$_tl_d" ] && continue
      mkdir -p "$TFLINT_PLUGIN_DIR/${_tl_d%/*}"
      mv "$_tl_new/$_tl_d" "$TFLINT_PLUGIN_DIR/$_tl_d" 2>/dev/null || true
      # another run renamed its copy first: ours went inside it
      rm -rf "$TFLINT_PLUGIN_DIR/$_tl_d/${_tl_d##*/}"
    done
    rm -rf "$_tl_new"
  fi
  _tl_rc=0
  while IFS= read -r _tl_d; do
    "$_tl_bin" --chdir "$_tl_d" --config "$FORGE_TMP/tflint.hcl" --format compact --force > "$FORGE_TMP/tfl.out" 2>&1
    # "path:line:col: Severity - message (rule)": every issue counts, whatever tflint's severity
    grep -E '^[^[:space:]:]+:[0-9]+:[0-9]+: (Error|Warning|Notice) - ' "$FORGE_TMP/tfl.out" | sed -E 's/: (Error|Warning|Notice) - /: /' > "$FORGE_TMP/tfl.found"
    if [ -s "$FORGE_TMP/tfl.found" ]; then cat "$FORGE_TMP/tfl.found"; _tl_rc=1
    elif grep -q '^Failed to\|^Error:' "$FORGE_TMP/tfl.out"; then
      printf 'forge:%s skipped: tflint could not run in %s (%s)\n' "$c_id" "$_tl_d" "$(grep -v '^[[:space:]]*$' "$FORGE_TMP/tfl.out" | tail -n 1)"
    fi
  done < "$FORGE_TMP/tfl.dirs"
  return "$_tl_rc"
}

# forge_trivy_config: the pinned trivy (its bundled checks, no update; an empty ignore file of
# Forge's, so .trivyignore does not apply) on each directory of a changed .tf / .tofu file,
# now and as it was at the base (the directory's files with the change undone): "path:0 ID
# title (resource, line N)" for each HIGH or CRITICAL Terraform misconfiguration (check,
# resource) the change introduced. Not with --all (no base to compare).
forge_trivy_config() {
  [ "$mode" != --all ] || return 0
  forge_tf_dirs > "$FORGE_TMP/tv.dirs"
  [ -s "$FORGE_TMP/tv.dirs" ] || return 0
  forge_release trivy _tr_bin || return 0
  : > "$FORGE_TMP/trivy.ignore"
  printf '{{- range . }}{{- $t := .Target }}{{- range .Misconfigurations }}{{ $t }}\t{{ .ID }}\t{{ .CauseMetadata.Resource }}\t{{ .CauseMetadata.StartLine }}\t{{ .Title }}\n{{ end }}{{ end }}' > "$FORGE_TMP/trivy.tpl"
  _tr_cache=${XDG_CACHE_HOME:-$HOME/.cache}/forge/trivy
  _tr_rc=0
  while IFS= read -r _tr_d; do
    # the directory as it was: its files now, with the change undone
    _tr_b=$FORGE_TMP/trivy-base/$_tr_d
    rm -rf "$_tr_b"; mkdir -p "$_tr_b"
    git ls-files --cached --others --exclude-standard -- "$_tr_d" 2>/dev/null \
      | awk -v d="$_tr_d" '{ p = $0; if (sub(/\/[^\/]*$/, "", p) == 0) p = "."; if (p == d) print }' > "$FORGE_TMP/tv.files"
    while IFS= read -r _tr_f; do [ -f "$_tr_f" ] && cp "$_tr_f" "$_tr_b/"; done < "$FORGE_TMP/tv.files"
    awk -v d="$_tr_d" '{ p = $0; if (sub(/\/[^\/]*$/, "", p) == 0) p = "."; if (p == d) print }' "$FORGE_TMP/changed" > "$FORGE_TMP/tv.changed"
    while IFS= read -r _tr_f; do
      rm -f "$_tr_b/${_tr_f##*/}"
      [ -f "$FORGE_TMP/base/$_tr_f" ] && cp "$FORGE_TMP/base/$_tr_f" "$_tr_b/"
    done < "$FORGE_TMP/tv.changed"
    for _tr_side in now then; do
      [ "$_tr_side" = now ] && _tr_dir=$_tr_d || _tr_dir=$_tr_b
      "$_tr_bin" config --quiet --cache-dir "$_tr_cache" --misconfig-scanners terraform --severity HIGH,CRITICAL --skip-check-update \
        --ignorefile "$FORGE_TMP/trivy.ignore" --format template --template "@$FORGE_TMP/trivy.tpl" "$_tr_dir" > "$FORGE_TMP/trivy.$_tr_side" 2> "$FORGE_TMP/trivy.err" \
        || { printf 'forge:%s skipped: trivy could not scan %s (%s)\n' "$c_id" "$_tr_d" "$(grep -v '^[[:space:]]*$' "$FORGE_TMP/trivy.err" | tail -n 1)"; continue 2; }
    done
    awk -F '\t' -v d="$_tr_d" 'FILENAME == ARGV[1] { old[$2 "\t" $3] = 1; next }
      NF >= 5 && !(($2 "\t" $3) in old) { printf "%s%s:0 %s %s (%s, line %s)\n", (d == "." ? "" : d "/"), $1, $2, $5, $3, $4; bad = 1 }
      END { exit bad }' "$FORGE_TMP/trivy.then" "$FORGE_TMP/trivy.now" || _tr_rc=1
  done < "$FORGE_TMP/tv.dirs"
  return "$_tr_rc"
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

# forge_has_tool <name>[@<dir>]: system tools (runtimes, infrastructure CLIs) via PATH,
# others via tool() (in workspace package <dir> when given).
forge_has_tool() {
  case $1 in
    *@*) ( _fh_root=$(pwd); cd "${1#*@}" 2>/dev/null && FORGE_WS_ROOT=$_fh_root forge_has_tool "${1%%@*}" ) ;;
    uv|uvx|bun|docker) command -v "$1" >/dev/null 2>&1 ;;
    tofu|helm|kubectl) forge_on_path "$1" ;;
    terraform) forge_on_path "$(forge_tf_bin)" ;;
    kustomize) forge_on_path kustomize || forge_on_path kubectl ;;
    # Forge's pinned ty runs through uvx (§15.2)
    ty) command -v uvx >/dev/null 2>&1 ;;
    *) [ -n "$(tool "$1")" ] ;;
  esac
}

# forge_tool_label <name>: how a missing tool is named: the binary the engine would run.
forge_tool_label() {
  case $1 in
    terraform) forge_tf_bin ;;
    kustomize) printf 'kustomize or kubectl' ;;
    *) printf '%s' "$1" ;;
  esac
}

# forge_on_path <name>: an executable <name> is on PATH (not a shell function of that name).
forge_on_path() {
  _op_ifs=$IFS
  IFS=:
  for _op_d in $PATH; do
    if [ -n "$_op_d" ] && [ -f "$_op_d/$1" ] && [ -x "$_op_d/$1" ]; then IFS=$_op_ifs; return 0; fi
  done
  IFS=$_op_ifs
  return 1
}

# forge_tf_bin: the project's Terraform CLI: tofu when it uses OpenTofu (a *.tofu file, or
# a .terraform.lock.hcl naming registry.opentofu.org providers), else terraform.
forge_tf_bin() {
  if [ -z "${_tf_bin:-}" ]; then
    _tf_bin=terraform
    _tf_files=$(git ls-files --cached --others --exclude-standard -- '*.tofu' '*.terraform.lock.hcl' 2>/dev/null)
    while IFS= read -r _tf_f; do
      case $_tf_f in
        *.tofu) _tf_bin=tofu; break ;;
        *.terraform.lock.hcl) grep -q 'registry\.opentofu\.org/' "$_tf_f" 2>/dev/null && { _tf_bin=tofu; break; } ;;
      esac
    done <<EOF
$_tf_files
EOF
  fi
  printf '%s' "$_tf_bin"
}

# forge_terraform <args>: the project's Terraform CLI (forge_tf_bin) from PATH.
forge_terraform() { command "$(forge_tf_bin)" "$@"; }

# forge_kustomize <args>: kustomize from PATH, else `kubectl kustomize`.
forge_kustomize() {
  if forge_on_path kustomize; then command kustomize "$@"; else command kubectl kustomize "$@"; fi
}

# forge_missing_tool: prints the first tool of $c_tools that is absent (in a package
# of $c_dirs: "<tool> (<dir>)").
forge_missing_tool() {
  _fm_ifs=$IFS
  IFS=' '
  for _fm_t in $c_tools; do
    if [ -n "${c_dirs:-}" ]; then
      for _fm_d in $c_dirs; do
        if ! forge_has_tool "$_fm_t@$_fm_d"; then IFS=$_fm_ifs; printf '%s (%s)' "$(forge_tool_label "$_fm_t")" "$_fm_d"; return 0; fi
      done
    elif ! forge_has_tool "$_fm_t"; then IFS=$_fm_ifs; forge_tool_label "$_fm_t"; return 0; fi
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

# forge_select <predicate>: FORGE_FILES=$(forge_filter <predicate>), without the
# subshell (a fork per check and run otherwise).
forge_select() {
  FORGE_FILES=''
  while read -r _ff_line; do
    [ -n "$_ff_line" ] && "$1" "$_ff_line" && FORGE_FILES="${FORGE_FILES:+$FORGE_FILES
}$_ff_line"
  done
  return 0
}

# forge_any <predicate>: succeeds when <predicate> holds for a line of stdin.
forge_any() {
  while read -r _ff_line; do
    [ -n "$_ff_line" ] && "$1" "$_ff_line" && return 0
  done
  return 1
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
# "bunx squawk-cli@<v>" runs the package's one binary from bunx's cache; with
# "-p squawk-cli@<v> squawk" bunx installs the package again at every run, and
# runs at the same time then kill each other's install.
forge_squawk() {
  if command -v bunx >/dev/null 2>&1; then
    bunx "squawk-cli@$FORGE_SQUAWK_VERSION" --reporter gcc "$@" > "$FORGE_TMP/squawk" 2>&1
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
# -> the revision files (file role @alembic-migrations) that define them.
forge_alembic_files() {
  forge_role_files @alembic-migrations | grep '\.py$' > "$FORGE_TMP/alembic.all"
  awk '{ print $1 }' | while read -r _fa_id; do
    # shellcheck disable=SC2046
    [ -n "$_fa_id" ] && [ -s "$FORGE_TMP/alembic.all" ] && grep -lE "^revision[^=]*=[[:space:]]*['\"]$_fa_id['\"]" $(cat "$FORGE_TMP/alembic.all") 2>/dev/null
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
    printf '%s\n' "$FORGE_FILES" | while IFS= read -r _at_f; do
      case $_at_f in *.py) forge_role @alembic-migrations "$_at_f" && printf '%s\n' "$_at_f" ;; esac
    done
    return 0
  fi
  forge_role_files @alembic-migrations | grep '\.py$' | sort > "$FORGE_TMP/alembic.revs"
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
# op.create_foreign_key(..., [...], ...)) also leads an index: the first column of an
# op.create_index, sa.Index, primary key or unique constraint, index=True, unique=True or
# primary_key=True on the column, or the first column of a CREATE INDEX. PostgreSQL does
# not index referencing columns; skipped when the database URL is MySQL (which does).
# Run as advice (forge_advise): such an index is often a good idea, not a requirement.
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
          else if (name == "Index") { split(strs(args, 1), ia, SUBSEP); cover(ia[2]) }
          else cover(strs(args, 0))
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
          print f ":" fk[col] ": foreign key column \"" col "\" gets no index that leads with it in this revision; PostgreSQL and SQLite do not index referencing columns, so joins on it and deletes of the referenced rows scan the table. If it is queried or the parent rows are deleted, add op.create_index(..., [\"" col "\"]) (index=True on the model mapped_column, then autogenerate); otherwise say why not."
          bad = 1
        }
        exit bad
      }' "$_ai_f" || _ai_rc=1
  done < "$FORGE_TMP/alembic.targets"
  return "$_ai_rc"
}

# forge_alembic_data_downgrade: a revision whose upgrade() changes rows (UPDATE, INSERT,
# DELETE through execute(), sa.update/insert/delete, op.bulk_insert, an ORM Session) has a
# downgrade() that changes rows the same ways (the inverse) or raises (it cannot be
# reverted). A downgrade that only passes, prints or alters the schema reports success
# and leaves the data. Run as advice (forge_advise): Claude may say why not.
forge_alembic_data_downgrade() {
  forge_alembic_targets > "$FORGE_TMP/alembic.targets"
  : > "$FORGE_TMP/alembic.data"
  while IFS= read -r _ad_f; do
    [ -f "$_ad_f" ] || continue
    awk -v f="$_ad_f" '
      function changes_rows(t,   u) {
        u = toupper(t)
        return (t ~ /execute\(/ && (u ~ /(^|[^A-Z_])UPDATE[ \t\n]+[^ \t\n]+[ \t\n]+SET[ \t\n]/ || u ~ /INSERT[ \t\n]+INTO[ \t\n]/ || u ~ /DELETE[ \t\n]+FROM[ \t\n]/)) \
          || t ~ /(^|[^A-Za-z0-9_.])(sa\.)?(update|insert|delete)\([A-Za-z_]/ || t ~ /\.(update|insert|delete)\(\)/ \
          || t ~ /op\.bulk_insert\(/ || t ~ /(^|[^A-Za-z0-9_])Session\(/
      }
      { line = $0; sub(/\r$/, "", line) }
      /^def[ \t]+upgrade[ \t]*\(/ { sec = "up"; next }
      /^def[ \t]+downgrade[ \t]*\(/ { sec = "down"; dl = NR; next }
      line ~ /^[^ \t#]/ { sec = ""; next }
      sec == "up" && line !~ /^[ \t]*#/ { up = up line "\n" }
      sec == "down" {
        t = line; sub(/^[ \t]+/, "", t); sub(/[ \t]+$/, "", t)
        if (indoc) { if (index(t, q)) indoc = 0; next }
        if (t == "" || t ~ /^#/) next
        if (t ~ /^("""|\047\047\047)/) { q = substr(t, 1, 3); if (!index(substr(t, 4), q)) indoc = 1; next }
        if (t ~ /^raise([ \t(]|$)/) raises = 1
        down = down line "\n"
      }
      END {
        if (changes_rows(up) && !raises && !changes_rows(down))
          print f ":" (dl ? dl : 1) ": upgrade() changes rows but downgrade() neither changes them back (op.execute with UPDATE/INSERT/DELETE, sa.update/insert/delete, op.bulk_insert) nor raises, so alembic downgrade reports success and leaves the rows changed. Write the inverse, or raise an error that says the revision cannot be reverted."
      }' "$_ad_f" >> "$FORGE_TMP/alembic.data"
  done < "$FORGE_TMP/alembic.targets"
  [ -s "$FORGE_TMP/alembic.data" ] || return 0
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

# forge_prisma_drops <file>...: "<file>:<line>: <statement>" for each DROP COLUMN or DROP
# TABLE in the given Prisma migrations. Every migration.sql (sorted) is read to follow
# the tables' columns (CREATE TABLE, ADD / DROP / RENAME COLUMN), so a SQLite table
# redefinition (CREATE TABLE "new_X" ..., DROP TABLE "X", ALTER TABLE "new_X" RENAME
# TO "X") is a drop only of the columns "new_X" lacks: "... (redefinition drops column
# "C")". A redefined table that no earlier migration creates is not reported.
forge_prisma_drops() {
  printf '%s\n' "$@" > "$FORGE_TMP/prisma.targets"
  { git ls-files -co --exclude-standard -- 'prisma/migrations/*/migration.sql' 2>/dev/null; printf '%s\n' "$@"; } \
    | sort -u > "$FORGE_TMP/prisma.history"
  set -f
  # shellcheck disable=SC2046
  awk '
    function unq(t) { gsub(/[";`[:space:]]/, "", t); return t }
    function name_after(u, re,   t) { t = u; sub(re, "", t); sub(/[[:space:](].*$/, "", t); return unq(t) }
    # the column names of a CREATE TABLE body: the first identifier of each top-level item
    function columns(stmt,   p, body, d, i, c, item, out, n, name) {
      p = index(stmt, "("); if (!p) return ""
      body = substr(stmt, p + 1); d = 0; item = ""; out = ""
      for (i = 1; i <= length(body); i++) {
        c = substr(body, i, 1)
        if (c == "(") d++
        if (c == ")") { if (d == 0) break; d-- }
        if (c == "," && d == 0) { out = out item_col(item); item = ""; continue }
        item = item c
      }
      return out item_col(item)
    }
    function item_col(item,   t) {
      t = item; sub(/^[[:space:]]+/, "", t)
      if (toupper(t) ~ /^(CONSTRAINT|PRIMARY|FOREIGN|UNIQUE|CHECK|KEY|INDEX)([^A-Z_]|$)/) return ""
      if (!match(t, /^("[^"]+"|`[^`]+`|[A-Za-z_][A-Za-z0-9_]*)/)) return ""
      return SUBSEP unq(substr(t, RSTART, RLENGTH))
    }
    function has_col(list, c) { return index(list SUBSEP, SUBSEP c SUBSEP) > 0 }
    function del_col(list, c,   x) { x = list SUBSEP; sub(SUBSEP c SUBSEP, SUBSEP, x); return substr(x, 1, length(x) - 1) }
    function statement(f, line, stmt,   u, t, c, n, a, i, old, gone) {
      u = toupper(stmt)
      if (u ~ /^[[:space:]]*CREATE[[:space:]]+TABLE/) { t = name_after(u, "^[[:space:]]*CREATE[[:space:]]+TABLE[[:space:]]+(IF[[:space:]]+NOT[[:space:]]+EXISTS[[:space:]]+)?"); cols[t] = toupper(columns(stmt)); return }
      if (u ~ /^[[:space:]]*ALTER[[:space:]]+TABLE/) {
        t = name_after(u, "^[[:space:]]*ALTER[[:space:]]+TABLE[[:space:]]+(ONLY[[:space:]]+)?(IF[[:space:]]+EXISTS[[:space:]]+)?")
        if (u ~ /RENAME[[:space:]]+TO[[:space:]]/) {
          n = name_after(u, ".*RENAME[[:space:]]+TO[[:space:]]+")
          if ((f SUBSEP n) in dropped) {
            old = dropped[f SUBSEP n]; gone = ""
            c = split(substr(old, 2), a, SUBSEP)
            for (i = 1; i <= c; i++) if (!has_col(cols[t], a[i])) gone = gone (gone == "" ? "" : ", ") "\"" a[i] "\""
            if (f in target && gone != "") print f ":" dline[f SUBSEP n] ": " dtext[f SUBSEP n] " (redefinition drops column " gone ")"
            delete dropped[f SUBSEP n]; delete pend[f SUBSEP n]
          }
          cols[n] = cols[t]; return
        }
        if (u ~ /RENAME[[:space:]]+COLUMN/) { a[1] = name_after(u, ".*RENAME[[:space:]]+COLUMN[[:space:]]+"); a[2] = name_after(u, ".*[[:space:]]TO[[:space:]]+"); cols[t] = del_col(cols[t], a[1]) SUBSEP a[2]; return }
        while (match(u, /ADD[[:space:]]+(COLUMN[[:space:]]+)?("[^"]+"|[A-Z_][A-Z0-9_]*)/)) { c = substr(u, RSTART, RLENGTH); sub(/^ADD[[:space:]]+(COLUMN[[:space:]]+)?/, "", c); c = unq(c); if (c !~ /^(CONSTRAINT|PRIMARY|FOREIGN|UNIQUE|CHECK)$/) cols[t] = cols[t] SUBSEP c; u = substr(u, RSTART + RLENGTH) }
        u = toupper(stmt)
        while (match(u, /DROP[[:space:]]+COLUMN[[:space:]]+(IF[[:space:]]+EXISTS[[:space:]]+)?("[^"]+"|[A-Z_][A-Z0-9_]*)/)) { c = substr(u, RSTART, RLENGTH); sub(/^DROP[[:space:]]+COLUMN[[:space:]]+(IF[[:space:]]+EXISTS[[:space:]]+)?/, "", c); cols[t] = del_col(cols[t], unq(c)); u = substr(u, RSTART + RLENGTH) }
        if (f in target && toupper(stmt) ~ /DROP[[:space:]]+COLUMN/) print f ":" line ": " text
        return
      }
      if (u ~ /^[[:space:]]*DROP[[:space:]]+TABLE/) {
        t = name_after(u, "^[[:space:]]*DROP[[:space:]]+TABLE[[:space:]]+(IF[[:space:]]+EXISTS[[:space:]]+)?")
        # a redefinition renames "new_<t>" to <t> later in the same file
        # (its old columns unknown, without the earlier migrations: not reported)
        if (("NEW_" t) in cols) { if (t in cols) { dropped[f SUBSEP t] = cols[t]; dline[f SUBSEP t] = line; dtext[f SUBSEP t] = text; pend[f SUBSEP t] = 1 } }
        else if (f in target) print f ":" line ": " text
        delete cols[t]
      }
    }
    FNR == 1 {
      for (k in pend) { split(k, kk, SUBSEP); if ((k in dropped) && (kk[1] in target)) print kk[1] ":" dline[k] ": " dtext[k]; delete pend[k]; delete dropped[k] }
      f = FILENAME; stmt = ""; incomment = 0
    }
    FILENAME == ARGV[1] { target[$0] = 1; next }
    FILENAME == ARGV[2] { next }
    {
      l = $0
      if (incomment) { if (index(l, "*/")) { incomment = 0; l = substr(l, index(l, "*/") + 2) } else next }
      if (l ~ /^[[:space:]]*\/\*/ && !index(l, "*/")) { incomment = 1; next }
      if (l ~ /^[[:space:]]*--/) next
      if (stmt == "" && l ~ /^[[:space:]]*$/) next
      if (stmt == "") { sline = FNR; text = l; sub(/^[[:space:]]+/, "", text) }
      stmt = stmt " " l
      if (index(l, ";")) { statement(f, sline, stmt); stmt = "" }
    }
    END { for (k in pend) { split(k, kk, SUBSEP); if ((k in dropped) && (kk[1] in target)) print kk[1] ":" dline[k] ": " dtext[k] } }
  ' "$FORGE_TMP/prisma.targets" /dev/null $(cat "$FORGE_TMP/prisma.history")
  _pd_rc=$?
  set +f
  return "$_pd_rc"
}

# Drizzle: drizzle.config.* names the schema files (`schema`: a path, a glob, a
# directory or an array of them), the migrations folder (`out`, default ./drizzle)
# and the `dialect`. forge_drizzle_config writes "key<TAB>value" lines (one per
# schema entry, ./ removed) to $FORGE_TMP/drizzle.config; fails without a config.
forge_drizzle_config() {
  FORGE_DZ_CONFIG=''
  for _dz_f in drizzle.config.ts drizzle.config.mts drizzle.config.cts drizzle.config.js drizzle.config.mjs drizzle.config.cjs drizzle.config.json; do
    if [ -f "$_dz_f" ]; then FORGE_DZ_CONFIG=$_dz_f; break; fi
  done
  [ -n "$FORGE_DZ_CONFIG" ] || return 1
  awk '
    { s = s $0 "\n" }
    function val(key,   re, v) {
      re = "[^A-Za-z0-9_$]\"?" key "\"?[[:space:]]*:[[:space:]]*(\\[[^]]*\\]|\"[^\"]*\"|\047[^\047]*\047|`[^`]*`)"
      if (!match(s, re)) return ""
      v = substr(s, RSTART, RLENGTH); sub(/^[^:]*:[[:space:]]*/, "", v); return v
    }
    function emit(key, v,   q) {
      while (match(v, /"[^"]*"|\047[^\047]*\047|`[^`]*`/)) {
        q = substr(v, RSTART + 1, RLENGTH - 2); v = substr(v, RSTART + RLENGTH)
        sub(/^\.\//, "", q); sub(/\/+$/, "", q)
        if (q != "") print key "\t" q
      }
    }
    END {
      # drop comments, outside strings
      t = ""; q = ""; n = length(s)
      for (i = 1; i <= n; i++) {
        c = substr(s, i, 1); d = substr(s, i, 2)
        if (q != "") { t = t c; if (c == "\\") { t = t substr(s, ++i, 1) } else if (c == q) q = ""; continue }
        if (d == "//") { while (i <= n && substr(s, i, 1) != "\n") i++; t = t " "; continue }
        if (d == "/*") { i += 2; while (i <= n && substr(s, i, 2) != "*/") i++; i++; t = t " "; continue }
        if (c == "\"" || c == "\047" || c == "`") q = c
        t = t (c == "\n" ? " " : c)
      }
      s = t
      gsub(/migrations[[:space:]]*:[[:space:]]*\{[^}]*\}/, "", s)
      s = " " s
      o = val("out"); if (o == "") print "out\tdrizzle"; else emit("out", o)
      emit("dialect", val("dialect")); emit("schema", val("schema"))
    }' "$FORGE_DZ_CONFIG" > "$FORGE_TMP/drizzle.config"
}

# forge_drizzle_get <key>: the first value of <key> from forge_drizzle_config.
forge_drizzle_get() { awk -F '\t' -v k="$1" '$1 == k { print $2; exit }' "$FORGE_TMP/drizzle.config"; }

# forge_drizzle_is_schema <path>: succeeds when <path> is one of the configured schema
# entries, matches one as a glob, or sits under one as a directory.
forge_drizzle_is_schema() {
  while IFS="$(printf '\t')" read -r _ds_k _ds_v; do
    [ "$_ds_k" = schema ] || continue
    _ds_g=$(printf '%s' "$_ds_v" | sed 's/\*\*/*/g')
    # shellcheck disable=SC2254
    case $1 in "$_ds_v"|"$_ds_v"/*|$_ds_g) return 0 ;; esac
  done < "$FORGE_TMP/drizzle.config"
  return 1
}

# forge_drizzle_tables <file>: the file without what does not reach the database
# (blank lines, comments, imports, type exports, relations()/defineRelations() calls).
forge_drizzle_tables() {
  awk '
    inc { if (/\*\//) inc = 0; next }
    /^[[:space:]]*\/\*/ { if (!/\*\//) inc = 1; next }
    /^[[:space:]]*(\/\/|$)/ { next }
    /^[[:space:]]*(import[[:space:]{*]|export[[:space:]]+(type|interface)[[:space:]]|export[[:space:]]*\*)/ { next }
    depth == 0 && /(^|[^A-Za-z0-9_$])(defineRelations|relations)[[:space:]]*\(/ { inrel = 1 }
    inrel {
      t = $0; o = gsub(/\(/, "", t); t = $0; c = gsub(/\)/, "", t); depth += o - c
      if (depth <= 0) { depth = 0; inrel = 0 }
      next
    }
    { sub(/^[[:space:]]+/, ""); sub(/[[:space:]]+$/, ""); print }' "$1" 2>/dev/null
}

# forge_drizzle_migration_present: a change to a schema file that reaches the database
# comes with a generated migration in the out folder: the SQL plus meta/_journal.json and
# a meta/*_snapshot.json (drizzle-kit 0.x), or <out>/<name>/migration.sql and its
# snapshot.json (drizzle-kit 1.x). Not with --all (no change to compare).
forge_drizzle_migration_present() {
  [ "$mode" != --all ] || return 0
  forge_drizzle_config || return 0
  grep -q "^schema$(printf '\t')" "$FORGE_TMP/drizzle.config" || {
    printf 'forge:%s skipped: no schema path found in %s\n' "$c_id" "$FORGE_DZ_CONFIG"; return 0; }
  _dm_out=$(forge_drizzle_get out)
  : > "$FORGE_TMP/dz.schema"
  while IFS= read -r _dm_f; do
    forge_drizzle_is_schema "$_dm_f" || continue
    forge_drizzle_tables "$_dm_f" > "$FORGE_TMP/dz.now"
    forge_drizzle_tables "$FORGE_TMP/base/$_dm_f" > "$FORGE_TMP/dz.then"
    cmp -s "$FORGE_TMP/dz.now" "$FORGE_TMP/dz.then" || printf '%s\n' "$_dm_f" >> "$FORGE_TMP/dz.schema"
  done < "$FORGE_TMP/changed"
  [ -s "$FORGE_TMP/dz.schema" ] || return 0
  _dm_sql=0; _dm_journal=0; _dm_snap=0
  while IFS= read -r _dm_f; do
    case $_dm_f in
      "$_dm_out"/meta/_journal.json) _dm_journal=1 ;;
      "$_dm_out"/meta/*_snapshot.json|"$_dm_out"/*/snapshot.json) _dm_snap=1 ;;
      "$_dm_out"/*/migration.sql) _dm_sql=1; _dm_journal=1 ;;
      "$_dm_out"/*.sql) _dm_sql=1 ;;
    esac
  done < "$FORGE_TMP/existing"
  # drizzle-kit 1.x keeps no journal: a <name>/migration.sql stands in for it
  [ "$_dm_sql" = 1 ] && [ "$_dm_journal" = 1 ] && [ "$_dm_snap" = 1 ] && return 0
  while IFS= read -r _dm_f; do
    printf '%s: the Drizzle schema changed, but %s/ has no new migration with its journal and snapshot\n' "$_dm_f" "$_dm_out"
  done < "$FORGE_TMP/dz.schema"
  printf 'Generate it (drizzle-kit generate) and keep the SQL, meta/_journal.json and the snapshot together; never write them by hand.\n'
  return 1
}

# forge_role_files <@role>: the repository's files (tracked, and untracked but not ignored)
# of a file role (§15.4), by forge_role (rendered with the role's effective patterns).
forge_role_files() {
  git ls-files -co --exclude-standard 2>/dev/null | while IFS= read -r _rf_f; do
    forge_role "$1" "$_rf_f" && printf '%s\n' "$_rf_f"
  done
  return 0
}

# forge_applied_migrations <@role>...: migrations (the files of the roles) that exist in the
# base are not modified or deleted; a change goes in a new migration. Not with --all (no base).
forge_applied_migrations() {
  [ "$mode" != --all ] || return 0
  _am_rc=0
  while IFS= read -r _am_f; do
    for _am_r in "$@"; do
      forge_role "$_am_r" "$_am_f" || continue
      if [ -f "$_am_f" ]; then _am_what=modified; else _am_what=deleted; fi
      printf '%s: an existing migration was %s; it may already be applied somewhere, so write a new migration instead\n' "$_am_f" "$_am_what"
      _am_rc=1
      break
    done
  done < "$FORGE_TMP/inbase"
  return "$_am_rc"
}

# forge_typeorm_pending <@datasource-role> <@esm-role>: TypeORM finds no schema change
# the migrations lack. The data source is the first file of <@datasource-role>
# (@typeorm-datasource: no default, a code config names it); its migrations run on the
# local database, then `migration:generate --check` compares the entities with it. A
# TypeScript data source runs through the project's typeorm-ts-node-esm (a file of
# <@esm-role>, @esm-modules) or typeorm-ts-node-commonjs.
forge_typeorm_pending() {
  _tp_ds=$(forge_role_files "$1" | head -n 1)
  if [ -z "$_tp_ds" ]; then
    printf 'forge:%s skipped: file role @typeorm-datasource names no file; set the data source in forge.yaml (files: {typeorm-datasource: {set: [src/data-source.ts]}})\n' "$c_id"
    return 0
  fi
  case $_tp_ds in
    *.ts|*.mts|*.cts) if forge_role "$2" "$_tp_ds"; then _tp_bin=typeorm-ts-node-esm; else _tp_bin=typeorm-ts-node-commonjs; fi ;;
    *) _tp_bin=typeorm ;;
  esac
  if [ -z "$(tool "$_tp_bin")" ]; then
    printf 'forge:%s skipped: %s not installed\n' "$c_id" "$_tp_bin"
    return 0
  fi
  if ! forge_exec_tool "$_tp_bin" migration:run -d "$_tp_ds" > "$FORGE_TMP/typeorm.out" 2>&1; then
    printf 'forge:%s skipped: %s migration:run -d %s failed: %s\n' "$c_id" "$_tp_bin" "$_tp_ds" "$(grep . "$FORGE_TMP/typeorm.out" | tail -n 1)"
    return 0
  fi
  forge_exec_tool "$_tp_bin" migration:generate -d "$_tp_ds" --check "$FORGE_TMP/typeorm-check" > "$FORGE_TMP/typeorm.out" 2>&1
  if grep -q 'Unexpected changes in database schema were found' "$FORGE_TMP/typeorm.out"; then
    printf 'The entities changed, but no migration has the change: generate one (%s migration:generate -d %s <path>), review it and commit it.\n' "$_tp_bin" "$_tp_ds"
    cat "$FORGE_TMP/typeorm.out"
    return 1
  fi
  grep -q 'No changes in database schema were found' "$FORGE_TMP/typeorm.out" && return 0
  printf 'forge:%s skipped: %s migration:generate --check failed: %s\n' "$c_id" "$_tp_bin" "$(grep . "$FORGE_TMP/typeorm.out" | tail -n 1)"
  return 0
}

# forge_drizzle_applied: migrations (SQL and snapshots in the out folder) that exist in
# the base are not modified or deleted; a change goes in a new migration.
forge_drizzle_applied() {
  [ "$mode" != --all ] || return 0
  forge_drizzle_config || return 0
  _da_out=$(forge_drizzle_get out)
  _da_rc=0
  while IFS= read -r _da_f; do
    case $_da_f in
      "$_da_out"/*.sql|"$_da_out"/meta/*_snapshot.json|"$_da_out"/*/snapshot.json)
        if [ -f "$_da_f" ]; then _da_what=modified; else _da_what=deleted; fi
        printf '%s: an existing migration was %s; generate a new migration instead (drizzle-kit generate)\n' "$_da_f" "$_da_what"
        _da_rc=1 ;;
    esac
  done < "$FORGE_TMP/inbase"
  return "$_da_rc"
}

# forge_drizzle_squawk [files]: Squawk (§13.1) on the changed SQL of the out folder
# when the dialect is postgresql; with no files (--since, --all), on the newest migration.
forge_drizzle_squawk() {
  forge_drizzle_config || return 0
  [ "$(forge_drizzle_get dialect)" = postgresql ] || return 0
  _dq_out=$(forge_drizzle_get out)
  if [ "$#" -eq 0 ]; then
    set -- $(git ls-files -- "$_dq_out/*.sql" | sort | tail -n 1)
  else
    for _dq_f in "$@"; do
      shift
      case $_dq_f in "$_dq_out"/*.sql) set -- "$@" "$_dq_f" ;; esac
    done
  fi
  forge_squawk_files "$@"
}

# forge_vue_vfor_keys <files>: in the templates of .vue files, each element with v-for
# (not <template>) also binds a key (:key, v-bind:key). Prints "path:line: ..." per element
# (the line of its v-for). Comments, <script> and <style> are skipped; quotes are respected.
forge_vue_vfor_keys() {
  _vk_rc=0
  for _vk_f in "$@"; do
    case $_vk_f in *.vue) ;; *) continue ;; esac
    [ -f "$_vk_f" ] || continue
    awk -v f="$_vk_f" '
      { s = s $0 "\n" }
      END {
        n = length(s); ln = 1; skip = ""
        for (i = 1; i <= n; i++) {
          c = substr(s, i, 1)
          if (c == "\n") { ln++; continue }
          if (c != "<") continue
          if (substr(s, i, 4) == "<!--") {
            j = index(substr(s, i + 4), "-->"); if (!j) break
            t = substr(s, i, j + 6); ln += gsub(/\n/, "", t); i += j + 5; continue
          }
          if (!match(substr(s, i + 1, 1), /[A-Za-z\/]/)) continue
          # one tag, quotes respected
          start = ln; q = ""; tag = ""
          for (i++; i <= n; i++) {
            c = substr(s, i, 1)
            if (c == "\n") ln++
            if (q != "") { if (c == q) q = ""; tag = tag c; continue }
            if (c == "\"" || c == "\047") q = c
            else if (c == ">") break
            tag = tag c
          }
          name = tag; sub(/[[:space:]].*$/, "", name); if (length(name) > 1) sub(/\/$/, "", name); shown = name; name = tolower(name)
          if (skip != "") { if (name == "/" skip) skip = ""; continue }
          if ((name == "script" || name == "style") && tag !~ /\/$/) { skip = name; continue }
          tag = " " tag
          if (name == "template" || tag !~ /[[:space:]]v-for[[:space:]]*=/) continue
          if (tag ~ /[[:space:]](:key|v-bind:key)[[:space:]]*=/) continue
          match(tag, /[[:space:]]v-for[[:space:]]*=/); pre = substr(tag, 1, RSTART)
          printf "%s:%d: <%s> has v-for without :key\n", f, start + gsub(/\n/, "", pre), shown
          bad = 1
        }
        exit bad
      }' "$_vk_f" || _vk_rc=1
  done
  return "$_vk_rc"
}

# OpenAPI clients generated by openapi-typescript, orval or @hey-api/openapi-ts: their
# files say so in their header, and a package.json script runs the generator.
FORGE_OPENAPI_MARK='auto-generated by (openapi-typescript|@hey-api/openapi-ts)|generated by orval'
FORGE_OPENAPI_GEN='"[^"]+"[[:space:]]*:[[:space:]]*"[^"]*(openapi-typescript|orval|openapi-ts)'
# forge_openapi_pairs: the "key": "value" pairs of a package.json on stdin that drive the
# generator (a script running it, its dependency version), one per line, whitespace aside.
forge_openapi_pairs() {
  awk '{ t = t $0 " " } END {
    while (match(t, /"[^"]*"[[:space:]]*:[[:space:]]*"[^"]*"/)) {
      p = substr(t, RSTART, RLENGTH); t = substr(t, RSTART + RLENGTH)
      if (p ~ /(openapi-typescript|orval|openapi-ts)/) { gsub(/[[:space:]]+/, " ", p); print p }
    }
  }'
}

# forge_openapi_spec <path>: an OpenAPI or Swagger document (JSON or YAML with an
# openapi / swagger version near its top).
forge_openapi_spec() {
  case $1 in *.json|*.yaml|*.yml) ;; *) return 1 ;; esac
  [ -f "$1" ] && head -c 4096 "$1" | grep -qE "[\"']?(openapi|swagger)[\"']?[[:space:]]*:[[:space:]]*[\"']?[0-9]"
}

# forge_openapi_generated <path>: a file whose header says one of the generators wrote it.
forge_openapi_generated() { [ -f "$1" ] && head -n 15 "$1" | grep -qiE "$FORGE_OPENAPI_MARK"; }

# forge_openapi_sync regenerate | hand-edits: compares the changed OpenAPI documents with
# the changed generated files. regenerate: a changed document needs a changed generated
# file. hand-edits: a changed generated file needs a changed document or generator config
# (orval.config.*, openapi-ts.config.*, redocly.*, or a package.json line naming the
# generator: its script or its dependency version), when the repository
# tracks a document (a client generated from a URL is not checked). Not with --all.
forge_openapi_sync() {
  [ "$mode" != --all ] || return 0
  _os_pkgs=$(git ls-files -- package.json '*/package.json')
  [ -n "$_os_pkgs" ] || return 0
  # shellcheck disable=SC2086
  _os_script=$(grep -hoE "$FORGE_OPENAPI_GEN" $_os_pkgs 2>/dev/null | head -n 1 | cut -d '"' -f 2)
  [ -n "$_os_script" ] || return 0
  forge_filter forge_openapi_spec < "$FORGE_TMP/existing" > "$FORGE_TMP/openapi.specs"
  forge_filter forge_openapi_generated < "$FORGE_TMP/existing" > "$FORGE_TMP/openapi.generated"
  case $1 in
    regenerate)
      [ -s "$FORGE_TMP/openapi.specs" ] && [ ! -s "$FORGE_TMP/openapi.generated" ] || return 0
      while IFS= read -r _os_f; do
        printf '%s: the OpenAPI document changed, but no generated client file did\n' "$_os_f"
      done < "$FORGE_TMP/openapi.specs"
      printf 'Regenerate the client (the "%s" script) and keep both changes together.\n' "$_os_script" ;;
    hand-edits)
      [ -s "$FORGE_TMP/openapi.generated" ] && [ ! -s "$FORGE_TMP/openapi.specs" ] || return 0
      grep -qE '(^|/)(orval\.config\.[a-z]+|openapi-ts\.config\.[a-z]+|redocly\.[a-z]+)$' "$FORGE_TMP/existing" && return 0
      # a package.json counts when its generator lines (script, dependency) changed
      while IFS= read -r _os_p; do
        case $_os_p in package.json|*/package.json) ;; *) continue ;; esac
        forge_openapi_pairs < "$_os_p" > "$FORGE_TMP/openapi.now" 2>/dev/null
        forge_openapi_pairs < "$FORGE_TMP/base/$_os_p" > "$FORGE_TMP/openapi.then" 2>/dev/null
        cmp -s "$FORGE_TMP/openapi.now" "$FORGE_TMP/openapi.then" || return 0
      done < "$FORGE_TMP/existing"
      [ -n "$(git ls-files -- '*.json' '*.yaml' '*.yml' | forge_filter forge_openapi_spec | head -n 1)" ] || return 0
      while IFS= read -r _os_f; do
        printf '%s: generated from the OpenAPI document (its header says so), but neither the document nor the generator config changed\n' "$_os_f"
      done < "$FORGE_TMP/openapi.generated"
      printf 'Change the OpenAPI document (or the server it is generated from), then regenerate with the "%s" script; never edit generated files by hand.\n' "$_os_script" ;;
  esac
  return 1
}

# forge_related_py_tests <files>: changed test files plus tests/**/test_<module>.py
# for each changed module, one per line; at Stop, also the test files edited during
# the turn (<snapshot>/touched, by test-edit.sh), even when the edit was reverted.
forge_related_py_tests() {
  {
  if [ "${mode:-}" = --turn ] && [ -f "$FORGE_ARG/touched" ]; then
    while IFS= read -r _fr_f; do
      case ${_fr_f##*/} in test_*.py|*_test.py) [ -f "$_fr_f" ] && printf '%s\n' "$_fr_f" ;; esac
    done < "$FORGE_ARG/touched"
  fi
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
  done
  } | sort -u
}

# forge_state_file <name>: an absolute path for a tool's cache file (tsc's build info)
# under the repository root's .forge/state/ (ignored by git), never in the project: in
# a workspace package (FORGE_WS_ROOT set), one per package, "<package dir with / as
# ->.<name>", so packages neither share it nor get a .forge/ of their own.
forge_state_file() {
  _sf_root=${FORGE_WS_ROOT:-$(pwd)}
  _sf_pkg=''
  if [ -n "${FORGE_WS_ROOT:-}" ]; then
    _sf_pkg=$(pwd)
    _sf_pkg=${_sf_pkg#"$FORGE_WS_ROOT"}
    _sf_pkg=$(printf '%s' "${_sf_pkg#/}" | tr '/' '-')
  fi
  mkdir -p "$_sf_root/.forge/state" 2>/dev/null
  printf '%s/.forge/state/%s%s' "$_sf_root" "${_sf_pkg:+$_sf_pkg.}" "$1"
}

# forge_in_dirs <fn> <files|all>: runs <fn> in each workspace package of $c_dirs, with
# FORGE_WS_ROOT set (tool() falls back to the root's install). "files": only in packages
# holding files of $FORGE_FILES, given to <fn> relative to the package. Relative paths
# in the output are made relative to the repository again, so diagnostics are
# attributed as at the root. Fails if <fn> fails in one package.
forge_in_dirs() {
  _fd_root=$(pwd)
  _fd_rc=0
  _fd_files=$FORGE_FILES
  _fd_ifs=$IFS
  IFS=' '
  set -- "$1" "$2" $c_dirs
  IFS=$_fd_ifs
  _fd_fn=$1
  _fd_form=$2
  shift 2
  for _fd_dir; do
    FORGE_FILES=$_fd_files
    if [ "$_fd_form" = files ]; then
      FORGE_FILES=$(printf '%s\n' "$_fd_files" | awk -v p="$_fd_dir/" 'index($0, p) == 1 { print substr($0, length(p) + 1) }')
      [ -n "$FORGE_FILES" ] || continue
    fi
    ( cd "$_fd_dir" && FORGE_WS_ROOT=$_fd_root && "$_fd_fn" ) > "$FORGE_TMP/dir.out" 2>&1 < /dev/null || _fd_rc=1
    forge_prefix_paths "$_fd_dir" < "$FORGE_TMP/dir.out"
  done
  FORGE_FILES=$_fd_files
  return "$_fd_rc"
}

# forge_prefix_paths <dir>: stdin with the relative paths that start a diagnostic
# (the forms forge_diagnostics reads) or name a failing test (Vitest "FAIL  [|project| ]<file>",
# pytest "FAILED <file>::…" and "ERROR <file>", as forge_failed_tests reads them)
# prefixed with <dir>/; "../" is resolved.
forge_prefix_paths() {
  awk -v d="$1/" '
    BEGIN { ansi = sprintf("%c", 27) "\\[[0-9;]*[A-Za-z]" }
    {
      line = $0
      gsub(ansi, "", line)
      if (match(line, /^[[:space:]]*FAIL +(\|[^|]*\| )?/) || match(line, /^(FAILED|ERROR) /)) {
        lead = substr(line, 1, RLENGTH); rest = substr(line, RLENGTH + 1)
        if (rest !~ /^\// && rest ~ /^[^[:space:]]+\.[A-Za-z]+( |::|$)/) {
          p = d rest
          while (sub(/(^|\/)[^\/.][^\/]*\/\.\.\//, "/", p)) sub(/^\//, "", p)
          print lead p
          next
        }
      }
      match(line, /^[[:space:]]*(-->[[:space:]]*)?(Would reformat: )?/)
      lead = substr(line, 1, RLENGTH); rest = substr(line, RLENGTH + 1)
      if (rest !~ /^\// && (rest ~ /^[^[:space:]:(]+([(][0-9]+,[0-9]+[)]|:[0-9]+| format )/ || lead ~ /Would reformat: $/)) {
        p = d rest
        while (sub(/(^|\/)[^\/.][^\/]*\/\.\.\//, "/", p)) sub(/^\//, "", p)
        print lead p
      } else print $0
    }'
}

# forge_exec_check <fn> <timeout>: runs <fn> with output in $FORGE_TMP/out; 124 when
# it ran past <timeout> seconds and was stopped. Under Stop (FORGE_DEADLINE, in epoch
# seconds) the limit is at most what is left of the run's budget; a check with no time
# left is not started. FORGE_EXEC_LIMIT is the limit applied, FORGE_EXEC_WHY "timeout"
# or "budget".
forge_exec_check() {
  set -f
  FORGE_EXEC_LIMIT=$2
  FORGE_EXEC_WHY=timeout
  # under Stop, every check fits in what is left of the run's budget (FORGE_DEADLINE)
  if [ -n "${FORGE_DEADLINE:-}" ]; then
    _fe_left=$((FORGE_DEADLINE - $(date +%s)))
    [ "$_fe_left" -gt 0 ] || _fe_left=0
    if [ -z "$FORGE_EXEC_LIMIT" ] || [ "$_fe_left" -lt "$FORGE_EXEC_LIMIT" ]; then FORGE_EXEC_LIMIT=$_fe_left; FORGE_EXEC_WHY=budget; fi
  fi
  if [ "$FORGE_EXEC_LIMIT" = 0 ]; then
    : > "$FORGE_TMP/out"
    set +f
    return 124
  fi
  if [ -n "$FORGE_EXEC_LIMIT" ]; then
    rm -f "$FORGE_TMP/timedout"
    "$1" > "$FORGE_TMP/out" 2>&1 &
    _fe_pid=$!
    ( sleep "$FORGE_EXEC_LIMIT"; : > "$FORGE_TMP/timedout"; printf 'forge: timed out after %ss\n' "$FORGE_EXEC_LIMIT" >> "$FORGE_TMP/out"; forge_kill_tree "$_fe_pid" ) >/dev/null 2>&1 &
    _fe_watch=$!
    wait "$_fe_pid" 2>/dev/null
    _fe_rc=$?
    forge_kill_watch "$_fe_watch"
    if [ -f "$FORGE_TMP/timedout" ]; then rm -f "$FORGE_TMP/timedout"; _fe_rc=124; fi
  else
    ( "$1" ) > "$FORGE_TMP/out" 2>&1 < /dev/null
    _fe_rc=$?
  fi
  set +f
  return "$_fe_rc"
}

# forge_excerpt <n>: at most <n> lines of a failed run's output (stdin), the failure
# first. When the whole output fits it is printed as is. Otherwise: its summary lines
# (vitest "Test Files … failed" and "Tests … failed", pytest's short summary "FAILED …"
# and "ERROR …" and its "=== … failed … ===" line, "forge: timed out"), then its
# failure details (each vitest "FAIL" line with the error lines of its block and the
# first stack frame outside node_modules; pytest "____ test ____" headers, "E " lines
# and "path:line: Error" locations); when the output has neither, the lines that
# mention an error or a failure (not warnings). Then the rest of the output in order
# (warnings included) while <n> allows, and "… (<k> more lines)" for what is left out.
forge_excerpt() {
  awk -v n="$1" '
    BEGIN { esc = sprintf("%c", 27) "\\[[0-9;]*[A-Za-z]" }
    { l = $0; gsub(esc, "", l); if (length(l) > 300) l = substr(l, 1, 297) "..."; line[NR] = l }
    function key(i, c) { if (!(i in cls)) { cls[i] = c; nk[c]++ } }
    END {
      if (NR <= n) { for (i = 1; i <= NR; i++) print line[i]; exit }
      blk = 0; e = 0
      for (i = 1; i <= NR; i++) {
        l = line[i]
        if (l ~ /^forge: timed out/ || l ~ /^ *Test Files .*failed/ || l ~ /^ *Tests .*failed/ || l ~ /^(FAILED|ERROR) / || l ~ /^=+ .*([0-9]+ (failed|errors?)).* =+$/) { key(i, 1); blk = 0; continue }
        if (l ~ /^ *FAIL  /) { key(i, 2); blk = 1; nb = 0; frame = 0; continue }
        if (blk) {
          if (index(l, "⎯")) { blk = 0; continue }
          if (l ~ /^ *[0-9]*\|/ || l ~ /^ *$/) continue
          if (nb < 5 && (nb == 0 || l ~ /Error|Caused by|[Ee]xpected|Received|^ *[-+] / || index(l, "→"))) { key(i, 2); nb++; continue }
          if (!frame && index(l, "❯") && l !~ /node_modules/) { key(i, 2); frame = 1 }
          continue
        }
        if (l ~ /^_{3,} .* _{3,}$/) { key(i, 2); e = 0; continue }
        if (l ~ /^E   / && e < 4) { key(i, 2); e++; continue }
        if (l ~ /^[^ :]+\.py:[0-9]+: [A-Za-z]*(Error|Exception)/) { key(i, 2); continue }
      }
      if (!nk[1] && !nk[2])
        for (i = 1; i <= NR; i++)
          if (line[i] ~ /[Ee]rror|ERROR|FAIL|[Ff]ail(ed|ure)|[Pp]anic|Traceback|[Rr]efused/ && !(line[i] ~ /[Ww]arn/ && line[i] !~ /[Ee]rror/)) key(i, 3)
      shown = 0; sep = 0
      for (c = 1; c <= 3; c++)
        for (i = 1; i <= NR && shown < n - 1; i++)
          if ((i in cls) && cls[i] == c) { print line[i]; shown++ }
      if (shown && shown < n - 2) { print "--- output:"; sep = 1 }
      for (i = 1; i <= NR && shown + sep < n - 1; i++)
        if (!(i in cls)) { print line[i]; shown++ }
      if (NR > shown) printf "... (%d more lines)\n", NR - shown
    }'
}

# forge_diagnostics <output> <file|line> [print]: sorts the error diagnostics of a
# failed run by what they name (tsc "path(l,c)", "path:l:c" from Python and JS
# linters, "--> path:l:c"; "Would reformat: path" and Biome's "path format" name a
# file only; absolute paths under the repository are made relative; warning-only
# lines are ignored, unless the check's c_warn is 1: its warnings count as errors). A diagnostic counts when it names a file of $FORGE_TMP/changed
# ("file"), or an added line of $FORGE_TMP/added ("line"; one naming only a file, or
# line 0, counts when that file changed). Prints "changed" when one counts, "other" when
# they name only other files or lines, "warnings" when the output has warnings and no
# error at all (a tool that exits non-zero on warnings), "none" when no line names a file. With
# "print", prints the lines that count instead. ANSI colour sequences are ignored, so a
# tool that colours its output (tsc with "pretty": true) is attributed the same way.
forge_diagnostics() {
  awk -v r1="$(pwd)" -v r2="$(pwd -P)" -v level="$2" -v mode="$3" -v warn="${c_warn:-0}" '
    BEGIN { ansi = sprintf("%c", 27) "\\[[0-9;]*[A-Za-z]" }
    FILENAME == ARGV[1] { ch[$0] = 1; next }
    FILENAME == ARGV[2] { if ($2 == "*") whole[$1] = 1; else ad[$1 "\t" $2] = 1; next }
    {
      line = $0
      gsub(ansi, "", line)
      if (warn != 1 && line ~ /[Ww]arning/ && line !~ /[Ee]rror/) {
        # a warning diagnostic: a location, then "warning" as a word (not DeprecationWarning)
        if (line ~ /^[[:space:]]*(-->[[:space:]]*)?[^[:space:]:(]+([(][0-9]+,[0-9]+[)]|:[0-9]+)/ && line ~ /(^|[^A-Za-z])[Ww]arning([^A-Za-z]|$)/) warned = 1
        next
      }
      if ((line ~ /[Ee]rror/ || warn == 1 && line ~ /[Ww]arning/) && line !~ /(^|[^0-9])0 (errors?|warnings?)([^A-Za-z]|$)/) erred = 1
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
      if (p !~ /^\.forge\//) project = 1
      if (level == "line") counts = (p in whole) || (ln == "" || ln == 0 ? (p in ch) : ((p "\t" ln) in ad))
      else counts = (p in ch)
      if (counts) { hit = 1; if (mode == "print") print $0 }
    }
    END { if (mode != "print") print (hit ? "changed" : named && !project ? "forge" : named ? "other" : (warned && !erred) ? "warnings" : "none") }' "$FORGE_TMP/changed" "$FORGE_TMP/added" "$1"
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
      forge_any "forge_when_$1" < "$FORGE_TMP/changed" || return 0
    fi
    forge_select "forge_scope_$1" < "$FORGE_TMP/existing"
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
  forge_exec_check "$_fc_fn" "$c_timeout"
  case $? in
    0) grep '^forge:' "$FORGE_TMP/out"; return 0 ;;
    124)
      if [ "$FORGE_EXEC_WHY" = budget ]; then
        printf 'forge:%s failed: stopped after %ss: the Stop hook'"'"'s time budget (%s s, FORGE_STOP_BUDGET) ran out before it finished; its output until then:\n' "$c_id" "$FORGE_EXEC_LIMIT" "${FORGE_BUDGET:-}"
      else
        printf 'forge:%s failed: timed out after %ss and was stopped (a hung test, or a service that never answers?); its output until then:\n' "$c_id" "$c_timeout"
      fi
      grep -v '^forge: timed out after' "$FORGE_TMP/out" | forge_excerpt 15
      return 1 ;;
  esac
  # advice (§10.7) is shown as the check gave it, never attributed
  if [ -f "$(forge_advised_mark)" ]; then
    printf 'forge:%s failed\n' "$c_id"
    forge_excerpt 15 < "$FORGE_TMP/out"
    return 1
  fi
  # outside --all, only errors in the change count (§10.1 step 5)
  _fc_level=$(forge_attribute_level "$c_stage" "$_fc_whole")
  # findings only in Forge's own helper scripts (.forge/helpers/, checked by integrity)
  # are not the project's, in every mode
  if [ -n "$_fc_level" ] && [ "$(forge_diagnostics "$FORGE_TMP/out" "$_fc_level")" = forge ]; then
    printf 'forge:%s ok (its findings are only in Forge'"'"'s own .forge/ files)\n' "$c_id"
    return 0
  fi
  if [ "$mode" != --all ] && [ -n "$_fc_level" ]; then
    case $(forge_diagnostics "$FORGE_TMP/out" "$_fc_level") in
      other)
        if [ "$_fc_level" = line ]; then
          printf 'forge:%s ok for the added lines (findings remain on lines this change did not add: sh .forge/check.sh --all)\n' "$c_id"
        else
          printf 'forge:%s ok for the changed files (errors remain in unchanged files: sh .forge/check.sh --all)\n' "$c_id"
        fi
        return 0 ;;
      warnings)
        if [ "$_fc_level" = line ]; then printf 'forge:%s ok for the added lines (only warnings)\n' "$c_id"
        else printf 'forge:%s ok for the changed files (only warnings)\n' "$c_id"
        fi
        return 0 ;;
      changed)
        printf 'forge:%s failed\n' "$c_id"
        forge_diagnostics "$FORGE_TMP/out" "$_fc_level" print | first_lines 15
        return 1 ;;
    esac
  fi
  # Stop: failing tests are re-run at the start of the turn (§10.6)
  if [ "$c_stage" = test ] && [ "$mode" = --turn ] && [ "$c_rerun" = 1 ]; then
    forge_test_compare "$1"
    return
  fi
  printf 'forge:%s failed\n' "$c_id"
  if [ "$c_stage" = test ]; then forge_excerpt 25; else forge_excerpt 15; fi < "$FORGE_TMP/out"
  return 1
}

# ---- failing tests compared with the start of the turn (Stop, §10.6)

# forge_failed_tests: the failing tests a run reports (stdin), one id per line:
# Vitest "FAIL  [|project| ]<file> > <describe> > <test>" (a file that failed to
# load: "<file>"), pytest's short summary "FAILED <node id>" and "ERROR <node id>".
forge_failed_tests() {
  awk 'BEGIN { esc = sprintf("%c", 27) "\\[[0-9;]*[A-Za-z]" }
    { gsub(esc, "") }
    /^ *FAIL  / { t = $0; sub(/^ *FAIL +/, "", t); sub(/ \[ .* \]$/, "", t) }
    /^(FAILED|ERROR) [^ ]/ { t = $0; sub(/^[A-Z]+ /, "", t); sub(/ - .*$/, "", t) }
    t != "" { sub(/[[:space:]]+$/, "", t); print t; t = "" }' | LC_ALL=C sort -u
}

# forge_test_files: the test file of each id on stdin (Vitest: relative to its
# project, which a Vitest filter matches as part of the path), one per line.
forge_test_files() {
  sed -e 's/^|[^|]*| //' -e 's/ > .*$//' -e 's/::.*$//' | LC_ALL=C sort -u
}

# forge_env_errors: the errors in a run's output (stdin) that point at the
# environment rather than the code (a connection refused, a failed DNS lookup, a
# timeout), as short phrases, at most 5.
forge_env_errors() {
  awk 'BEGIN { svc["5432"] = "Postgres"; svc["3306"] = "MySQL"; svc["6379"] = "Redis"; svc["27017"] = "MongoDB"
           svc["1433"] = "SQL Server"; svc["9200"] = "Elasticsearch"; svc["5672"] = "RabbitMQ"; svc["11211"] = "Memcached" }
    /^forge: timed out/ || /^(FAILED|ERROR) / { next }
    {
      p = ""
      if (match($0, /ECONNREFUSED [^ ,;)]+:[0-9]+/)) {
        a = substr($0, RSTART + 13, RLENGTH - 13); port = a; sub(/.*:/, "", port)
        p = ((port in svc) ? svc[port] : "a service") " refused the connection at " a " (ECONNREFUSED)"
      } else if (match($0, /port [0-9]+ failed: Connection refused/)) {
        port = substr($0, RSTART + 5); sub(/ .*/, "", port)
        p = ((port in svc) ? svc[port] : "a service") " refused the connection on port " port
      } else if (match($0, /(EAI_AGAIN|ENOTFOUND) [^ ,;)'"'"'"]+/)) {
        h = substr($0, RSTART, RLENGTH); c = h; sub(/ .*/, "", c); sub(/^[^ ]+ /, "", h)
        p = "DNS lookup for " h " failed (" c "): no network?"
      } else if ($0 ~ /[Cc]onnection refused|ECONNRESET|ETIMEDOUT|EHOSTUNREACH|ENETUNREACH|getaddrinfo|[Cc]ould not connect|[Tt]imed out|[Tt]imeout of [0-9]+ ?ms|Name or service not known|Temporary failure in name resolution/) {
        p = $0; gsub(/^[[:space:]]+|[[:space:]]+$/, "", p)
        if (length(p) > 160) p = substr(p, 1, 157) "..."
      }
      if (p != "" && !(p in seen)) { seen[p] = 1; if (++k <= 5) print p }
    }'
}

# forge_base_tree <dir>: the project as it was at the start of the turn, in <dir>:
# the files git sees now (tracked and untracked, not ignored), with this turn's
# changes undone from $FORGE_TMP/base (files added during the turn left out). Of the
# ignored paths: Python virtual environments are linked, with the project's own code
# they import (editable installs) taken from the copy (PYTHONPATH in
# $FORGE_TMP/base-tree.pythonpath); each node_modules directory is rebuilt from
# links to its packages, a link to a package of the repository (a workspace package,
# a file: dependency) pointing to that package in the copy, and test-run caches
# (.vite, .vitest, .cache) left out; caches and reports that test runs rewrite
# (__pycache__, .pytest_cache, coverage, …) are left out; everything else (.env, a
# SQLite file, dist/) is copied, so a test run there never writes the user's files.
# Fails, with the reason in $FORGE_TMP/base-tree.why, when the copy cannot be
# isolated: what Python would import from the working tree instead of the copy, or
# ignored files over FORGE_BASE_COPY_KB (262144) to copy.
# The repository, its index, its stash and its working tree are not touched.
forge_base_tree() {
  mkdir -p "$1" || return 1
  _bt_root=$(pwd -P)
  : > "$FORGE_TMP/base-tree.why"; : > "$FORGE_TMP/base-tree.pythonpath"; : > "$FORGE_TMP/base-tree.venvs"; : > "$FORGE_TMP/base-tree.copy"
  git ls-files -z --cached --others --exclude-standard 2>/dev/null | tr '\000' '\n' | while IFS= read -r _bt_f; do
    if [ -f "$_bt_f" ] || [ -L "$_bt_f" ]; then printf './%s\n' "$_bt_f"; fi
  done > "$FORGE_TMP/base-tree.files"
  tar -cf - -T "$FORGE_TMP/base-tree.files" 2>/dev/null | ( cd "$1" && tar -xf - ) || return 1
  while IFS= read -r _bt_f; do
    rm -f "$1/$_bt_f"
    if [ -f "$FORGE_TMP/base/$_bt_f" ]; then mkdir -p "$1/$(dirname "$_bt_f")" && cp "$FORGE_TMP/base/$_bt_f" "$1/$_bt_f"; fi
  done < "$FORGE_TMP/changed"
  git ls-files -z --others --ignored --exclude-standard --directory 2>/dev/null | tr '\000' '\n' > "$FORGE_TMP/base-tree.ignored"
  while IFS= read -r _bt_f; do
    _bt_f=${_bt_f%/}
    case $_bt_f in ''|.forge/state|.forge/state/*) continue ;; esac
    if [ -e "$1/$_bt_f" ] || [ -L "$1/$_bt_f" ]; then continue; fi
    case ${_bt_f##*/} in
      __pycache__|.pytest_cache|.mypy_cache|.ruff_cache|.hypothesis|.coverage|.coverage.*|coverage|htmlcov|.nyc_output|.vite|.vitest|.turbo|.eslintcache|*.tsbuildinfo) continue ;;
    esac
    mkdir -p "$1/$(dirname "$_bt_f")" || return 1
    if [ -d "$_bt_f" ] && [ -f "$_bt_f/pyvenv.cfg" ]; then
      ln -s "$_bt_root/$_bt_f" "$1/$_bt_f" && printf '%s\n' "$_bt_f" >> "$FORGE_TMP/base-tree.venvs"
    elif [ "${_bt_f##*/}" = node_modules ] && [ -d "$_bt_f" ]; then
      forge_base_modules "$_bt_f" "$1" || return 1
    else
      printf '%s\n' "$_bt_f" >> "$FORGE_TMP/base-tree.copy"
    fi
  done < "$FORGE_TMP/base-tree.ignored"
  if [ -s "$FORGE_TMP/base-tree.copy" ]; then
    _bt_kb=$(while IFS= read -r _bt_f; do du -sk "$_bt_f" 2>/dev/null; done < "$FORGE_TMP/base-tree.copy" | awk '{ s += $1 } END { print s + 0 }')
    if [ "$_bt_kb" -gt "${FORGE_BASE_COPY_KB:-262144}" ]; then
      printf 'the files git ignores here (%s KiB, e.g. %s) are too large to copy' "$_bt_kb" "$(head -n 1 "$FORGE_TMP/base-tree.copy")" > "$FORGE_TMP/base-tree.why"
      return 1
    fi
    while IFS= read -r _bt_f; do cp -RP "$_bt_f" "$1/$_bt_f" 2>/dev/null; done < "$FORGE_TMP/base-tree.copy"
  fi
  [ -s "$FORGE_TMP/base-tree.venvs" ] || return 0
  forge_base_python "$1"
}

# forge_base_modules <node_modules path> <base dir>: <base dir>/<path> as a directory
# of links to the packages of <path>; a package that is a link into the repository
# outside node_modules (a workspace package) is linked to its copy in <base dir>.
forge_base_modules() {
  mkdir -p "$2/$1" || return 1
  for _bm_e in "$1"/* "$1"/.[!.]* "$1"/..?*; do
    [ -e "$_bm_e" ] || [ -L "$_bm_e" ] || continue
    case ${_bm_e##*/} in .vite|.vitest|.cache) continue ;; esac
    if [ -d "$_bm_e" ] && [ ! -L "$_bm_e" ]; then
      case ${_bm_e##*/} in
        @*)
          mkdir -p "$2/$_bm_e" || return 1
          for _bm_s in "$_bm_e"/*; do
            [ -e "$_bm_s" ] || [ -L "$_bm_s" ] || continue
            forge_base_link "$_bm_s" "$2" || return 1
          done
          continue ;;
      esac
    fi
    forge_base_link "$_bm_e" "$2" || return 1
  done
}

# forge_base_link <path> <base dir>: <base dir>/<path> a link to <path> in the working
# tree, or, when <path> is a link to a directory of the repository outside
# node_modules, to that directory in <base dir>.
forge_base_link() {
  _bl_to=$_bt_root/$1
  if [ -L "$1" ] && [ -d "$1" ]; then
    _bl_real=$(cd -P "$1" 2>/dev/null && pwd -P)
    case $_bl_real/ in
      "$_bt_root"/*)
        _bl_rel=${_bl_real#"$_bt_root"/}
        case /$_bl_rel/ in */node_modules/*) ;; *) _bl_to=$2/$_bl_rel ;; esac ;;
    esac
  fi
  ln -s "$_bl_to" "$2/$1"
}

# FORGE_PY_LEAKS: run by a virtual environment's python as <script> <root> <venv>: the
# first project modules (packages with __init__.py, modules) on its path or in its
# editable-install finders that resolve under <root> (outside <venv>), not elsewhere.
FORGE_PY_LEAKS='import importlib.util, os, sys
root, venv = os.path.realpath(sys.argv[1]), os.path.realpath(sys.argv[2])
def real(p):
    p = os.path.realpath(p)
    return (p == root or p.startswith(root + os.sep)) and not (p == venv or p.startswith(venv + os.sep))
names = set()
for p in sys.path:
    if p and os.path.isdir(p) and real(p):
        for e in os.listdir(p):
            if e.endswith(".py") and e[:-3].isidentifier(): names.add(e[:-3])
            elif e.isidentifier() and os.path.isfile(os.path.join(p, e, "__init__.py")): names.add(e)
for m in list(sys.modules.values()):
    mapping = getattr(m, "MAPPING", None)
    if (getattr(m, "__name__", "") or "").startswith("__editable__") and isinstance(mapping, dict):
        names.update(k for k, v in mapping.items() if isinstance(v, str) and real(v))
leak = []
for n in sorted(x for x in names if not x.startswith("__")):
    try:
        s = importlib.util.find_spec(n)
    except Exception:
        continue
    if s is None: continue
    where = s.origin if s.origin not in (None, "namespace", "built-in", "frozen") else next(iter(s.submodule_search_locations or []), None)
    if where and real(where): leak.append(n)
print(" ".join(leak[:5]))
'

# forge_base_python <base dir>: for each virtual environment of
# $FORGE_TMP/base-tree.venvs, the repository paths its .pth files and editable-install
# finders (__editable___*_finder.py MAPPING) add become their copies in <base dir>,
# first on PYTHONPATH ($FORGE_TMP/base-tree.pythonpath). Then the environment's
# python, run in <base dir> with that PYTHONPATH, checks that every module it could
# import from the working tree resolves to the copy; fails (base-tree.why) otherwise.
forge_base_python() {
  while IFS= read -r _bp_v; do
    for _bp_s in "$_bp_v"/lib/python*/site-packages "$_bp_v"/lib64/python*/site-packages; do
      [ -d "$_bp_s" ] || continue
      for _bp_p in "$_bp_s"/*.pth; do
        [ -f "$_bp_p" ] && grep -v -e '^import[[:space:]]' -e '^#' "$_bp_p"
      done
      for _bp_p in "$_bp_s"/__editable__*finder.py; do
        [ -f "$_bp_p" ] && grep -E '^MAPPING' "$_bp_p" | grep -oE "'/[^']+'|\"/[^\"]+\"" | tr -d "'\"" | while IFS= read -r _bp_m; do dirname "$_bp_m"; done
      done
    done
  done < "$FORGE_TMP/base-tree.venvs" | while IFS= read -r _bp_p; do
    _bp_p=${_bp_p%/}
    for _bp_r in "$_bt_root" "$FORGE_ROOT"; do
      case $_bp_p/ in "$_bp_r"/*) _bp_rel=${_bp_p#"$_bp_r"}; printf '%s%s\n' "$1" "$_bp_rel"; break ;; esac
    done
  done | awk '!seen[$0]++' > "$FORGE_TMP/base-tree.pythonpath"
  _bp_pp=$(awk 'NR > 1 { printf ":" } { printf "%s", $0 }' "$FORGE_TMP/base-tree.pythonpath")
  while IFS= read -r _bp_v; do
    [ -x "$_bp_v/bin/python" ] || continue
    _bp_vr=$(cd -P "$_bp_v" && pwd -P)
    if ! _bp_leak=$(cd "$1" && PYTHONPATH=$_bp_pp${PYTHONPATH:+:$PYTHONPATH} "$_bt_root/$_bp_v/bin/python" -c "$FORGE_PY_LEAKS" "$_bt_root" "$_bp_vr" 2>/dev/null); then
      printf 'could not check which code %s/bin/python imports' "$_bp_v" > "$FORGE_TMP/base-tree.why"
      return 1
    fi
    if [ -n "$_bp_leak" ]; then
      printf '%s/bin/python imports %s from the working tree, not from the copy' "$_bp_v" "$_bp_leak" > "$FORGE_TMP/base-tree.why"
      return 1
    fi
  done < "$FORGE_TMP/base-tree.venvs"
  return 0
}

# forge_rerun_in_base: the rerun form of check $_fb_n on $FORGE_FILES, in the base tree.
forge_rerun_in_base() {
  cd "$FORGE_TMP/base-tree" || return 1
  if [ -s "$FORGE_TMP/base-tree.pythonpath" ]; then
    PYTHONPATH=$(awk 'NR > 1 { printf ":" } { printf "%s", $0 }' "$FORGE_TMP/base-tree.pythonpath")${PYTHONPATH:+:$PYTHONPATH}
    export PYTHONPATH
  fi
  "forge_check_rerun_$_fb_n"
}

# forge_test_compare <n>: test check <n> failed under --turn ($FORGE_TMP/out). Its
# failing test files are run again in the tree as it was at the start of the turn
# and the failing tests compared. A test failing now that passed then (or could not
# be compared) is a new failure: return 1, new failures first. When every failing
# test also fails then: return 3, with the tests, the errors that look environmental
# and what to do; they are also appended to $FORGE_UNVERIFIED for the Stop hook
# ("test<TAB><check> <id>", "env<TAB><phrase>").
forge_test_compare() {
  cp "$FORGE_TMP/out" "$FORGE_TMP/test.now"
  forge_failed_tests < "$FORGE_TMP/test.now" > "$FORGE_TMP/failing.now"
  if [ ! -s "$FORGE_TMP/failing.now" ]; then
    printf 'forge:%s failed\n' "$c_id"
    forge_excerpt 25 < "$FORGE_TMP/test.now"
    return 1
  fi
  rm -rf "$FORGE_TMP/base-tree"
  _ft_why=''
  if ! forge_base_tree "$FORGE_TMP/base-tree"; then
    _ft_why=$(cat "$FORGE_TMP/base-tree.why" 2>/dev/null)
    [ -n "$_ft_why" ] || _ft_why='the copy of the tree could not be made'
  else
    FORGE_FILES=$(forge_test_files < "$FORGE_TMP/failing.now")
    _fb_n=$1
    forge_exec_check forge_rerun_in_base "$c_timeout"
    _ft_rc=$?
    forge_failed_tests < "$FORGE_TMP/out" > "$FORGE_TMP/failing.base"
    if [ "$_ft_rc" -eq 124 ]; then
      _ft_why="the re-run there was stopped after ${FORGE_EXEC_LIMIT}s"
      [ "$FORGE_EXEC_WHY" != budget ] || _ft_why="$_ft_why, when the Stop hook's time budget ran out"
    elif [ "$_ft_rc" -ne 0 ] && [ ! -s "$FORGE_TMP/failing.base" ]; then _ft_why='the re-run there failed without naming a failing test'
    fi
  fi
  rm -rf "$FORGE_TMP/base-tree"
  # not comparable: every failing test counts as new (never excused on a guess)
  if [ -n "$_ft_why" ]; then
    printf 'forge:%s failed: %s failing test(s); comparison with the start of the turn was not possible (%s), so they count as new:\n' "$c_id" "$(awk 'END { print NR }' "$FORGE_TMP/failing.now")" "$_ft_why"
    first_lines 8 < "$FORGE_TMP/failing.now" | sed 's/^/  /'
    forge_excerpt 20 < "$FORGE_TMP/test.now"
    return 1
  fi
  if [ -s "$FORGE_TMP/failing.base" ]; then
    grep -vxF -f "$FORGE_TMP/failing.base" "$FORGE_TMP/failing.now" > "$FORGE_TMP/failing.new"
    grep -xF -f "$FORGE_TMP/failing.base" "$FORGE_TMP/failing.now" > "$FORGE_TMP/failing.old"
  else
    cp "$FORGE_TMP/failing.now" "$FORGE_TMP/failing.new"
    : > "$FORGE_TMP/failing.old"
  fi
  _ft_new=$(awk 'END { print NR }' "$FORGE_TMP/failing.new")
  _ft_old=$(awk 'END { print NR }' "$FORGE_TMP/failing.old")
  if [ "$_ft_new" -gt 0 ]; then
    printf 'forge:%s failed: %s failing test(s) passed at the start of the turn:\n' "$c_id" "$_ft_new"
    first_lines 8 < "$FORGE_TMP/failing.new" | sed 's/^/  /'
    [ "$_ft_old" -eq 0 ] || printf '(%s other failing test(s) also fail at the start of the turn: not caused by this change)\n' "$_ft_old"
    forge_excerpt 20 < "$FORGE_TMP/test.now"
    return 1
  fi
  forge_env_errors < "$FORGE_TMP/test.now" > "$FORGE_TMP/env"
  printf 'forge:%s %s failing test(s) also fail at the start of the turn (re-run there), so this change did not cause them, but it is not verified by them either:\n' "$c_id" "$_ft_old"
  first_lines 8 < "$FORGE_TMP/failing.old" | sed 's/^/  /'
  _ft_compose=''
  for _ft_f in compose.yaml compose.yml docker-compose.yml docker-compose.yaml; do
    if [ -f "$_ft_f" ]; then _ft_compose=$_ft_f; break; fi
  done
  if [ -s "$FORGE_TMP/env" ]; then
    printf 'Their errors look environmental:\n'
    sed 's/^/  /' "$FORGE_TMP/env"
    if [ -n "$_ft_compose" ]; then
      printf 'Set up what they need (e.g. docker compose up -d with %s) and run them again. ' "$_ft_compose"
    else
      printf 'Set up what they need (the services, the network) and run them again. '
    fi
  else
    printf 'Check whether they relate to the task: fix them if so, and run them again. '
  fi
  printf 'If that is not possible, stop again without changing files: the stop is then allowed and the user is told these tests could not be verified.\n'
  forge_excerpt 12 < "$FORGE_TMP/test.now"
  if [ -n "${FORGE_UNVERIFIED:-}" ]; then
    sed "s|^|test	$c_id |" "$FORGE_TMP/failing.old" >> "$FORGE_UNVERIFIED"
    sed 's/^/env	/' "$FORGE_TMP/env" >> "$FORGE_UNVERIFIED"
  fi
  return 3
}

# forge_advised_mark: the file that marks check $c_id as having given advice in this
# run (§10.7): its failure is advice, not a failing check.
forge_advised_mark() { printf '%s/advised.%s' "$FORGE_TMP" "$(printf '%s' "$c_id" | tr -c 'A-Za-z0-9_-' '_')"; }

# forge_run_stage <stage>: every check of <stage>; fails if one fails. The output of
# a check that gave advice goes to $FORGE_TMP/advice.out, printed at the end.
forge_run_stage() {
  _fs_i=1
  _fs_rc=0
  while [ "$_fs_i" -le "$FORGE_NCHECKS" ]; do
    forge_check_meta_"$_fs_i"
    if [ "$c_stage" = "$1" ]; then
      forge_run_check "$_fs_i" > "$FORGE_TMP/check.out"
      case $? in
        # most checks print nothing (not in the change): no cat then
        0) [ ! -s "$FORGE_TMP/check.out" ] || cat "$FORGE_TMP/check.out" ;;
        3) cat "$FORGE_TMP/check.out"; [ "$_fs_rc" -ne 0 ] || _fs_rc=3 ;;
        *)
          # advice goes after every real failure (check.sh prints it last)
          if [ -f "$(forge_advised_mark)" ]; then cat "$FORGE_TMP/check.out" >> "$FORGE_TMP/advice.out"; advised=1
          else cat "$FORGE_TMP/check.out"; _fs_rc=1
          fi ;;
      esac
    fi
    _fs_i=$((_fs_i + 1))
  done
  [ "$_fs_rc" -eq 0 ] || fail=$_fs_rc
  return "$_fs_rc"
}

# forge_file_stage <stage> <list>: forge_file_run then forge_file_report.
forge_file_stage() {
  forge_file_run "$1" "$2"
  forge_file_report "$1"
}

# forge_file_run <stage> <list>: the {files} form of each check of <stage>, run once
# on the files of <list> (one path per line) in its scope (PostToolUse); absent tools
# are skipped. Outputs of failed runs go to $FORGE_TMP/stage-<stage>.<n>, their
# numbers to $FORGE_TMP/stage-<stage>.failed, for forge_file_report.
forge_file_run() {
  _fp_i=1
  : > "$FORGE_TMP/stage-$1.failed"
  while [ "$_fp_i" -le "$FORGE_NCHECKS" ]; do
    forge_check_meta_"$_fp_i"
    if [ "$c_stage" = "$1" ] && [ "$c_files" = 1 ] && ! forge_missing_tool >/dev/null; then
      forge_select "forge_scope_$_fp_i" < "$2"
      if [ -n "$FORGE_FILES" ] && ! forge_exec_check "forge_check_changed_$_fp_i" "$c_timeout"; then
        mv "$FORGE_TMP/out" "$FORGE_TMP/stage-$1.$_fp_i"
        printf '%s\n' "$_fp_i" >> "$FORGE_TMP/stage-$1.failed"
      fi
    fi
    _fp_i=$((_fp_i + 1))
  done
  return 0
}

# forge_file_report <stage>: prints the failures forge_file_run recorded; lint findings
# only on the lines of $FORGE_TMP/added. Fails when one is printed.
forge_file_report() {
  _fr_rc=0
  while read -r _fr_i; do
    forge_check_meta_"$_fr_i"
    _fr_out=$FORGE_TMP/stage-$1.$_fr_i
    _fr_level=$(forge_attribute_level "$1" 0)
    _fr_what=none
    [ -n "$_fr_level" ] && _fr_what=$(forge_diagnostics "$_fr_out" "$_fr_level")
    case $_fr_what in
      other|warnings) ;;
      changed)
        printf 'forge:%s\n' "$c_id"
        forge_diagnostics "$_fr_out" "$_fr_level" print | first_lines 15
        _fr_rc=1 ;;
      *)
        printf 'forge:%s\n' "$c_id"
        first_lines 15 < "$_fr_out"
        _fr_rc=1 ;;
    esac
  done < "$FORGE_TMP/stage-$1.failed"
  return "$_fr_rc"
}

# forge_file_hashes <list>: "path<TAB>blob hash" for each file of <list>.
forge_file_hashes() {
  while read -r _fh_f; do printf '%s\t%s\n' "$_fh_f" "$(git hash-object -- "$_fh_f" 2>/dev/null)"; done < "$1"
}

# forge_file_fixes <list>: the safe fixes (fix_run) of each lint check, run once on the
# files of <list> in its scope. For each file they changed, prints "forge: auto-fixed
# <codes> in <file>" (codes from ruff's --show-fixes summary, per file), so Claude knows
# its view of the file is stale; the files go to $FORGE_TMP/noted.
forge_file_fixes() {
  _fx_i=1
  : > "$FORGE_TMP/fix.codes"
  forge_file_hashes "$1" > "$FORGE_TMP/fix.before"
  while [ "$_fx_i" -le "$FORGE_NCHECKS" ]; do
    forge_check_meta_"$_fx_i"
    if [ "$c_stage" = lint ] && [ "$c_fix" = 1 ] && ! forge_missing_tool >/dev/null; then
      forge_select "forge_scope_$_fx_i" < "$1"
      if [ -n "$FORGE_FILES" ]; then
        forge_exec_check "forge_check_fix_$_fx_i" "$c_timeout"
        # ruff: "Fixed N errors:", then "- <path>:" and "    1 × I001 (unsorted-imports)" lines
        awk '/^Fixed [0-9]+ error/ { on = 1; next }
          on && /^- .*:$/ { f = substr($0, 3, length($0) - 3); next }
          on && /^ +[0-9]+ / && match($0, /[A-Z]+[0-9]+ \(/) { print f "\t" substr($0, RSTART, RLENGTH - 2) }
          /^$/ { on = 0 }' "$FORGE_TMP/out" >> "$FORGE_TMP/fix.codes"
      fi
    fi
    _fx_i=$((_fx_i + 1))
  done
  while IFS='	' read -r _fx_f _fx_h; do
    [ "$(git hash-object -- "$_fx_f" 2>/dev/null)" != "$_fx_h" ] || continue
    _fx_codes=$(awk -F '\t' -v f="$_fx_f" '($1 == f || $1 == "") && !seen[$2]++ { printf "%s%s", (n++ ? ", " : ""), $2 }' "$FORGE_TMP/fix.codes")
    printf 'forge: auto-fixed %s in %s; re-read it before editing it again\n' "${_fx_codes:-safe lint fixes}" "$_fx_f"
    printf '%s\n' "$_fx_f" >> "$FORGE_TMP/noted"
  done < "$FORGE_TMP/fix.before"
  return 0
}

# forge_lint_fixes <list> <before>: after a lint run that writes (Biome's check --write
# applies the safe fixes and formats in one pass), "forge: auto-fixed safe lint fixes in
# <file>" for each file of <list> it changed beyond formatting: the copy saved before it
# (<before>/<path>), formatted (forge_format_copy), differs from the file now. Without a
# formatter for the file, any change counts. Files already noted are skipped.
forge_lint_fixes() {
  while read -r _lf_f; do
    [ -f "$2/$_lf_f" ] && ! cmp -s "$2/$_lf_f" "$_lf_f" || continue
    grep -qxF "$_lf_f" "$FORGE_TMP/noted" 2>/dev/null && continue
    forge_format_copy "$_lf_f" "$2/$_lf_f" "$FORGE_TMP/lf.fmt" && cmp -s "$FORGE_TMP/lf.fmt" "$_lf_f" && continue
    printf 'forge: auto-fixed safe lint fixes in %s; re-read it before editing it again\n' "$_lf_f"
    printf '%s\n' "$_lf_f" >> "$FORGE_TMP/noted"
  done < "$1"
  rm -f "$FORGE_TMP/lf.fmt"
  return 0
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
          other|warnings) ;;
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
    case $_mt_t in *@*) _mt_n="$(forge_tool_label "${_mt_t%%@*}") (${_mt_t#*@})" ;; *) _mt_n=$(forge_tool_label "$_mt_t") ;; esac
    forge_has_tool "$_mt_t" || _mt="${_mt:+$_mt, }$_mt_n"
  done
  if ! command -v bunx >/dev/null 2>&1 && ! command -v uvx >/dev/null 2>&1; then
    _mt="${_mt:+$_mt, }bunx or uvx (ast-grep)"
  fi
  for _mt_l in $FORGE_TYPES_LANGS; do
    case $_mt_l in
      python) command -v uvx >/dev/null 2>&1 || _mt="${_mt:+$_mt, }uvx (types engine: ty $FORGE_TY_VERSION)" ;;
      typescript) command -v bun >/dev/null 2>&1 || _mt="${_mt:+$_mt, }bun (types engine: the project's TypeScript)" ;;
    esac
  done
  printf '%s' "$_mt"
}

# forge_required_tools: every tool the checks use, one per line (<tool>@<dir> in a workspace package).
forge_required_tools() {
  _fq_i=1
  while [ "$_fq_i" -le "$FORGE_NCHECKS" ]; do
    forge_check_meta_"$_fq_i"
    printf '%s\n' "$c_tools" | tr ' ' '\n' | awk -v d="$c_dirs" 'NF { if (d == "") print; else { n = split(d, ds, " "); for (i = 1; i <= n; i++) print $0 "@" ds[i] } }'
    _fq_i=$((_fq_i + 1))
  done | awk 'NF && !seen[$0]++'
}

# ---- engine capabilities (SPECS §15): types

# The languages whose types engine the active rules use, and the engines' pins.
FORGE_TYPES_LANGS='@@TYPES_LANGS@@'
FORGE_TY_VERSION='@@TY_VERSION@@'
FORGE_ENGINE_PYTHON='@@ENGINE_PYTHON@@'
# the adapters' revision: a running daemon of another Forge version is restarted
FORGE_TYPES_REV='@@TYPES_REV@@'

# forge_ty <args>: Forge's pinned ty (§15.2), on the engine's Python, with the project's
# environment (.venv) and configuration ([tool.ty], ty.toml).
forge_ty() { uvx -q --python "$FORGE_ENGINE_PYTHON" --from "ty==$FORGE_TY_VERSION" ty "$@"; }

# forge_types_up <lang>: the language's daemon runs, with the pinned engine (started
# when it does not: by the first hook that needs it, or by SessionStart). Prints why
# and fails when it cannot start. Registers the hook's session, if any.
forge_types_up() {
  _tu_lang=$1
  _tu_d=$FORGE_STATE/types/$1
  case $1 in
    python) _tu_want="ty $FORGE_TY_VERSION $FORGE_TYPES_REV" ;;
    # the project's own TypeScript: a change of its version changes package.json or a lock file (restart)
    typescript) _tu_want="typescript (the project's) $FORGE_TYPES_REV" ;;
    *) printf 'no types engine for %s\n' "$1"; return 1 ;;
  esac
  forge_types_running "$1" "$_tu_want" && return 0
  forge_state_dir && mkdir -p "$FORGE_STATE/types" || { printf 'cannot create %s\n' "$FORGE_STATE/types"; return 1; }
  # one starter at a time; another one's start is waited for (its lock holds its pid)
  _tu_n=0
  until mkdir "$_tu_d.lock" 2>/dev/null; do
    _tu_p=$(cat "$_tu_d.lock/pid" 2>/dev/null)
    if [ -n "$_tu_p" ] && ! kill -0 "$_tu_p" 2>/dev/null; then rm -rf "$_tu_d.lock"; continue; fi
    [ "$_tu_n" -lt "${FORGE_TYPES_START_TIMEOUT:-60}" ] || { printf 'another hook is still starting the %s engine\n' "$1"; return 1; }
    sleep 1
    _tu_n=$((_tu_n + 1))
  done
  # this shell's own pid ($$ is the script's, which a background subshell outlives)
  _tu_me=$(exec sh -c 'echo "$PPID"')
  printf '%s\n' "$_tu_me" > "$_tu_d.lock/pid"
  # started meanwhile by the hook that held the lock
  if forge_types_running "$1" "$_tu_want"; then rm -rf "$_tu_d.lock"; return 0; fi
  # another pin or Forge: stopped (and waited for) before its state is replaced
  forge_types_stop "$1"
  rm -rf "$_tu_d"
  mkdir -p "$_tu_d/sessions" && mkfifo "$_tu_d/in" "$_tu_d/started" || { rm -rf "$_tu_d.lock"; printf 'cannot create %s\n' "$_tu_d"; return 1; }
  printf '%s\n' "$_tu_want" > "$_tu_d/want"
  case $1 in
    python) set -- uvx -q --python "$FORGE_ENGINE_PYTHON" --from "ty==$FORGE_TY_VERSION" python "$(pwd -P)/.forge/helpers/types_python.py" "$(pwd -P)" "$_tu_d" ;;
    typescript) set -- bun "$(pwd -P)/.forge/helpers/types_typescript.mjs" "$(pwd -P)" "$_tu_d" ;;
  esac
  # the engine detaches and says "ready ..." or "failed ..." on the started FIFO; a
  # launcher that fails before (no network for a first download) says "exited <code>".
  # Those two write on this shell's descriptor (7), not by path: the reader holds the
  # FIFO open for writing too, so it would wait forever for a line that cannot be
  # written once the state directory has been removed.
  exec 7<>"$_tu_d/started"
  { "$@" < /dev/null >> "$_tu_d/log" 2>&1 || printf 'exited %s\n' "$?" >&7; } &
  { sleep "${FORGE_TYPES_START_TIMEOUT:-60}" & printf '%s\n' "$!" > "$_tu_d.lock/sleep"; wait "$!"
    printf 'failed: not started within %ss\n' "${FORGE_TYPES_START_TIMEOUT:-60}" >&7; } >/dev/null 2>&1 &
  _tu_w=$!
  IFS= read -r _tu_l <&7
  forge_types_unwatch "$_tu_w" "$_tu_d.lock/sleep"
  exec 7<&-
  rm -rf "$_tu_d.lock"
  case $_tu_l in
    ready*)
      [ -z "${FORGE_INPUT:-}" ] || : > "$_tu_d/sessions/$(session_id)"
      FORGE_TYPES_STARTED="${FORGE_TYPES_STARTED:+$FORGE_TYPES_STARTED
}$_tu_lang"
      return 0 ;;
  esac
  printf 'the %s engine (%s) did not start: %s (log: %s)\n' "$_tu_lang" "$_tu_want" "$_tu_l" "$_tu_d/log"
  return 1
}

# forge_types_running <lang> <want>: its daemon runs with this engine; registers the
# hook's session with it.
forge_types_running() {
  forge_types_alive "$1" && [ "$(cat "$FORGE_STATE/types/$1/want" 2>/dev/null)" = "$2" ] || return 1
  [ -z "${FORGE_INPUT:-}" ] || : > "$FORGE_STATE/types/$1/sessions/$(session_id)"
}

# forge_types_down: stops the daemons this run started when no session uses them, and
# waits until they have exited (check.sh in CI or a pre-commit hook: nothing is left running).
forge_types_down() {
  for _td_l in ${FORGE_TYPES_STARTED:-}; do
    [ -n "$(ls "$FORGE_STATE/types/$_td_l/sessions" 2>/dev/null)" ] || forge_types_stop "$_td_l"
  done
  return 0
}

# forge_types_filter <matches>: keeps in the file of ast-grep --json=stream matches
# (with --include-metadata) only those whose forge_where the language's types engine
# confirms (types, §15.1; declared names, §15.6); matches without forge_where stay. An
# engine that cannot answer is not a pass: "forge:forge/types ..." lines name the rules
# left unchecked, and it fails.
forge_types_filter() {
  grep -q '"forge_where"' "$1" 2>/dev/null || return 0
  grep -v '"forge_where"' "$1" > "$1.kept"
  _tf_rc=0
  _tf_base=${FORGE_TYPES_TIMEOUT:-20}
  for _tf_lang in $FORGE_TYPES_LANGS; do
    case $_tf_lang in python) _tf_sg='"language":"Python"' ;; typescript) _tf_sg='"language":"Tsx"' ;; esac
    _tf_q=$1.$_tf_lang
    rm -rf "$_tf_q" && mkdir "$_tf_q" || return 1
    grep '"forge_where"' "$1" | grep -F "$_tf_sg" > "$_tf_q/req"
    [ -s "$_tf_q/req" ] || continue
    # After an edit (FORGE_TYPES_EDIT=1, post-edit.sh) an engine still loading the project's
    # tsconfig projects is waited for at most FORGE_TYPES_LOADING_WAIT seconds (5): the edit
    # is not held up, and the Stop check, which waits, checks these matches.
    _tf_t=$_tf_base
    if [ "${FORGE_TYPES_EDIT:-}" = 1 ] && [ -f "$FORGE_STATE/types/$_tf_lang/loading" ]; then _tf_t=${FORGE_TYPES_LOADING_WAIT:-5}; fi
    # not in a subshell: forge_types_up records what it started (FORGE_TYPES_STARTED)
    if forge_types_up "$_tf_lang" > "$_tf_q/why" && FORGE_TYPES_TIMEOUT=$_tf_t forge_types_ask "$_tf_lang" "$_tf_q" > "$_tf_q/answer" \
      && ! grep -q '^error	' "$_tf_q/answer"; then
      grep -v '^unchecked	' "$_tf_q/answer" >> "$1.kept"
      # a match whose metavariable is bound to code that is not an expression: not checked, never passed
      if grep -q '^unchecked	' "$_tf_q/answer"; then
        grep '^unchecked	' "$_tf_q/answer" | while IFS='	' read -r _ _tf_id _tf_why; do
          printf 'forge:forge/types %s was not checked: %s\n' "$_tf_id" "$_tf_why"
        done
        _tf_rc=1
      fi
    elif [ "${FORGE_TYPES_EDIT:-}" = 1 ] && [ -f "$FORGE_STATE/types/$_tf_lang/loading" ]; then
      grep -o '"ruleId":"[^"]*"' "$_tf_q/req" | sort -u | sed 's/^"ruleId":"//; s/"$//' | while IFS= read -r _tf_id; do
        printf 'forge:forge/types %s was not checked after this edit: the %s engine (%s) is still loading the project; the Stop check runs it\n' "$_tf_id" "$_tf_lang" "$(cat "$FORGE_STATE/types/$_tf_lang/ready" 2>/dev/null)"
      done
    else
      _tf_why=$(cat "$_tf_q/why")
      [ -n "$_tf_why" ] || _tf_why=$(grep '^error	' "$_tf_q/answer" 2>/dev/null | head -n 1 | cut -f 2-)
      grep -o '"ruleId":"[^"]*"' "$_tf_q/req" | sort -u | sed 's/^"ruleId":"//; s/"$//' | while IFS= read -r _tf_id; do
        printf 'forge:forge/types %s was not checked: %s\n' "$_tf_id" "${_tf_why:-the types engine gave no answer}"
      done
      _tf_rc=1
    fi
  done
  mv "$1.kept" "$1"
  return "$_tf_rc"
}
