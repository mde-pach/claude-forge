# @module kubernetes
# Kubernetes and Helm: kube-linter, kubeconform, helm lint, chart versions.

# ---- Helm charts: version bump

# forge_chart_ver: the top-level version of a Chart.yaml on stdin.
forge_chart_ver() { sed -n 's/^version:[[:space:]]*//p' | head -n 1 | sed 's/[[:space:]]#.*$//' | tr -d "\"' \t\r"; }

# forge_chart_version: in a chart-testing repository (ct.yaml or ct.yml at the root, its
# check-version-increment not false), each chart under ct's chart-dirs (default charts) with a
# changed file keeps the base's Chart.yaml version: "chart <dir> changed but …". A new chart is fine.
forge_chart_version() {
  for _cv_ct in ct.yaml ct.yml ''; do [ -z "$_cv_ct" ] || [ -f "$_cv_ct" ] && break; done
  [ -n "$_cv_ct" ] || return 0
  grep -qE '^check-version-increment:[[:space:]]*false' "$_cv_ct" && return 0
  _cv_dirs=$(awk '
    /^chart-dirs:/ { on = 1; v = $0; sub(/^chart-dirs:[[:space:]]*/, "", v); if (v ~ /^\[/) { gsub(/[][",\047]/, " ", v); n = split(v, a, /[[:space:]]+/); for (i = 1; i <= n; i++) if (a[i] != "") print a[i]; on = 0 }; next }
    on && /^[[:space:]]+-/ { v = $0; sub(/^[[:space:]]+-[[:space:]]*/, "", v); gsub(/["\047]/, "", v); sub(/[[:space:]]+$/, "", v); sub(/\/$/, "", v); print v; next }
    on && /^[^[:space:]#]/ { on = 0 }' "$_cv_ct")
  [ -n "$_cv_dirs" ] || _cv_dirs=charts
  while IFS= read -r _cv_f; do
    _cv_d=$(dirname "$_cv_f")
    while [ "$_cv_d" != . ] && [ ! -f "$_cv_d/Chart.yaml" ]; do _cv_d=$(dirname "$_cv_d"); done
    [ -f "$_cv_d/Chart.yaml" ] && printf '%s\n' "$_cv_d"
  done < "$FORGE_TMP/changed" | sort -u | while IFS= read -r _cv_c; do
    _cv_in=''
    for _cv_cd in $_cv_dirs; do case $_cv_c in "$_cv_cd"/*) _cv_in=1 ;; esac; done
    [ -n "$_cv_in" ] && base_file "$_cv_c/Chart.yaml" "$FORGE_TMP/chart.base" || continue
    _cv_now=$(forge_chart_ver < "$_cv_c/Chart.yaml")
    _cv_was=$(forge_chart_ver < "$FORGE_TMP/chart.base")
    [ -n "$_cv_was" ] && [ "$_cv_now" = "$_cv_was" ] || continue
    printf 'chart %s changed but its Chart.yaml version is still %s; bump it (chart-testing checks it, and a published chart is otherwise not released)\n' "$_cv_c" "$_cv_now"
  done | grep . && return 1
  return 0
}

# ---- Kubernetes (kubernetes/*)

# forge_kube_manifest <path>: a .yaml / .yml whose first document has top-level apiVersion and
# kind (the kubernetes pack's detection, §5.1), outside a Helm chart's templates/.
forge_kube_manifest() {
  case $1 in *.yaml|*.yml) ;; *) return 1 ;; esac
  case $1 in */templates/*|templates/*) [ -f "${1%%templates/*}Chart.yaml" ] && return 1 ;; esac
  [ -f "$1" ] && awk '
    /^(---|\.\.\.)([[:space:]]|$)/ { if (body) exit; next }
    /^[[:space:]]*(#|$)/ || /^%/ { next }
    { body = 1 }
    /^apiVersion:/ { a = 1 } /^kind:/ { k = 1 }
    END { exit !(a && k) }' "$1"
}

# forge_kube_linter [--no-new-ignores] <check>...: the pinned kube-linter with only the named
# checks (a config of Forge's own: the project's .kube-linter.yaml does not apply) on the changed
# manifests (forge_kube_manifest; every one with --all), each linted with the manifests of its
# directory (dangling-service and mismatching-selector need the objects next to it). Prints
# "path:0 check: message (object)" for the changed files. --no-new-ignores: a changed manifest
# also may not gain ignore-check.kube-linter.io annotations (kube-linter cannot be told to
# disregard them).
forge_kube_linter() {
  _kl_ign=0
  [ "$1" = --no-new-ignores ] && { _kl_ign=1; shift; }
  forge_scope_files | forge_filter forge_kube_manifest > "$FORGE_TMP/kl.changed"
  [ -s "$FORGE_TMP/kl.changed" ] || return 0
  _kl_rc=0
  if [ "$_kl_ign" = 1 ]; then
    while IFS= read -r _kl_f; do
      _kl_now=$(grep -c 'ignore-check\.kube-linter\.io/' "$_kl_f")
      _kl_then=0
      [ -f "$FORGE_TMP/base/$_kl_f" ] && _kl_then=$(grep -c 'ignore-check\.kube-linter\.io/' "$FORGE_TMP/base/$_kl_f")
      if [ "$_kl_now" -gt "$_kl_then" ]; then
        printf '%s:0 adds ignore-check.kube-linter.io annotations (%s -> %s); fix the manifest instead of silencing the check\n' "$_kl_f" "$_kl_then" "$_kl_now"; _kl_rc=1
      fi
    done < "$FORGE_TMP/kl.changed"
  fi
  forge_release kube-linter _kl_bin || return "$_kl_rc"
  awk '{ d = $0; if (sub(/\/[^\/]*$/, "", d) == 0) d = "."; print d }' "$FORGE_TMP/kl.changed" | sort -u > "$FORGE_TMP/kl.dirs"
  git ls-files --cached --others --exclude-standard -- '*.yaml' '*.yml' 2>/dev/null \
    | awk 'FILENAME == ARGV[1] { want[$0] = 1; next } { d = $0; if (sub(/\/[^\/]*$/, "", d) == 0) d = "."; if (d in want) print }' "$FORGE_TMP/kl.dirs" - \
    | forge_filter forge_kube_manifest | sort -u > "$FORGE_TMP/kl.files"
  { printf 'checks:\n  doNotAutoAddDefaults: true\n  include:\n'; for _kl_c in "$@"; do printf '    - %s\n' "$_kl_c"; done; } > "$FORGE_TMP/kl.yaml"
  set -f
  # shellcheck disable=SC2046
  "$_kl_bin" lint --config "$FORGE_TMP/kl.yaml" --format plain --with-color=false $(cat "$FORGE_TMP/kl.files") > "$FORGE_TMP/kl.out" 2>&1
  set +f
  awk -v r1="$(pwd)/" -v r2="$(pwd -P)/" '
    BEGIN { ansi = sprintf("%c", 27) "\\[[0-9;]*m" }
    FILENAME == ARGV[1] { want[$0] = 1; next }
    { gsub(ansi, "") }
    match($0, /: \(object: [^)]*\) /) {
      f = substr($0, 1, RSTART - 1); obj = substr($0, RSTART + 11, RLENGTH - 13); msg = substr($0, RSTART + RLENGTH)
      if (index(f, r1) == 1) f = substr(f, length(r1) + 1); else if (index(f, r2) == 1) f = substr(f, length(r2) + 1)
      if (!(f in want)) next
      check = msg; sub(/^.*\(check: /, "", check); sub(/,.*$/, "", check); sub(/ \(check: .*$/, "", msg)
      print f ":0 " check ": " msg " (" obj ")"; bad = 1
    }
    END { exit bad }' "$FORGE_TMP/kl.changed" "$FORGE_TMP/kl.out" || _kl_rc=1
  return "$_kl_rc"
}

# kubeconform's schemas: Kubernetes' own (default) and the CRDs catalog, cached across runs.
FORGE_KUBECONFORM_CRDS='https://raw.githubusercontent.com/datreeio/CRDs-catalog/main/{{.Group}}/{{.ResourceKind}}_{{.ResourceAPIVersion}}.json'

# forge_kubeconform <label> <file>...: the pinned kubeconform in strict mode (no unknown
# field) on the files ("-" for stdin), against the schemas of Kubernetes (its newest) and the
# CRDs catalog. "<label>: <kind> <name>: <message>" per invalid document. A document whose
# apiVersion has no schema is a finding when its group is one of Kubernetes' own (no group, a
# group without a dot, or *.k8s.io: a removed apiVersion), else reported skipped; schemas
# that cannot be downloaded (offline) are reported skipped, never passed.
forge_kubeconform() {
  forge_release kubeconform _kc_bin || return 0
  _kc_label=$1; shift
  _kc_cache=${XDG_CACHE_HOME:-$HOME/.cache}/forge/kubeconform
  mkdir -p "$_kc_cache" 2>/dev/null || _kc_cache=$FORGE_TMP
  "$_kc_bin" -strict -output json -cache "$_kc_cache" -schema-location default -schema-location "$FORGE_KUBECONFORM_CRDS" "$@" > "$FORGE_TMP/kc.json" 2> "$FORGE_TMP/kc.err"
  awk -v id="$c_id" -v label="$_kc_label" '
    function flush() {
      if (status == "statusInvalid") { print label ": " kind " " name ": " msg; bad = 1 }
      else if (status == "statusError") {
        g = version; if (g ~ /\//) sub(/\/[^\/]*$/, "", g); else g = ""
        if (msg ~ /^could not find schema/) {
          if (g == "" || g !~ /\./ || g ~ /\.k8s\.io$/) { print label ": " kind " " name ": apiVersion " version " is not served by current Kubernetes (no schema); use its current apiVersion"; bad = 1 }
          else print "forge:" id " skipped: " label ": no schema for " kind " (" version ")"
        } else if (msg ~ /^failed downloading schema/) print "forge:" id " skipped: " label ": the schema for " kind " could not be downloaded (offline?)"
        else { print label ": " kind " " name ": " msg; bad = 1 }
      }
      status = ""
    }
    function val(line,   v) { v = line; sub(/^[^:]*: "/, "", v); sub(/",?$/, "", v); gsub(/\\"/, "\"", v); return v }
    /^    \{$/ { status = ""; kind = ""; name = ""; version = ""; msg = "" }
    /^      "kind": / { kind = val($0) } /^      "name": / { name = val($0) } /^      "version": / { version = val($0) }
    /^      "status": / { status = val($0) } /^      "msg": / { msg = val($0); sub(/^problem validating schema\. Check JSON formatting: jsonschema validation failed with .* - /, "", msg) }
    /^    \},?$/ { flush() }
    END { exit bad }' "$FORGE_TMP/kc.json"
}

# forge_helm: for each Helm chart with changed files (the nearest Chart.yaml above a changed
# in-scope file; every chart with --all), `helm lint --strict` and its `helm template` output
# validated by forge_kubeconform. A chart whose dependencies are not built (charts/) is
# reported skipped (helm dependency build). Findings are the chart's, not attributed to lines.
forge_helm() {
  forge_scope_files | while IFS= read -r _hm_f; do
    _hm_d=$(dirname "$_hm_f")
    while :; do
      [ -f "$_hm_d/Chart.yaml" ] && { printf '%s\n' "${_hm_d#./}"; break; }
      [ "$_hm_d" = . ] && break
      _hm_d=$(dirname "$_hm_d")
    done
  done | sort -u > "$FORGE_TMP/helm.charts"
  _hm_rc=0
  while IFS= read -r _hm_c; do
    command helm lint --strict "$_hm_c" > "$FORGE_TMP/helm.out" 2>&1
    _hm_lint=$?
    if grep -q 'chart directory is missing these dependencies' "$FORGE_TMP/helm.out"; then
      printf 'forge:%s skipped: %s: its dependencies are not built (helm dependency build %s)\n' "$c_id" "$_hm_c" "$_hm_c"; continue
    fi
    if [ "$_hm_lint" != 0 ]; then
      grep -E '^\[(ERROR|WARNING)\]|^Error' "$FORGE_TMP/helm.out" | grep -v '^Error: [0-9]* chart(s) linted' | sed "s|^|$_hm_c: helm lint: |"
      _hm_rc=1; continue
    fi
    if ! command helm template forge "$_hm_c" > "$FORGE_TMP/helm.yaml" 2> "$FORGE_TMP/helm.err"; then
      printf '%s: helm template: %s\n' "$_hm_c" "$({ grep '^Error' "$FORGE_TMP/helm.err"; grep -v '^[[:space:]]*$' "$FORGE_TMP/helm.err"; } | head -n 1)"; _hm_rc=1; continue
    fi
    forge_kubeconform "$_hm_c" "$FORGE_TMP/helm.yaml" || _hm_rc=1
  done < "$FORGE_TMP/helm.charts"
  return "$_hm_rc"
}
