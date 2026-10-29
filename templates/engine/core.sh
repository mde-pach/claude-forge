# @module core
# The change set and the checks: what changed against which base, added lines, running a check
# and attributing its diagnostics, the forbid and count rules, the per-edit runs. Every
# generated script has it. Expects IFS set to a newline, FORGE_TMP, the generated checks
# (FORGE_NCHECKS, forge_check_meta_<n>, forge_scope_<n>, forge_when_<n>, forge_check_run_<n>,
# forge_check_changed_<n>) and lib.sh (tool, first_lines, base and snapshot helpers). Each
# check sets c_id and its siblings (forge_check_meta_<n>); a module reads the change set from
# FORGE_TMP: changed, existing, deleted, inbase, base/<path>, added.

# Forge's own files in the repository: .forge/ (check.sh, its helper scripts) and the
# project plugin (.claude/skills/forge-project/: hooks, guard.rules). They are never part
# of a change or of --all's scope: integrity (check.sh step 1) covers them, the project's
# rules and tools do not.
FORGE_OWN_ERE='^(\.forge|\.claude/skills/forge-project)/'

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
      FORGE_BASE_REV=''
      read -r FORGE_BASE_REV 2>/dev/null < "$FORGE_SNAP/head" ;;
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
  esac | grep -vE "$FORGE_OWN_ERE" > "$FORGE_TMP/changed"
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

# forge_format_base <path> <base-copy> <out>: forge_format_copy of a base copy, kept since
# the base does not change: with a turn snapshot (FORGE_SNAP) in <snapshot>/formatted/
# (<snapshot>/unformattable/ when the formatter failed, <snapshot>/same/ when formatting
# leaves it as it is: returns 3, no copy written), else in <state>/base-format/<base
# commit>/ (one commit's at a time). post-edit and Stop then format each base once. The
# copies go when a formatter's config file at the root (pyproject.toml, ruff.toml,
# biome.json, .prettierrc*, .editorconfig, package.json, …) is newer than they are.
forge_format_base() {
  _fb_c=''
  if [ -n "${FORGE_SNAP:-}" ] && [ -d "$FORGE_SNAP" ]; then _fb_c=$FORGE_SNAP
  elif [ -n "${FORGE_STATE:-}" ] && [ -d "$FORGE_STATE" ] && [ -n "${FORGE_BASE_REV:-}" ]; then
    _fb_c=$FORGE_STATE/base-format/$FORGE_BASE_REV
    [ -d "$_fb_c" ] || { rm -rf "$FORGE_STATE/base-format"; mkdir -p "$_fb_c" 2>/dev/null; } || _fb_c=''
  fi
  if [ -z "$_fb_c" ]; then forge_format_copy "$@"; return; fi
  if [ -f "$_fb_c/format.stamp" ]; then
    # shellcheck disable=SC3013
    for _fb_f in pyproject.toml ruff.toml .ruff.toml biome.json biome.jsonc .prettierrc .prettierrc.json .prettierrc.yaml .prettierrc.yml .prettierrc.toml prettier.config.js prettier.config.mjs .editorconfig package.json dprint.json; do
      if [ -f "$_fb_f" ] && [ "$_fb_f" -nt "$_fb_c/format.stamp" ]; then rm -rf "$_fb_c/formatted" "$_fb_c/unformattable" "$_fb_c/same" "$_fb_c/format.stamp"; break; fi
    done
  fi
  [ -f "$_fb_c/format.stamp" ] || : > "$_fb_c/format.stamp"
  [ -f "$_fb_c/same/$1" ] && return 3
  if [ -f "$_fb_c/formatted/$1" ]; then cp "$_fb_c/formatted/$1" "$3"; return; fi
  [ -f "$_fb_c/unformattable/$1" ] && return 1
  forge_format_copy "$@"
  _fb_rc=$?
  # formatting leaves the base as it is: kept as such, so the next edit needs no copy
  [ "$_fb_rc" = 0 ] && cmp -s "$2" "$3" && _fb_rc=3
  case $_fb_rc in
    0) forge_dirname "$_fb_c/formatted/$1"; mkdir -p "$FORGE_DIR" && cp "$3" "$_fb_c/formatted/$1" ;;
    1) forge_dirname "$_fb_c/unformattable/$1"; mkdir -p "$FORGE_DIR" && : > "$_fb_c/unformattable/$1" ;;
    3) forge_dirname "$_fb_c/same/$1"; mkdir -p "$FORGE_DIR" && : > "$_fb_c/same/$1" ;;
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
  # the plain added lines without a formatted base, or when formatting leaves the base as it is (3)
  if [ ! -f "$2" ] || [ ! -s "$FORGE_TMP/al.raw" ] || [ "${FORGE_PLAIN_ADDED:-0}" = 1 ] || ! forge_format_base "$1" "$2" "$FORGE_TMP/al.base"; then
    forge_cat "$FORGE_TMP/al.raw"
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
  # (the next file overwrites them)
  return 0
}

