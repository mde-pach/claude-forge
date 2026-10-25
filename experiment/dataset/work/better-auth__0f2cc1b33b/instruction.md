# `auth generate --output` fails on first run when the config imports the file it generates

## Problem

Some setups (for example the Convex integration guide's template) have an auth config that imports the schema file `auth generate --output <path>` is meant to create, e.g. `import schema from "./schema"` in `convex/betterAuth/auth.ts` with `--output convex/betterAuth/schema.ts`. On a first run that file does not exist, so loading the config throws `Cannot find module './schema'` and generation never runs.

Separately, `--output` paths are not consistently resolved against the CLI's `--cwd`. Some checks resolve against `process.cwd()` instead, notably the Prisma generator's existence/overwrite checks. So when `--cwd` differs from the launch directory, the wrong file is read or written.

## Requirements

### `getConfig` (packages/cli `utils/get-config`)
- Accepts a new optional option `outputPath?: string`, alongside `cwd`, `configPath` and `shouldThrowOnError`.
- A relative `outputPath` is resolved against the given `cwd`, never `process.cwd()`.
- Recovery applies when loading the config fails with a `MODULE_NOT_FOUND` error for a **relative** specifier (`./` or `../`). The specifier, resolved from the importing file, must point at exactly the resolved output path; source extensions such as `.ts` and `.js` are ignored in the comparison. The output file must also not exist yet. In that case:
  - Create an empty placeholder file at that path (creating directories as needed).
  - Retry loading.
  - Return the config normally. The placeholder remains, with empty content.
- If the retry fails for any other reason, remove the placeholder and rethrow that error. Its message is unchanged, e.g. `Cannot find module './other'`.
- Never create a placeholder for:
  - unrelated missing relative imports;
  - bare package specifiers (e.g. `import schema from "schema"`), even if the name matches the output file's basename.

  These errors propagate as before.

### `auth generate`
- Resolve `--output` against `--cwd`, use that resolved path for the final write, and pass it on to the schema generators.
- If a placeholder was created during this run (the output file did not exist beforehand), remove it whenever the command ends without writing a schema:
  - no config found;
  - adapter error;
  - "already up to date";
  - user aborts.
- Generators must handle absolute `file` paths without re-joining them to `process.cwd()`. The Prisma generator in particular must check, and read, the existing schema at the absolute path given.

### Model-name resolution (`initGetDefaultModelName` in `@better-auth/core`)
- When a string is both a canonical schema key and some other table's `modelName`, the exact schema-key match wins.
  - With `user: { modelName: "account" }` and `account: { modelName: "identity" }`, resolving `"account"` returns `"account"`.
  - Resolving `"identity"` still returns `"account"`.
- `modelName` lookup is only a fallback when no schema key matches.
