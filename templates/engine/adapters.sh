# @module adapters
# Tool adapters (§16): a check names what it needs from a tool (`forge_do <tool> <op>`); the
# adapter of the installed version says how to invoke it, or that this version cannot. Which
# rule codes the installed version has is asked from the binary where it can tell.

# One line per range and operation (src/adapters): tool, first version, first version after
# the range ("" when open), operation, arguments ("-" when the range cannot).
FORGE_ADAPTERS='@@ADAPTERS@@'

# One line per tool: tool, Forge's pin (its own tools; "" when the project's), the newest
# release the adapter was proven on, the exit status and output line of a usage error ("" when
# the tool has none), how it is run ("project", "path", or the function running Forge's
# pinned release), the function picking which adapter drives the project's tool (""), and the
# arguments printing its version.
FORGE_ADAPTER_META='@@ADAPTER_META@@'

# The codes the active rules' tool targets expect: tool, code, first stable release, the
# codes and the names the same rule had (comma-separated).
FORGE_TOOL_CODES='@@TOOL_CODES@@'

# forge_adapter_meta <tool>: sets FORGE_AM_PIN, FORGE_AM_VERIFIED, FORGE_AM_UEXIT,
# FORGE_AM_UPATTERN, FORGE_AM_RUNNER, FORGE_AM_PICK and FORGE_AM_VARGS from the tool's
# FORGE_ADAPTER_META line; fails when the tool has no adapter. Shell builtins only.
forge_adapter_meta() {
  FORGE_AM_PIN='' FORGE_AM_VERIFIED='' FORGE_AM_UEXIT='' FORGE_AM_UPATTERN='' FORGE_AM_RUNNER='' FORGE_AM_PICK='' FORGE_AM_VARGS='' FORGE_AM_PKG=''
  _am_ifs=$IFS
  IFS='
'
  for _am_l in $FORGE_ADAPTER_META; do
    case $_am_l in
      "$1	"*)
        IFS='	'
        # shellcheck disable=SC2086
        set -f; set -- $_am_l; set +f
        IFS=$_am_ifs
        FORGE_AM_PIN=${2:-} FORGE_AM_VERIFIED=${3:-} FORGE_AM_UEXIT=${4:-} FORGE_AM_UPATTERN=${5:-} FORGE_AM_RUNNER=${6:-project} FORGE_AM_PICK=${7:-} FORGE_AM_VARGS=${8:---version} FORGE_AM_PKG=${9:-}
        [ "$FORGE_AM_PKG" != - ] || FORGE_AM_PKG=''
        [ "$FORGE_AM_PIN" != - ] || FORGE_AM_PIN=''
        [ "$FORGE_AM_UEXIT" != - ] || FORGE_AM_UEXIT=''
        [ "$FORGE_AM_UPATTERN" != - ] || FORGE_AM_UPATTERN=''
        [ "$FORGE_AM_PICK" != - ] || FORGE_AM_PICK=''
        return 0 ;;
    esac
  done
  IFS=$_am_ifs
  return 1
}

# forge_adapter_tool <tool>: FORGE_AD_TOOL, the adapter that drives the project's <tool>: its
# own, or the one its pick function sets in FORGE_PICKED (tofu for terraform in an OpenTofu
# project).
forge_adapter_tool() {
  FORGE_AD_TOOL=$1
  forge_adapter_meta "$1" || return 0
  [ -n "$FORGE_AM_PICK" ] || return 0
  FORGE_PICKED=$1
  $FORGE_AM_PICK
  FORGE_AD_TOOL=$FORGE_PICKED
}

# forge_tool_version <tool>: the installed version of the project's <tool> (Forge's pin for
# its own), asked once (its version arguments, the first x.y[.z] printed) and kept until the
# binary changes (forge_fact: the executable, its directory and the package's that it
# resolves to are watched). Fails when the tool is absent or says no version.
forge_tool_version() {
  forge_tool_v "$1" || return 1
  printf '%s\n' "$FORGE_TV"
}

