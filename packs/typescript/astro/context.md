Astro. `.astro` components render on the server; the default output is static HTML.
`.astro` files are type-checked with `astro check`, not `tsc`.
Content collections are declared in `src/content.config.ts` with the `glob()` / `file()` loaders; entries are identified by `entry.id` (no `slug`).
`Astro.glob()` is removed: use `getCollection()` or `import.meta.glob()`. View transitions use `<ClientRouter />`.
