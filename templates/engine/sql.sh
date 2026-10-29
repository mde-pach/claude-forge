# @module sql
# Databases and migrations: sqlfluff, Squawk, Alembic, Prisma, Drizzle, TypeORM, applied
# migrations.

# ---- SQL style: sqlfluff

# forge_sqlfluff [file...]: when the project configures sqlfluff (.sqlfluff, [tool.sqlfluff] in
# pyproject.toml, [sqlfluff] in setup.cfg or tox.ini; the dialect lives there), the project's
# sqlfluff lint on the given .sql files (all of the project's with none); "path:line:column:
# CODE: message" per violation. Not configured: nothing to check.
forge_sqlfluff() {
  [ -f .sqlfluff ] || grep -qE '^\[tool\.sqlfluff(\]|\.)' pyproject.toml 2>/dev/null || grep -qE '^\[sqlfluff(\]|:)' setup.cfg tox.ini 2>/dev/null || return 0
  forge_has_tool sqlfluff || { forge_for_agent || printf 'forge:%s skipped: sqlfluff not installed\n' "$c_id"; return 0; }
  for _sf_f in "$@"; do shift; case $_sf_f in *.sql) [ -f "$_sf_f" ] && set -- "$@" "$_sf_f" ;; esac; done
  [ "$#" -gt 0 ] || [ "$mode" = --all ] || return 0
  [ "$#" -gt 0 ] || set -- .
  set -f
  forge_do sqlfluff lint "$@" > "$FORGE_TMP/sqlfluff" 2>&1
  _sf_rc=$?
  set +f
  case $_sf_rc in 0) return 0 ;; 1) ;; *) forge_for_agent || printf 'forge:%s skipped: sqlfluff could not run (%s)\n' "$c_id" "$(grep -v '^[[:space:]]*$' "$FORGE_TMP/sqlfluff" | tail -n 1)"; return 0 ;; esac
  awk '/^::(error|warning|notice) / {
      meta = $0; sub(/^::[a-z]+ /, "", meta); msg = meta; sub(/^[^:]*::/, "", msg); sub(/::.*$/, "", meta)
      f = meta; sub(/^.*file=/, "", f); sub(/,.*$/, "", f); sub(/^\.\//, "", f)
      l = meta; sub(/^.*line=/, "", l); sub(/,.*$/, "", l)
      c = meta; if (c ~ /col=/) { sub(/^.*col=/, "", c); sub(/,.*$/, "", c) } else c = 1
      print f ":" l ":" c ": " msg; bad = 1 }
    END { exit !bad }' "$FORGE_TMP/sqlfluff" || { printf 'forge:%s failed\n' "$c_id"; cat "$FORGE_TMP/sqlfluff"; }
  return 1
}

# Migration linting with Squawk: only the changes that break the running version of
# the application during a deploy fail (renames, type changes, new required columns,
# DROP DATABASE); Squawk's other advice is not enforced.
FORGE_SQUAWK_VERSION="@@SQUAWK_VERSION@@"

FORGE_SQUAWK_BLOCKING='renaming-column|renaming-table|changing-column-type|adding-required-field|adding-not-nullable-field|ban-drop-database'

# forge_pinned_squawk <args>: Forge's pinned Squawk (forge_pinned): through bunx, else uvx.
# "bunx squawk-cli@<v>" runs the package's one binary from bunx's cache; with
# "-p squawk-cli@<v> squawk" bunx installs the package again at every run, and
# runs at the same time then kill each other's install.
forge_pinned_squawk() {
  if command -v bunx >/dev/null 2>&1; then bunx "squawk-cli@$FORGE_SQUAWK_VERSION" "$@"
  else uvx -q --from "squawk-cli==$FORGE_SQUAWK_VERSION" squawk "$@"
  fi
}

# forge_squawk [--stdin-filepath <path>] [file...]: the pinned Squawk through bunx or uvx,
# on the files (or stdin, reported as <path>); gcc-style output in $FORGE_TMP/squawk. Exit
# status 0 when Squawk ran (with or without findings). The project cannot silence it: Squawk
# runs with Forge's own config (an empty .squawk.toml: the project's excluded_rules and
# excluded_paths do not apply) on copies of the SQL whose "squawk-ignore" comments are
# rewritten into plain comments (same lines and columns), from the copies' directory so the
# reported paths are the repository's.
forge_squawk() {
  _sq_src=$FORGE_TMP/squawk.src
  rm -rf "$_sq_src"; mkdir -p "$_sq_src" || return 2
  : > "$FORGE_TMP/squawk.toml"
  if [ "${1:-}" = --stdin-filepath ]; then
    forge_squawk_unignore > "$_sq_src/.stdin"
  else
    for _sq_f in "$@"; do
      mkdir -p "$_sq_src/$(dirname "$_sq_f")" && forge_squawk_unignore < "$_sq_f" > "$_sq_src/$_sq_f" || return 2
    done
  fi
  if ! command -v bunx >/dev/null 2>&1 && ! command -v uvx >/dev/null 2>&1; then
    printf 'bunx or uvx is needed\n' > "$FORGE_TMP/squawk"
    return 2
  fi
  ( cd "$_sq_src" && if [ -f .stdin ]; then forge_do squawk lint "$FORGE_TMP/squawk.toml" "$@" < .stdin; else forge_do squawk lint "$FORGE_TMP/squawk.toml" "$@"; fi ) > "$FORGE_TMP/squawk" 2>&1
  _sq_rc=$?
  # Squawk exits 1 whenever it warns, which is almost always
  [ "$_sq_rc" -eq 0 ] || grep -qE ':[0-9]+:[0-9]+: (warning|error): ' "$FORGE_TMP/squawk"
}

# forge_squawk_unignore: stdin with Squawk's ignore directives (squawk-ignore,
# squawk-ignore-file) turned into plain comments of the same length.
forge_squawk_unignore() { sed 's/squawk-ignore/squawk-IGNORE/g'; }