# forge_sg_report <json> [json]: ast-grep --json=stream matches as
# "forge:<rule> path:line:column <description>" (from 1); unless $mode is --all, only
# matches that overlap a line of $FORGE_TMP/added (a match spanning an edited line counts).
# With "json", those matches as they are (for forge_types_filter).
forge_sg_report() {
  [ -s "$1" ] || return 0
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
    if [ "$g_mode" = no-increase ] || [ "$g_mode" = no-decrease ]; then
      while IFS= read -r _gc_f; do
        [ -f "$1/$_gc_f" ] && forge_grep_scope_"$_gc_i" "$_gc_f" || continue
        printf '%s\t%s\t%s\n' "$g_id" "$_gc_f" "$(grep -c "$g_flags" -- "$g_pattern" "$1/$_gc_f")"
      done < "$2"
    fi
    _gc_i=$((_gc_i + 1))
  done
}

# forge_grep_forbid <file-list> [advise]: "forge:<rule> path:line <description>" for each
# match of a forbid grep rule (and, with advise, of an advise one: after an edit only);
# unless $mode is --all, only on lines of $FORGE_TMP/added.
forge_grep_forbid() {
  # first, one grep per file and flag set (-E, -iE) with every pattern in scope: most
  # files match none, and are done with in one or two processes
  _gf_any=''
  while IFS= read -r _gf_f; do
    [ -f "$_gf_f" ] || continue
    _gf_i=1
    _gf_e='' _gf_ie=''
    while [ "$_gf_i" -le "$FORGE_NGREPS" ]; do
      forge_grep_meta_"$_gf_i"
      if { [ "$g_mode" = forbid ] || { [ "$g_mode" = advise ] && [ "${2:-}" = advise ]; }; } && forge_grep_scope_"$_gf_i" "$_gf_f"; then
        if [ "$g_flags" = -iE ]; then _gf_ie="$_gf_ie
$g_pattern"; else _gf_e="$_gf_e
$g_pattern"; fi
      fi
      _gf_i=$((_gf_i + 1))
    done
    if { [ -n "$_gf_e" ] && grep -qE -e "${_gf_e#?}" -- "$_gf_f" 2>/dev/null; } \
      || { [ -n "$_gf_ie" ] && grep -qiE -e "${_gf_ie#?}" -- "$_gf_f" 2>/dev/null; }; then _gf_any="$_gf_any
$_gf_f"; fi
  done < "$1"
  [ -n "$_gf_any" ] || return 0
  printf '%s\n' "${_gf_any#?}" > "$FORGE_TMP/gf.files"
  set -- "$FORGE_TMP/gf.files" "${2:-}"
  # the added lines, one "<path><TAB><line>" per line, looked up with case
  # (at most 500 lines: past that, awk filters, a shell string of them growing slowly)
  _gf_ad='
'
  _gf_big=0
  if [ -f "$FORGE_TMP/added" ]; then
    _gf_c=0
    while IFS= read -r _gf_l; do
      _gf_c=$((_gf_c + 1))
      if [ "$_gf_c" -gt 500 ]; then _gf_big=1; break; fi
      _gf_ad="$_gf_ad$_gf_l
"
    done < "$FORGE_TMP/added"
  fi
  _gf_i=1
  while [ "$_gf_i" -le "$FORGE_NGREPS" ]; do
    forge_grep_meta_"$_gf_i"
    if [ "$g_mode" = forbid ] || { [ "$g_mode" = advise ] && [ "${2:-}" = advise ]; }; then
      while IFS= read -r _gf_f; do
        [ -f "$_gf_f" ] && forge_grep_scope_"$_gf_i" "$_gf_f" || continue
        grep -n "$g_flags" -- "$g_pattern" "$_gf_f" 2>/dev/null > "$FORGE_TMP/gf.out"
        if [ "$_gf_big" = 1 ]; then
          awk -F: -v f="$_gf_f" -v id="$g_id" -v d="$g_desc" -v all="$([ "$mode" = --all ] && printf 1)" '
            FILENAME == ARGV[1] { split($0, a, "\t"); if (a[2] == "*") whole[a[1]] = 1; else ad[a[1] "\t" a[2]] = 1; next }
            all != "" || (f in whole) || ((f "\t" $1) in ad) { printf "forge:%s %s:%s %s\n", id, f, $1, d }' "$FORGE_TMP/added" "$FORGE_TMP/gf.out"
          continue
        fi
        while IFS=: read -r _gf_n _; do
          # on an added line (or the file is new), unless every line counts (--all)
          case "$_gf_ad" in *"
$_gf_f	$_gf_n
"*|*"
$_gf_f	*
"*) ;; *) [ "$mode" = --all ] || continue ;; esac
          printf 'forge:%s %s:%s %s\n' "$g_id" "$_gf_f" "$_gf_n" "$g_desc"
        done < "$FORGE_TMP/gf.out"
      done < "$1"
    fi
    _gf_i=$((_gf_i + 1))
  done
}

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

