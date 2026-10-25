## CLI fails to load SvelteKit auth configs that import `$app/env/private` / `$app/env/public`

### Problem

SvelteKit 2.63+ offers *explicit environment variables*. Projects opt in through `experimental.explicitEnvironmentVariables`. In this mode, variables are declared in the project's `src/env.ts` and imported from two virtual modules:

```ts
import { MY_SECRET_KEY } from "$app/env/private";
import { MY_PUBLIC_KEY } from "$app/env/public";
```

The better-auth CLI already stubs SvelteKit's virtual modules so it can load a project's auth config outside of Vite. Covered modules include `$env/static/*`, `$env/dynamic/*` and `$app/*`. The two modules above are not stubbed, so any auth config that imports from them fails to load. This affects `getConfig` and every command built on it, such as schema generation.

### Requirements

- When the CLI loads a config (for example via `getConfig({ cwd, configPath, shouldThrowOnError: true })`), imports from `$app/env/private` and `$app/env/public` must resolve without error.
- Any named import from either module must evaluate to the current `process.env` value of the variable with that name.
  - The names are arbitrary. They are whatever the project declares in `src/env.ts`, which the CLI does not parse. Do not limit them to a fixed list.
  - Do not apply any prefix (such as `PUBLIC_`) or prefix-based filtering. In this mode, the public/private split comes from per-variable config, not from naming.
  - Example: with `MY_SECRET_KEY=secret` and `MY_PUBLIC_KEY=public` set in the environment, the imports above yield `"secret"` and `"public"`. A config such as `betterAuth({ appName: MY_SECRET_KEY === "secret" && MY_PUBLIC_KEY === "public" ? "ok" : "bad", ... })` then loads with `appName: "ok"`.
- An imported name that is not set in the environment must resolve to `undefined` rather than throwing.
- The existing stubs for `$env/*`, `$app/*` and Vite asset imports must keep their current behaviour.
