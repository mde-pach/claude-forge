# @module openapi
# OpenAPI: linting, the ruleset ratchet, breaking changes, generated clients in sync.

# ---- OpenAPI: lint and config ratchet

FORGE_REDOCLY_VERSION="@@REDOCLY_VERSION@@"

FORGE_SPECTRAL_VERSION="@@SPECTRAL_VERSION@@"

# forge_openapi_lint: when a YAML or JSON file changed (or with --all), each OpenAPI document
# of the repository (forge_openapi_spec) linted by the project's declared linter: the pinned
# Redocly CLI with redocly.yaml, the pinned Spectral CLI with .spectral.y*ml / .json (both when
# both are configured), else Redocly's recommended rules; through bunx. "path:line:column:
# error rule: message" per error.
forge_openapi_lint() {
  git ls-files --cached --others --exclude-standard -- '*.yaml' '*.yml' '*.json' 2>/dev/null | grep -vE "$FORGE_OWN_ERE" | forge_filter forge_openapi_spec > "$FORGE_TMP/oal.roots"
  [ -s "$FORGE_TMP/oal.roots" ] || return 0
  command -v bunx >/dev/null 2>&1 || { forge_for_agent || printf 'forge:%s skipped: bunx is needed to run the OpenAPI linter\n' "$c_id"; return 0; }
  _ol_rc=0; _ol_any=''
  _ol_spectral=''
  for _ol_s in .spectral.yaml .spectral.yml .spectral.json; do [ -f "$_ol_s" ] && _ol_spectral=$_ol_s && break; done
  if [ -f redocly.yaml ] || [ -z "$_ol_spectral" ]; then
    _ol_any=1
    set -f
    # shellcheck disable=SC2046
    forge_do redocly lint $(cat "$FORGE_TMP/oal.roots") > "$FORGE_TMP/oal.out" 2> "$FORGE_TMP/oal.err"
    _ol_r=$?
    set +f
    if [ "$_ol_r" -gt 1 ] || { [ "$_ol_r" = 1 ] && ! grep -q '^::error ' "$FORGE_TMP/oal.out"; }; then
      forge_for_agent || printf 'forge:%s skipped: redocly could not run (%s)\n' "$c_id" "$(grep -v '^[[:space:]]*$' "$FORGE_TMP/oal.err" | tail -n 1)"
    else
      awk '/^::error / {
          meta = $0; sub(/^::error /, "", meta); msg = meta; sub(/^[^:]*::/, "", msg); sub(/::.*$/, "", meta)
          sub(/%0A.*$/, "", msg)
          t = meta; sub(/^.*title=/, "", t); sub(/,.*$/, "", t)
          f = meta; sub(/^.*file=/, "", f); sub(/,.*$/, "", f)
          l = meta; sub(/^.*line=/, "", l); sub(/,.*$/, "", l)
          c = meta; sub(/^.*col=/, "", c); sub(/,.*$/, "", c)
          print f ":" l ":" c ": error " t ": " msg; bad = 1 }
        END { exit bad }' "$FORGE_TMP/oal.out" || _ol_rc=1
    fi
  fi
  if [ -n "$_ol_spectral" ]; then
    set -f
    # shellcheck disable=SC2046
    forge_do spectral lint "$_ol_spectral" $(cat "$FORGE_TMP/oal.roots") > "$FORGE_TMP/oal.out" 2>&1
    _ol_r=$?
    set +f
    if ! grep -qE '^[^[:space:]]+:[0-9]+:[0-9]+ (error|warning|information|hint) ' "$FORGE_TMP/oal.out" && [ "$_ol_r" != 0 ]; then
      forge_for_agent || printf 'forge:%s skipped: spectral could not run (%s)\n' "$c_id" "$(grep -v '^[[:space:]]*$' "$FORGE_TMP/oal.out" | tail -n 1)"
    else
      awk '$2 == "error" && $1 ~ /:[0-9]+:[0-9]+$/ { r = $3; f = $1; m = $0; sub(/^[^ ]+ [^ ]+ [^ ]+ /, "", m); print f ": error " r ": " m; bad = 1 } END { exit bad }' "$FORGE_TMP/oal.out" || _ol_rc=1
    fi
  fi
  return "$_ol_rc"
}

