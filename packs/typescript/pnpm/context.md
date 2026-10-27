pnpm is the package manager: `pnpm add`, `pnpm install`; npm, yarn and bun installs ask.
pnpm settings (overrides, allowBuilds, minimumReleaseAge, nodeLinker, …) go in `pnpm-workspace.yaml`; a `pnpm` field in package.json and pnpm settings in `.npmrc` (registry and auth only) are rejected.