# ---- package manager policies (typescript/npm, typescript/pnpm, typescript/yarn): the
# config checks are modes of .forge/helpers/pm_policy.mjs (forge_helper bun pm_policy.mjs <mode>).

# forge_accept_hint: after a ratchet-style forge_helper run that found something (run as
# "forge_helper … || forge_accept_hint"), says how the user accepts a change; fails.
forge_accept_hint() {
  printf 'Restore it, or have the user accept the change in forge.yaml: accept: [{rule: %s, change: "<line above>", reason: "..."}]\n' "$c_id"
  return 1
}

# forge_scope_files: $FORGE_FILES, else (--all, or a when_changed file changed and none of the
# check's own) every tracked file that exists in the check's scope (forge_scope_<n> of the
# check running, FORGE_CHECK_N), Forge's own files left out; one per line.
forge_scope_files() {
  if [ -n "$FORGE_FILES" ]; then printf '%s\n' "$FORGE_FILES"; return 0; fi
  git ls-files 2>/dev/null | grep -vE "$FORGE_OWN_ERE" | while IFS= read -r _sf_f; do
    [ -f "$_sf_f" ] || continue
    [ -z "${FORGE_CHECK_N:-}" ] || "forge_scope_$FORGE_CHECK_N" "$_sf_f" || continue
    printf '%s\n' "$_sf_f"
  done
}

