# @module repository
# The repository: leaked secrets and credential files, large files, build artifacts, generated
# files, editorconfig.

# ---- secrets (secrets/*)

# Forge's gitleaks config: gitleaks' default rules without generic-api-key (it decides on
# identifier names and entropy), plus vendor key formats the defaults lack.
FORGE_GITLEAKS_CONFIG='title = "forge"

[extend]
useDefault = true
disabledRules = ["generic-api-key"]

[[rules]]
id = "langsmith-api-key"
description = "LangSmith API key"
regex = "\\blsv2_(?:pt|sk)_[a-f0-9]{32}_[a-f0-9]{10}\\b"
keywords = ["lsv2_"]

[[rules]]
id = "pinecone-api-key"
description = "Pinecone API key"
regex = "\\bpcsk_[A-Za-z0-9_]{40,}\\b"
keywords = ["pcsk_"]'

# forge_gitleaks: "path:line rule: description" for each secret the pinned gitleaks finds in
# the in-scope files (forge_scope_files), scanned as copies with Forge's config only: the
# project's .gitleaks.toml, GITLEAKS_CONFIG, .gitleaksignore and inline gitleaks:allow
# comments do not apply. Secrets are redacted; findings count on added lines (lint).
forge_gitleaks() {
  forge_release gitleaks _gl_bin || return 0
  _gl=$FORGE_TMP/gitleaks
  rm -rf "$_gl"
  mkdir -p "$_gl/scan" "$_gl/ignore" || return 1
  forge_scope_files > "$_gl/files"
  [ -s "$_gl/files" ] || return 0
  tar -cf - -T "$_gl/files" 2>/dev/null | ( cd "$_gl/scan" && tar -xf - 2>/dev/null )
  printf '%s\n' "$FORGE_GITLEAKS_CONFIG" > "$_gl/config.toml"
  printf '{{range .}}{{.File}}:{{.StartLine}} {{.RuleID}}: {{.Description}}\n{{end}}' > "$_gl/report.tmpl"
  if ! ( cd "$_gl/scan" && "$_gl_bin" dir --no-banner --no-color --redact -l error -c "$_gl/config.toml" -i "$_gl/ignore" --ignore-gitleaks-allow \
      --exit-code 0 --report-format template --report-template "$_gl/report.tmpl" --report-path "$_gl/report" . ) > "$_gl/err" 2>&1; then
    printf 'forge:%s skipped: gitleaks could not run (%s)\n' "$c_id" "$(grep -v '^[[:space:]]*$' "$_gl/err" | tail -n 1)"
    return 0
  fi
  sed 's|^\./||' "$_gl/report" | sort -u > "$_gl/found"
  [ -s "$_gl/found" ] || return 0
  cat "$_gl/found"
  return 1
}

# Credential containers: keystores and private keys by their file type, and the auth files
# of pip, netrc and git (secrets/no-credential-files).
FORGE_CREDENTIAL_FILES='(^|/)([^/]+\.(p12|pfx|jks|keystore|key)|id_(rsa|ecdsa|ed25519)|\.pypirc|\.netrc|\.git-credentials)$'

# forge_credential_files: "path:0 …" for each tracked or added credential container file,
# and "path:line …" for each .npmrc line giving _authToken, _auth or _password a literal
# value (a ${VAR} reference is fine); lint attribution keeps those of the change.
forge_credential_files() {
  _cf_rc=0
  { git ls-files 2>/dev/null; cat "$FORGE_TMP/existing"; } | sort -u > "$FORGE_TMP/cred.files"
  while IFS= read -r _cf_f; do
    [ -f "$_cf_f" ] || continue
    if printf '%s\n' "$_cf_f" | grep -qE "$FORGE_CREDENTIAL_FILES"; then
      printf '%s:0 is a credential file; keep it out of the repository (git rm --cached, .gitignore) and load it from a secret store\n' "$_cf_f"; _cf_rc=1
    fi
    case $_cf_f in
      .npmrc|*/.npmrc)
        grep -nE '(_authToken|_auth|_password)[[:space:]]*=[[:space:]]*[^$[:space:]]' "$_cf_f" 2>/dev/null \
          | awk -v f="$_cf_f" -F ':' '{ print f ":" $1 " sets a registry credential literally; use ${NPM_TOKEN}-style environment references"; bad = 1 } END { exit bad }' || _cf_rc=1 ;;
    esac
  done < "$FORGE_TMP/cred.files"
  return "$_cf_rc"
}

