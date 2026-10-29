# @module tests
# Tests (§10.5, §10.6): test integrity, the untested-change advice, and re-running failing
# tests on the tree at the start of the turn.

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
  ( cd "$1" && forge_do ast-grep scan "$FORGE_ROOT/.forge/ast-grep/sgconfig.yml" --inline-rules "$FORGE_TEST_BLOCKS" --filter '^forge-test' $(cat "$2") ) > "$FORGE_TMP/blocks.json"
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
    forge_for_agent || printf 'forge:%s skipped: ast-grep unavailable\n' "$c_id"
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

# Test files of the packs' conventions (pytest, Vitest, Playwright), as a POSIX ERE.
FORGE_TEST_PATH_ERE='(^|/)(tests?|__tests__|e2e)/|(^|/)(test_[^/]*|[^/]*_test|conftest)\.py$|\.(test|spec)\.[^/]*$'

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
  forge_do ast-grep scan "$FORGE_ROOT/.forge/ast-grep/sgconfig.yml" --inline-rules "$FORGE_SNAP_VALUES" --filter '^forge-snap' $(cat "$FORGE_TMP/snap.cand") > "$FORGE_TMP/snap.json"
  _ss_rc=$?
  set +f
  [ "$_ss_rc" -le 1 ] || { forge_for_agent || printf 'forge:%s skipped: ast-grep unavailable\n' "$c_id"; return 0; }
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
