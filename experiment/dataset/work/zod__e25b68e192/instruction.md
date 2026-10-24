## Unreachable declarations survive tree-shaking in esbuild bundles

When a consumer bundles Zod v4 with **esbuild** (bundle + minify + tree-shaking, ESM output), several module-level declarations end up in every bundle even when the importing code never uses them. Rollup already removes them, so only esbuild users pay this cost. It is significant: a bundle containing only `z.boolean()` is roughly 10% larger, gzipped, than it needs to be.

### Reproduction

Use esbuild with `bundle: true`, `minify: true`, `format: "esm"`, `target: "es2020"` and `treeShaking: true` to bundle this entry:

```ts
import * as z from "zod/mini"; // or the classic "zod" entrypoint
console.log(z.boolean().parse(true));
```

The output still contains:

- the `$brand` symbol, visible as the string `"zod_brand"`;
- the number-format range table `NUMBER_FORMAT_RANGES`, visible as references to `MAX_SAFE_INTEGER`;
- the ISO date regex (`regexes.date`), visible as the `02-29` leap-day fragment of its pattern.

The classic entrypoint also retains the registry symbols `$output` and `$input`, visible as the strings `"ZodOutput"` and `"ZodInput"`.

### Requirements

1. **`zod/mini` entrypoint.** Bundle the program above with esbuild using those settings. The minified output must:
   - still contain the boolean schema (the text `boolean`);
   - contain none of `zod_brand`, `MAX_SAFE_INTEGER` or `02-29`.
2. **Classic `zod` entrypoint.** Bundle the same program with the same settings. The output must:
   - still contain `boolean`;
   - contain neither `ZodOutput` nor `ZodInput`.
3. **Unchanged runtime behaviour and public API.**
   - `$brand`, `$output` and `$input` remain exported unique symbols with the same descriptions (`"zod_brand"`, `"ZodOutput"`, `"ZodInput"`).
   - `NUMBER_FORMAT_RANGES` keeps the same type and values.
   - The exported `date` regex keeps exactly the same `.source` and `.flags`.
   - Schemas that *do* use these declarations behave exactly as before. Examples are ISO date validation, number formats such as `int32` and `safeint`, and branding.
4. **No Rollup regression.** Rollup output must not get larger.
5. **Build tooling.** `esbuild` must be available as a dev dependency of the `zod` package so these bundles can be produced from within the package's test environment.
