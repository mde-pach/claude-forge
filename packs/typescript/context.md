TypeScript, strict. Bun is the runtime and package manager.
TypeScript 6 deprecates `baseUrl`, `moduleResolution: "node10"`, `target: "es5"` and `esModuleInterop: false`, and TypeScript 7 removes them: migrate them, never add `ignoreDeprecations`.
No `!` non-null assertions and no `as unknown as T`: narrow with a check, or parse with a schema.
