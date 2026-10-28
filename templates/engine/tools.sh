# @module tools
# Running tools: the project's own (forge_exec_tool), system ones on PATH, Forge's pinned
# release binaries, engine helpers with a runtime; and small shared readers (TOML values,
# durations, versions, dependencies).

# forge_ratchet <runtime> <script>: the config ratchet. Runs .forge/helpers/<script>
# with the project's runtime on the changed files and their base copies; each printed
# line is a loosened setting, unless forge.yaml records it as accepted.
forge_ratchet() {
  [ -f ".forge/helpers/$2" ] || { printf 'forge:%s skipped: .forge/helpers/%s is missing (run forge apply)\n' "$c_id" "$2"; return 0; }
  set -f
  # shellcheck disable=SC2046
  "$1" ".forge/helpers/$2" "$FORGE_TMP/base" $(cat "$FORGE_TMP/existing") > "$FORGE_TMP/ratchet" 2>&1
  _fr_rc=$?
  set +f
  case $_fr_rc in
    0) return 0 ;;
    1) ;;
    *) printf 'forge:%s %s\n' "$c_id" "$(grep '^skipped:' "$FORGE_TMP/ratchet" | head -n 1 | grep . || printf 'skipped: %s exited with %s' "$1" "$_fr_rc")"; return 0 ;;
  esac
  while IFS= read -r _fr_l; do
    [ -f forge.yaml ] && grep -qF -- "$_fr_l" forge.yaml && continue
    printf '%s\n' "$_fr_l"
  done < "$FORGE_TMP/ratchet" > "$FORGE_TMP/ratchet.open"
  [ -s "$FORGE_TMP/ratchet.open" ] || return 0
  cat "$FORGE_TMP/ratchet.open"
  printf 'Restore the setting, or have the user accept the change in forge.yaml: accept: [{rule: %s, change: "<line above>", reason: "..."}]\n' "$c_id"
  return 1
}

# forge_helper <runtime> <script> [file...]: runs .forge/helpers/<script> with the
# project's runtime, as `<script> <FORGE_TMP> [file...]`
# (FORGE_TMP holds changed, existing, inbase and base/ from forge_change_set). Exit 0:
# nothing found; 1: findings printed ("path:line message"; a line also present in
# forge.yaml is accepted); 3: cannot run, its "skipped: ..." line is printed.
forge_helper() {
  [ -f ".forge/helpers/$2" ] || { printf 'forge:%s skipped: .forge/helpers/%s is missing (run forge apply)\n' "$c_id" "$2"; return 0; }
  _fh_rt=$1; _fh_s=$2; shift 2
  "$_fh_rt" ".forge/helpers/$_fh_s" "$FORGE_TMP" "$@" > "$FORGE_TMP/helper" 2>&1
  case $? in
    0) return 0 ;;
    1) ;;
    *) printf 'forge:%s %s\n' "$c_id" "$(grep '^skipped:' "$FORGE_TMP/helper" | head -n 1 | grep . || printf 'skipped: %s could not run' "$_fh_s")"; return 0 ;;
  esac
  _fh_rc=0
  while IFS= read -r _fh_l; do
    [ -f forge.yaml ] && grep -qF -- "$_fh_l" forge.yaml && continue
    printf '%s\n' "$_fh_l"; _fh_rc=1
  done < "$FORGE_TMP/helper"
  return "$_fh_rc"
}

