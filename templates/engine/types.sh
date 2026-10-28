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
    _ty_ifs=$IFS
    IFS='
'
    # shellcheck disable=SC2046
    set -- check $(forge_ty_args) "$@"
    IFS=$_ty_ifs
  fi
  uvx -q --python "$FORGE_ENGINE_PYTHON" --from "ty==$FORGE_TY_VERSION" ty "$@"
}

# forge_ty_args: the arguments `ty check` needs here, one per line: --python <sys.prefix>
# when ty would not find the project's environment itself (forge_ty_env), and -c
# analysis.replace-imports-with-any=[…] with the untyped packages (forge_ty_untyped).
forge_ty_args() {
  # in the base tree (forge_typecheck_in_base): the arguments of the same directory of the working tree
  if [ -n "${FORGE_BASE_AT:-}" ]; then
    _ta_here=$(pwd -P)
    case $_ta_here in
      "$FORGE_BASE_AT"|"$FORGE_BASE_AT"/*) ( cd "$FORGE_BASE_OF${_ta_here#"$FORGE_BASE_AT"}" && FORGE_BASE_AT='' forge_ty_args ); return ;;
    esac
  fi
  forge_ty_env || return 0
  [ -z "$FORGE_TY_PASS" ] || printf -- '--python\n%s\n' "$FORGE_TY_PASS"
  _ta_any=$(forge_ty_untyped | forge_ty_any_value)
  [ -z "$_ta_any" ] || printf -- '-c\nanalysis.replace-imports-with-any = %s\n' "$_ta_any"
}

# forge_ty_any_value: the module names of stdin as ty's replace-imports-with-any value,
# each with its submodules: ["a", "a.**", …]. Nothing when there is none. ty adds it to
# a list the project's config already sets: the project's entries stay.
forge_ty_any_value() {
  awk 'NF { s = s (s == "" ? "" : ", ") "\"" $0 "\", \"" $0 ".**\"" } END { if (s != "") print "[" s "]" }'
}

# forge_types_untyped_filter: the untyped modules of stdin the types engine gets as Any: not
# those of a package the active rules ask about (FORGE_TYPES_PACKAGES).
forge_types_untyped_filter() {
  awk -v keep="$(printf '%s' "$FORGE_TYPES_PACKAGES" | tr '\n' ' ')" 'BEGIN { n = split(keep, k, " "); for (i = 1; i <= n; i++) asked[k[i]] = 1 }
    { top = $0; sub(/\..*/, "", top); if (!(top in asked)) print }'
}

# forge_ty_env: the Python environment ty checks the project with, as ty finds it
# (environment.python in its config, $VIRTUAL_ENV, .venv), else the project's
# environment forge_py_env finds (uv's, Poetry's, the interpreter on PATH), which ty
# must be given: under uvx, the python it would find on PATH is uvx's own. Sets
# FORGE_TY_PY (its interpreter) and FORGE_TY_PASS (the sys.prefix to give ty, or "").
# Fails when there is none.
forge_ty_env() {
  FORGE_TY_PY=''
  FORGE_TY_PASS=''
  _te_cfg=$(forge_toml_get ty.toml environment python)
  [ -n "$_te_cfg" ] || _te_cfg=$(forge_toml_get pyproject.toml tool.ty.environment python)
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

# forge_ty_untyped: the top-level modules (or namespace package portions) installed in
# the environment ty checks with (forge_ty_env) that ship no typing, one per line, which
# ty is told to check as Any (FORGE_TY_UNTYPED any). Left out: modules ty resolves
# without that environment (its bundled standard library stubs, the project's own
# code). Computed by .forge/helpers/untyped_python.py (FORGE_TY_UNTYPED_HELPER) and kept
# as a fact until the environment's site-packages, ty's search roots, the project's ty
# configuration or Forge's pins change: a package upgraded to one that ships typing
# leaves the list by itself. Prints nothing when switched off or without an environment.
forge_ty_untyped() {
  [ "$FORGE_TY_UNTYPED" = any ] || return 0
  forge_ty_env || return 0
  _tn_py=$FORGE_TY_PY
  _tn_sites=$(forge_py_sites "$_tn_py") || return 0
  _tn_h=${FORGE_TY_UNTYPED_HELPER:-.forge/helpers/untyped_python.py}
  [ -f "$_tn_h" ] || return 0
  forge_fact ty-untyped "$FORGE_TY_VERSION $FORGE_ENGINE_PYTHON $_tn_py $_tn_sites $(cksum < "$_tn_h") $(cksum ty.toml pyproject.toml 2>/dev/null)" forge_ty_untyped_compute "$_tn_h" "$_tn_sites"
}

# forge_ty_untyped_compute <helper> <sites>: runs the helper on the pinned ty's Python
# (the "watch" lines it prints are the directories its answer depends on, forge_fact).
forge_ty_untyped_compute() {
  _uc_v=$(printf '%s\n' "$2" | sed -n 's/^version //p')
  _uc_ifs=$IFS
  IFS='
'
  # shellcheck disable=SC2046
  set -- "$1" "$(pwd -P)" "$_uc_v" $(printf '%s\n' "$2" | sed -n 's/^site //p')
  IFS=$_uc_ifs
  uvx -q --python "$FORGE_ENGINE_PYTHON" --from "ty==$FORGE_TY_VERSION" python "$@" 2>/dev/null
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
      _tu_any=$(forge_ty_untyped | forge_types_untyped_filter)
      _tu_want="ty $FORGE_TY_VERSION $FORGE_TYPES_REV any:$(printf '%s' "$_tu_any" | forge_sha256 | cut -c 1-12)" ;;
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
      set -- env FORGE_TY_PYTHON="$_tu_py" FORGE_TY_PREFIX="$_tu_prefix" FORGE_TY_ANY="$_tu_any" uvx -q --python "$FORGE_ENGINE_PYTHON" --from "ty==$FORGE_TY_VERSION" python "$(pwd -P)/.forge/helpers/types_python.py" "$(pwd -P)" "$_tu_d" ;;
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