# forge_yaml_rules <file>: "rule<TAB>level" for each entry of the top-level rules: map of a
# Redocly or Spectral YAML config (a scalar level, or the severity: of an object), then
# "extends<TAB><entry>" for each extends entry and "overrides<TAB><n>" for each overrides item.
forge_yaml_rules() {
  awk '
    function val(s) { sub(/^[^:]*:[[:space:]]*/, "", s); sub(/[[:space:]]+#.*$/, "", s); gsub(/["\047]/, "", s); sub(/[[:space:]]+$/, "", s); return s }
    function key(s) { sub(/^[[:space:]]+/, "", s); sub(/^-[[:space:]]*/, "", s); sub(/[[:space:]]*:.*$/, "", s); gsub(/["\047]/, "", s); return s }
    { sub(/\r$/, "") }
    /^[[:space:]]*(#|$)/ { next }
    /^[^[:space:]]/ {
      sect = key($0); ind = ""
      if (sect == "extends") { v = val($0); if (v ~ /^\[/) { gsub(/[][]/, "", v); n = split(v, a, /[[:space:]]*,[[:space:]]*/); for (i = 1; i <= n; i++) if (a[i] != "") print "extends\t" a[i] } else if (v != "") print "extends\t" v }
      next
    }
    {
      match($0, /^[[:space:]]+/); w = RLENGTH
      if (ind == "") ind = w
    }
    sect == "extends" && w == ind { v = $0; sub(/^[[:space:]]+-[[:space:]]*/, "", v); sub(/^\[/, "", v); gsub(/["\047\]]/, "", v); sub(/,.*$/, "", v); if (v != "") print "extends\t" v; next }
    sect == "overrides" && w == ind && $0 ~ /^[[:space:]]+-/ { print "overrides\t" ++no; next }
    sect == "rules" && w == ind { r = key($0); v = val($0); if (v != "") print r "\t" v; else pend = r; next }
    sect == "rules" && w > ind && pend != "" && key($0) == "severity" { print pend "\t" val($0); pend = ""; next }
  ' "$1" 2>/dev/null
}

# forge_lint_level <level>: a Redocly / Spectral level as a number (higher is stricter).
forge_lint_level() {
  case $1 in error|0) echo 4 ;; warn|warning|1) echo 3 ;; info|information|2) echo 2 ;; hint|3) echo 1 ;; off|false|-1) echo 0 ;; true|on) echo 4 ;; *) echo '' ;; esac
}

# forge_openapi_ratchet: for each changed redocly.yaml / .redocly.yaml / .spectral.y*ml present
# in the base, a rule lowered (error > warn > info|hint > off) or removed, an extends entry
# removed, an overrides item added; for a changed .redocly.lint-ignore.yaml, entries added.
# One "path: …" line each, unless forge.yaml accepts it.
forge_openapi_ratchet() {
  while IFS= read -r _or_f; do
    case $_or_f in redocly.yaml|.redocly.yaml|.spectral.yaml|.spectral.yml|.redocly.lint-ignore.yaml) ;; *) continue ;; esac
    [ -f "$FORGE_TMP/base/$_or_f" ] || continue
    if [ "$_or_f" = .redocly.lint-ignore.yaml ]; then
      _or_was=$(grep -cE '^[[:space:]]*-' "$FORGE_TMP/base/$_or_f"); _or_now=$(grep -cE '^[[:space:]]*-' "$_or_f" 2>/dev/null || echo 0)
      [ "${_or_now:-0}" -gt "$_or_was" ] && printf '%s: ignored problems %s -> %s\n' "$_or_f" "$_or_was" "$_or_now"
      continue
    fi
    forge_yaml_rules "$FORGE_TMP/base/$_or_f" > "$FORGE_TMP/or.then"
    if [ -f "$_or_f" ]; then forge_yaml_rules "$_or_f" > "$FORGE_TMP/or.now"; else : > "$FORGE_TMP/or.now"; fi
    while IFS='	' read -r _or_k _or_v; do
      case $_or_k in
        extends) grep -qxF "extends	$_or_v" "$FORGE_TMP/or.now" || printf '%s: extends no longer has %s\n' "$_or_f" "$_or_v" ;;
        overrides) ;;
        *)
          _or_n=$(awk -F '\t' -v k="$_or_k" '$1 == k { print $2; exit }' "$FORGE_TMP/or.now")
          if [ -z "$_or_n" ]; then printf '%s: rule %s no longer set (was %s)\n' "$_or_f" "$_or_k" "$_or_v"
          else
            _or_a=$(forge_lint_level "$_or_v"); _or_b=$(forge_lint_level "$_or_n")
            [ -n "$_or_a" ] && [ -n "$_or_b" ] && [ "$_or_b" -lt "$_or_a" ] && printf '%s: rule %s %s -> %s\n' "$_or_f" "$_or_k" "$_or_v" "$_or_n"
          fi ;;
      esac
    done < "$FORGE_TMP/or.then"
    awk -F '\t' -v f="$_or_f" 'FILENAME == ARGV[1] { if ($1 != "extends" && $1 != "overrides" && !($1 in seen)) { seen[$1] = 1 } if ($1 == "overrides") o++; next }
      $1 == "overrides" { n++ }
      $1 != "extends" && $1 != "overrides" && !($1 in seen) && ($2 == "off" || $2 == "false" || $2 == "-1") { print f ": rule " $1 " turned off" }
      END { if (n > o) print f ": overrides gains " (n - o) " entr" (n - o == 1 ? "y" : "ies") }' "$FORGE_TMP/or.then" "$FORGE_TMP/or.now"
    true
  done < "$FORGE_TMP/changed" | forge_unaccepted
}