# forge_toml_get <file> <section> <key>: the value of <key> in [<section>] ("" for
# the top level), quotes and comments removed; nothing when absent. One-line values only.
forge_toml_get() {
  awk -v want="$2" -v key="$3" '
    /^[[:space:]]*\[/ { s = $0; sub(/^[[:space:]]*\[[[:space:]]*/, "", s); sub(/[[:space:]]*\].*$/, "", s); next }
    s == want {
      line = $0; sub(/^[[:space:]]*/, "", line)
      if (substr(line, 1, length(key)) != key) next
      rest = substr(line, length(key) + 1)
      if (rest !~ /^[[:space:]]*=/) next
      sub(/^[[:space:]]*=[[:space:]]*/, "", rest)
      if (rest ~ /^"/) { sub(/^"/, "", rest); sub(/".*$/, "", rest) }
      else if (rest ~ /^\047/) { sub(/^\047/, "", rest); sub(/\047.*$/, "", rest) }
      else sub(/[[:space:]]*(#.*)?$/, "", rest)
      print rest; exit
    }' "$1" 2>/dev/null
}

# forge_duration_seconds <text>: a relative duration ("7 days", "1 week", "72 hours",
# P7D, P1W, PT72H) in seconds; nothing when it is not one.
forge_duration_seconds() {
  printf '%s\n' "$1" | awk '{
    v = tolower($0)
    if (v ~ /^p[0-9]+w$/) print substr(v, 2) * 604800
    else if (v ~ /^p[0-9]+d$/) print substr(v, 2) * 86400
    else if (v ~ /^pt[0-9]+h$/) print substr(v, 3) * 3600
    else if (v ~ /^[0-9]+[[:space:]]*(weeks?|days?|hours?)$/) {
      n = v + 0; u = v; sub(/^[0-9]+[[:space:]]*/, "", u)
      print n * (u ~ /^w/ ? 604800 : u ~ /^d/ ? 86400 : 3600)
    }
  }'
}

# forge_version_ge <a> <b>: succeeds when version a >= b (numeric dot-separated parts).
forge_version_ge() {
  awk -v a="$1" -v b="$2" 'BEGIN {
    na = split(a, x, "."); nb = split(b, y, ".")
    for (i = 1; i <= (na > nb ? na : nb); i++) { p = x[i] + 0; q = y[i] + 0; if (p > q) exit 0; if (p < q) exit 1 }
    exit 0 }'
}

# Forge's one release cooldown (3 days, in seconds): the minimum age of a version the
# package managers' cooldown settings must ask for, and of a newly added package (new_deps).
FORGE_COOLDOWN_SECONDS=259200

# forge_pkg_deps <package.json>: "section<TAB>name<TAB>spec" for each entry of
# dependencies, devDependencies and optionalDependencies (what lock files record;
# pnpm's lock does not record peerDependencies), sorted.
forge_pkg_deps() {
  [ -f "$1" ] || return 0
  awk '
    { buf = buf $0 "\n" }
    END {
      n = length(buf); depth = 0; top = ""; key = ""; i = 1
      while (i <= n) {
        c = substr(buf, i, 1)
        if (c == "\"") {
          j = i + 1; s = ""
          while (j <= n) {
            d = substr(buf, j, 1)
            if (d == "\\") { s = s substr(buf, j, 2); j += 2; continue }
            if (d == "\"") break
            s = s d; j++
          }
          i = j + 1; k = i
          while (k <= n && substr(buf, k, 1) ~ /[ \t\r\n]/) k++
          if (substr(buf, k, 1) == ":") { key = s; if (depth == 1) top = s; i = k + 1; continue }
          if (depth == 2 && top ~ /^(dependencies|devDependencies|optionalDependencies)$/) print top "\t" key "\t" s
          continue
        }
        if (c == "{" || c == "[") depth++
        else if (c == "}" || c == "]") depth--
        i++
      }
    }' "$1" | sort
}

# forge_check_active <rule-id>: the rule has a check in this check.sh.
forge_check_active() {
  (
    _ca_i=1
    while [ "$_ca_i" -le "${FORGE_NCHECKS:-0}" ]; do
      forge_check_meta_"$_ca_i"
      [ "$c_id" = "$1" ] && exit 0
      _ca_i=$((_ca_i + 1))
    done
    exit 1
  )
}

# ---- pinned release binaries (§13.5): tools with no PyPI or npm distribution, run from
# their GitHub release, pinned by version and by each platform's sha256.
# "<name> <version> <platform> <url> <sha256>" per line; platform = `uname -s`-`uname -m`.
FORGE_RELEASES='@@RELEASES@@'

# forge_release_bin <name>: prints the path of the pinned <name> binary. The first use on a
# machine downloads the release asset (curl or wget; in check.sh, outside the Bash
# sandbox), checks its sha256, unpacks the binary (.tar.gz, .zip, or the asset itself) and
# keeps it in ${XDG_CACHE_HOME:-~/.cache}/forge/bin/<name>-<version>-<platform>/<name>.
# Fails, printing why (one line), when the platform has no pinned asset, the download
# fails (offline) or the asset does not match its sha256: the caller reports the check
# skipped, never passed.
forge_release_bin() {
  _rb_plat=$(uname -s)-$(uname -m)
  _rb_row=$(printf '%s\n' "$FORGE_RELEASES" | awk -v n="$1" -v p="$_rb_plat" '$1 == n && $3 == p { print $2 " " $4 " " $5; exit }')
  if [ -z "$_rb_row" ]; then printf 'no pinned %s release binary for %s' "$1" "$_rb_plat"; return 1; fi
  _rb_ver=${_rb_row%% *}; _rb_url=${_rb_row#* }; _rb_sha=${_rb_url#* }; _rb_url=${_rb_url%% *}
  _rb_dir=${XDG_CACHE_HOME:-$HOME/.cache}/forge/bin/$1-$_rb_ver-$_rb_plat
  if [ -f "$_rb_dir/$1" ] && [ -x "$_rb_dir/$1" ]; then printf '%s' "$_rb_dir/$1"; return 0; fi
  mkdir -p "$_rb_dir" 2>/dev/null && _rb_tmp=$(mktemp -d "$_rb_dir/.fetch.XXXXXX" 2>/dev/null) \
    || { printf 'cannot write the cache directory %s' "$_rb_dir"; return 1; }
  if command -v curl >/dev/null 2>&1; then curl -fsSL --retry 2 --connect-timeout 15 -o "$_rb_tmp/asset" "$_rb_url" 2>/dev/null
  elif command -v wget >/dev/null 2>&1; then wget -q -T 15 -O "$_rb_tmp/asset" "$_rb_url" 2>/dev/null
  else rm -rf "$_rb_tmp"; printf 'curl or wget is needed to fetch %s %s' "$1" "$_rb_ver"; return 1
  fi || { rm -rf "$_rb_tmp"; printf 'could not download %s %s (offline?)' "$1" "$_rb_ver"; return 1; }
  _rb_have=$(forge_sha256 < "$_rb_tmp/asset")
  if [ "$_rb_have" != "$_rb_sha" ]; then
    rm -rf "$_rb_tmp"; printf 'the downloaded %s %s does not match its pinned sha256 (got %s)' "$1" "$_rb_ver" "$_rb_have"; return 1
  fi
  case $_rb_url in
    *.tar.gz|*.tgz) ( cd "$_rb_tmp" && tar -xzf asset "$1" ) >/dev/null 2>&1 ;;
    *.zip)
      if ! command -v unzip >/dev/null 2>&1; then rm -rf "$_rb_tmp"; printf 'unzip is needed to unpack %s %s' "$1" "$_rb_ver"; return 1; fi
      ( cd "$_rb_tmp" && unzip -q -o asset "$1" ) >/dev/null 2>&1 ;;
    *) mv "$_rb_tmp/asset" "$_rb_tmp/$1" ;;
  esac
  if [ ! -f "$_rb_tmp/$1" ] || ! chmod 755 "$_rb_tmp/$1" || ! mv -f "$_rb_tmp/$1" "$_rb_dir/$1"; then
    rm -rf "$_rb_tmp"; printf 'could not unpack %s %s' "$1" "$_rb_ver"; return 1
  fi
  rm -rf "$_rb_tmp"
  printf '%s' "$_rb_dir/$1"
}

