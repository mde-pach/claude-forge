# @module terraform
# Terraform and OpenTofu: validate, tflint, trivy.

# ---- Terraform / OpenTofu (terraform/*)

# forge_tf_dirs: the directories of the in-scope .tf / .tofu files (forge_scope_files), one per line.
forge_tf_dirs() {
  forge_scope_files | grep -E '\.(tf|tofu)$' | awk '{ d = $0; if (sub(/\/[^\/]*$/, "", d) == 0) d = "."; print d }' | sort -u
}

# forge_tf_validate: `validate` in each directory of a changed .tf / .tofu file (every one with
# --all), with the project's Terraform CLI (forge_terraform): "path:line: Error: summary" per
# error. A directory validate cannot judge before `init` (providers or modules not installed)
# is reported skipped: init downloads providers and configures the backend, which a check does not do.
forge_tf_validate() {
  _tv_rc=0
  forge_tf_dirs > "$FORGE_TMP/tf.dirs"
  # which CLI, decided from the repository root (forge_tf_pick looks at its files)
  forge_tf_pick
  while IFS= read -r _tv_d; do
    ( cd "$_tv_d" && forge_do terraform validate ) > "$FORGE_TMP/tf.out" 2>&1 && continue
    if grep -qE '(terraform|tofu) init' "$FORGE_TMP/tf.out"; then
      printf 'forge:%s skipped: %s is not initialised (%s init)\n' "$c_id" "$_tv_d" "$(forge_tf_bin)"; continue
    fi
    _tv_rc=1
    awk -v d="$_tv_d" '
      function flush() { if (err != "") print (loc != "" ? loc ": " : d ": ") "Error: " err; err = ""; loc = "" }
      /^Error: / { flush(); err = substr($0, 8); next }
      err != "" && loc == "" && match($0, /^  on [^ ]+ line [0-9]+/) {
        n = split(substr($0, 6, RLENGTH - 5), w, " "); loc = (d == "." ? "" : d "/") w[1] ":" w[3]; next
      }
      /^Warning: / { flush() }
      END { flush() }' "$FORGE_TMP/tf.out"
  done < "$FORGE_TMP/tf.dirs"
  return "$_tv_rc"
}

# tflint's provider rulesets, pinned: "<provider> <version>" per line (terraform/tflint).
FORGE_TFLINT_RULESETS='@@TFLINT_RULESETS@@'

