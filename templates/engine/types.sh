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

# forge_ty <args>: Forge's pinned ty (§15.2), on the engine's Python, with the project's
# environment and configuration ([tool.ty], ty.toml). ty finds .venv and $VIRTUAL_ENV
# itself; any other environment of the project (forge_py_env: uv's, Poetry's, the
# interpreter on PATH) is given to `ty check` as --python <its sys.prefix>: under uvx,
# the python ty would find on PATH is uvx's own.
forge_ty() {
  if [ "${1:-}" = check ] && forge_ty_python; then
    shift
    set -- check --python "$FORGE_PY_PREFIX" "$@"
  fi
  uvx -q --python "$FORGE_ENGINE_PYTHON" --from "ty==$FORGE_TY_VERSION" ty "$@"
}

# forge_ty_python: the project's Python environment must be given to ty (FORGE_PY,
# FORGE_PY_PREFIX): there
# is no .venv here and no $VIRTUAL_ENV, and forge_py_env finds one.
forge_ty_python() {
  [ ! -d .venv ] && [ -z "${VIRTUAL_ENV:-}" ] && forge_py_env
}

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
    python)
      # the project's environment when ty would not find it (forge_ty)
      _tu_py=''
      _tu_prefix=''
      if forge_ty_python; then _tu_py=$FORGE_PY; _tu_prefix=$FORGE_PY_PREFIX; fi
      set -- env FORGE_TY_PYTHON="$_tu_py" FORGE_TY_PREFIX="$_tu_prefix" uvx -q --python "$FORGE_ENGINE_PYTHON" --from "ty==$FORGE_TY_VERSION" python "$(pwd -P)/.forge/helpers/types_python.py" "$(pwd -P)" "$_tu_d" ;;
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
