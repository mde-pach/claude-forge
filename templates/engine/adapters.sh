# @module adapters
# Tool adapters (§16): a check names what it needs from a tool (`forge_do <tool> <op>`); the
# adapter of the installed version says how to invoke it, or that this version cannot. Which
# rule codes the installed version has is asked from the binary where it can tell.

# One line per range and operation (src/adapters): tool, first version, first version after
# the range ("" when open), operation, arguments ("-" when the range cannot).
FORGE_ADAPTERS='@@ADAPTERS@@'

# One line per tool: tool, Forge's pin (its own tools; "" when the project's), the newest
# release the adapter was proven on, the exit status and output line of a usage error.
FORGE_ADAPTER_META='@@ADAPTER_META@@'

# The codes the active rules' tool targets expect: tool, code, first stable release, the
# codes and the names the same rule had (comma-separated).
FORGE_TOOL_CODES='@@TOOL_CODES@@'

# forge_adapter_meta <tool> <field>: a field of the tool's FORGE_ADAPTER_META line (2: pin,
# 3: verified, 4: usage exit, 5: usage pattern).
forge_adapter_meta() {
  printf '%s\n' "$FORGE_ADAPTER_META" | awk -F '\t' -v t="$1" -v f="$2" '$1 == t { print $f; exit }'
}

# forge_tool_version <tool>: the installed version of the project's <tool> (Forge's pin for
# its own), asked once (`<tool> --version`) and kept until the binary changes (forge_fact).
# Fails when the tool is absent or says no version.
forge_tool_version() {
  _tv_pin=$(forge_adapter_meta "$1" 2)
  if [ -n "$_tv_pin" ]; then printf '%s\n' "$_tv_pin"; return 0; fi
  _tv_cmd=$(tool "$1")
  [ -n "$_tv_cmd" ] || return 1
  case $_tv_cmd in "bun run --bun "*) _tv_bin=node_modules/.bin/$1 ;; *) _tv_bin=$_tv_cmd ;; esac
  forge_fact tool-version "$1 $_tv_cmd $(ls -lL "$_tv_bin" 2>/dev/null)" forge_tool_version_ask "$1"
}

forge_tool_version_ask() {
  forge_exec_tool "$1" --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+(\.[0-9]+)?' | head -n 1 | grep .
}

# forge_adapter <tool> <version> <op>: how this version does <op>. Returns 0 and sets
# FORGE_AD_ARGS (its arguments); 1 when its range cannot (FORGE_AD_NEED: the first version
# that can, "" when none); 2 when no range covers the version (FORGE_AD_COVER: the versions
# that are).
forge_adapter() {
  _ad_out=$(printf '%s\n' "$FORGE_ADAPTERS" | awk -F '\t' -v t="$1" -v v="$2" -v op="$3" '
    function cmp(a, b,   x, y, i, n) {
      split(a, x, "."); split(b, y, ".")
      for (i = 1; i <= 3; i++) { if (x[i] + 0 < y[i] + 0) return -1; if (x[i] + 0 > y[i] + 0) return 1 }
      return 0
    }
    $1 != t { next }
    { if (first == "" || cmp($2, first) < 0) first = $2 }
    $4 == op && cmp(v, $2) >= 0 && ($3 == "" || cmp(v, $3) < 0) { found = 1; args = $5 }
    $4 == op && $5 != "-" && ($3 == "" || cmp(v, $3) < 0) && (need == "" || cmp($2, need) < 0) { need = $2 }
    $4 == op && cmp(v, $2) >= 0 && ($3 == "" || cmp(v, $3) < 0) { inrange = 1 }
    END {
      if (!inrange) { print "none\t" first; exit }
      if (args == "-") { print "cannot\t" need; exit }
      print "ok\t" args
    }')
  case $_ad_out in
    ok*) FORGE_AD_ARGS=${_ad_out#ok	}; return 0 ;;
    cannot*) FORGE_AD_NEED=${_ad_out#cannot	}; return 1 ;;
    *) FORGE_AD_COVER="$1 ${_ad_out#none	} and later"; return 2 ;;
  esac
}

