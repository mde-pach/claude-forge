You are working in the git repository in the current directory. Make the change described below. Work autonomously: nobody will answer questions.

# Default English error messages lost in tree-shaken bundles (zod v4)

## Problem

When an application bundles `zod/v4` with a bundler that honours `sideEffects: false`, every validation error message becomes the generic fallback `"Invalid input"`. Examples of such bundlers include Rollup with `@rollup/plugin-node-resolve` and aggressive tree-shaking (`preset: "smallest"`), as well as webpack and esbuild. The published package contains stub `package.json` files that mark modules as side-effect free. The registration of English as the default locale currently happens as a module-level side effect, so these bundlers drop it.

Unbundled usage works today. The bug appears only in the shipped build after tree-shaking.

## Required behaviour

- English must be registered as the default locale no later than the first construction of any classic `zod` schema (any `ZodType`). This must hold even when tree-shaking removes module-level side effects from the built package.
- The default must only fill in a missing locale. It must never overwrite a locale the user has configured.
- After a reset with `z.config({ localeError: undefined })`, `z.config().localeError` stays `undefined` until a schema is constructed. Constructing any schema, for example `z.enum(["km", "mi"])`, sets `z.config().localeError` to a defined value. Parsing then produces English messages.
- An explicit `z.config(z.locales.xx())` call takes precedence regardless of ordering:
  - if it is called **before** any schema is constructed, that locale stays in effect;
  - if it is called **after** a schema is constructed, it replaces the English default.

## Expected messages

For `z.enum(["km", "mi"]).safeParse("feet")`:

- **Default (English):** the first issue's message is `Invalid option: expected one of "km"|"mi"`. This must hold both in normal usage and when `import * as z from "zod/v4"` is bundled from the **built** package with Rollup (`@rollup/plugin-node-resolve`, `treeshake: { preset: "smallest", annotations: true }`), output as ESM and executed.
- **With `z.config(z.locales.fr())`:** the message is `Option invalide : une valeur parmi "km"|"mi" attendue`. The same Rollup-bundled scenario must also produce this message.

## Build-output verification

Verifying the fix requires bundling the built output of `packages/zod`, which provides the `v4/index.js` entry. The build must be up to date with `src`, so run `pnpm build` before checking the bundled behaviour.

If a regression check of this kind is added, it must fail with a clear error in two cases, rather than passing against a stale build:

- the built entry is missing;
- the built entry is older than the sources.

`rollup` and `@rollup/plugin-node-resolve` must be available as dev dependencies wherever such a check lives (for example, a `packages/treeshake` workspace package).
