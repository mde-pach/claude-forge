# @module supply-chain
# Known vulnerabilities in dependencies (osv-scanner).

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
  forge_do osv-scanner scan "$FORGE_TMP/osv.toml" -L "$1" > "$FORGE_TMP/osv.md" 2> "$FORGE_TMP/osv.err"
  # 0 no vulnerability, 1 vulnerabilities, 128 no package in the lock file (nothing to be
  # vulnerable); 127 any other error (unreadable lock file, api.osv.dev unreachable)
  case $? in
    0|1|128) ;;
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
