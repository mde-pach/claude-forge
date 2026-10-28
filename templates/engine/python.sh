# @module python
# Python libraries and packaging: public names and type stubs, uv's release cooldown.

# forge_py_public <file>: the public names of a Python module, one per line: the
# entries of __all__ when it has one, else its top-level def and class names
# without a leading underscore.
forge_py_public() {
  awk '
    /^__all__[ \t]*(:[^=]*)?\+?=/ { all = 1; inall = 1 }
    inall {
      s = $0
      while (match(s, /["\047][A-Za-z_][A-Za-z0-9_]*["\047]/)) { a[substr(s, RSTART + 1, RLENGTH - 2)] = 1; s = substr(s, RSTART + RLENGTH) }
      if ($0 ~ /[\])]/) inall = 0
      next
    }
    /^(async[ \t]+)?def[ \t]+[A-Za-z]/ || /^class[ \t]+[A-Za-z]/ {
      s = $0; sub(/^(async[ \t]+)?(def|class)[ \t]+/, "", s); match(s, /^[A-Za-z0-9_]+/); d[substr(s, 1, RLENGTH)] = 1
    }
    END { if (all) { for (k in a) print k } else for (k in d) print k }' "$1" 2>/dev/null | sort -u
}

# forge_py_module <path>: the module path inside its top-level package
# ("experimental/query_client" for logfire/experimental/query_client.py and for
# logfire-api/logfire_api/experimental/query_client.pyi): directories with an
# __init__.py or __init__.pyi count as packages, the topmost one is dropped.
forge_py_module() {
  _pm_d=$(dirname "$1"); _pm_m=$(basename "$1"); _pm_m=${_pm_m%.py}; _pm_m=${_pm_m%.pyi}
  _pm_top=''
  while [ "$_pm_d" != . ] && [ "$_pm_d" != / ] && { [ -f "$_pm_d/__init__.py" ] || [ -f "$_pm_d/__init__.pyi" ]; }; do
    [ -n "$_pm_top" ] && _pm_m="$_pm_top/$_pm_m"
    _pm_top=$(basename "$_pm_d"); _pm_d=$(dirname "$_pm_d")
  done
  printf '%s\n' "$_pm_m"
}

# forge_stub_sync: for each changed module ($FORGE_FILES) that gained public names
# (forge_py_public, compared with the base), each .pyi stub in the repository with
# the same module path (forge_py_module) must mention every new name.
forge_stub_sync() {
  git ls-files --cached --others --exclude-standard -- '*.pyi' 2>/dev/null | while IFS= read -r _ss_s; do
    [ -f "$_ss_s" ] && printf '%s\t%s\n' "$(forge_py_module "$_ss_s")" "$_ss_s"
  done > "$FORGE_TMP/stubs"
  [ -s "$FORGE_TMP/stubs" ] || return 0
  for _ss_f in $FORGE_FILES; do
    case $_ss_f in *.py) ;; *) continue ;; esac
    _ss_mod=$(forge_py_module "$_ss_f")
    awk -F '\t' -v m="$_ss_mod" '$1 == m { print $2 }' "$FORGE_TMP/stubs" > "$FORGE_TMP/stubs.for"
    [ -s "$FORGE_TMP/stubs.for" ] || continue
    forge_py_public "$_ss_f" > "$FORGE_TMP/names.now"
    : > "$FORGE_TMP/names.then"
    if [ -f "$FORGE_TMP/base/$_ss_f" ]; then
      { forge_py_public "$FORGE_TMP/base/$_ss_f"
        awk '/^(async[ \t]+)?(def|class)[ \t]+[A-Za-z_]/ { s = $0; sub(/^(async[ \t]+)?(def|class)[ \t]+/, "", s); match(s, /^[A-Za-z0-9_]+/); print substr(s, 1, RLENGTH) }' "$FORGE_TMP/base/$_ss_f"
      } | sort -u > "$FORGE_TMP/names.then"
    fi
    grep -vxF -f "$FORGE_TMP/names.then" "$FORGE_TMP/names.now" > "$FORGE_TMP/names.new"
    [ -s "$FORGE_TMP/names.new" ] || continue
    while IFS= read -r _ss_s; do
      _ss_miss=''
      while IFS= read -r _ss_n; do
        awk -v id="$_ss_n" '
          { s = $0
            while (i = index(s, id)) {
              b = substr(s, i - 1, 1); a = substr(s, i + length(id), 1)
              if ((i == 1 || b !~ /[A-Za-z0-9_]/) && a !~ /[A-Za-z0-9_]/) { f = 1; exit }
              s = substr(s, i + length(id))
            } }
          END { exit !f }' "$_ss_s" || _ss_miss="${_ss_miss:+$_ss_miss, }$_ss_n"
      done < "$FORGE_TMP/names.new"
      [ -n "$_ss_miss" ] && printf 'The stub %s does not have %s, new in %s; add it there too.\n' "$_ss_s" "$_ss_miss" "$_ss_f"
    done < "$FORGE_TMP/stubs.for"
  done
}

# forge_uv_cooldown: [tool.uv] (or uv.toml) exclude-newer is a relative duration of at
# least 3 days, and required-version asks for uv >= 0.9.17 (older uv ignores relative values).
forge_uv_cooldown() {
  if [ -f uv.toml ]; then _uc_f=uv.toml; _uc_s=''; else _uc_f=pyproject.toml; _uc_s=tool.uv; fi
  _uc_rc=0
  _uc_v=$(forge_toml_get "$_uc_f" "$_uc_s" exclude-newer)
  _uc_n=$(forge_duration_seconds "$_uc_v")
  if [ -z "$_uc_n" ] || [ "$_uc_n" -lt "$FORGE_COOLDOWN_SECONDS" ]; then
    printf '%s: [%s] exclude-newer is %s; set a relative duration of at least 3 days, e.g. exclude-newer = "7 days", so a freshly published (possibly compromised) version is not resolved.\n' "$_uc_f" "${_uc_s:-top level}" "${_uc_v:-unset}"
    _uc_rc=1
  fi
  _uc_r=$(forge_toml_get "$_uc_f" "$_uc_s" required-version)
  _uc_min=$(printf '%s' "$_uc_r" | sed -n -E 's/^[[:space:]]*(>=|==|~=|>)[[:space:]]*([0-9][0-9.]*).*$/\2/p')
  if [ -z "$_uc_min" ] || ! forge_version_ge "$_uc_min" 0.9.17; then
    printf '%s: [%s] required-version is %s; set required-version = ">=0.9.17" (older uv ignores a relative exclude-newer).\n' "$_uc_f" "${_uc_s:-top level}" "${_uc_r:-unset}"
    _uc_rc=1
  fi
  return "$_uc_rc"
}
