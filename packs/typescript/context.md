TypeScript, strict. Bun is the runtime and package manager.
TypeScript 6 deprecates `baseUrl`, `moduleResolution: "node10"`, `target: "es5"` and `esModuleInterop: false`, and TypeScript 7 removes them: migrate them; `ignoreDeprecations` in a tsconfig is rejected.
`!` non-null assertions and `as unknown as T` are rejected: narrow with a check, or parse with a schema.
Before writing a utility, search the codebase for an existing helper (warn-once and logging helpers, `util.*` functions such as an existing property setter) and reuse it instead of adding a near-duplicate.