# forge_tool_v <tool>: FORGE_TV, forge_tool_version's answer, without a subshell; fails as it does.
forge_tool_v() {
  FORGE_TV=''
  forge_adapter_meta "$1"
  if [ -n "$FORGE_AM_PIN" ]; then FORGE_TV=$FORGE_AM_PIN; return 0; fi
  case $FORGE_AM_RUNNER in
    project)
      forge_tool_lookup "$1"
      [ -n "$FORGE_TOOL_CMD" ] || return 1
      case $FORGE_TOOL_BIN in /*) _tv_bin=$FORGE_TOOL_BIN ;; *) _tv_bin=$PWD/$FORGE_TOOL_BIN ;; esac
      _tv_key="$1 $PWD $FORGE_TOOL_CMD" ;;
    *)
      forge_path_lookup "$1"
      [ -n "$FORGE_FOUND" ] || return 1
      _tv_bin=$FORGE_FOUND
      _tv_key="$1 $FORGE_FOUND" ;;
  esac
  forge_fact tool-version "$_tv_key" forge_tool_version_ask "$1" "$_tv_bin" "$FORGE_AM_RUNNER" "$FORGE_AM_VARGS" "$FORGE_AM_PKG" || return 1
  [ -n "$FORGE_FACT" ] || return 1
  FORGE_TV=$FORGE_FACT
}

# forge_tool_version_ask <tool> <executable> <runner> <version args> [<package>]: the version
# the package's package.json says, when the adapter names one and it is found up from the
# executable's real path (an npm package's bin); else the first x.y[.z] the tool prints.
forge_tool_version_ask() {
  forge_tool_watch "$2"
  if [ -n "${5:-}" ]; then
    _va_d=$(realpath "$2" 2>/dev/null || readlink -f "$2" 2>/dev/null)
    while [ -n "$_va_d" ] && [ "$_va_d" != / ]; do
      _va_d=${_va_d%/*}
      [ -n "$_va_d" ] || break
      if [ -f "$_va_d/package.json" ] && grep -q "^[[:space:]]*\"name\"[[:space:]]*:[[:space:]]*\"$5\"" "$_va_d/package.json"; then
        printf 'watch %s\n' "$_va_d/package.json"
        sed -n 's/^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"\([0-9][^"]*\)".*/\1/p' "$_va_d/package.json" | grep -oE '^[0-9]+\.[0-9]+(\.[0-9]+)?' | head -n 1 | grep . && return 0
        break
      fi
    done
  fi
  _va_ifs=$IFS
  IFS=' '
  # shellcheck disable=SC2086
  set -- "$1" "$2" "$3" $4
  IFS=$_va_ifs
  _va_t=$1
  _va_bin=$2
  _va_run=$3
  shift 3
  case $_va_run in
    project) forge_exec_tool "$_va_t" "$@" ;;
    *) "$_va_bin" "$@" ;;
  esac 2>&1 | grep -oE '[0-9]+\.[0-9]+(\.[0-9]+)?' | head -n 1 | grep .
}

# forge_tool_watch <executable>: the "watch" lines (forge_fact) of what a reinstall or an
# upgrade changes: the executable, its directory, and the file and directory it resolves to
# (a package manager's link into the package, whose files may keep their archive's times).
forge_tool_watch() {
  printf 'watch %s\nwatch %s\n' "$1" "${1%/*}"
  _tw_real=$(realpath "$1" 2>/dev/null || readlink -f "$1" 2>/dev/null)
  [ -z "$_tw_real" ] || [ "$_tw_real" = "$1" ] && return 0
  # only a directory on the way is a link (/var to /private/var): the same file, already watched;
  # its parent directory would be one that changes all the time (the system's temporary directory)
  _tw_dir=$(cd "${1%/*}" 2>/dev/null && pwd -P)
  [ "$_tw_dir/${1##*/}" = "$_tw_real" ] && return 0
  printf 'watch %s\nwatch %s\nwatch %s\n' "$_tw_real" "${_tw_real%/*}" "${_tw_real%/*}/.."
}

