### kubernetes

Detected by a YAML file whose first document has top-level `apiVersion` and `kind` (`yaml_keys`, §5.1; manifest file names are the project's), or by `**/kustomization.yaml`, `**/kustomization.yml`, `**/Kustomization`, `**/Chart.yaml`.

| Rule | Targets |
|---|---|
| `kubernetes/kube-linter` | `check` lint: `forge_kube_linter --no-new-ignores <checks>`: pinned kube-linter (§13.5) with a config of Forge's own (`doNotAutoAddDefaults`, `include` the checks; a project `.kube-linter.yaml` does not apply) on the changed manifests (first document with `apiVersion` and `kind`, outside a chart's `templates/`), each linted with the manifests of its directory; findings of changed files at line 0: `run-as-non-root`, `privilege-escalation-container`, `privileged-container`, `no-read-only-root-fs`, `unset-memory-requirements`, `latest-tag`, `host-network`, `host-pid`, `host-ipc`, `docker-sock`, `sensitive-host-mounts`, `drop-net-raw-capability`, `mismatching-selector`, `dangling-service`, `deprecated-service-account-field`, `invalid-target-ports`, `liveness-port`, `readiness-port`, `startup-port`, `unsafe-sysctls`, `duplicate-env-var`; a changed manifest also may not gain `ignore-check.kube-linter.io/` annotations (kube-linter has no flag to disregard them) |
| `kubernetes/rbac-advice` | `check` lint, advisory (§10.7): `forge_kube_linter wildcard-in-rules cluster-admin-role-binding` |
| `kubernetes/no-plain-secret` | `ast-grep` forbid (`yaml`, not under `templates/`): `data` / `stringData` of a document with `apiVersion: v1`, `kind: Secret` and no top-level `sops` |

context.md: what kube-linter enforces, no ignore annotations, Secrets through SealedSecret / ExternalSecret / SOPS, RBAC without wildcards or `cluster-admin`.

### kubernetes/helm

Detected by `**/Chart.yaml`.

| Rule | Targets |
|---|---|
| `kubernetes/helm/lint` | `check` lint: `forge_helm` (needs `helm`, a system tool): for each chart with changed files (the nearest `Chart.yaml` above them), `helm lint --strict`, then `helm template forge <chart>` validated by `forge_kubeconform`: pinned kubeconform (§13.5) `-strict` against Kubernetes' schemas and the CRDs catalog, cached in `~/.cache/forge/kubeconform`; a document with no schema is a finding when its group is Kubernetes' own (none, no dot, or `*.k8s.io`: an apiVersion current Kubernetes no longer serves), else `skipped`; schemas that cannot be downloaded are `skipped`; a chart whose dependencies are not built is `skipped` (`helm dependency build`). Findings name the chart, not lines |
| `kubernetes/helm/version-bump` | `check` lint: `forge_chart_version`, only with chart-testing's `ct.yaml` / `ct.yml` at the root (its `check-version-increment` not `false`): each chart under its `chart-dirs` (default `charts`) with a changed file whose `Chart.yaml` `version` equals the base's; a new chart is fine |

### kubernetes/kustomize

Detected by `**/kustomization.yaml`, `**/kustomization.yml` or `**/Kustomization`.

| Rule | Targets |
|---|---|
| `kubernetes/kustomize/no-deprecated-fields` | `ast-grep` forbid on `kustomization.y*ml`: a top-level `bases`, `patchesStrategicMerge`, `patchesJson6902`, `commonLabels`, `imageTags`, `vars` or `helmChartInflationGenerator` key |
