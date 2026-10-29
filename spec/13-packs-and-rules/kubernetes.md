<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `kubernetes`

### kubernetes

Kubernetes manifests, linted with kube-linter.

Detected by: a file matching `**/kustomization.yaml`, `**/kustomization.yml`, `**/Kustomization`, `**/Chart.yaml`; or a line matching `^kind:[ \t]*["']?(Deployment\|StatefulSet\|DaemonSet\|ReplicaSet\|ReplicationController\|Pod\|Job\|CronJob\|Service\|Ingress\|IngressClass\|ConfigMap\|Secret\|Namespace\|PersistentVolume\|PersistentVolumeClaim\|StorageClass\|ServiceAccount\|Role\|RoleBinding\|ClusterRole\|ClusterRoleBinding\|NetworkPolicy\|HorizontalPodAutoscaler\|PodDisruptionBudget\|LimitRange\|ResourceQuota\|PriorityClass\|CustomResourceDefinition\|MutatingWebhookConfiguration\|ValidatingWebhookConfiguration\|Gateway\|HTTPRoute)["']?[ \t]*(#.*)?$` in a file matching `**/*.yaml`, `**/*.yml`; or a line matching `^apiVersion:[ \t]*["']?(apps\|batch\|autoscaling\|policy\|networking\.k8s\.io\|rbac\.authorization\.k8s\.io\|storage\.k8s\.io\|apiextensions\.k8s\.io\|admissionregistration\.k8s\.io\|scheduling\.k8s\.io\|gateway\.networking\.k8s\.io)/v[0-9]` in a file matching `**/*.yaml`, `**/*.yml`.

Context (in `.claude/rules/forge-*.md`, §7):

- Kubernetes. Containers run as non-root with a read-only root filesystem, no privilege escalation, a memory limit and a pinned image tag (not `latest`); no host network, PID or IPC, no docker socket or sensitive host mounts.
- `ignore-check.kube-linter.io/*` annotations are not added to silence a finding; fix the manifest.
- Secrets are not committed as `kind: Secret` with `data` or `stringData`; use SealedSecret, ExternalSecret or SOPS-encrypted files.
- RBAC grants named verbs and resources, not `*`, and does not bind `cluster-admin`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `kubernetes/kube-linter` | Changed workloads pass kube-linter's run-as-non-root, privilege-escalation-container, privileged-container, no-read-only-root-fs, unset-memory-requirements, latest-tag, host-network, host-pid, host-ipc, docker-sock, sensitive-host-mounts, drop-net-raw-capability, mismatching-selector, dangling-service, deprecated-service-account-field, invalid-target-ports, liveness-port, readiness-port, startup-port, unsafe-sysctls and duplicate-env-var checks, and gain no ignore-check.kube-linter.io annotations. | any: `**/*.yaml`, `**/*.yml` | check lint: a shell script calling `forge_kube_linter` |
| `kubernetes/no-plain-secret` | No committed v1 Secret with data or stringData (base64 is not encryption) unless the document is SOPS-encrypted (a top-level sops key); use SealedSecret, ExternalSecret or SOPS. | yaml: `**/*.yaml`, `**/*.yml`, `!**/templates/**` | ast-grep forbid |
| `kubernetes/rbac-advice` | Changed RBAC objects pass kube-linter's wildcard-in-rules and cluster-admin-role-binding checks (grant named verbs and resources, not * or cluster-admin). | any: `**/*.yaml`, `**/*.yml` | check lint: `forge_advise forge_kube_linter wildcard-in-rules cluster-admin-role-binding`<br>advice, never blocks |

### kubernetes/helm

Helm charts, linted and rendered.

Detected by: a file matching `**/Chart.yaml`.
Child of `kubernetes`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `kubernetes/helm/chart-api-v2` | Chart.yaml declares apiVersion: v2 (Helm 3); v1 with requirements.yaml is the Helm 2 format. | any: `**/Chart.yaml` | grep forbid `^apiVersion:[[:space:]]*["']?v1["']?[[:space:]]*$` |
| `kubernetes/helm/lint` | A changed chart passes helm lint --strict, and its helm template output validates with kubeconform in strict mode (no unknown fields, no apiVersion current Kubernetes no longer serves). | any | check lint: `forge_helm` |
| `kubernetes/helm/version-bump` | In a chart-testing repository (ct.yaml or ct.yml; check-version-increment not false), a chart under its chart-dirs (default charts/) with changed files bumps its Chart.yaml version. | any | check lint: `forge_chart_version` |

### kubernetes/kustomize

Kustomize bases and overlays.

Detected by: a file matching `**/kustomization.yaml`, `**/kustomization.yml`, `**/Kustomization`.
Child of `kubernetes`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `kubernetes/kustomize/no-deprecated-fields` | kustomization files do not use the fields kustomize v5 deprecates: bases, patchesStrategicMerge, patchesJson6902, commonLabels, imageTags, vars, helmChartInflationGenerator. Use resources, patches, labels, images, replacements and helmCharts (kustomize edit fix rewrites them). | yaml: `**/kustomization.yaml`, `**/kustomization.yml` | ast-grep forbid |