# forge_adapter <tool> <version> <op>: how this version does <op>. Returns 0 and sets
# FORGE_AD_ARGS (its arguments); 1 when its range cannot (FORGE_AD_NEED: the first version
# that can, "" when none); 2 when no range covers the version (FORGE_AD_COVER: the versions
# that are).
forge_adapter() {
  _ad_first='' _ad_need='' _ad_args='' _ad_in=0
  _ad_ifs=$IFS
  IFS='
'
  set -f
  for _ad_l in $FORGE_ADAPTERS; do
    case $_ad_l in "$1	"*) ;; *) continue ;; esac
    IFS='	'
    # shellcheck disable=SC2086
    set -- "$1" "$2" "$3" $_ad_l
    IFS='
'
    # $4 tool, $5 from, $6 to (or "" when open: then $6 is the op), the op, the arguments
    if [ "$#" -eq 7 ]; then _ad_from=$5 _ad_to='' _ad_op=$6 _ad_a=$7; else _ad_from=$5 _ad_to=$6 _ad_op=$7 _ad_a=$8; fi
    if [ -z "$_ad_first" ] || forge_version_lt "$_ad_from" "$_ad_first"; then _ad_first=$_ad_from; fi
    [ "$_ad_op" = "$3" ] || continue
    _ad_below=1
    [ -z "$_ad_to" ] || forge_version_lt "$2" "$_ad_to" || _ad_below=0
    if [ "$_ad_below" = 1 ] && [ "$_ad_a" != - ] && { [ -z "$_ad_need" ] || forge_version_lt "$_ad_from" "$_ad_need"; }; then _ad_need=$_ad_from; fi
    if [ "$_ad_below" = 1 ] && ! forge_version_lt "$2" "$_ad_from"; then _ad_in=1; _ad_args=$_ad_a; fi
  done
  set +f
  IFS=$_ad_ifs
  if [ "$_ad_in" = 0 ]; then FORGE_AD_COVER="$1 $_ad_first and later"; return 2; fi
  if [ "$_ad_args" = - ]; then FORGE_AD_NEED=$_ad_need; return 1; fi
  FORGE_AD_ARGS=$_ad_args
  return 0
}

