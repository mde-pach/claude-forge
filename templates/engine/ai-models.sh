# @module ai-models
# Model ids: deprecated and unknown Claude model ids on added lines.

# forge_model_ids <deprecated> <known>: for each quoted model id on an added line of an
# in-scope file ($FORGE_FILES, $FORGE_TMP/added), optionally after pydantic-ai's
# anthropic: / openai: or LiteLLM's anthropic/ / openai/ prefix: an id of <deprecated>
# ("<id>|<retires>|<replacement>;…", rendered from packs/ai-models/models.tsv) prints
# "<path>:<line> <id> retires <date>; use <replacement>"; a dated Claude id
# (claude-<family>-<version>-20YYMMDD) not in <known> (the data file's Claude ids,
# space-separated) prints that it is not a published model id. Run as advice (forge_advise).
forge_model_ids() {
  for _mi_f in $FORGE_FILES; do
    [ -f "$_mi_f" ] || continue
    awk -F '\t' -v f="$_mi_f" -v dep="$1" -v known="$2" -v q="'" '
      BEGIN {
        re = "[\"" q "`][A-Za-z0-9._:/@-]+[\"" q "`]"
        n = split(dep, rows, ";")
        for (i = 1; i <= n; i++) if (split(rows[i], c, "|") == 3) { ret[c[1]] = c[2]; rep[c[1]] = c[3] }
        n = split(known, k, " "); for (i = 1; i <= n; i++) kn[k[i]] = 1
      }
      FILENAME == ARGV[1] { if ($1 == f) { if ($2 == "*") all = 1; else want[$2] = 1 } next }
      all || (FNR in want) {
        s = $0
        while (match(s, re)) {
          id = substr(s, RSTART + 1, RLENGTH - 2); s = substr(s, RSTART + RLENGTH)
          sub(/^(anthropic|openai)[:\/]/, "", id)
          if (id in ret) {
            if (seen[FNR, id]++) continue
            if (ret[id] == "-") printf "%s:%d %s is deprecated; use %s\n", f, FNR, id, rep[id]
            else printf "%s:%d %s retires %s; use %s\n", f, FNR, id, ret[id], rep[id]
          } else if (id ~ /^claude-(opus|sonnet|haiku|fable|mythos)-[0-9][0-9-]*-20[0-9][0-9][0-9][0-9][0-9][0-9]$/ && !(id in kn)) {
            if (seen[FNR, id]++) continue
            printf "%s:%d %s is not a published Claude model id; use its alias (e.g. claude-opus-5-5)\n", f, FNR, id
          }
        }
      }' "$FORGE_TMP/added" "$_mi_f"
  done
}