# forge_do <tool> <op> [args]: <op> by the installed <tool>, as its adapter says (§16). The
# caller's arguments replace an {args} word of the adapter's, else follow them. A version
# that cannot (its range has no such operation, or no range covers it) is not a failure: the
# check is "not enforced", with why (forge_do_note). A usage error of the command line (the
# adapter's exit status and line) is never a finding either: the check is skipped with the
# tool's error. Then, after a lint, the codes the active rules expect that this version does
# not have are noted (forge_tool_codes).
forge_do() {
  _do_t=$1
  _do_op=$2
  shift 2
  if ! _do_v=$(forge_tool_version "$_do_t"); then
    forge_do_note "skipped: the version of $_do_t could not be read"
    return 0
  fi
  forge_adapter "$_do_t" "$_do_v" "$_do_op"
  case $? in
    1) forge_do_note "not enforced: $_do_t $_do_v cannot $_do_op${FORGE_AD_NEED:+ (it needs $_do_t $FORGE_AD_NEED or later)}"; return 0 ;;
    2) forge_do_note "not enforced: no Forge adapter covers $_do_t $_do_v (Forge drives $FORGE_AD_COVER)"; return 0 ;;
  esac
  # the adapter's words, the caller's arguments at {args} or after them
  _do_n=$#
  _do_put=0
  _do_ifs=$IFS
  IFS=' '
  for _do_w in $FORGE_AD_ARGS; do
    if [ "$_do_w" = '{args}' ]; then
      _do_i=0
      for _do_a in "$@"; do
        [ "$_do_i" -lt "$_do_n" ] && set -- "$@" "$_do_a"
        _do_i=$((_do_i + 1))
      done
      _do_put=1
    else
      set -- "$@" "$_do_w"
    fi
  done
  IFS=$_do_ifs
  if [ "$_do_put" = 0 ]; then
    _do_i=0
    for _do_a in "$@"; do
      [ "$_do_i" -lt "$_do_n" ] && set -- "$@" "$_do_a"
      _do_i=$((_do_i + 1))
    done
  fi
  shift "$_do_n"
  _do_err=$(mktemp "${FORGE_TMP:-${TMPDIR:-/tmp}}/do-err.XXXXXX") || return 1
  if [ "$_do_t" = ty ]; then forge_ty "$@" 2> "$_do_err"; else forge_exec_tool "$_do_t" "$@" 2> "$_do_err"; fi
  _do_rc=$?
  if [ "$_do_rc" = "$(forge_adapter_meta "$_do_t" 4)" ] && grep -qE "$(forge_adapter_meta "$_do_t" 5)" "$_do_err"; then
    # a command line this version rejects is not a finding (a base copy on stdin fails instead)
    case " $* " in *" - "*) ;; *)
      if [ -n "${c_id:-}" ]; then
        forge_do_note "skipped: $_do_t $_do_v rejected the command ($_do_t $(IFS=' '; printf '%s' "$*")): $(grep -iE '^error' "$_do_err" | head -n 1 | sed 's/^[Ee]rror: //')"
        rm -f "$_do_err"
        return 0
      fi ;;
    esac
  fi
  cat "$_do_err" >&2
  rm -f "$_do_err"
  if [ "$_do_op" = lint ] && [ -n "${c_id:-}" ]; then
    _do_miss=$(forge_tool_codes "$_do_t" | awk -F '\t' -v t="$_do_t" '$2 == "-" { n++; s = s (s == "" ? "" : ", ") $1 " needs " t " ≥ " $3 } END { if (n) print n "\t" s }')
    [ -z "$_do_miss" ] || forge_do_note "not enforced for ${_do_miss%%	*} code(s) $_do_t $_do_v does not have: ${_do_miss#*	}"
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
# operation: ruff `rule --all --output-format json`) once per binary (forge_fact); a rule is
# the installed one with one of the rule's codes and one of its names, not in preview, in a
# release from which it reports the rules' examples (the history's first release, §16.4).
forge_tool_codes() {
  printf '%s\n' "$FORGE_TOOL_CODES" | grep -q "^$1	" || return 0
  _tk_v=$(forge_tool_version "$1") || return 0
  forge_adapter "$1" "$_tk_v" rules || return 0
  _tk_cmd=$(tool "$1")
  _tk_have=$(forge_fact tool-rules "$1 $_tk_v $_tk_cmd $(ls -lL "${_tk_cmd##* }" 2>/dev/null)" forge_tool_rules "$1" "$FORGE_AD_ARGS") || return 0
  { printf '%s\n' "$_tk_have" | sed 's/^/have	/'; printf '%s\n' "$FORGE_TOOL_CODES" | sed 's/^/want	/'; } | awk -F '\t' -v t="$1" -v v="$_tk_v" '
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
  _ac_first=$(printf '%s\n' "$FORGE_ADAPTERS" | awk -F '\t' -v t="$1" -v v="$2" '
    function cmp(a, b,   x, y, i) {
      split(a, x, "."); split(b, y, ".")
      for (i = 1; i <= 3; i++) { if (x[i] + 0 < y[i] + 0) return -1; if (x[i] + 0 > y[i] + 0) return 1 }
      return 0
    }
    $1 != t { next }
    { if (first == "" || cmp($2, first) < 0) first = $2 }
    cmp(v, $2) >= 0 && ($3 == "" || cmp(v, $3) < 0) { in_range = 1 }
    END { if (!in_range) print first }')
  [ -z "$_ac_first" ] && return 0
  FORGE_AD_COVER="$1 $_ac_first and later"
  return 1
}

# forge_tool_report: what the installed versions of the tools the active rules drive cannot
# do, one line each (SessionStart, forge status): a version no adapter covers, one newer than
# the adapter was proven on, an operation a check needs that the version cannot do, the codes
# the rules expect that it does not have. Absent tools are forge_missing_tools'.
forge_tool_report() {
  { printf '%s\n' "$FORGE_ADAPTER_USES"; printf '%s\n' "$FORGE_TOOL_CODES"; } | awk -F '\t' 'NF { print $1 }' | sort -u | while IFS= read -r _rp_t; do
    forge_has_tool "$_rp_t" || continue
    _rp_v=$(forge_tool_version "$_rp_t") || { printf '%s: its version could not be read\n' "$_rp_t"; continue; }
    if ! forge_adapter_covers "$_rp_t" "$_rp_v"; then
      _rp_ids=$(printf '%s\n' "$FORGE_ADAPTER_USES" | awk -F '\t' -v t="$_rp_t" '$1 == t { print $3 }' | sort -u | tr '\n' ' ' | sed 's/ $//; s/ /, /g')
      printf '%s %s: no Forge adapter covers it (Forge drives %s)%s\n' "$_rp_t" "$_rp_v" "$FORGE_AD_COVER" "${_rp_ids:+: $_rp_ids not enforced}"
      continue
    fi
    _rp_ver=$(forge_adapter_meta "$_rp_t" 3)
    if [ -n "$_rp_ver" ] && ! forge_version_ge "$(printf '%s' "$_rp_ver" | cut -d. -f1-2)" "$(printf '%s' "$_rp_v" | cut -d. -f1-2)"; then
      printf '%s %s is newer than the releases Forge'"'"'s adapter was proven on (up to %s): driven as %s\n' "$_rp_t" "$_rp_v" "$_rp_ver" "$_rp_ver"
    fi
    printf '%s\n' "$FORGE_ADAPTER_USES" | awk -F '\t' -v t="$_rp_t" '$1 == t' | while IFS='	' read -r _ _rp_op _rp_id; do
      forge_adapter "$_rp_t" "$_rp_v" "$_rp_op" || printf '%s: not enforced: %s %s cannot %s%s\n' "$_rp_id" "$_rp_t" "$_rp_v" "$_rp_op" "${FORGE_AD_NEED:+ (it needs $_rp_t $FORGE_AD_NEED or later)}"
    done
    forge_tool_codes "$_rp_t" | awk -F '\t' -v t="$_rp_t" -v v="$_rp_v" '
      $2 == "-" { n++; s = s (s == "" ? "" : ", ") $1 " (needs " t " ≥ " $3 ")" }
      END { if (n) print t " " v " does not have " n " rule code(s) the active rules ask for (not enforced): " s }'
  done
}