# forge_version_lt <a> <b>: version <a> is older than <b> (x.y.z, numerically; builtins).
forge_version_lt() {
  _vl_a=$1.0.0.0 _vl_b=$2.0.0.0 _vl_n=0
  while [ "$_vl_n" -lt 3 ]; do
    _vl_x=${_vl_a%%.*} _vl_y=${_vl_b%%.*}
    _vl_x=${_vl_x%%[!0-9]*} _vl_y=${_vl_y%%[!0-9]*}
    [ "${_vl_x:-0}" -lt "${_vl_y:-0}" ] && return 0
    [ "${_vl_x:-0}" -gt "${_vl_y:-0}" ] && return 1
    _vl_a=${_vl_a#*.} _vl_b=${_vl_b#*.} _vl_n=$((_vl_n + 1))
  done
  return 1
}

# forge_do <tool> <op> [args]: <op> by the installed <tool>, as its adapter says (§16). The
# caller's arguments replace the adapter's {1}, {2}… words (by rank) and its {args} word (the
# others), else follow its words. A version
# that cannot (its range has no such operation, or no range covers it) is not a failure: the
# check is "not enforced", with why (forge_do_note). A usage error of the command line (the
# adapter's exit status and line) is never a finding either: the check is skipped with the
# tool's error. Then, after a lint, the codes the active rules expect that this version does
# not have are noted (forge_tool_codes).
forge_do() {
  forge_adapter_tool "$1"
  _do_t=$FORGE_AD_TOOL
  _do_op=$2
  shift 2
  if ! forge_tool_v "$_do_t"; then
    forge_do_note "skipped: the version of $_do_t could not be read"
    return 0
  fi
  _do_v=$FORGE_TV
  forge_adapter "$_do_t" "$_do_v" "$_do_op"
  case $? in
    1) forge_do_note "not enforced: $_do_t $_do_v cannot $_do_op${FORGE_AD_NEED:+ (it needs $_do_t $FORGE_AD_NEED or later)}"; return 0 ;;
    2) forge_do_note "not enforced: no Forge adapter covers $_do_t $_do_v (Forge drives $FORGE_AD_COVER)"; return 0 ;;
  esac
  # the adapter's words: {1}, {2}… the caller's arguments of that rank, {args} the others, which
  # else follow the adapter's words
  _do_n=$#
  _do_used=' '
  _do_put=0
  _do_ifs=$IFS
  IFS=' '
  set -f
  for _do_w in $FORGE_AD_ARGS; do
    case $_do_w in '{'[1-9]'}') _do_k=${_do_w#?}; _do_used="$_do_used${_do_k%?} " ;; esac
  done
  for _do_w in $FORGE_AD_ARGS; do
    case $_do_w in
      '{args}')
        _do_i=0
        for _do_a in "$@"; do
          _do_i=$((_do_i + 1))
          [ "$_do_i" -le "$_do_n" ] || break
          case $_do_used in *" $_do_i "*) ;; *) set -- "$@" "$_do_a" ;; esac
        done
        _do_put=1 ;;
      '{'[1-9]'}')
        _do_k=${_do_w#?}
        _do_k=${_do_k%?}
        _do_i=0
        for _do_a in "$@"; do
          _do_i=$((_do_i + 1))
          if [ "$_do_i" = "$_do_k" ]; then set -- "$@" "$_do_a"; break; fi
        done ;;
      *) set -- "$@" "$_do_w" ;;
    esac
  done
  set +f
  IFS=$_do_ifs
  if [ "$_do_put" = 0 ]; then
    _do_i=0
    for _do_a in "$@"; do
      _do_i=$((_do_i + 1))
      [ "$_do_i" -le "$_do_n" ] || break
      case $_do_used in *" $_do_i "*) ;; *) set -- "$@" "$_do_a" ;; esac
    done
  fi
  shift "$_do_n"
  forge_adapter_meta "$_do_t"
  _do_ux=$FORGE_AM_UEXIT
  _do_up=$FORGE_AM_UPATTERN
  _do_run=$FORGE_AM_RUNNER
  # the run's outputs: files of this run's own in FORGE_TMP (removed with it)
  FORGE_DO_SEQ=$((${FORGE_DO_SEQ:-0} + 1))
  if [ -n "${FORGE_TMP:-}" ] && [ -d "$FORGE_TMP" ]; then _do_err=$FORGE_TMP/do.$$.$FORGE_DO_SEQ.err
  else _do_err=$(mktemp "${TMPDIR:-/tmp}/do-err.XXXXXX") || return 1
  fi
  _do_out=$_do_err.out
  # the tool's output is kept until it has exited: a usage error, on either stream, is not passed on
  case $_do_run in
    project) forge_exec_tool "$_do_t" "$@" > "$_do_out" 2> "$_do_err" ;;
    path) command "$_do_t" "$@" > "$_do_out" 2> "$_do_err" ;;
    *)
      _do_ifs=$IFS
      IFS=' '
      # shellcheck disable=SC2086
      set -- $_do_run "$@"
      IFS=$_do_ifs
      "$@" > "$_do_out" 2> "$_do_err" ;;
  esac
  _do_rc=$?
  # (a tool may colour its error line, as many do in CI: the colour codes go before matching)
  if [ -n "$_do_ux" ] && case ",$_do_ux," in *",$_do_rc,"*) true ;; *) false ;; esac \
    && cat "$_do_out" "$_do_err" | sed "s/$(printf '\033')\[[0-9;]*m//g" | grep -qE "$_do_up"; then
    # a command line this version rejects is not a finding (a base copy formatted on stdin fails instead)
    case $_do_op in *-stdin) ;; *)
      if [ -n "${c_id:-}" ]; then
        _do_esc=$(printf '\033')
        _do_why=$(cat "$_do_err" "$_do_out" | sed "s/$_do_esc\[[0-9;]*m//g" | grep -iE '^(error|[a-z]+: error)' | head -n 1)
        [ -n "$_do_why" ] || _do_why=$(cat "$_do_err" "$_do_out" | sed "s/$_do_esc\[[0-9;]*m//g" | grep -E "$_do_up" | head -n 1)
        forge_do_note "skipped: $_do_t $_do_v rejected the command ($_do_t $(IFS=' '; printf '%s' "$*")): ${_do_why#[Ee]rror: }"
        [ -n "${FORGE_TMP:-}" ] || rm -f "$_do_err" "$_do_out"
        return 0
      fi ;;
    esac
  fi
  forge_cat "$_do_out"
  [ ! -s "$_do_err" ] || forge_cat "$_do_err" >&2
  [ -n "${FORGE_TMP:-}" ] || rm -f "$_do_err" "$_do_out"
  if [ "$_do_op" = lint ] && [ -n "${c_id:-}" ]; then
    # the codes this version does not have (builtins: the answer is a fact)
    forge_tool_codes_v "$_do_t"
    _do_n=0 _do_miss=''
    _do_ifs=$IFS
    IFS='