# ---- OpenAPI (openapi/no-breaking)

# forge_pinned_redocly, forge_pinned_spectral <args>: Forge's pinned Redocly CLI and Spectral
# through bunx (forge_pinned).
forge_pinned_redocly() { bunx "@redocly/cli@$FORGE_REDOCLY_VERSION" "$@"; }
forge_pinned_spectral() { bunx "@stoplight/spectral-cli@$FORGE_SPECTRAL_VERSION" "$@"; }

# forge_oasdiff: each OpenAPI document of the repository (forge_openapi_spec) that existed at the
# base, compared by the pinned oasdiff with its base version: "error at <path>, in API … [id]."
# for each breaking change (level ERR). The base side is a copy of the repository's YAML and
# JSON files as they were, so multi-file $refs resolve. A line recorded in forge.yaml (accept:
# change) is accepted. Not with --all (no base to compare).
forge_oasdiff() {
  [ "$mode" != --all ] || return 0
  git ls-files --cached --others --exclude-standard -- '*.yaml' '*.yml' '*.json' 2>/dev/null > "$FORGE_TMP/oas.all"
  forge_filter forge_openapi_spec < "$FORGE_TMP/oas.all" > "$FORGE_TMP/oas.roots"
  [ -s "$FORGE_TMP/oas.roots" ] || return 0
  forge_release oasdiff _oa_bin || return 0
  _oa_base=$FORGE_TMP/oas-base
  rm -rf "$_oa_base"; mkdir -p "$_oa_base"
  tar -cf - -T "$FORGE_TMP/oas.all" 2>/dev/null | ( cd "$_oa_base" && tar -xf - 2>/dev/null )
  while IFS= read -r _oa_f; do
    rm -f "$_oa_base/$_oa_f"
    if [ -f "$FORGE_TMP/base/$_oa_f" ]; then mkdir -p "$_oa_base/$(dirname "$_oa_f")" && cp "$FORGE_TMP/base/$_oa_f" "$_oa_base/$_oa_f"; fi
  done < "$FORGE_TMP/changed"
  : > "$FORGE_TMP/oas.out"
  while IFS= read -r _oa_f; do
    [ -f "$_oa_base/$_oa_f" ] || continue
    # 0 no breaking change, 1 one (--fail-on ERR), 100-107 could not compare (unreadable spec, usage)
    forge_do oasdiff diff "$_oa_base/$_oa_f" "$_oa_f" > "$FORGE_TMP/oas.one" 2>&1
    case $? in
      0|1) ;;
      *) printf 'forge:%s skipped: oasdiff could not compare %s (%s)\n' "$c_id" "$_oa_f" "$(grep -v '^[[:space:]]*$' "$FORGE_TMP/oas.one" | tail -n 1)"; continue ;;
    esac
    grep '^error at ' "$FORGE_TMP/oas.one" | sed "s|$_oa_base/||g; s/[[:space:]]*\$//" >> "$FORGE_TMP/oas.out"
  done < "$FORGE_TMP/oas.roots"
  _oa_rc=0
  while IFS= read -r _oa_l; do
    [ -f forge.yaml ] && grep -qF -- "$_oa_l" forge.yaml && continue
    printf '%s\n' "$_oa_l"; _oa_rc=1
  done < "$FORGE_TMP/oas.out"
  [ "$_oa_rc" = 0 ] || printf 'Restore what clients rely on, or have the user accept the break in forge.yaml: accept: [{rule: %s, change: "<line above>", reason: "..."}]\n' "$c_id"
  return "$_oa_rc"
}

# OpenAPI clients generated by openapi-typescript, orval or @hey-api/openapi-ts: their
# files say so in their header, and a package.json script runs the generator.
FORGE_OPENAPI_MARK='auto-generated by (openapi-typescript|@hey-api/openapi-ts)|generated by orval'

