# @module docker
# Dockerfiles: hadolint and Forge's own Dockerfile checks.

# Dockerfiles: pinned hadolint (hadolint-bin through uvx; its binary must match the
# upstream release's sha256), run once per check.sh run and shared by the rules.
FORGE_HADOLINT_VERSION="@@HADOLINT_VERSION@@"

FORGE_HADOLINT_SHA256='@@HADOLINT_SHAS@@'

# forge_dockerfiles: the in-scope files (forge_scope_files) in file role @dockerfile (§15.4).
forge_dockerfiles() {
  forge_scope_files | while IFS= read -r _df_f; do forge_role @dockerfile "$_df_f" && printf '%s\n' "$_df_f"; done
  return 0
}

# forge_hadolint_bin: prints the pinned hadolint binary (the hadolint-bin wheel's, through
# uvx) once its sha256 is the upstream release's; fails printing why.
forge_hadolint_bin() {
  command -v uvx >/dev/null 2>&1 || { printf 'uvx is needed to run hadolint'; return 1; }
  if ! _hb_bin=$(uvx -q --from "hadolint-bin==$FORGE_HADOLINT_VERSION" python -c 'import shutil; print(shutil.which("hadolint") or "")' 2>/dev/null) || [ -z "$_hb_bin" ]; then
    printf 'hadolint could not be installed (offline?)'; return 1
  fi
  _hb_plat=$(uname -s)-$(uname -m)
  _hb_want=$(printf '%s\n' "$FORGE_HADOLINT_SHA256" | awk -v p="$_hb_plat" '$1 == p { print $2 }')
  _hb_have=$(forge_sha256 < "$_hb_bin")
  if [ -z "$_hb_want" ]; then printf 'no upstream hadolint checksum for %s' "$_hb_plat"; return 1; fi
  if [ "$_hb_have" != "$_hb_want" ]; then printf 'the hadolint-bin %s binary does not match the upstream release (sha256 %s)' "$FORGE_HADOLINT_VERSION" "$_hb_have"; return 1; fi
  printf '%s' "$_hb_bin"
}

# forge_pinned_hadolint <args>: the pinned hadolint (forge_pinned): forge_hadolint_bin's, once
# per shell; 127 with why on stderr when it cannot be had.
forge_pinned_hadolint() {
  if [ -z "${FORGE_HADOLINT_BIN:-}" ]; then
    FORGE_HADOLINT_BIN=$(forge_hadolint_bin) || { printf 'forge: %s\n' "$FORGE_HADOLINT_BIN" >&2; FORGE_HADOLINT_BIN=''; return 127; }
  fi
  "$FORGE_HADOLINT_BIN" "$@"
}

