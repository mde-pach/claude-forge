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
    uv|bun) command -v "$1" >/dev/null 2>&1 ;;
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

# forge_kill_tree <pid>: kills a process and its descendants.
forge_kill_tree() {
  for _fk_c in $(ps -A -o pid= -o ppid= 2>/dev/null | awk -v p="$1" '$2 == p { print $1 }'); do
    forge_kill_tree "$_fk_c"
  done
  kill "$1" 2>/dev/null
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
    "$1" > "$FORGE_TMP/out" 2>&1 < /dev/null
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
# ("file"), or an added line of $FORGE_TMP/added ("line"; one naming only a file
# counts when that file changed). Prints "changed" when one counts, "other" when
# they name only other files or lines, "none" when no line names a file. With
# "print", prints the lines that count instead.
forge_diagnostics() {
  awk -v r1="$(pwd)" -v r2="$(pwd -P)" -v level="$2" -v mode="$3" '
    FILENAME == ARGV[1] { ch[$0] = 1; next }
    FILENAME == ARGV[2] { if ($2 == "*") whole[$1] = 1; else ad[$1 "\t" $2] = 1; next }
    {
      line = $0
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
      if (level == "line") counts = (p in whole) || (ln == "" ? (p in ch) : ((p "\t" ln) in ad))
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

# forge_required_tools: every tool the checks use, one per line.
forge_required_tools() {
  _fq_i=1
  while [ "$_fq_i" -le "$FORGE_NCHECKS" ]; do
    forge_check_meta_"$_fq_i"
    printf '%s\n' "$c_tools" | tr ' ' '\n'
    _fq_i=$((_fq_i + 1))
  done | awk 'NF && !seen[$0]++'
}