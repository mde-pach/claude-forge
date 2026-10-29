# @module types
# The types engine's client (§15.2, §15.3): starting the daemons and filtering ast-grep
# matches through them.

# ---- engine capabilities (SPECS §15): types

# The languages whose types engine the active rules use, and the engines' pins.
FORGE_TYPES_LANGS='@@TYPES_LANGS@@'

FORGE_TY_VERSION='@@TY_VERSION@@'

FORGE_ENGINE_PYTHON='@@ENGINE_PYTHON@@'

# the adapters' revision: a running daemon of another Forge version is restarted
FORGE_TYPES_REV='@@TYPES_REV@@'

# How ty sees installed packages that ship no typing (no py.typed, no .pyi, no
# <name>-stubs; PEP 561): "any" (forge.yaml python: {untyped-packages: any}, the
# default) checks their imports as Any, as mypy does; "source" lets ty infer them from
# their source (forge_ty_untyped).
FORGE_TY_UNTYPED='@@TY_UNTYPED@@'

# The Python packages the active rules' where conditions ask about (§15.1): the types engine
# must see their declarations, so it never gets them as Any, typed or not (§15.2).
FORGE_TYPES_PACKAGES='@@TYPES_PACKAGES@@'

# forge_ty <args>: Forge's pinned ty (§15.2), on the engine's Python, with the project's
# environment and configuration ([tool.ty], ty.toml). `ty check` gets forge_ty_args: the
# project's environment when ty would not find it, and the untyped packages as Any.
forge_ty() {
  if [ "${1:-}" = check ]; then
    shift
    forge_ty_args
    _ty_ifs=$IFS
    IFS='
'
    # shellcheck disable=SC2086
    set -- check $FORGE_TY_ARGV "$@"
    IFS=$_ty_ifs
  fi
  uvx -q --python "$FORGE_ENGINE_PYTHON" --from "ty==$FORGE_TY_VERSION" ty "$@"
}