# forge_hadolint <code>...: "path:line CODE: message" for the named hadolint codes.
forge_hadolint() {
  forge_dockerfiles > "$FORGE_TMP/dockerfiles"
  [ -s "$FORGE_TMP/dockerfiles" ] || return 0
  if ! cmp -s "$FORGE_TMP/dockerfiles" "$FORGE_TMP/hadolint.files" 2>/dev/null; then
    cp "$FORGE_TMP/dockerfiles" "$FORGE_TMP/hadolint.files"
    : > "$FORGE_TMP/hadolint.out"
    if ! FORGE_HADOLINT_BIN=$(forge_hadolint_bin); then
      printf 'skipped: %s\n' "$FORGE_HADOLINT_BIN" > "$FORGE_TMP/hadolint.skip"
      FORGE_HADOLINT_BIN=''
    else
      rm -f "$FORGE_TMP/hadolint.skip"
      printf 'ignored: []\n' > "$FORGE_TMP/hadolint.yaml"
      set -f
      # shellcheck disable=SC2046
      forge_do hadolint lint "$FORGE_TMP/hadolint.yaml" $(cat "$FORGE_TMP/dockerfiles") > "$FORGE_TMP/hadolint.out" 2>&1
      _fh_rc=$?
      set +f
      # --no-fail: 0 whatever it finds; 1 it could not run (an unreadable file). A Dockerfile it
      # cannot parse is "path:line:column message", exit 0: not checked, so not passed either
      if [ "$_fh_rc" != 0 ]; then
        printf 'skipped: hadolint could not run (%s)\n' "$(grep -v '^[[:space:]]*$' "$FORGE_TMP/hadolint.out" | tail -n 1)" > "$FORGE_TMP/hadolint.skip"
      elif _fh_parse=$(grep -E '^[^[:space:]]+:[0-9]+:[0-9]+ ' "$FORGE_TMP/hadolint.out" | head -n 1) && [ -n "$_fh_parse" ]; then
        printf 'skipped: hadolint could not parse %s\n' "$_fh_parse" > "$FORGE_TMP/hadolint.skip"
      fi
    fi
  fi
  if [ -f "$FORGE_TMP/hadolint.skip" ]; then printf 'forge:%s %s\n' "$c_id" "$(cat "$FORGE_TMP/hadolint.skip")"; return 0; fi
  _fh_codes=' '
  for _fh_c in "$@"; do _fh_codes="$_fh_codes$_fh_c "; done
  awk -v codes="$_fh_codes" '
    match($0, /^[^[:space:]]+:[0-9]+ [A-Z]+[0-9]+ [a-z]+: /) {
      split($0, w, " "); if (index(codes, " " w[2] " ") == 0) next
      msg = $0; sub(/^[^[:space:]]+ [^[:space:]]+ [a-z]+: /, "", msg)
      print w[1] " " w[2] ": " msg; bad = 1
    }
    END { exit bad }' "$FORGE_TMP/hadolint.out"
}

