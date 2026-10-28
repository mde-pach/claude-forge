# @module javascript
# JavaScript packages: lock files, install scripts, the release cooldown, major versions of
# related packages.

# forge_bun_cooldown: bunfig.toml [install] minimumReleaseAge is at least FORGE_COOLDOWN_SECONDS.
forge_bun_cooldown() {
  _bc_v=$(forge_toml_get bunfig.toml install minimumReleaseAge)
  case $_bc_v in ''|*[!0-9]*) ;; *) [ "$_bc_v" -ge "$FORGE_COOLDOWN_SECONDS" ] && return 0 ;; esac
  printf 'bunfig.toml: [install] minimumReleaseAge is %s; set it to at least 259200 (3 days, in seconds) so a freshly published (possibly compromised) version is not installed. Packages you must update sooner go in minimumReleaseAgeExcludes.\n' "${_bc_v:-unset}"
  return 1
}

# forge_trusted_deps: no package.json gains trustedDependencies entries (each one
# lets that package run install scripts); compared with the base, with bun.
forge_trusted_deps() {
  _td_rc=0
  while IFS= read -r _td_f; do
    case $_td_f in package.json|*/package.json) ;; *) continue ;; esac
    _td_new=$(bun -e '
      const fs = require("fs");
      const read = (p) => { try { const v = JSON.parse(fs.readFileSync(p, "utf8")).trustedDependencies; return Array.isArray(v) ? v : []; } catch { return []; } };
      const before = new Set(read(process.argv[1]));
      console.log(read(process.argv[2]).filter((d) => !before.has(d)).join(" "));' "$FORGE_TMP/base/$_td_f" "$_td_f") || continue
    if [ -n "$_td_new" ]; then
      printf '%s: trustedDependencies adds %s (its install scripts would run); the user adds trusted packages, not Claude\n' "$_td_f" "$_td_new"
      _td_rc=1
    fi
  done < "$FORGE_TMP/existing"
  return "$_td_rc"
}

FORGE_LOCKFILE_LINT_VERSION="@@LOCKFILE_LINT_VERSION@@"

# forge_lockfile_lint: the pinned lockfile-lint (through bunx) on each in-scope package-lock.json
# (forge_scope_files): every package resolves from registry.npmjs.org, over https, with a
# sha512 integrity. "path:0 <package>: <problem> (expected …, actual …)" per finding; when it
# cannot run (offline, no bunx), the check is reported skipped.
forge_lockfile_lint() {
  _ll_rc=0
  forge_scope_files > "$FORGE_TMP/ll.files"
  while IFS= read -r _ll_f; do
    case $_ll_f in package-lock.json|*/package-lock.json) ;; *) continue ;; esac
    [ -f "$_ll_f" ] || continue
    command -v bunx >/dev/null 2>&1 || { printf 'forge:%s skipped: bunx is needed to run lockfile-lint\n' "$c_id"; return 0; }
    bunx "lockfile-lint@$FORGE_LOCKFILE_LINT_VERSION" --type npm --path "$_ll_f" --allowed-hosts npm --validate-https --validate-integrity > "$FORGE_TMP/ll.out" 2>&1 && continue
    if ! grep -q '^detected ' "$FORGE_TMP/ll.out"; then
      printf 'forge:%s skipped: lockfile-lint could not run (%s)\n' "$c_id" "$(grep -v '^[[:space:]]*$' "$FORGE_TMP/ll.out" | sed 's/\x1b\[[0-9;]*m//g' | grep -v '^[[:space:]]*$' | tail -n 1)"
      return 0
    fi
    awk -v f="$_ll_f" '
      /^detected / { what = $0; sub(/^detected /, "", what); pkg = what; sub(/^.* for package: /, "", pkg); sub(/ for package: .*$/, "", what); if (match(pkg, /-[0-9a-f]+$/) && RLENGTH == 41) pkg = substr(pkg, 1, RSTART - 1); next }
      /^[[:space:]]+expected: / { want = $0; sub(/^[[:space:]]+expected: /, "", want); next }
      /^[[:space:]]+actual: / { got = $0; sub(/^[[:space:]]+actual: /, "", got); printf "%s:0 %s: %s (expected %s, actual %s)\n", f, pkg, what, want, got }' "$FORGE_TMP/ll.out"
    _ll_rc=1
  done < "$FORGE_TMP/ll.files"
  return "$_ll_rc"
}

FORGE_JS_LOCKS='pnpm-lock.yaml package-lock.json npm-shrinkwrap.json yarn.lock bun.lock bun.lockb'