# forge_release <name> <var>: sets <var> to the pinned binary's path; when it cannot be had,
# prints "forge:<id> skipped: <why>" for the current check and fails.
forge_release() {
  if _rl_bin=$(forge_release_bin "$1"); then eval "$2=\$_rl_bin"; return 0; fi
  printf 'forge:%s skipped: %s\n' "$c_id" "$_rl_bin"
  return 1
}

# forge_exec_tool <name> <args>: runs the project's own <name>; 127 when absent.
forge_exec_tool() {
  forge_tool_lookup "$1"
  _fx_cmd=$FORGE_TOOL_CMD
  [ -n "$_fx_cmd" ] || { printf 'forge: %s not installed\n' "$1" >&2; return 127; }
  shift
  _fx_ifs=$IFS
  IFS=' '
  # shellcheck disable=SC2086
  set -- $_fx_cmd "$@"
  IFS=$_fx_ifs
  "$@"
}

# forge_has_tool <name>[@<dir>]: system tools (runtimes, infrastructure CLIs) via PATH,
# others via tool() (in workspace package <dir> when given).
forge_has_tool() {
  case $1 in
    *@*) ( _fh_root=$(pwd); cd "${1#*@}" 2>/dev/null && FORGE_WS_ROOT=$_fh_root forge_has_tool "${1%%@*}" ) ;;
    uv|uvx|bun|docker) command -v "$1" >/dev/null 2>&1 ;;
    tofu|helm|kubectl|npm|pnpm) forge_on_path "$1" ;;
    # Yarn 1 is another tool (a .yarnrc.yml project needs Yarn 2 or later): absent then
    yarn) forge_on_path yarn && case $(yarn --version 2>/dev/null) in ''|0.*|1.*) false ;; esac ;;
    terraform) forge_on_path "$(forge_tf_bin)" ;;
    kustomize) forge_on_path kustomize || forge_on_path kubectl ;;
    # Forge's pinned ty runs through uvx (§15.2)
    ty) command -v uvx >/dev/null 2>&1 ;;
    *) [ -n "$(tool "$1")" ] ;;
  esac
}

# forge_tool_label <name>: how a missing tool is named: the binary the engine would run.
forge_tool_label() {
  case $1 in
    terraform) forge_tf_bin ;;
    kustomize) printf 'kustomize or kubectl' ;;
    yarn) printf 'yarn 2 or later' ;;
    *) printf '%s' "$1" ;;
  esac
}