# forge_squawk_blocking: prints the blocking findings of the last forge_squawk run
# (Squawk counts lines and columns from 0; printed from 1); fails if any.
forge_squawk_blocking() {
  grep -E ": (warning|error): ($FORGE_SQUAWK_BLOCKING) " "$FORGE_TMP/squawk" \
    | sed -E 's/^(.*):([0-9]+):([0-9]+): /\1 \2 \3 /' \
    | awk '{ f = $1; l = $2 + 1; c = $3 + 1; $1 = $2 = $3 = ""; sub(/^ +/, ""); print f ":" l ":" c ": " $0 }' > "$FORGE_TMP/squawk.found"
  cat "$FORGE_TMP/squawk.found"
  [ ! -s "$FORGE_TMP/squawk.found" ]
}

# forge_squawk_skipped: reports, as a forge: line of the current check, that Squawk could not run.
forge_squawk_skipped() {
  forge_for_agent || printf 'forge:%s skipped: squawk could not run (%s)\n' "$c_id" "$(grep -v '^[[:space:]]*$' "$FORGE_TMP/squawk" | tail -n 1)"
}

# forge_squawk_files <sql files>: lints migration SQL files.
forge_squawk_files() {
  [ "$#" -gt 0 ] || return 0
  forge_squawk "$@" || { forge_squawk_skipped; return 0; }
  forge_squawk_blocking
}

# forge_sql_postgres <file>: succeeds when the plain-SQL migration tool that owns <file> targets
# Postgres (Squawk lints Postgres only), from literal config only: Supabase always; Flyway
# (flyway.conf / conf/flyway.conf flyway.url, flyway.toml url) jdbc:postgresql:; sqitch
# (sqitch.plan) sqitch.conf engine = pg; Atlas (atlas.hcl) a url or dev of postgres(ql)://
# or docker://postgres; dbmate (none of these) the DATABASE_URL scheme (forge_db_url).
forge_sql_postgres() {
  case $1 in supabase/migrations/*) return 0 ;; esac
  if [ -f flyway.conf ] || [ -f conf/flyway.conf ] || [ -f flyway.toml ]; then
    cat flyway.conf conf/flyway.conf 2>/dev/null | grep -qE '^[[:space:]]*flyway\.url[[:space:]]*=[[:space:]]*jdbc:postgresql:' && return 0
    [ -f flyway.toml ] && grep -qE "^[[:space:]]*url[[:space:]]*=[[:space:]]*[\"']jdbc:postgresql:" flyway.toml && return 0
    return 1
  fi
  if [ -f sqitch.plan ]; then
    [ -f sqitch.conf ] && grep -qE '^[[:space:]]*engine[[:space:]]*=[[:space:]]*pg[[:space:]]*$' sqitch.conf
    return
  fi
  if [ -f atlas.hcl ]; then
    grep -qE '^[[:space:]]*(url|dev)[[:space:]]*=[[:space:]]*"(postgres(ql)?://|docker://postgres)' atlas.hcl
    return
  fi
  case $(forge_db_url) in postgres://*|postgresql://*) return 0 ;; esac
  return 1
}

# forge_squawk_role <@role> [file...]: Squawk (forge_squawk_files) on the files of the role among
# the given ones that target Postgres (forge_sql_postgres); with none given (--all), on the
# role's newest file by name, so an accepted change does not fail every later run.
forge_squawk_role() {
  _sr_role=$1; shift
  if [ "$#" -eq 0 ]; then
    set -f
    # shellcheck disable=SC2046
    set -- $(forge_role_files "$_sr_role" | sort | tail -n 1)
    set +f
  fi
  for _sr_f in "$@"; do
    shift
    [ -f "$_sr_f" ] && forge_role "$_sr_role" "$_sr_f" && forge_sql_postgres "$_sr_f" && set -- "$@" "$_sr_f"
  done
  forge_squawk_files "$@"
}

# forge_alembic_files: Alembic revision ids on stdin (as printed by `alembic heads`)
# -> the revision files (file role @alembic-migrations) that define them.
forge_alembic_files() {
  forge_role_files @alembic-migrations | grep '\.py$' > "$FORGE_TMP/alembic.all"
  awk '{ print $1 }' | while read -r _fa_id; do
    # shellcheck disable=SC2046
    [ -n "$_fa_id" ] && [ -s "$FORGE_TMP/alembic.all" ] && grep -lE "^revision[^=]*=[[:space:]]*['\"]${_fa_id}['\"]" $(cat "$FORGE_TMP/alembic.all") 2>/dev/null
  done
}

# forge_squawk_alembic <revision files>: lints the SQL each revision's upgrade emits
# (`alembic upgrade <down_revision>:<revision> --sql`, offline: no database needed).
# Merge revisions are skipped; when Alembic cannot render SQL, the revision is reported skipped.
forge_squawk_alembic() {
  _fs_rc=0
  for _fs_f in "$@"; do
    _fs_rev=$(sed -n "s/^revision[^=]*=[[:space:]]*['\"]\([^'\"]*\)['\"].*/\1/p" "$_fs_f" | head -n 1)
    _fs_down=$(sed -n 's/^down_revision[^=]*=[[:space:]]*//p' "$_fs_f" | head -n 1)
    [ -n "$_fs_rev" ] || continue
    case $_fs_down in
      \(*|\[*) continue ;;
      None*|'') _fs_range=$_fs_rev ;;
      *) _fs_range="$(printf '%s' "$_fs_down" | sed "s/^['\"]\([^'\"]*\)['\"].*/\1/"):$_fs_rev" ;;
    esac
    if ! forge_do alembic upgrade-sql "$_fs_range" > "$FORGE_TMP/alembic.sql" 2> "$FORGE_TMP/alembic.err"; then
      printf 'forge:%s skipped: alembic upgrade %s --sql failed for %s (%s)\n' "$c_id" "$_fs_range" "$_fs_f" "$(grep -v '^[[:space:]]*$' "$FORGE_TMP/alembic.err" | tail -n 1)"
      continue
    fi
    if ! forge_squawk --stdin-filepath "$_fs_f" < "$FORGE_TMP/alembic.sql"; then forge_squawk_skipped; return 0; fi
    if ! forge_squawk_blocking > "$FORGE_TMP/squawk.block"; then
      printf 'SQL of alembic upgrade %s --sql (line numbers are in that SQL):\n' "$_fs_range"
      cat "$FORGE_TMP/squawk.block"
      _fs_rc=1
    fi
  done
  return "$_fs_rc"
}