# forge_env_files: "path:0 …" for each tracked or added dotenv file (file role @env-files:
# .env, .env.*, not the templates of @env-template); then, when a template or a .gitignore is
# part of the change (or with --all), each template's directory whose .env git would not ignore.
forge_env_files() {
  _ef_rc=0
  { git ls-files 2>/dev/null; cat "$FORGE_TMP/existing"; } | sort -u > "$FORGE_TMP/env.files"
  while IFS= read -r _ef_f; do
    [ -f "$_ef_f" ] && forge_role @env-files "$_ef_f" || continue
    printf '%s:0 is a dotenv file; keep it out of the repository (git rm --cached, .gitignore) and commit a template (.env.example) instead\n' "$_ef_f"
    _ef_rc=1
  done < "$FORGE_TMP/env.files"
  if [ "$mode" != --all ]; then
    _ef_any=''
    while IFS= read -r _ef_f; do
      case $_ef_f in .gitignore|*/.gitignore) _ef_any=1 ;; *) forge_role @env-template "$_ef_f" && _ef_any=1 ;; esac
    done < "$FORGE_TMP/changed"
    [ -n "$_ef_any" ] || return "$_ef_rc"
  fi
  while IFS= read -r _ef_f; do
    [ -f "$_ef_f" ] && forge_role @env-template "$_ef_f" || continue
    _ef_d=$(dirname "$_ef_f")
    case $_ef_d in .) _ef_env=.env ;; *) _ef_env=$_ef_d/.env ;; esac
    git check-ignore -q --no-index -- "$_ef_env" 2>/dev/null && continue
    printf '%s is not git-ignored next to its template %s; add .env to .gitignore\n' "$_ef_env" "$_ef_f"
    _ef_rc=1
  done < "$FORGE_TMP/env.files"
  return "$_ef_rc"
}

# ---- git hygiene (git/*)

# forge_large_files <bytes>: each in-scope file (forge_scope_files) larger than <bytes> that is
# new or grew in the change, and that Git LFS does not store (its filter attribute is not lfs).
forge_large_files() {
  _lg_max=$1
  _lg_rc=0
  forge_scope_files > "$FORGE_TMP/large.files"
  while IFS= read -r _lg_f; do
    [ -f "$_lg_f" ] || continue
    _lg_n=$(wc -c < "$_lg_f" | tr -d ' ')
    [ "$_lg_n" -gt "$_lg_max" ] || continue
    if [ -f "$FORGE_TMP/base/$_lg_f" ] && [ "$(wc -c < "$FORGE_TMP/base/$_lg_f" | tr -d ' ')" -ge "$_lg_n" ]; then continue; fi
    [ "$(git check-attr filter -- "$_lg_f" 2>/dev/null | sed 's/^.*: filter: //')" = lfs ] && continue
    printf '%s:0 is %s KiB, over the %s KiB limit, and Git LFS does not store it: keep it out of the repository or track it with git lfs track\n' "$_lg_f" "$((_lg_n / 1024))" "$((_lg_max / 1024))"
    _lg_rc=1
  done < "$FORGE_TMP/large.files"
  return "$_lg_rc"
}

