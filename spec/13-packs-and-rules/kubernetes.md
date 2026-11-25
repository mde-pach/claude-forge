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

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `kubernetes/gitops-debug` | Finding why an Argo CD application or a Flux resource is out of sync, degraded or stuck on a live cluster. | on-demand | The task is to find why an Argo CD application or a Flux kustomization or Helm release fails to sync, reconcile or become healthy. | wherever its pack is |
| `kubernetes/manifest-diff` | Seeing what a manifest change would apply, by rendering it and diffing it against the cluster. | on-demand | The task changes Kubernetes manifests, a kustomization or chart values that a cluster deploys.<br>The task asks what a manifest change would change on the cluster. | wherever its pack is |
| `kubernetes/pod-debug` | Finding why a workload crashes or misbehaves on a live cluster, with kubectl. | on-demand | The task is to find why a pod, deployment or job crashes, restarts or misbehaves on a cluster. | wherever its pack is |

### kubernetes/argocd

Argo CD GitOps repositories (Applications, ApplicationSets, AppProjects) and Argo Rollouts.

Detected by: a line matching `^kind:[ \t]*["']?(Application\|ApplicationSet\|AppProject\|Rollout)["']?[ \t]*(#.*)?$` in a file matching `**/*.yaml`, `**/*.yml`.
Child of `kubernetes`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `kubernetes/argocd/cluster-crds` | Reading which Argo CD and Argo Rollouts kinds, versions and fields the target cluster really serves, before writing manifests for it. | on-demand | The task writes or changes Argo CD or Argo Rollouts manifests for a specific cluster. | wherever its pack is |
| `kubernetes/argocd/render-applications` | Mapping an Argo CD repository's Applications, ApplicationSets and projects, and rendering the manifests each one deploys. | on-demand | The task reviews or changes an Argo CD GitOps repository, its Applications, ApplicationSets or overlays. | wherever its pack is |

### kubernetes/flux

Flux GitOps repositories (Kustomizations, HelmReleases and their sources).

Detected by: a file matching `**/flux-system/gotk-components.yaml`; or a line matching `^apiVersion:[ \t]*["']?(kustomize\|helm\|source\|notification\|image)\.toolkit\.fluxcd\.io/` in a file matching `**/*.yaml`, `**/*.yml`.
Child of `kubernetes`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `kubernetes/flux/build-and-validate` | Inventorying a Flux repository, rendering each Kustomization as Flux would and validating the manifests against their schemas. | on-demand | The task writes or changes Flux manifests, Kustomizations, HelmReleases or overlays in a GitOps repository.<br>The task is to find why a Kubernetes overlay or deployed configuration differs from what is expected. | wherever its pack is |

### kubernetes/helm

Helm charts, linted and rendered.

Detected by: a file matching `**/Chart.yaml`.
Child of `kubernetes`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `kubernetes/helm/chart-api-v2` | Chart.yaml declares apiVersion: v2 (Helm 3); v1 with requirements.yaml is the Helm 2 format. | any: `**/Chart.yaml` | grep forbid `^apiVersion:[[:space:]]*["']?v1["']?[[:space:]]*$` |
| `kubernetes/helm/lint` | A changed chart passes helm lint --strict, and its helm template output validates with kubeconform in strict mode (no unknown fields, no apiVersion current Kubernetes no longer serves). | any | check lint: `forge_helm` |
| `kubernetes/helm/version-bump` | In a chart-testing repository (ct.yaml or ct.yml; check-version-increment not false), a chart under its chart-dirs (default charts/) with changed files bumps its Chart.yaml version; one bump per branch is enough. | any | check lint: `forge_chart_version` |

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `kubernetes/helm/render` | Reading what a Helm chart renders for given values, and what an upgrade would change. | on-demand | The task changes a Helm chart's templates or values.<br>The task is to debug what a Helm chart renders or why an install or upgrade fails. | wherever its pack is |

### kubernetes/istio

Istio service mesh configuration (traffic routing, security and telemetry resources).

Detected by: a line matching `^apiVersion:[ \t]*["']?(networking\|security\|telemetry\|extensions)\.istio\.io/` in a file matching `**/*.yaml`, `**/*.yml`.
Child of `kubernetes`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `kubernetes/istio/proxy-debug` | Checking the routing, endpoints and policies an Istio proxy really applied, with istioctl analyze and proxy-config. | on-demand | The task changes Istio routing or traffic policy, such as a VirtualService, DestinationRule, Gateway or AuthorizationPolicy.<br>The task is to find why traffic in a service mesh fails, or is routed, split or retried differently from its configuration. | wherever its pack is |

### kubernetes/kustomize

Kustomize bases and overlays.

Detected by: a file matching `**/kustomization.yaml`, `**/kustomization.yml`, `**/Kustomization`.
Child of `kubernetes`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `kubernetes/kustomize/no-deprecated-fields` | kustomization files do not use the fields kustomize v5 deprecates: bases, patchesStrategicMerge, patchesJson6902, commonLabels, imageTags, vars, helmChartInflationGenerator. Use resources, patches, labels, images, replacements and helmCharts (kustomize edit fix rewrites them). | yaml: `**/kustomization.yaml`, `**/kustomization.yml` | ast-grep forbid |

### kubernetes/linkerd

Linkerd, the service mesh the Kubernetes manifests run in.

Detected by: a line matching `linkerd\.io/inject:` in a file matching `**/*.yaml`, `**/*.yml`; or a line matching `^apiVersion:[ \t]*["']?(policy\|split\|multicluster)\.linkerd\.io/` in a file matching `**/*.yaml`, `**/*.yml`.
Child of `kubernetes`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `kubernetes/linkerd/linkerd-check` | Checking the Linkerd mesh and a workload's live traffic with the linkerd CLI. | on-demand | The task debugs or checks the traffic, mTLS or policies between workloads in a Linkerd mesh.<br>The task checks in the running cluster whether a workload is meshed by Linkerd or its traffic uses mTLS. | wherever its pack is |
