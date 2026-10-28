# @module env
# Environment variables: the .env template lists what the settings code reads.

# ---- Settings kept in sync with .env templates (python/env-example, typescript/env-example).

# forge_env_fields_py <file>: "line<TAB>VAR<TAB>required<TAB>class" for each field of a
# pydantic-settings BaseSettings subclass (env_prefix, alias, case_sensitive applied;
# required = no default). Fields with AliasChoices / AliasPath are left out.
forge_env_fields_py() {
  awk '
    function cls_of(l,   c) { c = l; sub(/^class[ \t]+/, "", c); sub(/[^A-Za-z0-9_].*$/, "", c); return c }
    { line = $0; sub(/\r$/, "", line) }
    line ~ /^class[ \t]/ { cls = (line ~ /\(.*BaseSettings.*\)[ \t]*:/) ? cls_of(line) : ""; body = ""; next }
    line ~ /^[^ \t#]/ { cls = ""; next }
    cls == "" || line ~ /^[ \t]*(#|$)/ { next }
    {
      ind = match(line, /[^ \t]/) - 1
      if (body == "") body = ind
    }
    FNR == NR {
      if (match(line, /env_prefix[ \t]*=[ \t]*["\047][^"\047]*["\047]/)) { p = substr(line, RSTART, RLENGTH); sub(/^[^"\047]*["\047]/, "", p); sub(/["\047]$/, "", p); pre[cls] = p }
      if (line ~ /case_sensitive[ \t]*=[ \t]*True/) cs[cls] = 1
      next
    }
    ind == body && line ~ /^[ \t]*[A-Za-z_][A-Za-z0-9_]*[ \t]*:/ {
      name = line; sub(/^[ \t]*/, "", name); sub(/[ \t]*:.*$/, "", name)
      rest = line; sub(/^[^:]*:/, "", rest)
      if (name == "model_config" || rest ~ /ClassVar|AliasChoices|AliasPath/) next
      key = pre[cls] name
      if (match(rest, /(validation_alias|alias)[ \t]*=[ \t]*["\047][^"\047]+["\047]/)) { key = substr(rest, RSTART, RLENGTH); sub(/^[^"\047]*["\047]/, "", key); sub(/["\047]$/, "", key) }
      if (!cs[cls]) key = toupper(key)
      req = 1
      if (match(rest, /[ \t]=([ \t]|$)/)) {
        v = substr(rest, RSTART + RLENGTH); sub(/^[ \t]+/, "", v)
        req = 0
        if (v ~ /^([A-Za-z_]+\.)?Field\(/) {
          sub(/^[^(]*\([ \t]*/, "", v)
          if (v ~ /^(\.\.\.|\))/) req = 1
          else if (v ~ /^[a-z_]+[ \t]*=/ && v !~ /default(_factory)?[ \t]*=/) req = 1
        }
      }
      print FNR "\t" key "\t" req "\t" cls
    }' "$1" "$1"
}

# forge_env_fields_ts <file>: the same for the keys of a Zod env schema (t3-env
# createEnv, or z.object(...) parsed from process.env): UPPER_CASE keys whose value is a
# z. chain on the same line; required = no .default(), .optional() or .catch().
forge_env_fields_ts() {
  awk '
    match($0, /^[ \t]*["\047]?[A-Z][A-Z0-9_]*["\047]?[ \t]*:[ \t]*z\./) {
      k = $0; sub(/^[ \t]*["\047]?/, "", k); sub(/[^A-Z0-9_].*$/, "", k)
      print FNR "\t" k "\t" ($0 ~ /\.(default|optional|catch)\(/ ? 0 : 1) "\tenv"
    }' "$1"
}

# forge_env_example <py|ts>: Stop and pre-commit, not --all. A settings field or env schema
# key added in the change appears in the .env template of the nearest directory that has
# one (.env.example, .env.template, .env.sample, .env.dist, .env.<name>.example): always
# when it has no default, and also with a default when the template already lists at
# least half of that class's variables. A template that lists none of them is not about it.
forge_env_example() {
  [ "$mode" = --all ] && return 0
  _ee_rc=0
  printf '%s\n' "$FORGE_FILES" > "$FORGE_TMP/env.files"
  while IFS= read -r _ee_f; do
    [ -f "$_ee_f" ] || continue
    case $1 in
      py) grep -q 'BaseSettings' "$_ee_f" || continue ;;
      ts) grep -qE 'createEnv\(|process\.env' "$_ee_f" && grep -q 'z\.' "$_ee_f" || continue ;;
    esac
    _ee_d=$(dirname "$_ee_f")
    while :; do
      _ee_t=$(ls -A "$_ee_d" 2>/dev/null | grep -E '^\.env(\.[A-Za-z0-9_-]+)?\.(example|template|sample|dist)$')
      [ -n "$_ee_t" ] || [ "$_ee_d" = . ] && break
      _ee_d=$(dirname "$_ee_d")
    done
    [ -n "$_ee_t" ] || continue
    _ee_names=''
    : > "$FORGE_TMP/env.keys"
    for _ee_x in $_ee_t; do
      [ "$_ee_d" = . ] && _ee_p=$_ee_x || _ee_p="$_ee_d/$_ee_x"
      _ee_names="${_ee_names:+$_ee_names, }$_ee_p"
      sed -n -E 's/^[[:space:]]*(export[[:space:]]+)?#?[[:space:]]*([A-Za-z_][A-Za-z0-9_]*)[[:space:]]*=.*/\2/p' "$_ee_p" >> "$FORGE_TMP/env.keys"
    done
    "forge_env_fields_$1" "$_ee_f" > "$FORGE_TMP/env.fields"
    awk -F '\t' -v f="$_ee_f" -v t="$_ee_names" '
      function found(k,   h) { if (k in have) return 1; for (h in have) if (index(h, k "_") == 1) return 1; return 0 }
      FILENAME == ARGV[1] { have[toupper($0)] = 1; next }
      { n++; ln[n] = $1; key[n] = $2; rq[n] = $3; g[n] = $4; tot[$4]++; if (found(toupper($2))) got[$4]++ }
      END {
        for (j = 1; j <= n; j++) {
          if (!got[g[j]] || found(toupper(key[j]))) continue
          if (rq[j] || got[g[j]] * 2 >= tot[g[j]]) {
            print f ":" ln[j] ": " key[j] " is not in " t "; add it there (a placeholder value is fine) so the next person to set up the project knows it exists"
            bad = 1
          }
        }
        exit bad
      }' "$FORGE_TMP/env.keys" "$FORGE_TMP/env.fields" || _ee_rc=1
  done < "$FORGE_TMP/env.files"
  return "$_ee_rc"
}
