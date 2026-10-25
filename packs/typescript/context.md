TypeScript, strict. Bun is the runtime and package manager.
TypeScript 6 deprecates `baseUrl`, `moduleResolution: "node10"`, `target: "es5"` and `esModuleInterop: false`, and TypeScript 7 removes them: migrate them; `ignoreDeprecations` in a tsconfig is rejected.
`!` non-null assertions and `as unknown as T` are rejected: narrow with a check, or parse with a schema.
Server code that fetches a URL taken from the request (searchParams, the body, form data, a Server Action argument) checks the host against an allow-list first (`isAllowedUrl()`, `ALLOWED_HOSTS`); a miss gets advice, not a block.