# ---- How applications use SQLAlchemy and Alembic (python/sqlalchemy/alembic rules).
# Each prints "path:line: message" (lint findings, attributed to added lines).

# forge_alembic_targets: the revision files to examine: the in-scope changed ones
# ($FORGE_FILES), else (--all) the head revisions' (no file names them as a
# down_revision), so revisions written before a rule was adopted do not fail CI.
forge_alembic_targets() {
  if [ -n "$FORGE_FILES" ]; then
    printf '%s\n' "$FORGE_FILES" | while IFS= read -r _at_f; do
      case $_at_f in *.py) forge_role @alembic-migrations "$_at_f" && printf '%s\n' "$_at_f" ;; esac
    done
    return 0
  fi
  forge_role_files @alembic-migrations | grep '\.py$' | sort > "$FORGE_TMP/alembic.revs"
  [ -s "$FORGE_TMP/alembic.revs" ] || return 0
  # shellcheck disable=SC2046
  awk '
    function ids(s,   out) {
      out = ""
      while (match(s, /["\047][^"\047]+["\047]/)) { out = out " " substr(s, RSTART + 1, RLENGTH - 2); s = substr(s, RSTART + RLENGTH) }
      return out
    }
    /^revision[^=]*=/ { r = $0; sub(/^[^=]*=/, "", r); split(ids(r), a, " "); if (a[1] != "") rev[FILENAME] = a[1] }
    /^down_revision[^=]*=/ { d = $0; sub(/^[^=]*=/, "", d); n = split(ids(d), a, " "); for (i = 1; i <= n; i++) down[a[i]] = 1 }
    END { for (f in rev) if (!(rev[f] in down)) print f }' $(cat "$FORGE_TMP/alembic.revs") | sort
}

# forge_alembic_sqlite: the project also runs its migrations on SQLite: alembic/env.py
# sets render_as_batch=True, or alembic.ini's sqlalchemy.url is a sqlite URL.
forge_alembic_sqlite() {
  grep -qE '^[^#]*render_as_batch[[:space:]]*=[[:space:]]*True' alembic/env.py 2>/dev/null \
    || grep -qE '^[[:space:]]*sqlalchemy\.url[[:space:]]*=[[:space:]]*sqlite' alembic.ini 2>/dev/null
}

# forge_alembic_batch: on such a project, operations SQLite cannot run as ALTER TABLE
# (alter_column, drop_column, constraints) go through op.batch_alter_table(), which
# recreates the table there. Operations under an `if` on the dialect are left alone.
forge_alembic_batch() {
  forge_alembic_sqlite || return 0
  _ab_rc=0
  forge_alembic_targets > "$FORGE_TMP/alembic.targets"
  while IFS= read -r _ab_f; do
    [ -f "$_ab_f" ] || continue
    awk -v f="$_ab_f" '
      { line = $0; sub(/\r$/, "", line) }
      line ~ /^[ \t]*(#|$)/ { next }
      {
        ind = match(line, /[^ \t]/) - 1
        if (skip != "") {
          if (ind > skip + 0) next
          if (ind == skip + 0 && line ~ /^[ \t]*(elif|else)[ \t:]/) next
          skip = ""
        }
        if (line ~ /^[ \t]*(if|elif)[ \t].*(dialect|sqlite|postgres|mysql)/) { skip = ind ""; next }
        if (match(line, /(^|[^A-Za-z0-9_.])op\.(alter_column|drop_column|create_foreign_key|drop_constraint|create_unique_constraint|create_check_constraint|create_primary_key)\(/)) {
          op = substr(line, RSTART, RLENGTH - 1); sub(/^[^o]*op\./, "", op)
          print f ":" NR ": op." op "() outside op.batch_alter_table(): SQLite cannot ALTER a table this way, and this project runs its migrations on SQLite too (render_as_batch=True or a sqlite URL). Use: with op.batch_alter_table(\"<table>\") as batch_op: batch_op." op "(...)"
          bad = 1
        }
      }
      END { exit bad }' "$_ab_f" || _ab_rc=1
  done < "$FORGE_TMP/alembic.targets"
  return "$_ab_rc"
}

# forge_alembic_fk_index: in each revision's upgrade(), a column that gets a foreign key
# (sa.Column(..., sa.ForeignKey(...)), sa.ForeignKeyConstraint([...]),
# op.create_foreign_key(..., [...], ...)) also leads an index: the first column of an
# op.create_index, sa.Index, primary key or unique constraint, index=True, unique=True or
# primary_key=True on the column, or the first column of a CREATE INDEX. PostgreSQL does
# not index referencing columns; skipped when the database URL is MySQL (which does).
# Run as advice (forge_advise): such an index is often a good idea, not a requirement.
forge_alembic_fk_index() {
  case $(forge_db_url) in mysql*|mariadb*) return 0 ;; esac
  _ai_rc=0
  forge_alembic_targets > "$FORGE_TMP/alembic.targets"
  while IFS= read -r _ai_f; do
    [ -f "$_ai_f" ] || continue
    awk -v f="$_ai_f" '
      # the position of the bracket closing the one at p (strings skipped)
      function close_of(s, p,   d, c, q, i, n) {
        d = 0; q = ""; n = length(s)
        for (i = p; i <= n; i++) {
          c = substr(s, i, 1)
          if (q != "") { if (c == "\\") i++; else if (c == q) q = ""; continue }
          if (c == "\"" || c == "\047") q = c
          else if (c == "(" || c == "[") d++
          else if (c == ")" || c == "]") { d--; if (d == 0) return i }
        }
        return n
      }
      function strs(t, all,   out) {
        out = ""
        while (match(t, /["\047][^"\047]*["\047]/)) {
          out = out (out == "" ? "" : SUBSEP) substr(t, RSTART + 1, RLENGTH - 2); t = substr(t, RSTART + RLENGTH)
          if (!all) break
        }
        return out
      }
      function first_list(t,   p) { p = index(t, "["); if (!p) return ""; t = substr(t, p); return substr(t, 1, close_of(t, 1)) }
      function line_of(p,   k) { for (k = nl; k > 1 && start[k] > p; k--) ; return first + k - 1 }
      function cover(list,   n, a, i) { n = split(list, a, SUBSEP); for (i = 1; i <= n; i++) cov[tolower(a[i])] = 1 }
      { line = $0; sub(/\r$/, "", line) }
      /^def[ \t]+upgrade[ \t]*\(/ { on = 1; first = NR + 1; next }
      on && line ~ /^[^ \t#]/ { on = 0 }
      on {
        if (line ~ /^[ \t]*#/) line = ""
        nl++; start[nl] = length(s) + 1; s = s line "\n"
      }
      END {
        rest = s; off = 0
        while (match(rest, /(^|[^A-Za-z0-9_])(Column|ForeignKeyConstraint|create_foreign_key|create_index|PrimaryKeyConstraint|UniqueConstraint|Index)\(/)) {
          name = substr(rest, RSTART, RLENGTH - 1); sub(/^[^A-Za-z]/, "", name)
          po = off + RSTART + RLENGTH - 1
          pc = close_of(s, po); args = substr(s, po + 1, pc - po - 1)
          if (name == "Column") {
            if (args ~ /^[ \t\n]*["\047]/) {
              col = tolower(strs(args, 0))
              if (args ~ /(^|[^A-Za-z0-9_])ForeignKey\(/ && !(col in fk)) { fk[col] = line_of(po); order[++nfk] = col }
              if (args ~ /(index|primary_key|unique)[ \t]*=[ \t]*True/) cov[col] = 1
            }
          } else if (name == "ForeignKeyConstraint" || name == "create_foreign_key") {
            col = tolower(strs(first_list(args), 0))
            if (col != "" && !(col in fk)) { fk[col] = line_of(po); order[++nfk] = col }
          } else if (name == "create_index") cover(strs(first_list(args), 0))
          else if (name == "Index") { split(strs(args, 1), ia, SUBSEP); cover(ia[2]) }
          else cover(strs(args, 0))
          off = po; rest = substr(s, po + 1)
        }
        u = toupper(s)
        while (match(u, /CREATE[ \t\n]+(UNIQUE[ \t\n]+)?INDEX[^(]*\([ \t\n]*/)) {
          t = substr(u, RSTART + RLENGTH); sub(/[^A-Z0-9_"].*$/, "", t); gsub(/"/, "", t)
          cov[tolower(t)] = 1; u = substr(u, RSTART + RLENGTH)
        }
        for (i = 1; i <= nfk; i++) {
          col = order[i]
          if (col in cov) continue
          print f ":" fk[col] ": foreign key column \"" col "\" gets no index that leads with it in this revision; PostgreSQL and SQLite do not index referencing columns, so joins on it and deletes of the referenced rows scan the table. If it is queried or the parent rows are deleted, add op.create_index(..., [\"" col "\"]) (index=True on the model mapped_column, then autogenerate); otherwise say why not."
          bad = 1
        }
        exit bad
      }' "$_ai_f" || _ai_rc=1
  done < "$FORGE_TMP/alembic.targets"
  return "$_ai_rc"
}

# forge_alembic_data_downgrade: a revision whose upgrade() changes rows (UPDATE, INSERT,
# DELETE through execute(), sa.update/insert/delete, op.bulk_insert, an ORM Session) has a
# downgrade() that changes rows the same ways (the inverse) or raises (it cannot be
# reverted). A downgrade that only passes, prints or alters the schema reports success
# and leaves the data. Run as advice (forge_advise): Claude may say why not.
forge_alembic_data_downgrade() {
  forge_alembic_targets > "$FORGE_TMP/alembic.targets"
  : > "$FORGE_TMP/alembic.data"
  while IFS= read -r _ad_f; do
    [ -f "$_ad_f" ] || continue
    awk -v f="$_ad_f" '
      function changes_rows(t,   u) {
        u = toupper(t)
        return (t ~ /execute\(/ && (u ~ /(^|[^A-Z_])UPDATE[ \t\n]+[^ \t\n]+[ \t\n]+SET[ \t\n]/ || u ~ /INSERT[ \t\n]+INTO[ \t\n]/ || u ~ /DELETE[ \t\n]+FROM[ \t\n]/)) \
          || t ~ /(^|[^A-Za-z0-9_.])(sa\.)?(update|insert|delete)\([A-Za-z_]/ || t ~ /\.(update|insert|delete)\(\)/ \
          || t ~ /op\.bulk_insert\(/ || t ~ /(^|[^A-Za-z0-9_])Session\(/
      }
      { line = $0; sub(/\r$/, "", line) }
      /^def[ \t]+upgrade[ \t]*\(/ { sec = "up"; next }
      /^def[ \t]+downgrade[ \t]*\(/ { sec = "down"; dl = NR; next }
      line ~ /^[^ \t#]/ { sec = ""; next }
      sec == "up" && line !~ /^[ \t]*#/ { up = up line "\n" }
      sec == "down" {
        t = line; sub(/^[ \t]+/, "", t); sub(/[ \t]+$/, "", t)
        if (indoc) { if (index(t, q)) indoc = 0; next }
        if (t == "" || t ~ /^#/) next
        if (t ~ /^("""|\047\047\047)/) { q = substr(t, 1, 3); if (!index(substr(t, 4), q)) indoc = 1; next }
        if (t ~ /^raise([ \t(]|$)/) raises = 1
        down = down line "\n"
      }
      END {
        if (changes_rows(up) && !raises && !changes_rows(down))
          print f ":" (dl ? dl : 1) ": upgrade() changes rows but downgrade() neither changes them back (op.execute with UPDATE/INSERT/DELETE, sa.update/insert/delete, op.bulk_insert) nor raises, so alembic downgrade reports success and leaves the rows changed. Write the inverse, or raise an error that says the revision cannot be reverted."
      }' "$_ad_f" >> "$FORGE_TMP/alembic.data"
  done < "$FORGE_TMP/alembic.targets"
  [ -s "$FORGE_TMP/alembic.data" ] || return 0
  cat "$FORGE_TMP/alembic.data"
  return 1
}

# forge_models_registered: a module of a models package that maps a table (a class with
# __tablename__) is imported by the package's __init__.py when that file is the registry
# (it imports other modules of the package that map tables), or by alembic/env.py:
# Base.metadata, which autogenerate and alembic check compare, knows only imported models.
forge_models_registered() {
  if [ -n "$FORGE_FILES" ]; then printf '%s\n' "$FORGE_FILES"
  else git ls-files -co --exclude-standard -- 'models/*.py' '*/models/*.py' 2>/dev/null; fi > "$FORGE_TMP/models.files"
  _mr_rc=0
  while IFS= read -r _mr_f; do
    case ${_mr_f##*/} in __init__.py|conftest.py|test_*) continue ;; *.py) ;; *) continue ;; esac
    [ -f "$_mr_f" ] || continue
    _mr_d=$(dirname "$_mr_f")
    _mr_m=$(basename "$_mr_f" .py)
    [ -f "$_mr_d/__init__.py" ] || continue
    awk '
      /^class[ \t]+[A-Za-z_]/ { c = $0; sub(/^class[ \t]+/, "", c); sub(/[^A-Za-z0-9_].*$/, "", c); cl = NR; next }
      /^[^ \t#\r]/ { c = "" }
      c != "" && /^[ \t]+__tablename__[ \t]*[:=]/ { print cl "\t" c; c = "" }' "$_mr_f" > "$FORGE_TMP/models.classes"
    [ -s "$FORGE_TMP/models.classes" ] || continue
    forge_mentions "$_mr_d/__init__.py" "$_mr_m" && continue
    forge_mentions alembic/env.py "$_mr_m" && continue
    # the registry: __init__.py imports another module of the package that maps a table
    _mr_reg=''
    for _mr_x in $(awk '
      /^[ \t]*from[ \t]+\.[ \t]+import[ \t]/ { s = $0; sub(/^.*import[ \t]+/, "", s); gsub(/[(),]/, " ", s); n = split(s, a, /[ \t]+/); for (i = 1; i <= n; i++) if (a[i] != "as") print a[i]; next }
      /^[ \t]*from[ \t]+[A-Za-z0-9_.]+[ \t]+import/ { s = $0; sub(/^[ \t]*from[ \t]+/, "", s); sub(/[ \t].*$/, "", s); sub(/^.*\./, "", s); print s; next }
      /^[ \t]*import[ \t]+[A-Za-z0-9_.]+/ { s = $0; sub(/^[ \t]*import[ \t]+/, "", s); sub(/[ \t,].*$/, "", s); sub(/^.*\./, "", s); print s }' "$_mr_d/__init__.py"); do
      [ "$_mr_x" != "$_mr_m" ] && [ -f "$_mr_d/$_mr_x.py" ] && grep -q '__tablename__' "$_mr_d/$_mr_x.py" && { _mr_reg=1; break; }
    done
    [ -n "$_mr_reg" ] || continue
    while IFS='	' read -r _mr_l _mr_c; do
      printf '%s:%s: model %s is not imported by %s/__init__.py, which imports the other models: Base.metadata will not have its table, so autogenerate writes no migration for it and alembic check passes. Import it there.\n' "$_mr_f" "$_mr_l" "$_mr_c" "$_mr_d"
    done < "$FORGE_TMP/models.classes"
    _mr_rc=1
  done < "$FORGE_TMP/models.files"
  return "$_mr_rc"
}

# forge_prisma_drops <file>...: "<file>:<line>: <statement>" for each DROP COLUMN or DROP
# TABLE in the given Prisma migrations. Every migration.sql (sorted) is read to follow
# the tables' columns (CREATE TABLE, ADD / DROP / RENAME COLUMN), so a SQLite table
# redefinition (CREATE TABLE "new_X" ..., DROP TABLE "X", ALTER TABLE "new_X" RENAME
# TO "X") is a drop only of the columns "new_X" lacks: "... (redefinition drops column
# "C")". A redefined table that no earlier migration creates is not reported.
forge_prisma_drops() {
  printf '%s\n' "$@" > "$FORGE_TMP/prisma.targets"
  { git ls-files -co --exclude-standard -- 'prisma/migrations/*/migration.sql' 2>/dev/null; printf '%s\n' "$@"; } \
    | sort -u > "$FORGE_TMP/prisma.history"
  set -f
  # shellcheck disable=SC2046
  awk '
    function unq(t) { gsub(/[";`[:space:]]/, "", t); return t }
    function name_after(u, re,   t) { t = u; sub(re, "", t); sub(/[[:space:](].*$/, "", t); return unq(t) }
    # the column names of a CREATE TABLE body: the first identifier of each top-level item
    function columns(stmt,   p, body, d, i, c, item, out, n, name) {
      p = index(stmt, "("); if (!p) return ""
      body = substr(stmt, p + 1); d = 0; item = ""; out = ""
      for (i = 1; i <= length(body); i++) {
        c = substr(body, i, 1)
        if (c == "(") d++
        if (c == ")") { if (d == 0) break; d-- }
        if (c == "," && d == 0) { out = out item_col(item); item = ""; continue }
        item = item c
      }
      return out item_col(item)
    }
    function item_col(item,   t) {
      t = item; sub(/^[[:space:]]+/, "", t)
      if (toupper(t) ~ /^(CONSTRAINT|PRIMARY|FOREIGN|UNIQUE|CHECK|KEY|INDEX)([^A-Z_]|$)/) return ""
      if (!match(t, /^("[^"]+"|`[^`]+`|[A-Za-z_][A-Za-z0-9_]*)/)) return ""
      return SUBSEP unq(substr(t, RSTART, RLENGTH))
    }
    function has_col(list, c) { return index(list SUBSEP, SUBSEP c SUBSEP) > 0 }
    function del_col(list, c,   x) { x = list SUBSEP; sub(SUBSEP c SUBSEP, SUBSEP, x); return substr(x, 1, length(x) - 1) }
    function statement(f, line, stmt,   u, t, c, n, a, i, old, gone) {
      u = toupper(stmt)
      if (u ~ /^[[:space:]]*CREATE[[:space:]]+TABLE/) { t = name_after(u, "^[[:space:]]*CREATE[[:space:]]+TABLE[[:space:]]+(IF[[:space:]]+NOT[[:space:]]+EXISTS[[:space:]]+)?"); cols[t] = toupper(columns(stmt)); return }
      if (u ~ /^[[:space:]]*ALTER[[:space:]]+TABLE/) {
        t = name_after(u, "^[[:space:]]*ALTER[[:space:]]+TABLE[[:space:]]+(ONLY[[:space:]]+)?(IF[[:space:]]+EXISTS[[:space:]]+)?")
        if (u ~ /RENAME[[:space:]]+TO[[:space:]]/) {
          n = name_after(u, ".*RENAME[[:space:]]+TO[[:space:]]+")
          if ((f SUBSEP n) in dropped) {
            old = dropped[f SUBSEP n]; gone = ""
            c = split(substr(old, 2), a, SUBSEP)
            for (i = 1; i <= c; i++) if (!has_col(cols[t], a[i])) gone = gone (gone == "" ? "" : ", ") "\"" a[i] "\""
            if (f in target && gone != "") print f ":" dline[f SUBSEP n] ": " dtext[f SUBSEP n] " (redefinition drops column " gone ")"
            delete dropped[f SUBSEP n]; delete pend[f SUBSEP n]
          }
          cols[n] = cols[t]; return
        }
        if (u ~ /RENAME[[:space:]]+COLUMN/) { a[1] = name_after(u, ".*RENAME[[:space:]]+COLUMN[[:space:]]+"); a[2] = name_after(u, ".*[[:space:]]TO[[:space:]]+"); cols[t] = del_col(cols[t], a[1]) SUBSEP a[2]; return }
        while (match(u, /ADD[[:space:]]+(COLUMN[[:space:]]+)?("[^"]+"|[A-Z_][A-Z0-9_]*)/)) { c = substr(u, RSTART, RLENGTH); sub(/^ADD[[:space:]]+(COLUMN[[:space:]]+)?/, "", c); c = unq(c); if (c !~ /^(CONSTRAINT|PRIMARY|FOREIGN|UNIQUE|CHECK)$/) cols[t] = cols[t] SUBSEP c; u = substr(u, RSTART + RLENGTH) }
        u = toupper(stmt)
        while (match(u, /DROP[[:space:]]+COLUMN[[:space:]]+(IF[[:space:]]+EXISTS[[:space:]]+)?("[^"]+"|[A-Z_][A-Z0-9_]*)/)) { c = substr(u, RSTART, RLENGTH); sub(/^DROP[[:space:]]+COLUMN[[:space:]]+(IF[[:space:]]+EXISTS[[:space:]]+)?/, "", c); cols[t] = del_col(cols[t], unq(c)); u = substr(u, RSTART + RLENGTH) }
        if (f in target && toupper(stmt) ~ /DROP[[:space:]]+COLUMN/) print f ":" line ": " text
        return
      }
      if (u ~ /^[[:space:]]*DROP[[:space:]]+TABLE/) {
        t = name_after(u, "^[[:space:]]*DROP[[:space:]]+TABLE[[:space:]]+(IF[[:space:]]+EXISTS[[:space:]]+)?")
        # a redefinition renames "new_<t>" to <t> later in the same file
        # (its old columns unknown, without the earlier migrations: not reported)
        if (("NEW_" t) in cols) { if (t in cols) { dropped[f SUBSEP t] = cols[t]; dline[f SUBSEP t] = line; dtext[f SUBSEP t] = text; pend[f SUBSEP t] = 1 } }
        else if (f in target) print f ":" line ": " text
        delete cols[t]
      }
    }
    FNR == 1 {
      for (k in pend) { split(k, kk, SUBSEP); if ((k in dropped) && (kk[1] in target)) print kk[1] ":" dline[k] ": " dtext[k]; delete pend[k]; delete dropped[k] }
      f = FILENAME; stmt = ""; incomment = 0
    }
    FILENAME == ARGV[1] { target[$0] = 1; next }
    FILENAME == ARGV[2] { next }
    {
      l = $0
      if (incomment) { if (index(l, "*/")) { incomment = 0; l = substr(l, index(l, "*/") + 2) } else next }
      if (l ~ /^[[:space:]]*\/\*/ && !index(l, "*/")) { incomment = 1; next }
      if (l ~ /^[[:space:]]*--/) next
      if (stmt == "" && l ~ /^[[:space:]]*$/) next
      if (stmt == "") { sline = FNR; text = l; sub(/^[[:space:]]+/, "", text) }
      stmt = stmt " " l
      if (index(l, ";")) { statement(f, sline, stmt); stmt = "" }
    }
    END { for (k in pend) { split(k, kk, SUBSEP); if ((k in dropped) && (kk[1] in target)) print kk[1] ":" dline[k] ": " dtext[k] } }
  ' "$FORGE_TMP/prisma.targets" /dev/null $(cat "$FORGE_TMP/prisma.history")
  _pd_rc=$?
  set +f
  return "$_pd_rc"
}

# Drizzle: drizzle.config.* names the schema files (`schema`: a path, a glob, a
# directory or an array of them), the migrations folder (`out`, default ./drizzle)
# and the `dialect`. forge_drizzle_config writes "key<TAB>value" lines (one per
# schema entry, ./ removed) to $FORGE_TMP/drizzle.config; fails without a config.
forge_drizzle_config() {
  FORGE_DZ_CONFIG=''
  for _dz_f in drizzle.config.ts drizzle.config.mts drizzle.config.cts drizzle.config.js drizzle.config.mjs drizzle.config.cjs drizzle.config.json; do
    if [ -f "$_dz_f" ]; then FORGE_DZ_CONFIG=$_dz_f; break; fi
  done
  [ -n "$FORGE_DZ_CONFIG" ] || return 1
  awk '
    { s = s $0 "\n" }
    function val(key,   re, v) {
      re = "[^A-Za-z0-9_$]\"?" key "\"?[[:space:]]*:[[:space:]]*(\\[[^]]*\\]|\"[^\"]*\"|\047[^\047]*\047|`[^`]*`)"
      if (!match(s, re)) return ""
      v = substr(s, RSTART, RLENGTH); sub(/^[^:]*:[[:space:]]*/, "", v); return v
    }
    function emit(key, v,   q) {
      while (match(v, /"[^"]*"|\047[^\047]*\047|`[^`]*`/)) {
        q = substr(v, RSTART + 1, RLENGTH - 2); v = substr(v, RSTART + RLENGTH)
        sub(/^\.\//, "", q); sub(/\/+$/, "", q)
        if (q != "") print key "\t" q
      }
    }
    END {
      # drop comments, outside strings
      t = ""; q = ""; n = length(s)
      for (i = 1; i <= n; i++) {
        c = substr(s, i, 1); d = substr(s, i, 2)
        if (q != "") { t = t c; if (c == "\\") { t = t substr(s, ++i, 1) } else if (c == q) q = ""; continue }
        if (d == "//") { while (i <= n && substr(s, i, 1) != "\n") i++; t = t " "; continue }
        if (d == "/*") { i += 2; while (i <= n && substr(s, i, 2) != "*/") i++; i++; t = t " "; continue }
        if (c == "\"" || c == "\047" || c == "`") q = c
        t = t (c == "\n" ? " " : c)
      }
      s = t
      gsub(/migrations[[:space:]]*:[[:space:]]*\{[^}]*\}/, "", s)
      s = " " s
      o = val("out"); if (o == "") print "out\tdrizzle"; else emit("out", o)
      emit("dialect", val("dialect")); emit("schema", val("schema"))
    }' "$FORGE_DZ_CONFIG" > "$FORGE_TMP/drizzle.config"
}

# forge_drizzle_get <key>: the first value of <key> from forge_drizzle_config.
forge_drizzle_get() { awk -F '\t' -v k="$1" '$1 == k { print $2; exit }' "$FORGE_TMP/drizzle.config"; }

# forge_drizzle_is_schema <path>: succeeds when <path> is one of the configured schema
# entries, matches one as a glob, or sits under one as a directory.
forge_drizzle_is_schema() {
  while IFS="$(printf '\t')" read -r _ds_k _ds_v; do
    [ "$_ds_k" = schema ] || continue
    _ds_g=$(printf '%s' "$_ds_v" | sed 's/\*\*/*/g')
    # shellcheck disable=SC2254
    case $1 in "$_ds_v"|"$_ds_v"/*|$_ds_g) return 0 ;; esac
  done < "$FORGE_TMP/drizzle.config"
  return 1
}

# forge_drizzle_tables <file>: the file without what does not reach the database
# (blank lines, comments, imports, type exports, relations()/defineRelations() calls).
forge_drizzle_tables() {
  awk '
    inc { if (/\*\//) inc = 0; next }
    /^[[:space:]]*\/\*/ { if (!/\*\//) inc = 1; next }
    /^[[:space:]]*(\/\/|$)/ { next }
    /^[[:space:]]*(import[[:space:]{*]|export[[:space:]]+(type|interface)[[:space:]]|export[[:space:]]*\*)/ { next }
    depth == 0 && /(^|[^A-Za-z0-9_$])(defineRelations|relations)[[:space:]]*\(/ { inrel = 1 }
    inrel {
      t = $0; o = gsub(/\(/, "", t); t = $0; c = gsub(/\)/, "", t); depth += o - c
      if (depth <= 0) { depth = 0; inrel = 0 }
      next
    }
    { sub(/^[[:space:]]+/, ""); sub(/[[:space:]]+$/, ""); print }' "$1" 2>/dev/null
}

# forge_drizzle_migration_present: a change to a schema file that reaches the database
# comes with a generated migration in the out folder: the SQL plus meta/_journal.json and
# a meta/*_snapshot.json (drizzle-kit 0.x), or <out>/<name>/migration.sql and its
# snapshot.json (drizzle-kit 1.x). Not with --all (no change to compare).
forge_drizzle_migration_present() {
  [ "$mode" != --all ] || return 0
  forge_drizzle_config || return 0
  grep -q "^schema$(printf '\t')" "$FORGE_TMP/drizzle.config" || {
    printf 'forge:%s skipped: no schema path found in %s\n' "$c_id" "$FORGE_DZ_CONFIG"; return 0; }
  _dm_out=$(forge_drizzle_get out)
  : > "$FORGE_TMP/dz.schema"
  while IFS= read -r _dm_f; do
    forge_drizzle_is_schema "$_dm_f" || continue
    forge_drizzle_tables "$_dm_f" > "$FORGE_TMP/dz.now"
    forge_drizzle_tables "$FORGE_TMP/base/$_dm_f" > "$FORGE_TMP/dz.then"
    cmp -s "$FORGE_TMP/dz.now" "$FORGE_TMP/dz.then" || printf '%s\n' "$_dm_f" >> "$FORGE_TMP/dz.schema"
  done < "$FORGE_TMP/changed"
  [ -s "$FORGE_TMP/dz.schema" ] || return 0
  _dm_sql=0; _dm_journal=0; _dm_snap=0
  while IFS= read -r _dm_f; do
    case $_dm_f in
      "$_dm_out"/meta/_journal.json) _dm_journal=1 ;;
      "$_dm_out"/meta/*_snapshot.json|"$_dm_out"/*/snapshot.json) _dm_snap=1 ;;
      "$_dm_out"/*/migration.sql) _dm_sql=1; _dm_journal=1 ;;
      "$_dm_out"/*.sql) _dm_sql=1 ;;
    esac
  done < "$FORGE_TMP/existing"
  # drizzle-kit 1.x keeps no journal: a <name>/migration.sql stands in for it
  [ "$_dm_sql" = 1 ] && [ "$_dm_journal" = 1 ] && [ "$_dm_snap" = 1 ] && return 0
  while IFS= read -r _dm_f; do
    printf '%s: the Drizzle schema changed, but %s/ has no new migration with its journal and snapshot\n' "$_dm_f" "$_dm_out"
  done < "$FORGE_TMP/dz.schema"
  printf 'Generate it (drizzle-kit generate) and keep the SQL, meta/_journal.json and the snapshot together; never write them by hand.\n'
  return 1
}

# forge_applied_migrations <@role>...: migrations (the files of the roles) that exist in the
# base are not modified or deleted; a change goes in a new migration. Not with --all (no base).
forge_applied_migrations() {
  [ "$mode" != --all ] || return 0
  _am_rc=0
  while IFS= read -r _am_f; do
    for _am_r in "$@"; do
      forge_role "$_am_r" "$_am_f" || continue
      if [ -f "$_am_f" ]; then _am_what=modified; else _am_what=deleted; fi
      printf '%s: an existing migration was %s; it may already be applied somewhere, so write a new migration instead\n' "$_am_f" "$_am_what"
      _am_rc=1
      break
    done
  done < "$FORGE_TMP/inbase"
  return "$_am_rc"
}

# forge_typeorm_pending <@datasource-role> <@esm-role>: TypeORM finds no schema change
# the migrations lack. The data source is the first file of <@datasource-role>
# (@typeorm-datasource: no default, a code config names it); its migrations run on the
# local database, then `migration:generate --check` compares the entities with it. A
# TypeScript data source runs through the project's typeorm-ts-node-esm (a file of
# <@esm-role>, @esm-modules) or typeorm-ts-node-commonjs.
forge_typeorm_pending() {
  _tp_ds=$(forge_role_files "$1" | head -n 1)
  if [ -z "$_tp_ds" ]; then
    printf 'forge:%s skipped: file role @typeorm-datasource names no file; set the data source in forge.yaml (files: {typeorm-datasource: {set: [src/data-source.ts]}})\n' "$c_id"
    return 0
  fi
  case $_tp_ds in
    *.ts|*.mts|*.cts) if forge_role "$2" "$_tp_ds"; then _tp_bin=typeorm-ts-node-esm; else _tp_bin=typeorm-ts-node-commonjs; fi ;;
    *) _tp_bin=typeorm ;;
  esac
  if [ -z "$(tool "$_tp_bin")" ]; then
    forge_for_agent || printf 'forge:%s skipped: %s not installed\n' "$c_id" "$_tp_bin"
    return 0
  fi
  if ! forge_do "$_tp_bin" migrate "$_tp_ds" > "$FORGE_TMP/typeorm.out" 2>&1; then
    printf 'forge:%s skipped: %s migration:run -d %s failed: %s\n' "$c_id" "$_tp_bin" "$_tp_ds" "$(grep . "$FORGE_TMP/typeorm.out" | tail -n 1)"
    return 0
  fi
  forge_do "$_tp_bin" migrations-check "$_tp_ds" "$FORGE_TMP/typeorm-check" > "$FORGE_TMP/typeorm.out" 2>&1
  if grep -q 'Unexpected changes in database schema were found' "$FORGE_TMP/typeorm.out"; then
    printf 'The entities changed, but no migration has the change: generate one (%s migration:generate -d %s <path>), review it and commit it.\n' "$_tp_bin" "$_tp_ds"
    cat "$FORGE_TMP/typeorm.out"
    return 1
  fi
  grep -q 'No changes in database schema were found' "$FORGE_TMP/typeorm.out" && return 0
  printf 'forge:%s skipped: %s migration:generate --check failed: %s\n' "$c_id" "$_tp_bin" "$(grep . "$FORGE_TMP/typeorm.out" | tail -n 1)"
  return 0
}

# forge_drizzle_applied: migrations (SQL and snapshots in the out folder) that exist in
# the base are not modified or deleted; a change goes in a new migration.
forge_drizzle_applied() {
  [ "$mode" != --all ] || return 0
  forge_drizzle_config || return 0
  _da_out=$(forge_drizzle_get out)
  _da_rc=0
  while IFS= read -r _da_f; do
    case $_da_f in
      "$_da_out"/*.sql|"$_da_out"/meta/*_snapshot.json|"$_da_out"/*/snapshot.json)
        if [ -f "$_da_f" ]; then _da_what=modified; else _da_what=deleted; fi
        printf '%s: an existing migration was %s; generate a new migration instead (drizzle-kit generate)\n' "$_da_f" "$_da_what"
        _da_rc=1 ;;
    esac
  done < "$FORGE_TMP/inbase"
  return "$_da_rc"
}

# forge_drizzle_squawk [files]: Squawk (§13.1) on the changed SQL of the out folder
# when the dialect is postgresql; with no files (--since, --all), on the newest migration.
forge_drizzle_squawk() {
  forge_drizzle_config || return 0
  [ "$(forge_drizzle_get dialect)" = postgresql ] || return 0
  _dq_out=$(forge_drizzle_get out)
  if [ "$#" -eq 0 ]; then
    set -- $(git ls-files -- "$_dq_out/*.sql" | sort | tail -n 1)
  else
    for _dq_f in "$@"; do
      shift
      case $_dq_f in "$_dq_out"/*.sql) set -- "$@" "$_dq_f" ;; esac
    done
  fi
  forge_squawk_files "$@"
}