# forge_dockerfile_records <file>: one "line<TAB>instruction" per instruction,
# continuation lines joined, comments dropped.
forge_dockerfile_records() {
  awk '
    function flush() { if (cur != "") print start "\t" cur; cur = "" }
    {
      line = $0; sub(/\r$/, "", line)
      if (cur == "") { if (line ~ /^[[:space:]]*(#|$)/) next; start = NR }
      else if (line ~ /^[[:space:]]*#/) next
      if (match(line, /\\[[:space:]]*$/)) { cur = cur substr(line, 1, RSTART - 1) " "; next }
      cur = cur line; flush()
    }
    END { flush() }' "$1"
}

# forge_docker_nonroot: the final stage of each Dockerfile runs as a non-root USER,
# set in that stage or inherited from the named stage it builds on; images that run
# non-root by default (cgr.dev/, dhi.io/, *nonroot*, *unprivileged*, *rootless*) are exempt.
forge_docker_nonroot() {
  _dn_rc=0
  forge_dockerfiles > "$FORGE_TMP/dockerfiles"
  while IFS= read -r _dn_f; do
    [ -f "$_dn_f" ] || continue
    forge_dockerfile_records "$_dn_f" | awk -F '\t' -v f="$_dn_f" '
      {
        args = $2; sub(/^[[:space:]]*[A-Za-z]+[[:space:]]*/, "", args); instr = toupper($2); sub(/[[:space:]].*$/, "", instr)
        if (instr == "FROM") {
          n++; split(args, w, /[[:space:]]+/); i = 1; while (w[i] ~ /^--/) i++
          img[n] = w[i]; name[n] = ""; user[n] = ""
          if (tolower(w[i + 1]) == "as") name[n] = tolower(w[i + 2])
        } else if (instr == "USER" && n) { split(args, w, /[[:space:]]+/); user[n] = w[1] }
      }
      END {
        if (!n) exit 0
        s = n; u = user[s]; b = img[s]
        for (guard = 0; u == "" && guard < 50; guard++) {
          k = 0; for (j = 1; j < s; j++) if (name[j] != "" && name[j] == tolower(b)) k = j
          if (!k) break
          s = k; u = user[k]; b = img[k]
        }
        if (u == "") {
          if (b ~ /^(cgr\.dev|dhi\.io)\// || tolower(b) ~ /nonroot|unprivileged|rootless/) exit 0
          print f ":0 the final stage has no USER (it runs as root); add a non-root USER"; exit 1
        }
        if (u ~ /^(root|0)(:.*)?$/) { print f ":0 the final stage runs as USER " u "; use a non-root user"; exit 1 }
      }' || _dn_rc=1
  done < "$FORGE_TMP/dockerfiles"
  return "$_dn_rc"
}

# forge_docker_secrets: no ARG or ENV named like a secret (PASSWORD, SECRET, TOKEN,
# API_KEY, PRIVATE_KEY, CREDENTIALS); *_FILE names are fine.
forge_docker_secrets() {
  _ds_rc=0
  forge_dockerfiles > "$FORGE_TMP/dockerfiles"
  while IFS= read -r _ds_f; do
    [ -f "$_ds_f" ] || continue
    forge_dockerfile_records "$_ds_f" | awk -F '\t' -v f="$_ds_f" '
      {
        instr = toupper($2); sub(/[[:space:]].*$/, "", instr)
        if (instr != "ARG" && instr != "ENV") next
        args = $2; sub(/^[[:space:]]*[A-Za-z]+[[:space:]]*/, "", args)
        n = split(args, w, /[[:space:]]+/)
        for (i = 1; i <= n; i++) {
          name = w[i]; sub(/=.*$/, "", name)
          if (instr == "ENV" && i > 1 && w[1] !~ /=/) break
          if (toupper(name) ~ /(PASSWORD|SECRET|TOKEN|API_?KEY|PRIVATE_KEY|CREDENTIALS)/ && toupper(name) !~ /_FILE$/) {
            print f ":" $1 " " instr " " name " puts a secret in the image or its history; use RUN --mount=type=secret,id=..."; bad = 1
          }
          if (instr == "ARG") break
        }
      }
      END { exit bad }' || _ds_rc=1
  done < "$FORGE_TMP/dockerfiles"
  return "$_ds_rc"
}

# forge_docker_locked: RUN installs from the lock file (npm ci, bun install
# --frozen-lockfile, uv sync --locked or --frozen, pnpm install --frozen-lockfile,
# yarn --immutable or --frozen-lockfile) and never adds packages (bun add, uv add).
forge_docker_locked() {
  _dl_rc=0
  forge_dockerfiles > "$FORGE_TMP/dockerfiles"
  while IFS= read -r _dl_f; do
    [ -f "$_dl_f" ] || continue
    forge_dockerfile_records "$_dl_f" | awk -F '\t' -v f="$_dl_f" '
      function why(c,   t) {
        t = " " c " "
        if (c ~ /^npm (install|i|add)( |$)/ && t !~ / (-g|--global) /) return "use npm ci"
        if (c ~ /^bun (install|i)( |$)/ && t !~ / --frozen-lockfile /) return "add --frozen-lockfile"
        if (c ~ /^bun add( |$)/) return "add dependencies to package.json and bun.lock, not in the image"
        if (c ~ /^uv sync( |$)/ && t !~ / (--locked|--frozen) /) return "add --locked (or --frozen)"
        if (c ~ /^uv add( |$)/) return "add dependencies to pyproject.toml and uv.lock, not in the image"
        if (c ~ /^pnpm (install|i)( |$)/ && t !~ / --frozen-lockfile /) return "add --frozen-lockfile"
        if (c ~ /^yarn( install)?( -[^ ]*)*$/ && t !~ / (--immutable|--frozen-lockfile) /) return "add --immutable (Yarn 2+) or --frozen-lockfile"
        return ""
      }
      {
        instr = toupper($2); sub(/[[:space:]].*$/, "", instr)
        if (instr != "RUN") next
        args = $2; sub(/^[[:space:]]*[A-Za-z]+[[:space:]]*/, "", args)
        while (args ~ /^--[^[:space:]]+[[:space:]]/) sub(/^--[^[:space:]]+[[:space:]]+/, "", args)
        n = split(args, cmds, /&&|\|\||;/)
        for (i = 1; i <= n; i++) {
          c = cmds[i]; gsub(/[[:space:]]+/, " ", c); sub(/^ /, "", c); sub(/ $/, "", c)
          while (c ~ /^([A-Za-z_][A-Za-z0-9_]*=[^ ]* |sudo )/) sub(/^[^ ]+ /, "", c)
          r = why(c)
          if (r != "") { print f ":" $1 " " c ": " r; bad = 1 }
        }
      }
      END { exit bad }' || _dl_rc=1
  done < "$FORGE_TMP/dockerfiles"
  return "$_dl_rc"
}

# forge_docker_ignore: each Dockerfile has a .dockerignore (<Dockerfile>.dockerignore,
# the one in its directory, or the one at the repository root) that excludes .env files.
forge_docker_ignore() {
  _di_rc=0
  forge_dockerfiles > "$FORGE_TMP/dockerfiles"
  while IFS= read -r _di_f; do
    [ -f "$_di_f" ] || continue
    _di_d=$(dirname "$_di_f")
    _di_ig=''
    [ "$_di_d" = . ] && _di_d='' || _di_d="$_di_d/"
    for _di_c in "$_di_f.dockerignore" "$_di_d.dockerignore" .dockerignore; do
      [ -f "$_di_c" ] && { _di_ig=$_di_c; break; }
    done
    if [ -z "$_di_ig" ]; then
      printf '%s:0 has no .dockerignore (next to it or at the root); add one that excludes .env files\n' "$_di_f"; _di_rc=1
    elif ! grep -qE '^[[:space:]]*((\*\*/)?\.env(\*|\.\*)?|\*\.env|\*|\*\*)[[:space:]]*$' "$_di_ig"; then
      printf '%s:0 %s does not exclude .env files; add .env* to it\n' "$_di_f" "$_di_ig"; _di_rc=1
    fi
  done < "$FORGE_TMP/dockerfiles"
  return "$_di_rc"
}

# ---- Dockerfiles: layer order

# forge_docker_copy_order: in each stage of each in-scope Dockerfile (forge_dockerfiles), a RUN
# that installs dependencies (npm ci / install, bun install, uv sync, pnpm install, yarn,
# pip install -r) after a COPY or ADD of the whole build context (. or ./): every code change
# then reinstalls them. "path:line …" at the RUN.
forge_docker_copy_order() {
  forge_dockerfiles > "$FORGE_TMP/dockerfiles"
  while IFS= read -r _co_f; do
    [ -f "$_co_f" ] || continue
    forge_dockerfile_records "$_co_f" | awk -F '\t' -v f="$_co_f" '
      {
        instr = toupper($2); sub(/[[:space:]].*$/, "", instr)
        args = $2; sub(/^[[:space:]]*[A-Za-z]+[[:space:]]*/, "", args)
        if (instr == "FROM") { all = 0; next }
        if (instr == "COPY" || instr == "ADD") {
          while (args ~ /^--[^[:space:]]+[[:space:]]/) sub(/^--[^[:space:]]+[[:space:]]+/, "", args)
          n = split(args, a, /[[:space:]]+/)
          for (i = 1; i < n; i++) if (a[i] == "." || a[i] == "./") all = NR
          next
        }
        if (instr != "RUN" || !all) next
        while (args ~ /^--[^[:space:]]+[[:space:]]/) sub(/^--[^[:space:]]+[[:space:]]+/, "", args)
        n = split(args, cmds, /&&|\|\||;/)
        for (i = 1; i <= n; i++) {
          c = cmds[i]; gsub(/[[:space:]]+/, " ", c); sub(/^ /, "", c); sub(/ $/, "", c)
          while (c ~ /^([A-Za-z_][A-Za-z0-9_]*=[^ ]* |sudo )/) sub(/^[^ ]+ /, "", c)
          if (c ~ /^(npm (ci|install|i)|bun (install|i)|uv sync|pnpm (install|i)|yarn( install)?|pip3? install( .*)? (-r|--requirement)|uv pip install( .*)? (-r|--requirement))( |$)/ && c !~ /^npm (install|i) .*(-g|--global)/) {
            printf "%s:%s dependencies are installed after the whole build context is copied, so any code change reinstalls them; copy the manifest and lock file, install, then COPY . .\n", f, $1
            bad = 1; break
          }
        }
      }
      END { exit bad }'
  done < "$FORGE_TMP/dockerfiles"
}
