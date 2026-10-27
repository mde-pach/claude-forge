# @module advice
# Advice (§10.7): findings reported once per turn.

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