'
    for _do_l in $FORGE_TCODES; do
      IFS='	'
      # shellcheck disable=SC2086
      set -- $_do_l
      IFS='
'
      [ "${2:-}" = - ] || continue
      _do_n=$((_do_n + 1))
      _do_miss="${_do_miss:+$_do_miss, }$1 needs $_do_t ≥ $3"
    done
    IFS=$_do_ifs
    [ "$_do_n" = 0 ] || forge_do_note "not enforced for $_do_n code(s) $_do_t $_do_v does not have: $_do_miss"
  fi
  return "$_do_rc"
}

# forge_do_note <text>: "forge:<check id> <text>", in $FORGE_TMP/notes under check.sh (the
# check prints it whatever its outcome, forge_run_check), else on stdout.
forge_do_note() {
  if [ -n "${FORGE_TMP:-}" ] && [ -n "${c_id:-}" ]; then printf 'forge:%s %s\n' "$c_id" "$1" >> "$FORGE_TMP/notes"
  else printf 'forge:%s %s\n' "${c_id:-forge}" "$1"
  fi
}

# forge_tool_codes <tool>: for each code the active rules expect of <tool> (FORGE_TOOL_CODES),
# "<code><TAB><the installed version's code for that rule, or -><TAB><its first stable
# release>". The installed version's rules are asked from the binary (the adapter's codes
# operation: ruff `rule --all --output-format json`); a rule is the installed one with one of
# the rule's codes and one of its names, not in preview, in a release from which it reports
# the rules' examples (the history's first release, §16.4). The answer is a fact (forge_fact)
# for the binary and the codes asked: a few lines, read back without a process.
forge_tool_codes() {
  forge_tool_codes_v "$1"
  [ -z "$FORGE_TCODES" ] || printf '%s\n' "$FORGE_TCODES"
}