# forge_unaccepted: the lines of stdin that forge.yaml does not accept (an accept entry
# holds the line exactly); when some remain, prints them and how to accept one, and fails.
forge_unaccepted() {
  while IFS= read -r _ua_l; do
    [ -n "$_ua_l" ] || continue
    [ -f forge.yaml ] && grep -qF -- "$_ua_l" forge.yaml && continue
    printf '%s\n' "$_ua_l"
  done > "$FORGE_TMP/unaccepted"
  [ -s "$FORGE_TMP/unaccepted" ] || return 0
  cat "$FORGE_TMP/unaccepted"
  printf 'Restore it, or have the user accept the change in forge.yaml: accept: [{rule: %s, change: "<line above>", reason: "..."}]\n' "$c_id"
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

# forge_role_files <@role>: the repository's files (tracked, and untracked but not ignored)
# of a file role (§15.4), by forge_role (rendered with the role's effective patterns).
forge_role_files() {
  git ls-files -co --exclude-standard 2>/dev/null | while IFS= read -r _rf_f; do
    forge_role "$1" "$_rf_f" && printf '%s\n' "$_rf_f"
  done
  return 0
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
    rm -f "$FORGE_TMP/exec.sleep"
    ( sleep "$FORGE_EXEC_LIMIT" & printf '%s\n' "$!" > "$FORGE_TMP/exec.sleep"; wait "$!"; : > "$FORGE_TMP/timedout"; printf 'forge: timed out after %ss\n' "$FORGE_EXEC_LIMIT" >> "$FORGE_TMP/out"; forge_kill_tree "$_fe_pid" ) >/dev/null 2>&1 &
    _fe_watch=$!
    wait "$_fe_pid" 2>/dev/null
    _fe_rc=$?
    forge_kill_watch "$_fe_watch" "$FORGE_TMP/exec.sleep"
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
# linters, "--> path:l:c", ESLint's stylish "<path>" then "  l:c  error …" lines,
# printed as "path:l:c  error …"; "Would reformat: path", Biome's "path format" and
# Prettier's "[warn] path" and "[error] path: …" name a file only; absolute paths under the repository are made relative; warning-only
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
      # ESLint (stylish): a file named by its absolute path alone on a line, then its
      # "  <l>:<c>  error|warning  <message>  <rule>" lines
      if (line ~ /^\/[^[:space:]]*$/) { hdr = line; next }
      sty = hdr != "" && line ~ /^[[:space:]]+[0-9]+:[0-9]+[[:space:]]+(error|warning)[[:space:]]/
      if (!sty && line ~ /^[^[:space:]]/) hdr = ""
      if (warn != 1 && line ~ /[Ww]arning/ && line !~ /[Ee]rror/) {
        # a warning diagnostic: a location, then "warning" as a word (not DeprecationWarning)
        if (line ~ /^[[:space:]]*(-->[[:space:]]*)?[^[:space:]:(]+([(][0-9]+,[0-9]+[)]|:[0-9]+)/ && line ~ /(^|[^A-Za-z])[Ww]arning([^A-Za-z]|$)/) warned = 1
        next
      }
      if ((line ~ /[Ee]rror/ || warn == 1 && line ~ /[Ww]arning/) && line !~ /(^|[^0-9])0 (errors?|warnings?)([^A-Za-z]|$)/) erred = 1
      sub(/^[[:space:]]*(-->[[:space:]]*)?/, "", line)
      ln = ""
      if (sty) { p = hdr; ln = line + 0 }
      else if (match(line, /^[^[:space:]:(]+([(][0-9]+,[0-9]+[)]|:[0-9]+(:[0-9]+)?)/)) {
        p = substr(line, 1, RLENGTH)
        loc = p; sub(/^[^[:space:]:(]+[(:]/, "", loc); ln = loc + 0
        sub(/([(][0-9]+,[0-9]+[)]|:[0-9]+(:[0-9]+)?)$/, "", p)
      } else if (match(line, /^Would reformat: [^[:space:]]+/)) p = substr(line, 18, RLENGTH - 17)
      else if (match(line, /^[^[:space:]:(]+ format /)) { p = line; sub(/ format .*$/, "", p) }
      # prettier --check: "[warn] <path>" per unformatted file, "[error] <path>: <error>"
      else if (line ~ /^\[warn\] [^[:space:]]+$/) { p = substr(line, 8); erred = 1 }
      else if (match(line, /^\[error\] [^[:space:]]+: /)) p = substr(line, 9, RLENGTH - 10)
      else next
      if (index(p, r1 "/") == 1) p = substr(p, length(r1) + 2)
      else if (index(p, r2 "/") == 1) p = substr(p, length(r2) + 2)
      sub(/^\.\//, "", p)
      named = 1
      if (p !~ /^\.forge\// && p !~ /^\.claude\/skills\/forge-project\//) project = 1
      if (level == "line") counts = (p in whole) || (ln == "" || ln == 0 ? (p in ch) : ((p "\t" ln) in ad))
      else counts = (p in ch)
      if (counts) { hit = 1; if (mode == "print") print (sty ? p ":" line : $0) }
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

# forge_typecheck_compare <fn>: a failed typecheck run (<fn>, output in $FORGE_TMP/out)
# with errors in changed files, compared with the base (§10.1 step 4): the same run in the
# base tree (forge_base_tree). Diagnostics are compared by their text, the location left
# out ("path:12:5: error[x] msg" is "error[x] msg"): of an error in a changed file, only as
# many count as the run now reports more of that text than the base did, in the whole
# project (code moved between files brings its errors along), those on added lines first.
# The others were there before the change. Prints "forge:<id> failed" and the errors that
# count, or "forge:<id> ok for the changed files (…)" when none does. When the base cannot
# be checked (no base tree, the run there stopped, or it failed naming no file), every
# error in the changed files counts, with why.
forge_typecheck_compare() {
  forge_diagnostics "$FORGE_TMP/out" file print > "$FORGE_TMP/tc.now"
  mv "$FORGE_TMP/out" "$FORGE_TMP/tc.out"
  : > "$FORGE_TMP/tc.base"
  _tc_why=''
  rm -rf "$FORGE_TMP/base-tree"
  if ! forge_base_tree "$FORGE_TMP/base-tree"; then
    _tc_why=$(cat "$FORGE_TMP/base-tree.why" 2>/dev/null)
    [ -n "$_tc_why" ] || _tc_why='the copy of the tree could not be made'
  else
    _tc_fn=$1
    forge_exec_check forge_typecheck_in_base "$c_timeout"
    case $? in
      0) ;;
      124)
        _tc_why="the run there was stopped after ${FORGE_EXEC_LIMIT}s"
        [ "$FORGE_EXEC_WHY" != budget ] || _tc_why="$_tc_why, when the Stop hook's time budget ran out" ;;
      *)
        if [ "$(cd "$FORGE_TMP/base-tree" && forge_diagnostics "$FORGE_TMP/out" file)" = none ]; then
          _tc_why='the run there failed naming no file'
        else
          forge_diag_keys < "$FORGE_TMP/out" > "$FORGE_TMP/tc.base"
        fi ;;
    esac
  fi
  rm -rf "$FORGE_TMP/base-tree"
  mv "$FORGE_TMP/tc.out" "$FORGE_TMP/out"
  if [ -n "$_tc_why" ]; then
    printf 'forge:%s failed (its errors in the changed files could not be compared with the base: %s; all of them count)\n' "$c_id" "$_tc_why"
    first_lines 15 < "$FORGE_TMP/tc.now"
    return 1
  fi
  forge_diag_keys < "$FORGE_TMP/out" > "$FORGE_TMP/tc.all"
  forge_diag_keys < "$FORGE_TMP/tc.now" > "$FORGE_TMP/tc.keys"
  # tc.cmp: "new<TAB><line>" per error that counts, then "old<TAB><count>"
  awk -F '\t' '
    FILENAME == ARGV[1] { base[$3]++; next }
    FILENAME == ARGV[2] { now[$3]++; next }
    {
      n++; key[n] = $3; onadd[n] = $1; line[n] = $0; sub(/^[^\t]*\t[^\t]*\t[^\t]*\t/, "", line[n])
    }
    END {
      for (k in now) left[k] = now[k] - base[k]
      # the errors on added lines are the new ones first, then the others in order
      for (pass = 1; pass >= 0; pass--)
        for (i = 1; i <= n; i++) if (onadd[i] == pass && left[key[i]] > 0) { left[key[i]]--; keep[i] = 1 }
      for (i = 1; i <= n; i++) if (keep[i]) print "new\t" line[i]; else old++
      print "old\t" old + 0
    }' "$FORGE_TMP/tc.base" "$FORGE_TMP/tc.all" "$FORGE_TMP/tc.keys" > "$FORGE_TMP/tc.cmp"
  _tc_old=$(awk -F '\t' '$1 == "old" { print $2 }' "$FORGE_TMP/tc.cmp")
  if ! grep -q '^new	' "$FORGE_TMP/tc.cmp"; then
    printf 'forge:%s ok for the changed files (their %s error(s) were already there before this change: sh .forge/check.sh --all)\n' "$c_id" "$_tc_old"
    return 0
  fi
  printf 'forge:%s failed\n' "$c_id"
  sed -n 's/^new	//p' "$FORGE_TMP/tc.cmp" | first_lines 15
  [ "$_tc_old" = 0 ] || printf '(%s other error(s) in the changed files were already there before this change)\n' "$_tc_old"
  return 1
}

# forge_typecheck_in_base: check function $_tc_fn in the base tree (forge_typecheck_compare).
# FORGE_BASE_OF (the working tree) and FORGE_BASE_AT (the base tree) let a tool that looks
# up the project's environment there (forge_ty_args) use the working tree's: the base
# tree is a temporary copy of the same project.
forge_typecheck_in_base() {
  FORGE_BASE_OF=$(pwd -P)
  cd "$FORGE_TMP/base-tree" || return 1
  FORGE_BASE_AT=$(pwd -P)
  "$_tc_fn"
}

# forge_diag_keys: for each diagnostic line of stdin (a location first: "path:12:5: …",
# "path(12,5): …", "--> path:12:5"), "<1 when on an added line, else 0><TAB><path><TAB>
# <its text after the location><TAB><the line>" ("path:12:5: error[x] msg" gives the text
# ": error[x] msg"); other lines are left out.
forge_diag_keys() {
  awk -v r1="$(pwd)" -v r2="$(pwd -P)" '
    BEGIN { ansi = sprintf("%c", 27) "\\[[0-9;]*[A-Za-z]" }
    FILENAME == ARGV[1] { if ($2 == "*") whole[$1] = 1; else ad[$1 "\t" $2] = 1; next }
    {
      line = $0
      gsub(ansi, "", line)
      sub(/^[[:space:]]*(-->[[:space:]]*)?/, "", line)
      if (!match(line, /^[^[:space:]:(]+([(][0-9]+,[0-9]+[)]|:[0-9]+(:[0-9]+)?)/)) next
      p = substr(line, 1, RLENGTH); text = substr(line, RLENGTH + 1)
      loc = p; sub(/^[^[:space:]:(]+[(:]/, "", loc); ln = loc + 0
      sub(/([(][0-9]+,[0-9]+[)]|:[0-9]+(:[0-9]+)?)$/, "", p)
      if (index(p, r1 "/") == 1) p = substr(p, length(r1) + 2)
      else if (index(p, r2 "/") == 1) p = substr(p, length(r2) + 2)
      sub(/^\.\//, "", p)
      gsub(/\t/, " ", text)
      print (((p in whole) || ((p "\t" ln) in ad)) ? 1 : 0) "\t" p "\t" text "\t" $0
    }' "$FORGE_TMP/added" -
}

# forge_for_agent: the output goes to Claude: a hook runs check.sh (FORGE_FOR_AGENT=1, Stop),
# or Claude Code's Bash tool does (CLAUDECODE=1: a commit's pre-commit, a check Claude runs).
# What a missing tool or an installed tool's version leaves unenforced is then left out: it is
# the person's to fix (forge apply, forge status), not Claude's (§8.1, §10).
forge_for_agent() { [ "${FORGE_FOR_AGENT:-}" = 1 ] || [ "${CLAUDECODE:-}" = 1 ]; }

# forge_tool_note <line>: prints a note about a missing or limited tool, unless for Claude.
forge_tool_note() { forge_for_agent || printf '%s\n' "$1"; }

# forge_run_check <n>: one check under check.sh (forge_run_check_one), then the notes its
# tool adapters left whatever its outcome (§16.3: what the installed version cannot do),
# unless the output goes to Claude (forge_for_agent).
forge_run_check() {
  rm -f "$FORGE_TMP/notes"
  forge_run_check_one "$1"
  _rn_rc=$?
  if [ -s "$FORGE_TMP/notes" ] && ! forge_for_agent; then awk '!seen[$0]++' "$FORGE_TMP/notes"; fi
  rm -f "$FORGE_TMP/notes"
  return "$_rn_rc"
}

# forge_run_check_one <n>: one check under check.sh ($mode and forge_change_set's files).
# --changed and --turn run the {files} form on the changed files when there is one;
# --since and --all run the whole-project form (read-only, as CI should).
forge_run_check_one() {
  forge_check_meta_"$1"
  FORGE_CHECK_N=$1
  FORGE_FILES=''
  if [ "$mode" != --all ]; then
    if [ "$c_when" = 1 ]; then
      forge_any "forge_when_$1" < "$FORGE_TMP/changed" || return 0
    fi
    forge_select "forge_scope_$1" < "$FORGE_TMP/existing"
    [ "$c_when" = 1 ] || [ -n "$FORGE_FILES" ] || return 0
  fi
  if _fc_missing=$(forge_missing_tool); then
    forge_tool_note "forge:$c_id skipped: $_fc_missing not installed"
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
  # findings only in Forge's own files (.forge/, .claude/skills/forge-project/, checked by
  # integrity) are not the project's, in every mode
  if [ -n "$_fc_level" ] && [ "$(forge_diagnostics "$FORGE_TMP/out" "$_fc_level")" = forge ]; then
    printf 'forge:%s ok (its findings are only in Forge'"'"'s own files)\n' "$c_id"
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
        # a typecheck's errors in the changed files that the base already has are not this change's
        if [ "$c_stage" = typecheck ]; then
          forge_typecheck_compare "$_fc_fn"
          return
        fi
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
          # (a helper's own skipped line stays a note: it is not advice)
          if [ -f "$(forge_advised_mark)" ]; then
            grep -F "forge:$c_id skipped: " "$FORGE_TMP/check.out"
            grep -vF "forge:$c_id skipped: " "$FORGE_TMP/check.out" >> "$FORGE_TMP/advice.out"; advised=1
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
# are skipped. FORGE_FILE_RAN=1 when one ran. Outputs of failed runs go to $FORGE_TMP/stage-<stage>.<n>, their
# numbers to $FORGE_TMP/stage-<stage>.failed, for forge_file_report.
forge_file_run() {
  _fp_i=1
  FORGE_FILE_RAN=0
  : > "$FORGE_TMP/stage-$1.failed"
  # lint checks whose fix run already linted these files (forge_file_fixes, fix_lints)
  [ "$1" != lint ] || [ -z "${FORGE_FIX_LINTED:-}" ] || forge_cat "$FORGE_TMP/fix.failed" > "$FORGE_TMP/stage-$1.failed"
  while [ "$_fp_i" -le "$FORGE_NCHECKS" ]; do
    case " ${FORGE_FIX_LINTED:-} " in *" $_fp_i "*) [ "$1" = lint ] && { _fp_i=$((_fp_i + 1)); continue; } ;; esac
    forge_check_meta_"$_fp_i"
    if [ "$c_stage" = "$1" ] && [ "$c_files" = 1 ] && ! forge_missing_tool >/dev/null; then
      forge_select "forge_scope_$_fp_i" < "$2"
      [ -z "$FORGE_FILES" ] || FORGE_FILE_RAN=1
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
# files of <list> in its scope. A check with fix_lints has its lint result from that run
# (FORGE_FIX_LINTED, $FORGE_TMP/fix.failed), and forge_file_run lint does not run it again. For each file they changed, prints "forge: auto-fixed
# <codes> in <file>" (codes from ruff's --show-fixes summary, per file), so Claude knows
# its view of the file is stale; the files go to $FORGE_TMP/noted.
forge_file_fixes() {
  _fx_i=1
  FORGE_FIX_LINTED=''
  : > "$FORGE_TMP/fix.codes"
  : > "$FORGE_TMP/fix.failed"
  # what each file was before: copies (FORGE_FIX_BEFORE, compared with cmp), else blob hashes
  [ -n "${FORGE_FIX_BEFORE:-}" ] || forge_file_hashes "$1" > "$FORGE_TMP/fix.before"
  while [ "$_fx_i" -le "$FORGE_NCHECKS" ]; do
    forge_check_meta_"$_fx_i"
    if [ "$c_stage" = lint ] && [ "$c_fix" = 1 ] && ! forge_missing_tool >/dev/null; then
      forge_select "forge_scope_$_fx_i" < "$1"
      if [ -n "$FORGE_FILES" ]; then
        if forge_exec_check "forge_check_fix_$_fx_i" "$c_timeout"; then _fx_rc=0; else _fx_rc=1; fi
        # a fix run that also lints (fix_lints: ruff check --fix reports what remains) is
        # the check's lint run on these files: forge_file_run lint skips it
        if [ "$c_fixlint" = 1 ]; then
          FORGE_FIX_LINTED="$FORGE_FIX_LINTED $_fx_i "
          [ "$_fx_rc" = 0 ] || { cp "$FORGE_TMP/out" "$FORGE_TMP/stage-lint.$_fx_i"; printf '%s\n' "$_fx_i" >> "$FORGE_TMP/fix.failed"; }
        fi
        # ruff: "Fixed N errors:", then "- <path>:" and "    1 × I001 (unsorted-imports)" lines
        forge_has_line "$FORGE_TMP/out" 'Fixed [0-9]*' && awk '/^Fixed [0-9]+ error/ { on = 1; next }
          on && /^- .*:$/ { f = substr($0, 3, length($0) - 3); next }
          on && /^ +[0-9]+ / && match($0, /[A-Z]+[0-9]+ \(/) { print f "\t" substr($0, RSTART, RLENGTH - 2) }
          /^$/ { on = 0 }' "$FORGE_TMP/out" >> "$FORGE_TMP/fix.codes"
      fi
    fi
    _fx_i=$((_fx_i + 1))
  done
  [ ! -s "$FORGE_TMP/fix.codes" ] && [ -n "${FORGE_FIX_BEFORE:-}" ] && ! forge_fixed_any "$1" && return 0
  [ -z "${FORGE_FIX_BEFORE:-}" ] || while IFS= read -r _fx_f; do printf '%s\t\n' "$_fx_f"; done < "$1" > "$FORGE_TMP/fix.before"
  while IFS='	' read -r _fx_f _fx_h; do
    if [ -n "${FORGE_FIX_BEFORE:-}" ]; then
      ! cmp -s "$FORGE_FIX_BEFORE/$_fx_f" "$_fx_f" || continue
    else
      [ "$(git hash-object -- "$_fx_f" 2>/dev/null)" != "$_fx_h" ] || continue
    fi
    _fx_codes=$(awk -F '\t' -v f="$_fx_f" '($1 == f || $1 == "") && !seen[$2]++ { printf "%s%s", (n++ ? ", " : ""), $2 }' "$FORGE_TMP/fix.codes")
    printf 'forge: auto-fixed %s in %s; re-read it before editing it again\n' "${_fx_codes:-safe lint fixes}" "$_fx_f"
    printf '%s\n' "$_fx_f" >> "$FORGE_TMP/noted"
  done < "$FORGE_TMP/fix.before"
  return 0
}

# forge_has_line <file> <pattern>: a line of <file> matches the shell pattern <pattern>* (builtins).
forge_has_line() {
  while IFS= read -r _hl_l || [ -n "$_hl_l" ]; do
    # shellcheck disable=SC2254
    case $_hl_l in $2*) return 0 ;; esac
    _hl_l=''
  done < "$1"
  return 1
}

# forge_fixed_any <list>: a file of <list> differs from its copy in FORGE_FIX_BEFORE.
forge_fixed_any() {
  while IFS= read -r _fa_f; do ! cmp -s "$FORGE_FIX_BEFORE/$_fa_f" "$_fa_f" && return 0; done < "$1"
  return 1
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
  [ ! -e "$FORGE_TMP/lf.fmt" ] || rm -f "$FORGE_TMP/lf.fmt"
  return 0
}

# forge_typecheck <file>: the full run of each typecheck check whose scope covers <file>;
# only errors in files of $FORGE_TMP/changed count ($FORGE_TMP/added must exist).
forge_typecheck() {
  _ft_i=1
  _ft_rc=0
  _ft_cs=''
  while [ "$_ft_i" -le "$FORGE_NCHECKS" ]; do
    forge_check_meta_"$_ft_i"
    if [ "$c_stage" = typecheck ] && forge_scope_"$_ft_i" "$1" && ! forge_missing_tool >/dev/null; then
      # ty: the types engine answers for the edited file (§10.5); ty check when it cannot
      _ft_note=''
      if [ "$c_tools" = ty ]; then
        forge_typecheck_engine "$1"
        case $? in
          0) _ft_i=$((_ft_i + 1)); continue ;;
          1) _ft_rc=1; _ft_i=$((_ft_i + 1)); continue ;;
        esac
        _ft_note="checked by ty check: $(cat "$FORGE_TMP/te.why" 2>/dev/null)"
      fi
      # the changed set (the turn's, as the Stop check computes it), once
      if [ -z "$_ft_cs" ]; then forge_change_set; _ft_cs=1; fi
      if ! forge_exec_check "forge_check_run_$_ft_i" "$c_timeout"; then
        # only errors in changed files wake Claude, and only those the change added (§10.1 step 4)
        case $(forge_diagnostics "$FORGE_TMP/out" file) in
          other|warnings) ;;
          changed)
            forge_typecheck_compare "forge_check_run_$_ft_i" > "$FORGE_TMP/tc.report" || {
              cat "$FORGE_TMP/tc.report"
              [ -z "$_ft_note" ] || printf '(%s)\n' "$_ft_note"
              _ft_rc=1
            } ;;
          *)
            printf 'forge:%s\n' "$c_id"
            first_lines 19 < "$FORGE_TMP/out"
            [ -z "$_ft_note" ] || printf '(%s)\n' "$_ft_note"
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
  _fq_seen='
'
  _fq_ifs=$IFS
  IFS=' '
  while [ "$_fq_i" -le "$FORGE_NCHECKS" ]; do
    forge_check_meta_"$_fq_i"
    for _fq_t in $c_tools; do
      for _fq_d in ${c_dirs:-.}; do
        [ -n "$c_dirs" ] && _fq_n=$_fq_t@$_fq_d || _fq_n=$_fq_t
        case $_fq_seen in *"
$_fq_n
"*) ;; *) _fq_seen="$_fq_seen$_fq_n
"; printf '%s\n' "$_fq_n" ;; esac
      done
    done
    _fq_i=$((_fq_i + 1))
  done
  IFS=$_fq_ifs
}