# forge_on_path <name>: an executable <name> is on PATH (not a shell function of that name).
forge_on_path() {
  _op_ifs=$IFS
  IFS=:
  for _op_d in $PATH; do
    if [ -n "$_op_d" ] && [ -f "$_op_d/$1" ] && [ -x "$_op_d/$1" ]; then IFS=$_op_ifs; return 0; fi
  done
  IFS=$_op_ifs
  return 1
}

# forge_tf_bin: the project's Terraform CLI: tofu when it uses OpenTofu (a *.tofu file, or
# a .terraform.lock.hcl naming registry.opentofu.org providers), else terraform.
forge_tf_bin() {
  if [ -z "${_tf_bin:-}" ]; then
    _tf_bin=terraform
    _tf_files=$(git ls-files --cached --others --exclude-standard -- '*.tofu' '*.terraform.lock.hcl' 2>/dev/null)
    while IFS= read -r _tf_f; do
      case $_tf_f in
        *.tofu) _tf_bin=tofu; break ;;
        *.terraform.lock.hcl) grep -q 'registry\.opentofu\.org/' "$_tf_f" 2>/dev/null && { _tf_bin=tofu; break; } ;;
      esac
    done <<EOF
$_tf_files
EOF
  fi
  printf '%s' "$_tf_bin"
}

# forge_terraform <args>: the project's Terraform CLI (forge_tf_bin) from PATH.
forge_terraform() { command "$(forge_tf_bin)" "$@"; }

# forge_kustomize <args>: kustomize from PATH, else `kubectl kustomize`.
forge_kustomize() {
  if forge_on_path kustomize; then command kustomize "$@"; else command kubectl kustomize "$@"; fi
}

# forge_missing_tool: prints the first tool of $c_tools that is absent (in a package
# of $c_dirs: "<tool> (<dir>)").
forge_missing_tool() {
  _fm_ifs=$IFS
  IFS=' '
  for _fm_t in $c_tools; do
    if [ -n "${c_dirs:-}" ]; then
      for _fm_d in $c_dirs; do
        if ! forge_has_tool "$_fm_t@$_fm_d"; then IFS=$_fm_ifs; printf '%s (%s)' "$(forge_tool_label "$_fm_t")" "$_fm_d"; return 0; fi
      done
    elif ! forge_has_tool "$_fm_t"; then IFS=$_fm_ifs; forge_tool_label "$_fm_t"; return 0; fi
  done
  IFS=$_fm_ifs
  return 1
}

# forge_db_url: DATABASE_URL from the environment, then .env, then alembic.ini.
forge_db_url() {
  if [ -n "${DATABASE_URL:-}" ]; then printf '%s' "$DATABASE_URL"; return 0; fi
  if [ -f .env ]; then
    _fd_url=$(sed -n 's/^[[:space:]]*\(export[[:space:]][[:space:]]*\)\{0,1\}DATABASE_URL[[:space:]]*=[[:space:]]*//p' .env \
      | tail -n 1 | sed -e 's/[[:space:]]*$//' -e 's/^"\(.*\)"$/\1/' -e "s/^'\(.*\)'\$/\1/")
    if [ -n "$_fd_url" ]; then printf '%s' "$_fd_url"; return 0; fi
  fi
  if [ -f alembic.ini ]; then
    sed -n 's/^[[:space:]]*sqlalchemy\.url[[:space:]]*=[[:space:]]*//p' alembic.ini | head -n 1 | sed 's/[[:space:]]*$//'
  fi
}

# forge_local_db: succeeds when the configured database is on this machine
# (localhost, 127.0.0.1, ::1, empty host or a Unix socket).
forge_local_db() {
  _fl_url=$(forge_db_url)
  [ -n "$_fl_url" ] || return 1
  case $_fl_url in *host=/*|*host=%2[Ff]*) return 0 ;; esac
  _fl_host=$(printf '%s' "$_fl_url" | sed -e 's|^[A-Za-z][A-Za-z0-9+.-]*://||' -e 's|[/?].*$||' -e 's|^.*@||')
  case $_fl_host in
    \[*) _fl_host=$(printf '%s' "$_fl_host" | sed 's|^\[\([^]]*\)\].*$|\1|') ;;
    *) _fl_host=$(printf '%s' "$_fl_host" | sed 's|:.*$||') ;;
  esac
  case $_fl_host in ''|localhost|127.0.0.1|::1|%2[Ff]*) return 0 ;; esac
  return 1
}

# forge_mentions <file> <name>: <name> appears as a word in <file>, outside comment lines.
forge_mentions() {
  grep -v '^[[:space:]]*#' "$1" 2>/dev/null | grep -qE "(^|[^A-Za-z0-9_])$2([^A-Za-z0-9_]|\$)"
}