# forge_tool_codes_v <tool>: FORGE_TCODES, forge_tool_codes' lines, without a subshell.
forge_tool_codes_v() {
  FORGE_TCODES=''
  _tk_want=''
  _tk_ifs=$IFS
  IFS='
'
  for _tk_l in $FORGE_TOOL_CODES; do
    # the code, the line's second field
    case $_tk_l in "$1	"*) _tk_r=${_tk_l#*	}; _tk_want="$_tk_want${_tk_want:+ }${_tk_r%%	*}" ;; esac
  done
  IFS=$_tk_ifs
  [ -n "$_tk_want" ] || return 0
  forge_tool_v "$1" || return 0
  _tk_v=$FORGE_TV
  forge_adapter "$1" "$_tk_v" rules || return 0
  forge_tool_lookup "$1"
  case $FORGE_TOOL_BIN in /*) _tk_bin=$FORGE_TOOL_BIN ;; *) _tk_bin=$PWD/$FORGE_TOOL_BIN ;; esac
  # the codes asked are part of the key: one answer per set of active rules
  forge_fact tool-codes "$1 $_tk_v $PWD $FORGE_TOOL_CMD $_tk_want" forge_tool_codes_ask "$1" "$FORGE_AD_ARGS" "$_tk_bin" "$_tk_v" || return 0
  FORGE_TCODES=$FORGE_FACT
}

# forge_tool_codes_ask <tool> <rules args> <executable> <version>: forge_tool_codes' answer,
# from the binary's rule table.
forge_tool_codes_ask() {
  forge_tool_watch "$3"
  _tc_have=$(forge_tool_rules "$1" "$2") || return 1
  { printf '%s\n' "$_tc_have" | sed 's/^/have	/'; printf '%s\n' "$FORGE_TOOL_CODES" | sed 's/^/want	/'; } | awk -F '\t' -v t="$1" -v v="$4" '
    function cmp(a, b,   x, y, i) {
      split(a, x, "."); split(b, y, ".")
      for (i = 1; i <= 3; i++) { if (x[i] + 0 < y[i] + 0) return -1; if (x[i] + 0 > y[i] + 0) return 1 }
      return 0
    }
    $1 == "have" { if ($4 == "false") stable[$2 "\t" $3] = 1; next }
    $2 != t { next }
    {
      nc = split($5, codes, ","); nn = split($6, names, ","); got = "-"
      for (i = 1; i <= nc && got == "-"; i++) for (j = 1; j <= nn; j++) if ((codes[i] "\t" names[j]) in stable) { got = codes[i]; break }
      # listed, but before the release from which it reports what the rules ask of it
      if (cmp(v, $4) < 0) got = "-"
      print $3 "\t" got "\t" $4
    }'
}

# forge_tool_rules <tool> <args>: "<code><TAB><name><TAB><preview>" per rule the binary lists
# (ruff's JSON: one key per line).
forge_tool_rules() {
  _tr_ifs=$IFS
  IFS=' '
  # shellcheck disable=SC2086
  set -- "$1" $2
  IFS=$_tr_ifs
  forge_exec_tool "$@" 2>/dev/null | awk '
    /^    "name": "/ { name = $0; sub(/^    "name": "/, "", name); sub(/",?$/, "", name) }
    /^    "code": "/ { code = $0; sub(/^    "code": "/, "", code); sub(/",?$/, "", code) }
    /^    "preview": / { p = $0; sub(/^    "preview": /, "", p); sub(/,?$/, "", p); preview = p }
    /^  }/ { if (code != "") print code "\t" name "\t" (preview == "" ? "false" : preview); code = ""; name = ""; preview = "" }
    END { if (NR == 0) exit 1 }'
}

# forge_adapter_covers <tool> <version>: a range of the tool's adapter covers the version;
# else FORGE_AD_COVER says which versions are.
forge_adapter_covers() {
  _ac_first='' _ac_in=0
  _ac_ifs=$IFS
  IFS='
'
  set -f
  for _ac_l in $FORGE_ADAPTERS; do
    case $_ac_l in "$1	"*) ;; *) continue ;; esac
    # tool, from, to ("" when open), op, arguments
    _ac_r=${_ac_l#*	}
    _ac_from=${_ac_r%%	*}
    _ac_r=${_ac_r#*	}
    _ac_to=${_ac_r%%	*}
    if [ -z "$_ac_first" ] || forge_version_lt "$_ac_from" "$_ac_first"; then _ac_first=$_ac_from; fi
    forge_version_lt "$2" "$_ac_from" && continue
    [ -z "$_ac_to" ] || forge_version_lt "$2" "$_ac_to" || continue
    _ac_in=1
  done
  set +f
  IFS=$_ac_ifs
  [ "$_ac_in" = 1 ] && return 0
  [ -z "$_ac_first" ] && return 0
  FORGE_AD_COVER="$1 $_ac_first and later"
  return 1
}

# forge_tool_report: what the installed versions of the tools the active rules drive cannot
# do, one line each (SessionStart, forge status): a version no adapter covers, one newer than
# the adapter was proven on, an operation a check needs that the version cannot do, the codes
# the rules expect that it does not have. Absent tools are forge_missing_tools'.
forge_tool_report() {
  # the tools the rules name, once each (builtins)
  _rp_seen='
'
  _rp_ifs=$IFS
  IFS='
'
  for _rp_l in $FORGE_ADAPTER_USES $FORGE_TOOL_CODES; do
    _rp_n=${_rp_l%%	*}
    case $_rp_seen in *"
$_rp_n
"*) continue ;; esac
    _rp_seen="$_rp_seen$_rp_n
"
    forge_has_tool "$_rp_n" || continue
    # the rules name the tool; the project's may be another adapter's (tofu for terraform)
    forge_adapter_tool "$_rp_n"
    _rp_t=$FORGE_AD_TOOL
    forge_tool_v "$_rp_t" || { printf '%s: its version could not be read\n' "$_rp_t"; continue; }
    _rp_v=$FORGE_TV
    # the uses of the tool: "<op> <rule id>" lines
    _rp_uses=''
    _rp_ids=''
    for _rp_u in $FORGE_ADAPTER_USES; do
      case $_rp_u in "$_rp_n	"*)
        _rp_u=${_rp_u#*	}
        _rp_uses="$_rp_uses
$_rp_u"
        case ", $_rp_ids, " in *", ${_rp_u#*	}, "*) ;; *) _rp_ids="$_rp_ids${_rp_ids:+, }${_rp_u#*	}" ;; esac ;;
      esac
    done
    if ! forge_adapter_covers "$_rp_t" "$_rp_v"; then
      printf '%s %s: no Forge adapter covers it (Forge drives %s)%s\n' "$_rp_t" "$_rp_v" "$FORGE_AD_COVER" "${_rp_ids:+: $_rp_ids not enforced}"
      continue
    fi
    forge_adapter_meta "$_rp_t"
    _rp_ver=$FORGE_AM_VERIFIED
    if [ -n "$_rp_ver" ] && ! forge_version_ge "$(forge_major_minor "$_rp_ver")" "$(forge_major_minor "$_rp_v")"; then
      printf '%s %s is newer than the releases Forge'"'"'s adapter was proven on (up to %s): driven as %s\n' "$_rp_t" "$_rp_v" "$_rp_ver" "$_rp_ver"
    fi
    for _rp_u in $_rp_uses; do
      _rp_op=${_rp_u%%	*}
      _rp_id=${_rp_u#*	}
      forge_adapter "$_rp_t" "$_rp_v" "$_rp_op" || printf '%s: not enforced: %s %s cannot %s%s\n' "$_rp_id" "$_rp_t" "$_rp_v" "$_rp_op" "${FORGE_AD_NEED:+ (it needs $_rp_t $FORGE_AD_NEED or later)}"
    done
    # the codes the active rules ask for that this release does not have ("<code><TAB>-<TAB><since>")
    forge_tool_codes_v "$_rp_t"
    _rp_k=0
    _rp_s=''
    for _rp_c in $FORGE_TCODES; do
      _rp_r=${_rp_c#*	}
      case $_rp_r in "-	"*) _rp_k=$((_rp_k + 1)); _rp_s="$_rp_s${_rp_s:+, }${_rp_c%%	*} (needs $_rp_t ≥ ${_rp_r#*	})" ;; esac
    done
    [ "$_rp_k" = 0 ] || printf '%s %s does not have %s rule code(s) the active rules ask for (not enforced): %s\n' "$_rp_t" "$_rp_v" "$_rp_k" "$_rp_s"
  done
  IFS=$_rp_ifs
}

# forge_major_minor <version>: its first two numbers (x.y), as cut -d. -f1-2 does.
forge_major_minor() {
  case $1 in
    *.*.*) _mm=${1#*.}; printf '%s.%s' "${1%%.*}" "${_mm%%.*}" ;;
    *) printf '%s' "$1" ;;
  esac
}