# forge_dep_major_match <package> <types-package>...: in each package.json of the check's
# scope ($FORGE_TMP/scope: the changed files, every file with --all) that declares
# <package> and a <types-package>, the lowest major version their ranges allow is the
# same (a types package of another major describes another API). The lowest major of a
# range is the smallest major its alternatives (||) start at: ^4.17.1, ~4.1, 4.x, >=4 <6
# and 4 || 5 are 4. A spec that names no lower version (*, latest, workspace:, catalog:,
# npm: aliases, git and file sources, <5) is reported as not compared, never passed.
forge_dep_major_match() {
  _dm_pkg=$1
  shift
  _dm_types=$(printf '%s ' "$@")
  _dm_rc=0
  while IFS= read -r _dm_f; do
    case $_dm_f in package.json|*/package.json) ;; *) continue ;; esac
    [ -f "$_dm_f" ] || continue
    forge_pkg_deps "$_dm_f" | awk -F '\t' -v pkg="$_dm_pkg" -v types="$_dm_types" -v f="$_dm_f" -v id="${c_id:-}" '
      function lowest(spec,   alts, n, i, a, m, best) {
        best = ""
        n = split(spec, alts, /[|][|]/)
        for (i = 1; i <= n; i++) {
          a = alts[i]
          gsub(/^[ \t]+|[ \t]+$/, "", a)
          if (a !~ /^(\^|~|>=|=|v)?[ \t]*v?[0-9]+([.]|[ \t]|$)/) return ""
          sub(/^(\^|~|>=|=)?[ \t]*v?/, "", a)
          m = a; sub(/[^0-9].*$/, "", m)
          if (best == "" || m + 0 < best + 0) best = m
        }
        return best
      }
      BEGIN { nt = split(types, ts, " "); for (i = 1; i <= nt; i++) want[ts[i]] = 1 }
      $2 == pkg { have = $3 }
      ($2 in want) { tspec[$2] = $3 }
      END {
        if (have == "") exit 0
        pm = lowest(have)
        for (t in tspec) {
          tm = lowest(tspec[t])
          if (pm == "" || tm == "") { printf "forge:%s skipped: %s: %s \"%s\" and %s \"%s\" are not both version ranges, so their majors were not compared\n", id, f, pkg, have, t, tspec[t]; continue }
          if (pm != tm) { printf "%s:0 %s \"%s\" (major %s) describes another API than %s \"%s\" (major %s): declare %s@^%s.0.0 (or the %s release you run)\n", f, t, tspec[t], tm, pkg, have, pm, t, pm, pkg; bad = 1 }
        }
        exit bad
      }' || _dm_rc=1
  done < "$FORGE_TMP/scope"
  return "$_dm_rc"
}

# forge_lockfile_updated: a package.json whose dependencies changed since the base
# needs a lock file whose changed lines name each changed dependency (added, removed or
# re-versioned), when the project has a pnpm, npm or Yarn lock file (next to it or at
# the root); a lock change that names none (a new blank line) does not count. bun.lock
# is left to typescript/bun/lock-sync (bun install --frozen-lockfile) when that check is
# active. Run with forge_once: at Stop it is reported once per finding (the package manager
# may not be able to run here); pre-commit and CI always fail on it.
forge_lockfile_updated() {
  _lu_rc=0
  while IFS= read -r _lu_f; do
    case $_lu_f in package.json|*/package.json) ;; *) continue ;; esac
    case /$_lu_f in */node_modules/*) continue ;; esac
    _lu_lock=''
    _lu_ifs=$IFS; IFS=' '
    for _lu_l in $FORGE_JS_LOCKS; do
      if [ -f "$(dirname "$_lu_f")/$_lu_l" ]; then _lu_lock=$(dirname "$_lu_f")/$_lu_l; break; fi
      if [ -f "$_lu_l" ]; then _lu_lock=$_lu_l; break; fi
    done
    IFS=$_lu_ifs
    _lu_lock=${_lu_lock#./}
    case $_lu_lock in
      '') continue ;;
      bun.lock|bun.lockb|*/bun.lock|*/bun.lockb) forge_check_active typescript/bun/lock-sync && continue ;;
    esac
    forge_pkg_deps "$FORGE_TMP/base/$_lu_f" > "$FORGE_TMP/deps.then"
    forge_pkg_deps "$_lu_f" > "$FORGE_TMP/deps.now"
    cmp -s "$FORGE_TMP/deps.then" "$FORGE_TMP/deps.now" && continue
    forge_diff_lines "$_lu_lock" > "$FORGE_TMP/lock.diff"
    awk -F '\t' '
      FILENAME == ARGV[1] { then[$1 " " $2] = $3; name[$1 " " $2] = $2; next }
      FILENAME == ARGV[2] { now[$1 " " $2] = $3; name[$1 " " $2] = $2; next }
      { diff = diff "\n" $0 }
      END {
        for (k in now) if (!(k in then)) w[k] = k " " now[k] " added"; else if (then[k] != now[k]) w[k] = k " " then[k] " -> " now[k]
        for (k in then) if (!(k in now)) w[k] = k " removed"
        for (k in w) if (!index(diff, name[k])) print w[k]
      }' "$FORGE_TMP/deps.then" "$FORGE_TMP/deps.now" "$FORGE_TMP/lock.diff" | sort > "$FORGE_TMP/deps.missing"
    [ -s "$FORGE_TMP/deps.missing" ] || continue
    _lu_what=$(head -n 5 "$FORGE_TMP/deps.missing" | awk 'NR > 1 { printf "; " } { printf "%s", $0 }')
    printf '%s: %s, but %s has no changed line for it\n' "$_lu_f" "$_lu_what" "$_lu_lock"
    _lu_rc=1
  done < "$FORGE_TMP/existing"
  [ "$_lu_rc" = 0 ] && return 0
  printf 'Run the package manager (bun install, pnpm install, npm install, yarn) to update the lock file; it is not edited by hand. If it cannot run here, tell the user.\n'
}