# forge_tflint: the pinned tflint in each directory of a changed .tf / .tofu file (every one
# with --all), with a config of Forge's own (a project .tflint.hcl cannot disable rules): the
# terraform ruleset's recommended preset, plus the pinned aws / google / azurerm ruleset when a
# .terraform.lock.hcl of the repository lists that provider. "path:line:column: message (rule)".
# Rulesets are installed once (tflint --init, which checks their signatures) under
# ~/.cache/forge/tflint-plugins; when that fails (offline), the check is reported skipped.
forge_tflint() {
  forge_tf_dirs > "$FORGE_TMP/tfl.dirs"
  [ -s "$FORGE_TMP/tfl.dirs" ] || return 0
  forge_release tflint _tl_bin || return 0
  printf 'plugin "terraform" {\n  enabled = true\n  preset  = "recommended"\n}\n' > "$FORGE_TMP/tflint.hcl"
  git ls-files --cached --others --exclude-standard -- '*.terraform.lock.hcl' 2>/dev/null > "$FORGE_TMP/tfl.locks"
  printf '%s\n' "$FORGE_TFLINT_RULESETS" | while IFS=' ' read -r _tl_p _tl_v; do
    [ -n "$_tl_p" ] || continue
    while IFS= read -r _tl_l; do
      if grep -qE "provider \"registry\.(terraform\.io|opentofu\.org)/hashicorp/$_tl_p\"" "$_tl_l" 2>/dev/null; then
        printf 'plugin "%s" {\n  enabled = true\n  version = "%s"\n  source  = "github.com/terraform-linters/tflint-ruleset-%s"\n}\n' "$_tl_p" "$_tl_v" "$_tl_p"
        break
      fi
    done < "$FORGE_TMP/tfl.locks"
  done >> "$FORGE_TMP/tflint.hcl"
  # the rulesets are pinned release binaries (forge_release_bin: sha256-checked), laid out
  # where tflint looks for an installed plugin, in a directory of this run's: no tflint --init
  # (GitHub's API, rate-limited) and no shared plugin cache
  TFLINT_PLUGIN_DIR=$FORGE_TMP/tflint-plugins
  export TFLINT_PLUGIN_DIR
  sed -n 's/^  source  = "github.com\/\(.*\)"$/\1/p' "$FORGE_TMP/tflint.hcl" > "$FORGE_TMP/tfl.sources"
  while IFS= read -r _tl_s; do
    _tl_n=${_tl_s##*/}
    _tl_v=$(grep -B1 "source  = \"github.com/$_tl_s\"" "$FORGE_TMP/tflint.hcl" | sed -n 's/^  version = "\(.*\)"$/\1/p')
    if ! _tl_p=$(forge_release_bin "$_tl_n"); then
      printf 'forge:%s skipped: %s\n' "$c_id" "$_tl_p"
      return 0
    fi
    mkdir -p "$TFLINT_PLUGIN_DIR/github.com/$_tl_s/$_tl_v"
    ln -s "$_tl_p" "$TFLINT_PLUGIN_DIR/github.com/$_tl_s/$_tl_v/$_tl_n"
  done < "$FORGE_TMP/tfl.sources"
  _tl_rc=0
  while IFS= read -r _tl_d; do
    # rulesets are go-plugin processes reached through a Unix socket under $TMPDIR, whose path
    # macOS caps at 104 bytes: a long TMPDIR keeps every plugin from starting
    ( TMPDIR=/tmp; export TMPDIR; forge_do tflint lint "$_tl_d" --config "$FORGE_TMP/tflint.hcl" ) > "$FORGE_TMP/tfl.out" 2>&1
    # "path:line:col: Severity - message (rule)": every issue counts, whatever tflint's severity
    grep -E '^[^[:space:]:]+:[0-9]+:[0-9]+: (Error|Warning|Notice) - ' "$FORGE_TMP/tfl.out" | sed -E 's/: (Error|Warning|Notice) - /: /' > "$FORGE_TMP/tfl.found"
    if [ -s "$FORGE_TMP/tfl.found" ]; then cat "$FORGE_TMP/tfl.found"; _tl_rc=1
    # anything else it printed (a file or config it cannot parse: "path:l:c: error - …") is a run that could not lint
    elif grep -q '^Failed to\|^Error:\|: error - ' "$FORGE_TMP/tfl.out"; then
      printf 'forge:%s skipped: tflint could not run in %s (%s)\n' "$c_id" "$_tl_d" "$(grep -v '^[[:space:]]*$' "$FORGE_TMP/tfl.out" | tail -n 1)"
    fi
  done < "$FORGE_TMP/tfl.dirs"
  return "$_tl_rc"
}

# forge_trivy_config: the pinned trivy (its bundled checks, no update; an empty ignore file of
# Forge's, so .trivyignore does not apply) on each directory of a changed .tf / .tofu file,
# now and as it was at the base (the directory's files with the change undone): "path:0 ID
# title (resource, line N)" for each HIGH or CRITICAL Terraform misconfiguration (check,
# resource) the change introduced. Not with --all (no base to compare).
forge_trivy_config() {
  [ "$mode" != --all ] || return 0
  forge_tf_dirs > "$FORGE_TMP/tv.dirs"
  [ -s "$FORGE_TMP/tv.dirs" ] || return 0
  forge_release trivy _tr_bin || return 0
  : > "$FORGE_TMP/trivy.ignore"
  printf '{{- range . }}{{- $t := .Target }}{{- range .Misconfigurations }}{{ $t }}\t{{ .ID }}\t{{ .CauseMetadata.Resource }}\t{{ .CauseMetadata.StartLine }}\t{{ .Title }}\n{{ end }}{{ end }}' > "$FORGE_TMP/trivy.tpl"
  _tr_cache=${XDG_CACHE_HOME:-$HOME/.cache}/forge/trivy
  _tr_rc=0
  while IFS= read -r _tr_d; do
    # the directory as it was: its files now, with the change undone
    _tr_b=$FORGE_TMP/trivy-base/$_tr_d
    rm -rf "$_tr_b"; mkdir -p "$_tr_b"
    git ls-files --cached --others --exclude-standard -- "$_tr_d" 2>/dev/null \
      | awk -v d="$_tr_d" '{ p = $0; if (sub(/\/[^\/]*$/, "", p) == 0) p = "."; if (p == d) print }' > "$FORGE_TMP/tv.files"
    while IFS= read -r _tr_f; do [ -f "$_tr_f" ] && cp "$_tr_f" "$_tr_b/"; done < "$FORGE_TMP/tv.files"
    awk -v d="$_tr_d" '{ p = $0; if (sub(/\/[^\/]*$/, "", p) == 0) p = "."; if (p == d) print }' "$FORGE_TMP/changed" > "$FORGE_TMP/tv.changed"
    while IFS= read -r _tr_f; do
      rm -f "$_tr_b/${_tr_f##*/}"
      [ -f "$FORGE_TMP/base/$_tr_f" ] && cp "$FORGE_TMP/base/$_tr_f" "$_tr_b/"
    done < "$FORGE_TMP/tv.changed"
    for _tr_side in now "then"; do
      [ "$_tr_side" = now ] && _tr_dir=$_tr_d || _tr_dir=$_tr_b
      forge_do trivy scan --cache-dir "$_tr_cache" --ignorefile "$FORGE_TMP/trivy.ignore" --template "@$FORGE_TMP/trivy.tpl" "$_tr_dir" > "$FORGE_TMP/trivy.$_tr_side" 2> "$FORGE_TMP/trivy.err" \
        || { printf 'forge:%s skipped: trivy could not scan %s (%s)\n' "$c_id" "$_tr_d" "$(grep -v '^[[:space:]]*$' "$FORGE_TMP/trivy.err" | tail -n 1)"; continue 2; }
    done
    awk -F '\t' -v d="$_tr_d" 'FILENAME == ARGV[1] { old[$2 "\t" $3] = 1; next }
      NF >= 5 && !(($2 "\t" $3) in old) { printf "%s%s:0 %s %s (%s, line %s)\n", (d == "." ? "" : d "/"), $1, $2, $5, $3, $4; bad = 1 }
      END { exit bad }' "$FORGE_TMP/trivy.then" "$FORGE_TMP/trivy.now" || _tr_rc=1
  done < "$FORGE_TMP/tv.dirs"
  return "$_tr_rc"
}
