# @module docs
# Documentation: immutable ADRs and docs that mention changed identifiers.

# forge_changed_ids: the public identifiers whose definition the change added,
# removed or edited, one per line, from the removed and added lines of the
# in-scope files ($FORGE_FILES and the deleted ones it names): Python def, class,
# annotated names (parameters, fields) and module-level assignments; TypeScript
# and JavaScript exported declarations, optional properties (name?:), typed
# members (name: T;) and methods with a modifier. A definition whose line only
# moved does not count. Private (_x), short (< 4) and common names are dropped.
forge_changed_ids() {
  { printf '%s\n' "$FORGE_FILES"; cat "$FORGE_TMP/deleted"; } | while IFS= read -r _ci_f; do
    printf '%s\n' "$_ci_f" | grep -Eq -- "$FORGE_TEST_PATH_ERE" && continue
    case $_ci_f in *.py|*.pyi) _ci_py=1 ;; *.ts|*.tsx|*.mts|*.cts|*.js|*.jsx|*.mjs|*.cjs) _ci_py=0 ;; *) continue ;; esac
    forge_diff_lines "$_ci_f" | sed "s/^/$_ci_py/"
  done | awk '
    function keep(id) {
      if (length(id) < 4 || id ~ /^_/ || (id in stop)) return
      ids[id] = 1
    }
    function def(id, sign, text) { keep(id); if (id in ids) { if (sign == "-") M[id SUBSEP text] = 1; else P[id SUBSEP text] = 1 } }
    function ident(s) { sub(/^[^A-Za-z_$]*/, "", s); match(s, /^[A-Za-z_$][A-Za-z0-9_$]*/); return substr(s, 1, RLENGTH) }
    BEGIN {
      n = split("self cls this args kwargs props options opts config settings params param value values data name type kind keys item items result results error errors message path file files input output default index count size text body request response context event callback handler children model state status client server string number boolean object array true false none null undefined return async await const function class export import from init main test start stop update create delete call load save open close read write parse format info debug warn logger user node tree root parent child source target mode level headers method query schema field fields label title description content length list dict tuple bool float bytes json None True False Exception ValueError TypeError KeyError RuntimeError else elif while with pass yield raise assert except finally lambda global nonlocal case break continue Args Returns Raises Yields Example Examples Note Notes Attributes", w, " ")
      for (i = 1; i <= n; i++) stop[w[i]] = 1
    }
    {
      py = substr($0, 1, 1); sign = substr($0, 2, 1); line = substr($0, 3)
      t = line; gsub(/^[ \t]+|[ \t\r]+$/, "", t)
      if (t == "") next
      if (py == 1) {
        if (t ~ /^#/) next
        if (t ~ /^(async[ \t]+)?def[ \t]/) {
          s = t; sub(/^(async[ \t]+)?def[ \t]+/, "", s); def(ident(s), sign, t)
          if (i = index(s, "(")) {
            s = substr(s, i + 1); m = split(s, a, ",")
            for (j = 1; j <= m; j++) { p = a[j]; sub(/^[ \t*]+/, "", p); if (p ~ /^[A-Za-z_][A-Za-z0-9_]*[ \t]*[:=,)]?/) def(ident(p), sign, t) }
          }
        } else if (t ~ /^class[ \t]/) { s = t; sub(/^class[ \t]+/, "", s); def(ident(s), sign, t) }
        else if (t ~ /^[A-Za-z_][A-Za-z0-9_]*[ \t]*:[ \t]*[^ \t=]/) {
          r = t; sub(/^[A-Za-z_][A-Za-z0-9_]*[ \t]*:[ \t]*/, "", r)
          if (r ~ /=/ || r ~ /^[A-Za-z_][A-Za-z0-9_.]*(\[.*)?([ \t]*\|[ \t]*[^ \t]+)*[ \t]*,?$/) def(ident(t), sign, t)
        }
        else if (line ~ /^[A-Za-z_][A-Za-z0-9_]*[ \t]*=[^=]/) def(ident(line), sign, t)
      } else {
        if (t ~ /^(\/\/|\/\*|\*)/) next
        if (t ~ /^export[ \t]/) {
          s = t; sub(/^export[ \t]+((default|declare|abstract|async)[ \t]+)*/, "", s)
          if (s ~ /^(function\*?|class|const|let|var|type|interface|enum|namespace)[ \t]/) { sub(/^[a-z]+\*?[ \t]+/, "", s); def(ident(s), sign, t) }
        } else if (t ~ /^(readonly[ \t]+)?[A-Za-z_$][A-Za-z0-9_$]*\?[ \t]*:/ || t ~ /^(readonly[ \t]+)?[A-Za-z_$][A-Za-z0-9_$]*[ \t]*:.*;$/) {
          s = t; sub(/^readonly[ \t]+/, "", s); def(ident(s), sign, t)
        } else if (t ~ /^((public|static|async|get|set|override)[ \t]+)+[A-Za-z_$][A-Za-z0-9_$]*[ \t]*[(<]/) {
          s = t; sub(/^((public|static|async|get|set|override)[ \t]+)+/, "", s); def(ident(s), sign, t)
        }
      }
    }
    END {
      for (k in M) if (!(k in P)) { split(k, x, SUBSEP); out[x[1]] = 1 }
      for (k in P) if (!(k in M)) { split(k, x, SUBSEP); out[x[1]] = 1 }
      for (id in out) print id
    }' | sort
}

# forge_adr_accepted <file>: the ADR's status is accepted: MADR front matter
# `status: accepted`, or a Status heading whose first non-empty line starts with Accepted.
forge_adr_accepted() {
  awk '
    NR == 1 && /^---[ \t]*$/ { fm = 1; next }
    fm && /^---[ \t]*$/ { fm = 0; next }
    fm { if (tolower($0) ~ /^status:[ \t]*["\047]?accepted["\047]?[ \t]*$/) ok = 1; next }
    /^#+[ \t]+[Ss][Tt][Aa][Tt][Uu][Ss][ \t]*$/ { st = 1; next }
    st && /^#/ { st = 0 }
    st && NF { w = tolower($1); gsub(/[^a-z]/, "", w); if (w == "accepted") ok = 1; st = 0 }
    END { exit !ok }' "$1"
}

# forge_adr_body <file>: the ADR without its status (the front matter status line, the
# content of the Status section).
forge_adr_body() {
  awk '
    NR == 1 && /^---[ \t]*$/ { fm = 1; print; next }
    fm && /^---[ \t]*$/ { fm = 0; print; next }
    fm && tolower($0) ~ /^status:/ { next }
    fm { print; next }
    /^#+[ \t]+[Ss][Tt][Aa][Tt][Uu][Ss][ \t]*$/ { st = 1; print; next }
    st && /^#/ { st = 0 }
    st { next }
    { print }' "$1"
}

# forge_adr_edits: the changed ADRs ($FORGE_FILES: file role @adr) that were accepted in
# the base and whose change touches more than their status; one line each.
forge_adr_edits() {
  for _ae_f in $FORGE_FILES; do
    case $_ae_f in *.md|*.mdx|*.markdown|*.rst) ;; *) continue ;; esac
    [ -f "$FORGE_TMP/base/$_ae_f" ] && [ -f "$_ae_f" ] || continue
    forge_adr_accepted "$FORGE_TMP/base/$_ae_f" || continue
    forge_adr_body "$FORGE_TMP/base/$_ae_f" > "$FORGE_TMP/adr.base"
    forge_adr_body "$_ae_f" > "$FORGE_TMP/adr.now"
    cmp -s "$FORGE_TMP/adr.base" "$FORGE_TMP/adr.now" && continue
    printf '%s: an accepted ADR was rewritten; an ADR is a record of a decision: only its status may change (superseded, deprecated), and the new decision goes in a new ADR that supersedes it\n' "$_ae_f"
  done
  return 0
}

