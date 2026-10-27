Kubernetes. Changed manifests are linted by kube-linter (Forge's config): containers run as non-root with a read-only root filesystem, no privilege escalation, a memory limit and a pinned image tag (not `latest`); no host network, PID or IPC, no docker socket or sensitive host mounts.
`ignore-check.kube-linter.io/*` annotations are not added to silence a finding; fix the manifest.
Secrets are not committed as `kind: Secret` with `data` or `stringData`; use SealedSecret, ExternalSecret or SOPS-encrypted files.
RBAC grants named verbs and resources, not `*`, and does not bind `cluster-admin`.