# forge_ty_args: FORGE_TY_ARGV, the arguments `ty check` needs here, one per line: --python
# <sys.prefix> when ty would not find the project's environment itself (forge_ty_env), and
# -c analysis.replace-imports-with-any = […] with the untyped packages (forge_ty_untyped).
# With the facts unchanged, shell builtins only. forge_ty_args_print prints them.
forge_ty_args() {
  FORGE_TY_ARGV=''
  # in the base tree (forge_typecheck_in_base): the arguments of the same directory of the working tree
  if [ -n "${FORGE_BASE_AT:-}" ]; then
    _ta_here=$(pwd -P)
    case $_ta_here in
      "$FORGE_BASE_AT"|"$FORGE_BASE_AT"/*) FORGE_TY_ARGV=$(cd "$FORGE_BASE_OF${_ta_here#"$FORGE_BASE_AT"}" && FORGE_BASE_AT='' forge_ty_args_print); return 0 ;;
    esac
  fi
  forge_ty_env || return 0
  [ -z "$FORGE_TY_PASS" ] || FORGE_TY_ARGV="--python
$FORGE_TY_PASS"
  forge_ty_untyped
  _ta_any=''
  _ta_ifs=$IFS
  IFS='
'
  for _ta_m in $FORGE_TY_UNTYPED_LIST; do _ta_any="${_ta_any:+$_ta_any, }\"$_ta_m\", \"$_ta_m.**\""; done
  IFS=$_ta_ifs
  # ty adds it to a list the project's config already sets: the project's entries stay
  [ -z "$_ta_any" ] || FORGE_TY_ARGV="${FORGE_TY_ARGV:+$FORGE_TY_ARGV
}-c
analysis.replace-imports-with-any = [$_ta_any]"
  return 0
}
forge_ty_args_print() {
  forge_ty_args
  [ -z "$FORGE_TY_ARGV" ] || printf '%s\n' "$FORGE_TY_ARGV"
}

# forge_types_untyped_filter: the untyped modules of stdin the types engine gets as Any: not
# those of a package the active rules ask about (FORGE_TYPES_PACKAGES).
forge_types_untyped_filter() {
  _uf_l=$(forge_cat /dev/stdin)
  forge_types_untyped_filter_v "$_uf_l"
  [ -z "$FORGE_TY_ANY" ] || printf '%s\n' "$FORGE_TY_ANY"
}
# forge_types_untyped_filter_v <modules>: FORGE_TY_ANY, forge_types_untyped_filter's answer (builtins).
forge_types_untyped_filter_v() {
  FORGE_TY_ANY=''
  _uf_ifs=$IFS
  IFS='
'
  set -f
  for _uf_m in $1; do
    _uf_top=${_uf_m%%.*}
    _uf_keep=0
    for _uf_k in $FORGE_TYPES_PACKAGES; do
      IFS=' '
      for _uf_w in $_uf_k; do [ "$_uf_w" != "$_uf_top" ] || _uf_keep=1; done
      IFS='
'
    done
    [ "$_uf_keep" = 1 ] || FORGE_TY_ANY="$FORGE_TY_ANY${FORGE_TY_ANY:+
}$_uf_m"
  done
  set +f
  IFS=$_uf_ifs
}

# forge_ty_env: the Python environment ty checks the project with, as ty finds it
# (environment.python in its config, $VIRTUAL_ENV, .venv), else the project's
# environment forge_py_env finds (uv's, Poetry's, the interpreter on PATH), which ty
# must be given: under uvx, the python it would find on PATH is uvx's own. Sets
# FORGE_TY_PY (its interpreter) and FORGE_TY_PASS (the sys.prefix to give ty, or "").
# Fails when there is none. Its config's environment.python is a fact (forge_fact).
forge_ty_env() {
  FORGE_TY_PY=''
  FORGE_TY_PASS=''
  _te_cfg=''
  if [ -f ty.toml ] || [ -f pyproject.toml ]; then
    forge_fact ty-environment "$PWD" forge_ty_config_python && _te_cfg=$FORGE_FACT
  fi
  if [ -n "$_te_cfg" ]; then _te_d=$_te_cfg
  elif [ -n "${VIRTUAL_ENV:-}" ]; then _te_d=$VIRTUAL_ENV
  elif [ -d .venv ]; then _te_d=.venv
  else
    forge_py_env || return 1
    FORGE_TY_PY=$FORGE_PY
    FORGE_TY_PASS=$FORGE_PY_PREFIX
    return 0
  fi
  if [ -f "$_te_d" ] && [ -x "$_te_d" ]; then FORGE_TY_PY=$_te_d; return 0; fi
  for _te_n in python python3; do
    if [ -x "$_te_d/bin/$_te_n" ]; then FORGE_TY_PY=$_te_d/bin/$_te_n; return 0; fi
  done
  return 1
}

# forge_ty_config_python: environment.python of ty's config (ty.toml, else [tool.ty]); watched: both files.
forge_ty_config_python() {
  printf 'watch %s\nwatch %s\n' "$PWD/ty.toml" "$PWD/pyproject.toml"
  _tp=$(forge_toml_get ty.toml environment python)
  [ -n "$_tp" ] || _tp=$(forge_toml_get pyproject.toml tool.ty.environment python)
  [ -z "$_tp" ] || printf '%s\n' "$_tp"
}

# forge_ty_untyped: FORGE_TY_UNTYPED_LIST, the top-level modules (or namespace package
# portions) installed in the environment ty checks with (forge_ty_env) that ship no typing,
# one per line, which ty is told to check as Any (FORGE_TY_UNTYPED any). Left out: modules ty
# resolves without that environment (its bundled standard library stubs, the project's own
# code). Computed by .forge/helpers/untyped_python.py (FORGE_TY_UNTYPED_HELPER) and kept as
# a fact until the environment's site-packages, ty's search roots, the project's ty
# configuration, the helper or Forge's pins change: a package upgraded to one that ships
# typing leaves the list by itself. Empty when switched off or without an environment.
# forge_ty_untyped_print prints it.
forge_ty_untyped() {
  FORGE_TY_UNTYPED_LIST=''
  [ "$FORGE_TY_UNTYPED" = any ] || return 0
  forge_ty_env || return 0
  _tn_py=$FORGE_TY_PY
  forge_py_sites "$_tn_py" || return 0
  _tn_sites=$FORGE_FACT
  _tn_h=${FORGE_TY_UNTYPED_HELPER:-.forge/helpers/untyped_python.py}
  case $_tn_h in /*) ;; *) _tn_h=$PWD/$_tn_h ;; esac
  [ -f "$_tn_h" ] || return 0
  forge_fact ty-untyped "$PWD $FORGE_TY_VERSION $FORGE_ENGINE_PYTHON $_tn_h $_tn_py $_tn_sites" forge_ty_untyped_compute "$_tn_h" "$_tn_sites" || return 0
  FORGE_TY_UNTYPED_LIST=$FORGE_FACT
}
forge_ty_untyped_print() {
  forge_ty_untyped
  [ -z "$FORGE_TY_UNTYPED_LIST" ] || printf '%s\n' "$FORGE_TY_UNTYPED_LIST"
}

# forge_ty_untyped_compute <helper> <sites>: runs the helper on the pinned ty's Python; the
# "watch" lines (the helper, the ty config, and the directories the helper names) are what
# the answer depends on (forge_fact).
forge_ty_untyped_compute() {
  printf 'watch %s\nwatch %s\nwatch %s\n' "$1" "$PWD/ty.toml" "$PWD/pyproject.toml"
  _uc_v=$(printf '%s\n' "$2" | sed -n 's/^version //p')
  _uc_ifs=$IFS
  IFS='
'
  # shellcheck disable=SC2046
  set -- "$1" "$(pwd -P)" "$_uc_v" $(printf '%s\n' "$2" | sed -n 's/^site //p')
  IFS=$_uc_ifs
  uvx -q --python "$FORGE_ENGINE_PYTHON" --from "ty==$FORGE_TY_VERSION" python "$@" 2>/dev/null
}

# forge_typecheck_engine <file>: the edit-time typecheck of <file> (a Python file of a
# check run by ty) answered by the running types engine (§10.5): the diagnostics of the
# file now and of its base copy (the turn's start, else HEAD: base_file) from ty server,
# compared by their text without the location (code moved within the file keeps its
# errors); an error counts only as many times as the file now has more of it than its
# base. Prints "forge:<id>" and the errors that count (warnings too with c_warn); 0 none,
# 1 some, 2 the engine could not answer (why in $FORGE_TMP/te.why: ty check runs instead).
forge_typecheck_engine() {
  if [ "${FORGE_EDIT_TYPECHECK_ENGINE:-auto}" = never ]; then printf 'FORGE_EDIT_TYPECHECK_ENGINE=never' > "$FORGE_TMP/te.why"; return 2; fi
  _te_has=0
  for _te_l in $FORGE_TYPES_LANGS; do [ "$_te_l" = python ] && _te_has=1; done
  if [ "$_te_has" = 0 ]; then printf 'no Python types engine in this setup' > "$FORGE_TMP/te.why"; return 2; fi
  forge_types_up python > "$FORGE_TMP/te.why" 2>&1 || return 2
  _te_q=$FORGE_TMP/te.q
  rm -rf "$_te_q" && mkdir -p "$_te_q" || return 2
  _te_b=$FORGE_TMP/te.base/$1
  base_file "$1" "$_te_b" || _te_b=''
  printf '%s\t%s\n' "$1" "$_te_b" > "$_te_q/req"
  if ! forge_types_ask python "$_te_q" diag > "$_te_q/answer" || grep -q '^error	' "$_te_q/answer"; then
    { printf 'the types engine did not answer'; grep '^error	' "$_te_q/answer" | head -n 1 | sed 's/^error	/: /'; } > "$FORGE_TMP/te.why"
    return 2
  fi
  awk -F '\t' -v warn="${c_warn:-0}" '
    function key(d) { sub(/^[^ ]*:[0-9]+:[0-9]+: /, "", d); return d }
    $1 != "now" && $1 != "base" { next }
    $2 !~ /^[^ ]*:[0-9]+:[0-9]+: error/ && !(warn == 1 && $2 ~ /^[^ ]*:[0-9]+:[0-9]+: warning/) { next }
    $1 == "base" { base[key($2)]++; next }
    { k = key($2); if (base[k] > 0) base[k]--; else print $2 }' "$_te_q/answer" > "$FORGE_TMP/te.new"
  [ -s "$FORGE_TMP/te.new" ] || return 0
  printf 'forge:%s\n' "$c_id"
  first_lines 19 < "$FORGE_TMP/te.new"
  return 1
}

# forge_types_up <lang>: the language's daemon runs, with the pinned engine (started
# when it does not: by the first hook that needs it, or by SessionStart). Prints why
# and fails when it cannot start. Registers the hook's session, if any.
forge_types_up() {
  _tu_lang=$1
  _tu_d=$FORGE_STATE/types/$1
  case $1 in
    # the untyped packages it checks as Any are part of what it runs with: a changed list restarts it
    python)
      # the questions' list leaves out the packages the rules ask about; the diagnostics' is the whole list (ty check's)
      forge_ty_untyped
      _tu_all=$FORGE_TY_UNTYPED_LIST
      forge_types_untyped_filter_v "$_tu_all"
      _tu_any=$FORGE_TY_ANY
      # the lists themselves (builtins: no hash to compute at each question)
      _tu_want="ty $FORGE_TY_VERSION $FORGE_TYPES_REV any:
$_tu_any
--
$_tu_all
end" ;;
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
    python)
      # the project's environment when ty would not find it, and the untyped packages (forge_ty_args)
      _tu_py=''
      _tu_prefix=''
      if forge_ty_env && [ -n "$FORGE_TY_PASS" ]; then _tu_py=$FORGE_TY_PY; _tu_prefix=$FORGE_TY_PASS; fi
      set -- env FORGE_TY_PYTHON="$_tu_py" FORGE_TY_PREFIX="$_tu_prefix" FORGE_TY_ANY="$_tu_any" FORGE_TY_ANY_ALL="$_tu_all" uvx -q --python "$FORGE_ENGINE_PYTHON" --from "ty==$FORGE_TY_VERSION" python "$(pwd -P)/.forge/helpers/types_python.py" "$(pwd -P)" "$_tu_d" ;;
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
  forge_types_alive "$1" && [ "$(forge_cat "$FORGE_STATE/types/$1/want" 2>/dev/null)" = "$2" ] || return 1
  [ -z "${FORGE_INPUT:-}" ] || { forge_session; : > "$FORGE_STATE/types/$1/sessions/$FORGE_SESSION"; }
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
  [ -s "$1" ] || return 0
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