# forge_doc_files: documentation in the repository (tracked or not ignored):
# Markdown, MDX and reStructuredText under a docs/ or doc/ directory, and README files.
FORGE_DOC_ERE='(^|/)(docs?/(.*/)?[^/]*\.(md|mdx|rst)|README[^/]*)$'

# forge_docs_mentions: when no documentation file changed in the change, the
# lines of the documentation that mention an identifier the change defined
# differently (forge_changed_ids): up to 5 "path:line identifier" lines. A name
# with an underscore or a capital inside counts anywhere; a plain word only in a
# code span or a fenced block.
forge_docs_mentions() {
  grep -Eq -- "$FORGE_DOC_ERE" "$FORGE_TMP/changed" && return 0
  forge_changed_ids > "$FORGE_TMP/doc.ids"
  [ -s "$FORGE_TMP/doc.ids" ] || return 0
  git ls-files --cached --others --exclude-standard 2>/dev/null | grep -E -- "$FORGE_DOC_ERE" | while IFS= read -r _dm_f; do
    [ -f "$_dm_f" ] && grep -qF -f "$FORGE_TMP/doc.ids" "$_dm_f" 2>/dev/null && printf '%s\n' "$_dm_f"
  done > "$FORGE_TMP/doc.files"
  [ -s "$FORGE_TMP/doc.files" ] || return 0
  # shellcheck disable=SC2046
  awk '
    function word(s, id,   i, b, a) {
      while (i = index(s, id)) {
        b = substr(s, i - 1, 1); a = substr(s, i + length(id), 1)
        if ((i == 1 || b !~ /[A-Za-z0-9_$]/) && a !~ /[A-Za-z0-9_$]/) return 1
        s = substr(s, i + length(id))
      }
      return 0
    }
    function spans(s,   n, p, i, out) { n = split(s, p, "`"); out = ""; for (i = 2; i <= n; i += 2) out = out " " p[i]; return out }
    FILENAME == ARGV[1] { ids[++nid] = $0; plain[nid] = ($0 !~ /_/ && $0 !~ /.[A-Z]/); next }
    FNR == 1 { fence = 0 }
    /^[ \t]*(```|~~~)/ { fence = !fence; next }
    {
      code = fence ? $0 : spans($0)
      for (j = 1; j <= nid; j++)
        if (word(plain[j] ? code : $0, ids[j])) { print FILENAME ":" FNR " " ids[j]; break }
    }' "$FORGE_TMP/doc.ids" $(cat "$FORGE_TMP/doc.files") > "$FORGE_TMP/doc.hits"
  [ -s "$FORGE_TMP/doc.hits" ] || return 0
  printf 'These docs mention identifiers you changed; check they still describe the behaviour:\n'
  awk 'NR <= 5 { print "  - " $0 } END { if (NR > 5) print "  (and " NR - 5 " more)" }' "$FORGE_TMP/doc.hits"
}