FORGE_OPENAPI_GEN='"[^"]+"[[:space:]]*:[[:space:]]*"[^"]*(openapi-typescript|orval|openapi-ts)'

# forge_openapi_pairs: the "key": "value" pairs of a package.json on stdin that drive the
# generator (a script running it, its dependency version), one per line, whitespace aside.
forge_openapi_pairs() {
  awk '{ t = t $0 " " } END {
    while (match(t, /"[^"]*"[[:space:]]*:[[:space:]]*"[^"]*"/)) {
      p = substr(t, RSTART, RLENGTH); t = substr(t, RSTART + RLENGTH)
      if (p ~ /(openapi-typescript|orval|openapi-ts)/) { gsub(/[[:space:]]+/, " ", p); print p }
    }
  }'
}

# forge_openapi_spec <path>: an OpenAPI or Swagger document (JSON or YAML with an
# openapi / swagger version near its top).
forge_openapi_spec() {
  case $1 in *.json|*.yaml|*.yml) ;; *) return 1 ;; esac
  [ -f "$1" ] && head -c 4096 "$1" | grep -qE "[\"']?(openapi|swagger)[\"']?[[:space:]]*:[[:space:]]*[\"']?[0-9]"
}

# forge_openapi_generated <path>: a file whose header says one of the generators wrote it.
forge_openapi_generated() { [ -f "$1" ] && head -n 15 "$1" | grep -qiE "$FORGE_OPENAPI_MARK"; }

# forge_openapi_sync regenerate | hand-edits: compares the changed OpenAPI documents with
# the changed generated files. regenerate: a changed document needs a changed generated
# file. hand-edits: a changed generated file needs a changed document or generator config
# (orval.config.*, openapi-ts.config.*, redocly.*, or a package.json line naming the
# generator: its script or its dependency version), when the repository
# tracks a document (a client generated from a URL is not checked). Not with --all.
forge_openapi_sync() {
  [ "$mode" != --all ] || return 0
  _os_pkgs=$(git ls-files -- package.json '*/package.json')
  [ -n "$_os_pkgs" ] || return 0
  # shellcheck disable=SC2086
  _os_script=$(grep -hoE "$FORGE_OPENAPI_GEN" $_os_pkgs 2>/dev/null | head -n 1 | cut -d '"' -f 2)
  [ -n "$_os_script" ] || return 0
  forge_filter forge_openapi_spec < "$FORGE_TMP/existing" > "$FORGE_TMP/openapi.specs"
  forge_filter forge_openapi_generated < "$FORGE_TMP/existing" > "$FORGE_TMP/openapi.generated"
  case $1 in
    regenerate)
      [ -s "$FORGE_TMP/openapi.specs" ] && [ ! -s "$FORGE_TMP/openapi.generated" ] || return 0
      while IFS= read -r _os_f; do
        printf '%s: the OpenAPI document changed, but no generated client file did\n' "$_os_f"
      done < "$FORGE_TMP/openapi.specs"
      printf 'Regenerate the client (the "%s" script) and keep both changes together.\n' "$_os_script" ;;
    hand-edits)
      [ -s "$FORGE_TMP/openapi.generated" ] && [ ! -s "$FORGE_TMP/openapi.specs" ] || return 0
      grep -qE '(^|/)(orval\.config\.[a-z]+|openapi-ts\.config\.[a-z]+|redocly\.[a-z]+)$' "$FORGE_TMP/existing" && return 0
      # a package.json counts when its generator lines (script, dependency) changed
      while IFS= read -r _os_p; do
        case $_os_p in package.json|*/package.json) ;; *) continue ;; esac
        forge_openapi_pairs < "$_os_p" > "$FORGE_TMP/openapi.now" 2>/dev/null
        forge_openapi_pairs < "$FORGE_TMP/base/$_os_p" > "$FORGE_TMP/openapi.then" 2>/dev/null
        cmp -s "$FORGE_TMP/openapi.now" "$FORGE_TMP/openapi.then" || return 0
      done < "$FORGE_TMP/existing"
      [ -n "$(git ls-files -- '*.json' '*.yaml' '*.yml' | forge_filter forge_openapi_spec | head -n 1)" ] || return 0
      while IFS= read -r _os_f; do
        printf '%s: generated from the OpenAPI document (its header says so), but neither the document nor the generator config changed\n' "$_os_f"
      done < "$FORGE_TMP/openapi.generated"
      printf 'Change the OpenAPI document (or the server it is generated from), then regenerate with the "%s" script; never edit generated files by hand.\n' "$_os_script" ;;
  esac
  return 1
}