# forge_artifacts <@role>: each file of the change in the role (dependency, cache and build
# output: node_modules/, __pycache__/, *.pyc, .next/, … by Forge's defaults) or under a
# directory holding a tool's own marker (pyvenv.cfg: a virtualenv; CACHEDIR.TAG: a cache,
# per the Cache Directory Tagging spec), reported once per shallowest such directory
# ("<dir>/ is not ignored …"; a file the role names alone: "<file> is not ignored …"). The
# change already leaves out ignored files.
forge_artifacts() {
  _ba_role=$1
  while IFS= read -r _ba_f; do
    [ -n "$_ba_f" ] || continue
    _ba_d=''; _ba_rest=$_ba_f; _ba_hit=''
    while :; do
      case $_ba_rest in */*) ;; *) break ;; esac
      _ba_d=${_ba_d:+$_ba_d/}${_ba_rest%%/*}
      _ba_rest=${_ba_rest#*/}
      if [ -f "$_ba_d/pyvenv.cfg" ] || [ -f "$_ba_d/CACHEDIR.TAG" ] || forge_role "$_ba_role" "$_ba_d/"; then _ba_hit=$_ba_d/; break; fi
    done
    [ -n "$_ba_hit" ] || ! forge_role "$_ba_role" "$_ba_f" || _ba_hit=$_ba_f
    [ -z "$_ba_hit" ] || printf '%s\n' "$_ba_hit"
  done < "$FORGE_TMP/existing" | sort -u > "$FORGE_TMP/artifacts"
  [ -s "$FORGE_TMP/artifacts" ] || return 0
  while IFS= read -r _ba_p; do
    printf '%s is not ignored: it is dependency, cache or build output; add it to .gitignore (and git rm -r --cached it if it was committed)\n' "$_ba_p"
  done < "$FORGE_TMP/artifacts"
  return 1
}

# forge_generated_edits: each changed in-scope file ($FORGE_FILES) marked generated:
# linguist-generated in .gitattributes (git check-attr), or "@generated" or "Code generated …
# DO NOT EDIT." in its first 15 lines. Run as advice (forge_advise): a regeneration changes it too.
forge_generated_edits() {
  printf '%s\n' "$FORGE_FILES" | while IFS= read -r _ge_f; do
    [ -f "$_ge_f" ] || continue
    if [ "$(git check-attr linguist-generated -- "$_ge_f" 2>/dev/null | sed 's/^.*: linguist-generated: //')" = true ]; then _ge_why='.gitattributes marks it linguist-generated'
    elif head -n 15 "$_ge_f" | grep -qE '@generated|Code generated .* DO NOT EDIT\.'; then _ge_why='its header says it is generated'
    else continue
    fi
    printf '%s is generated (%s): change its source or generator and regenerate it, rather than editing it by hand\n' "$_ge_f" "$_ge_why"
  done
}

# forge_unformatted <list>: the paths of <list> that no format check of this setup covers
# (a formatter owns the others), one per line.
forge_unformatted() {
  ( _uf_i=1
    while [ "$_uf_i" -le "$FORGE_NCHECKS" ]; do
      forge_check_meta_"$_uf_i"
      [ "$c_stage" = format ] && forge_filter "forge_scope_$_uf_i" < "$1"
      _uf_i=$((_uf_i + 1))
    done ) | sort -u > "$FORGE_TMP/formatted"
  grep -vxF -f "$FORGE_TMP/formatted" "$1"
  return 0
}

# forge_editorconfig: when the repository has an .editorconfig, the pinned editorconfig-checker
# on the in-scope files (forge_scope_files) no format check formats, with a config of Forge's
# own (the project's .editorconfig-checker.json excludes and disables do not apply);
# "path:line:column: message" per violation.
forge_editorconfig() {
  [ -f .editorconfig ] || return 0
  forge_scope_files > "$FORGE_TMP/ec.scope"
  forge_unformatted "$FORGE_TMP/ec.scope" > "$FORGE_TMP/ec.files"
  [ -s "$FORGE_TMP/ec.files" ] || return 0
  forge_release editorconfig-checker _ec_bin || return 0
  printf '{}\n' > "$FORGE_TMP/ec.json"
  set -f
  # shellcheck disable=SC2046
  "$_ec_bin" -config "$FORGE_TMP/ec.json" -no-color -format gcc $(cat "$FORGE_TMP/ec.files") > "$FORGE_TMP/ec.out" 2>&1
  _ec_rc=$?
  set +f
  case $_ec_rc in 0) return 0 ;; 1) ;; *) printf 'forge:%s skipped: editorconfig-checker could not run (%s)\n' "$c_id" "$(tail -n 1 "$FORGE_TMP/ec.out")"; return 0 ;; esac
  grep -E '^[^[:space:]:]+:[0-9]+:[0-9]+: ' "$FORGE_TMP/ec.out" | sed 's/: error: /: /'
  return 1
}
