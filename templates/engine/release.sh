# @module release
# Releases: hand-edited versions and release notes.

# ---- release tooling

# forge_json_version <package.json>: its top-level "version" (the first one at depth 1).
forge_json_version() {
  awk '{ buf = buf $0 "\n" } END {
    n = length(buf); d = 0; i = 1
    while (i <= n) {
      c = substr(buf, i, 1)
      if (c == "\"") {
        j = i + 1; s = ""
        while (j <= n) { e = substr(buf, j, 1); if (e == "\\") { s = s substr(buf, j, 2); j += 2; continue } if (e == "\"") break; s = s e; j++ }
        i = j + 1; k = i
        while (k <= n && substr(buf, k, 1) ~ /[ \t\r\n]/) k++
        if (d == 1 && substr(buf, k, 1) == ":" && s == "version") {
          k++; while (k <= n && substr(buf, k, 1) ~ /[ \t\r\n]/) k++
          if (substr(buf, k, 1) == "\"") { v = substr(buf, k + 1); sub(/".*$/, "", v); print v }
          exit
        }
        continue
      }
      if (c == "{" || c == "[") d++; else if (c == "}" || c == "]") d--
      i++
    }
  }' "$1" 2>/dev/null
}

# forge_manifest_version <file>: the package version a package.json or pyproject.toml ([project]) declares.
forge_manifest_version() {
  case $1 in
    *package.json) forge_json_version "$1" ;;
    *pyproject.toml) forge_toml_get "$1" project version ;;
  esac
}

# forge_version_changed: each changed package.json / pyproject.toml present in the base whose
# declared version differs from the base's: the release tooling sets versions.
forge_version_changed() {
  while IFS= read -r _vc_f; do
    case $_vc_f in package.json|*/package.json|pyproject.toml|*/pyproject.toml) ;; *) continue ;; esac
    case /$_vc_f in */node_modules/*) continue ;; esac
    [ -f "$_vc_f" ] && [ -f "$FORGE_TMP/base/$_vc_f" ] || continue
    _vc_was=$(forge_manifest_version "$FORGE_TMP/base/$_vc_f")
    _vc_now=$(forge_manifest_version "$_vc_f")
    [ "$_vc_was" != "$_vc_now" ] || continue
    printf '%s: the version changed (%s -> %s); the release tooling sets versions, so revert it unless this change is the release itself\n' "$_vc_f" "${_vc_was:-none}" "${_vc_now:-none}"
  done < "$FORGE_TMP/changed"
}

# forge_release_notes <@role>: Changesets (.changeset/config.json): a changed file of a
# package whose package.json (the nearest one above it) is not "private": true, with no
# .changeset/*.md added in the change. towncrier ([tool.towncrier] directory in towncrier.toml
# or pyproject.toml): a changed .py file outside <@role> (tests) and that directory, with no
# file added there.
forge_release_notes() {
  _rn_role=$1
  awk -F '\t' '$2 == "*" { print $1 }' "$FORGE_TMP/added" | sort -u > "$FORGE_TMP/new.files"
  if [ -f .changeset/config.json ]; then
    if ! grep -E '^\.changeset/[^/]+\.md$' "$FORGE_TMP/new.files" | grep -qvxF .changeset/README.md; then
      while IFS= read -r _rn_f; do
        case $_rn_f in .changeset/*|*/node_modules/*|node_modules/*) continue ;; esac
        _rn_d=$(dirname "$_rn_f")
        while [ "$_rn_d" != . ] && [ ! -f "$_rn_d/package.json" ]; do _rn_d=$(dirname "$_rn_d"); done
        [ -f "$_rn_d/package.json" ] || continue
        grep -qE '"private"[[:space:]]*:[[:space:]]*true' "$_rn_d/package.json" && continue
        printf '%s\n' "$_rn_d"
      done < "$FORGE_TMP/changed" | sort -u > "$FORGE_TMP/rn.pkgs"
      if [ -s "$FORGE_TMP/rn.pkgs" ]; then
        printf 'The change touches published packages (%s) and adds no changeset; add one (.changeset/<name>.md, e.g. bunx changeset) describing it, or say why it needs no release note\n' "$(awk 'NR > 1 { printf ", " } { printf "%s", $0 }' "$FORGE_TMP/rn.pkgs")"
      fi
    fi
  fi
  _rn_dir=''
  [ -f towncrier.toml ] && _rn_dir=$(forge_toml_get towncrier.toml tool.towncrier directory)
  [ -z "$_rn_dir" ] && [ -f pyproject.toml ] && _rn_dir=$(forge_toml_get pyproject.toml tool.towncrier directory)
  if [ -n "$_rn_dir" ]; then
    _rn_dir=${_rn_dir%/}; _rn_dir=${_rn_dir#./}
    grep -q "^$_rn_dir/" "$FORGE_TMP/new.files" && return 0
    while IFS= read -r _rn_f; do
      case $_rn_f in "$_rn_dir"/*) continue ;; *.py) ;; *) continue ;; esac
      [ -f "$_rn_f" ] && ! forge_role "$_rn_role" "$_rn_f" && { printf '%s\n' "$_rn_f"; break; }
    done < "$FORGE_TMP/changed" > "$FORGE_TMP/rn.src"
    [ -s "$FORGE_TMP/rn.src" ] && printf 'The change touches source code (%s) and adds no news fragment; add one in %s/ (towncrier create), or say why it needs no release note\n' "$(cat "$FORGE_TMP/rn.src")" "$_rn_dir"
  fi
  return 0
}
