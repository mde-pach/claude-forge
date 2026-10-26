npm is the package manager: `npm install`, `npm ci`; pnpm, yarn and bun installs ask.
Peer dependency conflicts are resolved, not hidden: `--legacy-peer-deps` and `--force` (flags or `.npmrc`) are rejected, and `npm audit fix --force` asks.
Dependencies come from the registry: git, GitHub and tarball URL specs, and `allow-git=all` / `allow-remote=all`, are rejected.
