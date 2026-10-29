<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `typescript`

### typescript

TypeScript projects, type-checked with tsc.

Detected by: a dependency on `typescript` (npm); or a file matching `tsconfig.json`.

Context (in `.claude/rules/forge-*.md`, §7):

- TypeScript, strict.
- No `ignoreDeprecations` in a tsconfig, and no new `baseUrl`, `moduleResolution: "node10"`, `target: "es5"` or `esModuleInterop: false` (deprecated in TypeScript 6, removed in 7).
- `!` non-null assertions and `as unknown as T` are rejected: narrow with a check, or parse with a schema.
- Type-only imports and re-exports use `import type` / `export type`. `JSON.parse` and `res.json()` results stay `unknown` until a schema validates them; literals take `satisfies T` or an annotation, not `as T`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/a11y` | Biome's accessibility rules are errors in JSX and in Vue, Svelte, Astro and HTML templates (the European Accessibility Act applies to e-commerce since June 2025); Biome lints templates with html.experimentalFullSupportEnabled. | any: `**/*.{tsx,jsx,vue,svelte,astro,html}` | tool biome `lint/a11y/useAltText`<br>tool biome `lint/a11y/useButtonType` |
| `typescript/array-sort-compare` | .sort() and .toSorted() always get a compare function (strings too, (a, b) => a.localeCompare(b)); the default compares as strings, so [10, 9].sort() is [10, 9]. | typescript | tool biome `lint/suspicious/useArraySortCompare`<br>tool eslint `@typescript-eslint/require-array-sort-compare` |
| `typescript/config-files` | Claude asks you before editing the Biome, TypeScript, ESLint, Oxlint, Deno, Prettier, package manager (pnpm-workspace.yaml, .npmrc, .yarnrc.yml, bunfig.toml) and Turborepo configs; package.json is covered by typescript/config-ratchet. | any | permission ask `Edit(**/biome.json)`<br>permission ask `Edit(**/biome.jsonc)`<br>permission ask `Edit(**/tsconfig*.json)`<br>permission ask `Edit(**/eslint.config.*)`<br>permission ask `Edit(**/.oxlintrc.json)`<br>permission ask `Edit(**/deno.json)`<br>permission ask `Edit(**/deno.jsonc)`<br>permission ask `Edit(**/.prettierrc*)`<br>permission ask `Edit(**/prettier.config.*)`<br>permission ask `Edit(**/pnpm-workspace.yaml)`<br>permission ask `Edit(**/.npmrc)`<br>permission ask `Edit(**/.yarnrc.yml)`<br>permission ask `Edit(**/bunfig.toml)`<br>permission ask `Edit(**/turbo.json)` |
| `typescript/config-ratchet` | Biome, tsconfig and package.json test scripts are not loosened (rules lowered or turned off, strict flags removed, paths excluded, --passWithNoTests-like flags added); loosening one is the user's decision: ask them. | any: `**/biome.json`, `**/biome.jsonc`, `**/tsconfig*.json`, `**/package.json` | check lint: `forge_ratchet bun ratchet.mjs` (when `**/biome.json`, `**/biome.jsonc`, `**/tsconfig*.json` or `**/package.json` changes) |
| `typescript/cors-credentials-wildcard` | CORS with credentials true names its allowed origins. origin "*" is refused by browsers with credentials; origin true, or a function that returns the origin it is given (alone or with a ?? / \|\| fallback, as an expression or a return), lets any site make credentialed requests. Covers cors (Express), hono/cors, @fastify/cors and Nest's enableCors. | typescript | ast-grep forbid (where `$CORS`, `$PLUGIN`, `$APP`, `$NEST`: types engine, §15) |
| `typescript/env-example` | A key added to a Zod env schema (t3-env createEnv, or z.object(...) parsed from process.env) is listed in the .env template next to it (.env.example, .env.template, .env.sample, .env.dist, .env.<name>.example): always when it has no .default() or .optional(), and also otherwise when the template already lists at least half of the schema's keys. Checked at Stop and pre-commit. | typescript | check lint: `forge_env_example ts` |
| `typescript/error-cause` | A catch block that throws a new error passes the caught one as its cause: new Error("…", { cause: err }). | typescript | tool biome `lint/style/useErrorCause` |
| `typescript/format-lint` | TypeScript and JavaScript files, and the Vue, Svelte, Astro and HTML templates Biome reads, are formatted and linted with Biome. | any: `**/*.{ts,tsx,mts,cts,js,jsx,mjs,cjs,vue,svelte,astro,html}` | check lint: `forge_do biome ci .` |
| `typescript/iframe-sandbox` | No iframe sandbox with both allow-scripts and allow-same-origin: together they let the framed page remove its own sandbox. | tsx | tool biome `lint/nursery/noUnsafeIframeSandbox` |
| `typescript/implicit-returns` | Every code path of a function that returns a value returns one (noImplicitReturns); a branch that falls through would yield undefined. | typescript | tool tsc `TS7030` |
| `typescript/lockfile-updated` | A package.json whose dependencies, devDependencies or optionalDependencies changed comes with a lock file (pnpm-lock.yaml, package-lock.json, yarn.lock or bun.lock, when the project has one) whose changed lines name each changed dependency: run the package manager to update it. A lock change that names none of them (a blank line) does not count. bun.lock is left to typescript/bun/lock-sync (bun install --frozen-lockfile) when that rule is active. At Stop it blocks once per turn, then accepts. | any | check lint: `forge_once forge_lockfile_updated` (when `**/package.json` changes) |
| `typescript/no-banned-types` | No Function, Object, String, Number, Boolean or {} as types; write the function signature, object shape or primitive type. | typescript | tool biome `lint/complexity/noBannedTypes` |
| `typescript/no-console` | No console.log, info or debug in application code; console.warn and console.error stay allowed. | typescript | tool biome `lint/suspicious/noConsole` |
| `typescript/no-const-enum` | No const enum; isolated transpilers and type stripping cannot inline it. Use a union of literals or an as const object. | typescript | tool biome `lint/suspicious/noConstEnum` |
| `typescript/no-deprecated` | No use of a declaration marked @deprecated (the project's, a library's, @types/node's): a call, construction, property, type or import TypeScript reports as deprecated (for an overloaded function, the signature the call resolves to), or an object literal key whose property is deprecated. Use the replacement the deprecation names. | typescript | ast-grep forbid (where `$N`: types engine, §15)<br>applies where `typescript >=5.0.0` (npm) declared |
| `typescript/no-double-assertion` | No double type assertions (x as unknown as T). Parse with a schema or narrow with a type guard. | typescript | ast-grep forbid |
| `typescript/no-dynamic-regexp` | A RegExp built from a non-literal pattern (new RegExp(input), RegExp(`^${p}`)) escapes it (RegExp.escape() where the runtime has it, ES2025 / Node 24, else an escaping helper) unless the text is meant as a pattern: unescaped input changes what matches and can backtrack for ever. | typescript | ast-grep advise<br>advice, never blocks |
| `typescript/no-empty-block` | No empty block statements; an intentionally empty block says why in a comment. | typescript | tool biome `lint/suspicious/noEmptyBlockStatements` |
| `typescript/no-eval` | No eval(). Parse data with JSON.parse or a schema; never run strings as code. | typescript | tool biome `lint/security/noGlobalEval`<br>ast-grep forbid |
| `typescript/no-explicit-any` | No explicit any. Use unknown with narrowing, or a Zod schema. | typescript | tool biome `lint/suspicious/noExplicitAny`<br>ast-grep forbid |
| `typescript/no-focused-tests` | No focused tests (.only). They silently turn every other test into a pass. | typescript: `@ts-tests`, `@e2e-tests` | tool biome `lint/suspicious/noFocusedTests`<br>ast-grep forbid |
| `typescript/no-hardcoded-session-secret` | No string literal as a signing secret (express-session, cookie-parser, better-auth, hono/jwt, jsonwebtoken sign/verify, @fastify/cookie, @fastify/session). Read it from the environment or a secret store; a secret in the code signs every session for whoever reads the repository. | typescript: `!**/*.test.*`, `!**/*.spec.*`, `!**/__tests__/**`, `!e2e/**` | ast-grep forbid (where `$OPTS`, `$PLUGIN`, `$JWT`, `$COOKIES`: types engine, §15) |
| `typescript/no-html-sinks` | No HTML built from a value written into the DOM (XSS): no innerHTML or outerHTML assigned anything but a constant string, and no insertAdjacentHTML, document.write, writeln, createContextualFragment or setHTMLUnsafe whose HTML argument is not constant. Set textContent, build elements, or sanitise in one place. | typescript | ast-grep forbid (where `$E`: types engine, §15) |
| `typescript/no-ignore-deprecations` | No "ignoreDeprecations" in tsconfig files. TypeScript 7 removes the options it silences; migrate them instead. | any: `tsconfig*.json`, `**/tsconfig*.json` | check lint: `! grep -n '"ignoreDeprecations"' /dev/null $(git ls-files '*tsconfig*.json')` (when `tsconfig*.json` or `**/tsconfig*.json` changes) |
| `typescript/no-implicit-any` | Implicit any is a type error (noImplicitAny, part of strict). | typescript | tool tsc `TS7006` |
| `typescript/no-new-suppressions` | No new suppression comments (biome-ignore, eslint-disable and oxlint-disable in any form, deno-lint-ignore, prettier-ignore, @ts-expect-error) or coverage ignore hints (istanbul ignore, c8 ignore, v8 ignore); a changed file may not have more than it had. Fix the finding instead. | typescript | ast-grep no-increase |
| `typescript/no-non-null-assertion` | No non-null assertions (x!). Check for null, use optional chaining, or narrow the type. | typescript | tool biome `lint/style/noNonNullAssertion`<br>ast-grep forbid |
| `typescript/no-object-literal-assertion` | No type assertion on an object or array literal ({ … } as T, [x] as T[]); it hides missing and misspelled fields. Annotate the variable, use satisfies T, or pass a type argument; as const and an empty [] are fine. | typescript | ast-grep forbid |
| `typescript/no-proto` | No __proto__; use Object.getPrototypeOf, Object.setPrototypeOf or Object.create. | typescript | tool biome `lint/suspicious/noProto` |
| `typescript/no-secrets` | No API keys, tokens or private keys in code. Read them from the environment. | typescript | tool biome `lint/security/noSecrets` |
| `typescript/no-stub` | No stub left behind. A throw of "Not implemented" (or "TODO", "unimplemented"), or a named function or method with an empty body, is a stub; overload signatures, interfaces and constructors with parameter properties are not. | typescript: `!**/*.d.ts` | ast-grep forbid |
| `typescript/no-swallowed-catch` | A catch block rethrows or uses the error it caught; .catch(() => {}) is not allowed either. Handle the error, log it, or let it propagate. | typescript | ast-grep forbid |
| `typescript/no-ts-ignore` | No @ts-ignore. Fix the type, or use @ts-expect-error with a reason. | typescript | tool biome `lint/suspicious/noTsIgnore`<br>ast-grep forbid |
| `typescript/no-ts-nocheck` | No @ts-nocheck. It turns off type checking for the whole file. | typescript | ast-grep forbid |
| `typescript/no-unchecked-json` | The result of JSON.parse() or a fetch Response's .json() is not asserted (as T) or annotated (const x: T =) to a type it was never checked against: take it as unknown and validate it with a schema. | typescript | ast-grep forbid (where `$R`: types engine, §15) |
| `typescript/no-unused-suppressions` | No biome-ignore comment that suppresses nothing. Biome reports these as warnings only (no setting makes them errors), so this check fails on them. | typescript | check lint: `! forge_do biome lint . 2>&1 \| grep -E '^[^[:space:]]+:[0-9]+:[0-9]+ suppressions/unused'` |
| `typescript/no-unused` | No unused imports or variables. | typescript | tool biome `lint/correctness/noUnusedImports`<br>tool biome `lint/correctness/noUnusedVariables` |
| `typescript/node-version-file` | The root .nvmrc / .node-version is a Node version (not an alias like lts/*) that satisfies package.json engines.node and devEngines.runtime (name node); a partial version (22) stands for the newest release of that line. | any: `.nvmrc`, `.node-version`, `package.json` | check lint: `forge_helper bun node_version.mjs` (when `.nvmrc`, `.node-version` or `package.json` changes) |
| `typescript/postmessage-origin` | window.postMessage is never sent to targetOrigin "*": the message reaches whatever page the frame has navigated to. Name the receiver's origin. | typescript | ast-grep forbid (where `$W`: types engine, §15) |
| `typescript/strict-function-types` | Function parameters are checked contravariantly (strictFunctionTypes, part of strict). | typescript | tool tsc `TS2322` |
| `typescript/strict-null-checks` | null and undefined are checked (strictNullChecks, part of strict). | typescript | tool tsc `TS18047` |
| `typescript/ts-expect-error-reason` | Every @ts-expect-error says why, after the directive. | typescript | ast-grep forbid |
| `typescript/typecheck` | The project passes tsc. | typescript | check typecheck: `forge_do tsc typecheck-incremental "$(forge_state_file tsc.tsbuildinfo)"` |
| `typescript/unchecked-index` | Indexed access can be undefined (noUncheckedIndexedAccess); check the element before using it. | typescript | tool tsc `TS18048` |
| `typescript/verbatim-imports` | Imports and re-exports used only as types are written import type / export type, so type stripping and isolated transpilers drop them instead of importing a module that has no such value at run time. | typescript | tool biome `lint/style/useImportType`<br>tool biome `lint/style/useExportType` |

### typescript/ai

The Vercel AI SDK (ai).

Detected by: a dependency on `ai` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- (when `ai >=7.0.0` (npm) declared) AI SDK 7: system prompts go in `instructions`; a `role: "system"` message in `messages` / `prompt` is rejected unless `allowSystemInMessages: true`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/ai/no-system-in-messages` | No { role: "system" } entry in the messages (or prompt) array literal of a generateText or streamText call from ai 7, unless the same options set allowSystemInMessages: true: ai 7 rejects it with InvalidPromptError. Put it in instructions. | typescript | ast-grep forbid (where `$F`: types engine, §15)<br>applies where `ai >=7.0.0` (npm) declared |

### typescript/angular

Angular 20+ templates and security APIs.

Detected by: a dependency on `@angular/core >=20.0.0` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Angular 20+: templates use built-in control flow (`@if`, `@for (x of xs; track x.id)`, `@switch`); `*ngIf`, `*ngFor` and `[ngSwitch]` are deprecated and rejected (`ng generate @angular/core:control-flow` migrates them).
- `DomSanitizer.bypassSecurityTrust*` turns off Angular's XSS protection and is rejected; bind values and let Angular sanitise them.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/angular/control-flow` | Templates use built-in control flow (@if, @for (...; track ...), @switch), not the deprecated *ngIf, *ngFor, [ngSwitch], *ngSwitchCase or *ngSwitchDefault. ng generate @angular/core:control-flow migrates them. | any: `**/*.html`, `**/*.ts` | grep forbid `\*ng(If\|For\|SwitchCase\|SwitchDefault)[[:space:]]*=\|\[ngSwitch\][[:space:]]*=` |
| `typescript/angular/no-bypass-sanitizer` | No DomSanitizer.bypassSecurityTrustHtml, Script, Style, Url or ResourceUrl: they turn off Angular's XSS protection for that value. Bind the value and let Angular sanitise it. | typescript | ast-grep forbid (where `$S`: types engine, §15) |
| `typescript/angular/typecheck` | The Angular compiler type-checks the templates: ngc -p <tsconfig> --noEmit for each tsconfig of file role @angular-tsconfig (each angular.json project's build tsConfig, else tsconfig.app.json); tsc does not read templates. | any: `**/*.ts`, `**/*.html` | check typecheck: a shell script calling `forge_do ngc typecheck`, `forge_role_files` |

### typescript/anthropic

Anthropic TypeScript SDK.

Detected by: a dependency on `@anthropic-ai/sdk` (npm).
Child of `typescript`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/anthropic/no-budget-tokens` | No thinking: { type: 'enabled', budget_tokens } in a messages call whose model is a literal that rejects it with a 400 (Opus 4.7 and later, Sonnet 5 and 5.5, Fable 5 and 5.1, Mythos 5 and 5.1, Haiku 5.5; Bedrock's anthropic. prefix included): use thinking: { type: 'adaptive' } and output_config: { effort }. | typescript | ast-grep forbid (where `$C`: types engine, §15) |
| `typescript/anthropic/no-forced-tool-choice` | No tool_choice: { type: 'any' } or { type: 'tool' } in a messages call whose model is a literal that rejects forced tool use with a 400 (claude-opus-5-5, claude-sonnet-5-5, claude-fable-5-1, claude-mythos-5-1): use tool_choice: { type: 'auto' } with strict: true tools and an instruction naming the tool, or structured outputs (output_config.format). | typescript | ast-grep forbid (where `$C`: types engine, §15) |

### typescript/apollo

Apollo Server (CSRF prevention, error responses).

Detected by: a dependency on `@apollo/server` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Apollo Server: `csrfPrevention` stays on (no `csrfPrevention: false`), and `includeStacktraceInErrorResponses` is never set to a literal `true`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/apollo/csrf-prevention` | No csrfPrevention: false in new ApolloServer({...}): it re-opens CSRF through simple GET and POST requests that browsers send cross-site without a preflight. | typescript | ast-grep forbid (where `$N`: types engine, §15) |
| `typescript/apollo/no-stacktrace` | No literal includeStacktraceInErrorResponses: true in new ApolloServer({...}): error responses then carry server stack traces in every environment, production included. | typescript | ast-grep forbid (where `$N`: types engine, §15) |

### typescript/astro

Astro sites and their type checking.

Detected by: a dependency on `astro` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Astro. `.astro` components render on the server; the default output is static HTML.
- `.astro` files are type-checked with `astro check`, not `tsc`.
- Content collections are declared in `src/content.config.ts` with the `glob()` / `file()` loaders; entries are identified by `entry.id` (no `slug`).
- `Astro.glob()` is removed: use `getCollection()` or `import.meta.glob()`. View transitions use `<ClientRouter />`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/astro/check` | The project passes astro check (types in .astro files). | any: `**/*.astro`, `**/*.ts`, `**/*.tsx` | check typecheck: a shell script calling `forge_do astro check` |
| `typescript/astro/no-set-html` | No set:html in .astro files; it injects unescaped HTML. Render the content as text or sanitise it in a component. | any: `**/*.astro` | grep forbid `set:html[[:space:]]*=` |

### typescript/astro/react

React components as Astro islands.

Detected by: a dependency on `@astrojs/react` (npm).
Child of `typescript/astro`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- React components in Astro render to static HTML; they become interactive only with a `client:*` directive (`client:load`, `client:idle`, `client:visible`, `client:only="react"`).
- Hooks and event handlers run only in hydrated islands.
- Props passed to a hydrated island must be serializable.

No rules of its own.

### typescript/aws-cdk

AWS CDK apps in TypeScript.

Detected by: a dependency on `aws-cdk-lib` (npm); or a file matching `cdk.json`.
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- AWS CDK: `RemovalPolicy.DESTROY` and `autoDeleteObjects: true` delete data with the stack: use them only where losing the data is intended; `cdk.context.json` is committed, never git-ignored.
- cdk-nag findings are fixed rather than suppressed: a changed file may not add `NagSuppressions.add…Suppressions` calls.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/aws-cdk/context-committed` | cdk.context.json is not git-ignored in a CDK app (cdk.json present); CDK caches context lookups there, and it is committed so synth stays deterministic. | any | check lint: a shell script of 1 lines (when `.gitignore`, `**/.gitignore` or `cdk.json` changes) |
| `typescript/aws-cdk/nag-suppressions` | A changed file does not add cdk-nag suppressions (NagSuppressions.add…Suppressions calls, also under an import alias); fix the finding instead. | typescript | ast-grep no-increase |
| `typescript/aws-cdk/removal-policy-destroy` | RemovalPolicy.DESTROY and autoDeleteObjects: true in a Bucket's props: they let cdk destroy (or a replacement) delete the data; keep them to throwaway stacks. | typescript | ast-grep advise (where `$E`, `$B`: types engine, §15)<br>advice, never blocks |

### typescript/aws-lambda

AWS Lambda Node.js handlers.

Detected by: a dependency on one of `@types/aws-lambda` (npm), `aws-lambda` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- AWS Lambda: handlers are `async (event, context) => result` and return or throw; callback handlers (a `Callback` parameter, `context.callbackWaitsForEmptyEventLoop`) are rejected (the Node.js 24 runtime does not run them).

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/aws-lambda/no-callback-handler` | No callback-style Lambda handler: no parameter annotated with aws-lambda's Callback and no context.callbackWaitsForEmptyEventLoop; the Node.js 24 runtime no longer runs callback handlers, async handlers work on every runtime. | typescript | ast-grep forbid (where `$C`: types engine, §15) |

### typescript/aws-s3

AWS SDK v3 S3 client.

Detected by: a dependency on one of `@aws-sdk/client-s3` (npm), `@aws-sdk/s3-presigned-post` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- AWS S3: uploads set no `public-read` / `public-read-write` ACL; objects are served through CloudFront or presigned GET URLs.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/aws-s3/no-public-acl` | No public-read / public-read-write ACL on S3 uploads (PutObjectCommand, CreateMultipartUploadCommand, CopyObjectCommand inputs, createPresignedPost Fields / Conditions): new buckets enforce bucket-owner ownership, so the request fails, or the object becomes public; serve it through CloudFront or presigned GET URLs. | typescript | ast-grep advise (where `$C`: types engine, §15)<br>advice, never blocks |

### typescript/bcrypt

bcrypt / bcryptjs password hashing.

Detected by: a dependency on one of `bcrypt` (npm), `bcryptjs` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- bcrypt / bcryptjs: async code awaits `hash()` / `compare()` / `genSalt()`; the `*Sync` variants block the event loop and are rejected in async functions.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/bcrypt/no-sync-hash-in-async` | No hashSync / compareSync / genSaltSync from bcrypt or bcryptjs (default, namespace or named import) in an async function: each call blocks the event loop for about 100 ms; await hash() / compare() / genSalt() instead. | typescript | ast-grep forbid |

### typescript/better-auth

Better Auth server configuration (CSRF, cookies, trusted origins).

Detected by: a dependency on `better-auth` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Better Auth: CSRF / origin checks and Secure cookies stay on (no `disableCSRFCheck: true`, no `useSecureCookies: false`); `trustedOrigins` lists real origins, never `"*"` or the request's own `Origin` header.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/better-auth/no-csrf-disable` | No disableCSRFCheck: true and no useSecureCookies: false in betterAuth({...}) (also under an import alias): they turn off the CSRF / origin check and the Secure cookie flag. | typescript | ast-grep forbid |
| `typescript/better-auth/schema-check` | Better Auth's own schema check passes (better-auth check schema --config <the file of role @better-auth-config>: the CLI of the auth package; its adapter drives it from 1.7.6, the first release with the command, and reports an older one not enforced): every table and field the config's plugins write exists in the Prisma client, the Drizzle schema or, with another adapter, the local database. A check that cannot run is reported, never passed. | any: `**/*.ts`, `**/*.tsx`, `**/*.mts`, `**/*.cts`, `**/*.js`, `**/*.jsx`, `**/*.mjs`, `**/*.cjs`, `**/*.prisma` | check migration: a shell script calling `forge_do better-auth schema-check`, `forge_role_files`, `forge_local_db` (timeout 180 s) |
| `typescript/better-auth/trusted-origins` | trustedOrigins in betterAuth({...}) has no "*" entry and is not a function returning the request's own Origin header (alone, or with a ?? / \|\| fallback): it is the CSRF and redirect allowlist. | typescript | ast-grep forbid |

### typescript/bullmq

BullMQ queues and workers.

Detected by: a dependency on `bullmq` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- BullMQ: the ioredis connection of a `Worker` is created with `maxRetriesPerRequest: null` (blocking commands must not fail after retries).

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/bullmq/worker-connection` | A BullMQ Worker whose connection is an inline new IORedis(…) passes maxRetriesPerRequest: null in that client's options; otherwise blocking commands fail after the retries and the worker stops processing jobs. | typescript | ast-grep forbid (where `$W`, `$R`: types engine, §15) |

### typescript/bun

Bun is the package manager.

Detected by: a file matching `bun.lock`, `bun.lockb`.
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Bun is the package manager. Tests that import from `bun:test` run with `bun test`. In them, new `.skip`, `.only`, `.todo`, `.failing`, `.if`/`.skipIf`/`.todoIf`, fewer `expect` calls and new mocks (`mock`, `mock.module`, `spyOn`) are rejected.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/bun/lock-sync` | bun.lock matches package.json. | any | check lint: `forge_do bun lock-check` (when `package.json` changes) |
| `typescript/bun/no-fewer-expects` | Assertions are not removed from tests that import from bun:test. | typescript: `**/*.test.*`, `**/*_test.*`, `**/*.spec.*`, `**/*_spec.*`, `!@e2e-tests` | ast-grep no-decrease |
| `typescript/bun/no-mock-increase` | A bun:test file may not gain mocks (mock(), mock.module(), spyOn(), jest.fn / vi.fn, also through an import alias such as import { mock as m }); test through the real code or a fake at the boundary. | typescript: `**/*.test.*`, `**/*_test.*`, `**/*.spec.*`, `**/*_spec.*`, `!@e2e-tests` | ast-grep no-increase |
| `typescript/bun/no-new-skip` | Tests that import from bun:test are not skipped, made conditional, marked failing or todo, or focused with .only. | typescript: `**/*.test.*`, `**/*_test.*`, `**/*.spec.*`, `**/*_spec.*`, `!@e2e-tests` | ast-grep no-increase |
| `typescript/bun/no-other-pm` | Dependencies are managed with bun, not npm, pnpm or yarn. | any | guard ask: command `{name: npm, sub: [install\|i\|ci\|add]}` or `{name: pnpm, sub: [add\|install\|i\|update\|up\|remove\|rm]}` or `{name: yarn, sub: [add\|install\|up\|upgrade\|remove]}` or `{name: yarn, words: ^yarn( --(immutable\|frozen-lockfile))?$}` |
| `typescript/bun/no-shell-raw` | Bun Shell escapes what it interpolates: no { raw: … } in a $`…` command, and no Bun.spawn/spawnSync of sh, bash or zsh -c with a command built from values (both bypass the escaping: command injection). Interpolate the values, or pass them as argv. | typescript | ast-grep forbid (where `$TAG`, `$SPAWN`: types engine, §15) |
| `typescript/bun/no-snapshot-update` | Updating bun test snapshots (-u, --update-snapshots) needs confirmation; the snapshots are part of the reviewed expectations. | any | guard ask: command `{name: bun, sub: [test], flags_any: [-u, --update-snapshots]}` |
| `typescript/bun/no-test-edits` | Tests in bun:test files that existed at the start of the turn are not changed (anything in them, formatting aside) or removed by Claude; adding tests is fine. Checked at Stop. | typescript: `**/*.test.*`, `**/*_test.*`, `**/*.spec.*`, `**/*_spec.*`, `!@e2e-tests` | check lint: a shell script calling `forge_test_edits` |
| `typescript/bun/release-age` | Bun installs only versions published at least 3 days ago (bunfig.toml [install] minimumReleaseAge >= 259200), so a compromised release is caught before it lands. Packages that must update sooner go in minimumReleaseAgeExcludes. | any | check lint: `forge_bun_cooldown` (when `bunfig.toml`, `package.json` or `bun.lock` changes) |
| `typescript/bun/test` | Tests that import from bun:test pass (bun test on those files). | typescript | check test: a shell script calling `forge_do bun test-files` |
| `typescript/bun/text-lockfile` | No binary bun.lockb; Bun's lock file is the text bun.lock, which reviews and merges read (bun install --save-text-lockfile --frozen-lockfile, then delete bun.lockb). | any | check lint: a shell script of 2 lines (when `**/bun.lockb` or `**/package.json` changes) |
| `typescript/bun/trusted-deps` | No new trustedDependencies entries in package.json: each one lets that package run its install scripts. The user trusts packages, not Claude. | any | check lint: `forge_trusted_deps` (when `**/package.json` changes) |

### typescript/cloudflare-workers

Cloudflare Workers projects (wrangler).

Detected by: a dependency on `wrangler` (npm); or a file matching `wrangler.toml`, `wrangler.json`, `wrangler.jsonc`.
Child of `typescript`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/cloudflare-workers/env-types-generated` | The wrangler types output (file role @wrangler-types) was generated from the current wrangler config: wrangler types <file> --check (wrangler 4.153.0 and later), which compares the config hash the file records. Regenerate with wrangler types after changing a binding. | any | check codegen: a shell script calling `forge_do wrangler codegen-check`, `forge_role_files` (when `@wrangler-config` or `@wrangler-types` changes)<br>applies where `wrangler >=4.153.0` (npm) declared |
| `typescript/cloudflare-workers/no-ctx-destructure` | The Workers ExecutionContext is not destructured (const { waitUntil } = ctx): a method taken off it loses its this and throws Illegal invocation when called. Call ctx.waitUntil(...) on the context. | typescript | ast-grep forbid (where `$C`: types engine, §15) |
| `typescript/cloudflare-workers/no-pass-through-on-exception` | No ctx.passThroughOnException(); it fails open to the origin and hides the error. Handle errors with try/catch and return a response; a proxy in front of an origin may keep it on purpose. | typescript | ast-grep advise (where `$C`: types engine, §15)<br>advice, never blocks |

### typescript/cloudflare-workers/stripe

Stripe on Cloudflare Workers (async webhook verification).

Detected by: a dependency on `stripe` (npm).
Child of `typescript/cloudflare-workers`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Stripe on Workers: webhooks are verified with `await stripe.webhooks.constructEventAsync(body, signature, secret)` (SubtleCrypto is async only; `constructEvent` throws there).

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/cloudflare-workers/stripe/constructevent-async` | In a Workers project, Stripe webhooks are verified with await stripe.webhooks.constructEventAsync(…), not constructEvent(…): on Workers Stripe signs with SubtleCrypto, which is async only, so the synchronous call throws. | typescript | ast-grep forbid (where `$S`: types engine, §15) |

### typescript/drizzle

Drizzle ORM schema, drizzle-kit migrations and raw SQL.

Detected by: a dependency on one of `drizzle-orm` (npm), `drizzle-kit` (npm); or a file matching `drizzle.config.*`.
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Drizzle: drizzle.config.* names the schema files (`schema`), the migrations folder (`out`, default `./drizzle`) and the `dialect`.
- A schema change is followed by `drizzle-kit generate`; the SQL and the metadata it writes are committed together, never written by hand.
- A migration that exists in the base is not modified or deleted; a change goes in a new migration. `drizzle-kit push` needs confirmation; `push --force` is refused.
- Values reach SQL through the `sql` template tag, which binds them; `sql.raw()` with a template or concatenation (or a variable built from one) is rejected.
- `db.delete(t)` and `db.update(t).set(…)` are chained with `.where(…)`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/drizzle/applied-migrations` | Migrations already in the base (SQL and snapshots in the drizzle.config out folder) are not modified or deleted; a change goes in a new migration. | any | check migration: `forge_drizzle_applied` |
| `typescript/drizzle/meta-files` | The migration journal and snapshots (meta/_journal.json, meta/*_snapshot.json) are written by drizzle-kit generate, never edited by hand. | any | permission deny `Edit(**/meta/_journal.json)`<br>permission deny `Edit(**/meta/*_snapshot.json)` |
| `typescript/drizzle/migration-lint` | With the postgresql dialect, new migrations in the out folder do not break the running app during a deploy (no column or table renames, type changes, new required columns or DROP DATABASE); Squawk lints the SQL. | any: `**/*.sql` | check migration: `forge_drizzle_squawk` (when `**/*.sql` changes) |
| `typescript/drizzle/migration-present` | A change to the Drizzle schema files (drizzle.config schema) that reaches the database comes with a generated migration in the out folder, with its meta/_journal.json and snapshot (drizzle-kit 1.x, <name>/migration.sql with snapshot.json). Relations, types, imports and comments do not count. | any | check migration: `forge_drizzle_migration_present` |
| `typescript/drizzle/no-push-force` | No drizzle-kit push --force; it auto-approves the data-loss statements push would otherwise ask about. | any | guard deny: command `{name: drizzle-kit, sub: [push], flags_any: [--force]}` |
| `typescript/drizzle/no-push` | drizzle-kit push changes the database without a migration; it needs confirmation (it is for prototyping, never for a shared or production database); push --force is refused. | any | guard ask: command `{name: drizzle-kit, sub: [push]}` |
| `typescript/drizzle/no-query-in-loop` | A Drizzle query (a QueryPromise: select, insert, update, delete, db.query.*.findMany / findFirst) is not awaited inside a loop body (for, for…of, for…in, while, do) or an async .map / .forEach / .flatMap callback; one query per item is N round trips. Fetch the rows in one query (inArray, a join, with: relations) or batch them. | typescript | ast-grep advise (where `$Q`: types engine, §15)<br>advice, never blocks |
| `typescript/drizzle/no-sql-raw` | No sql.raw() (also under an import alias of drizzle-orm's sql) built from a template with ${} or a concatenation, directly or through a variable declared from one in the same function or module; the sql`...` tag binds values as parameters (sql.identifier() for names). | typescript | ast-grep forbid |
| `typescript/drizzle/write-with-where` | A Drizzle delete(table) or update(table).set(…) on a database or transaction is chained with .where(…); without it every row of the table is deleted or updated. | typescript | ast-grep forbid (where `$DB`: types engine, §15) |

### typescript/effect

Effect programs (Effect.gen generators).

Detected by: a dependency on `effect` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Effect: inside `Effect.gen`, effects are composed with `yield*`; `Effect.runPromise` / `runSync` / `runFork` run only at the edge of the program, never inside a generator.
- Failures are typed: `yield* Effect.fail(new MyError())` (a `Data.TaggedError`), not `throw`, inside `Effect.gen`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/effect/no-run-inside-gen` | No Effect.runPromise / runSync / runFork (or their Exit / With variants) inside an Effect.gen generator: a nested runtime breaks interruption, context and tracing; compose with yield* instead. | typescript | ast-grep forbid |
| `typescript/effect/no-throw-in-gen` | No throw directly in an Effect.gen generator (nested non-generator functions aside): a thrown error becomes a defect, not a typed failure; use yield* Effect.fail(...) with a tagged error. | typescript | ast-grep forbid |

### typescript/elysia

Elysia routes (request body validation).

Detected by: a dependency on `elysia` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Elysia: a `post` / `put` / `patch` handler that reads `body` declares a `body` schema in its options (`{ body: t.Object(…) }`), so the body is validated and typed.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/elysia/body-schema` | An Elysia post / put / patch handler that destructures body passes a body schema in its options argument (t.Object(…), a Standard Schema); otherwise body is unvalidated input. | typescript | ast-grep forbid (where `$APP`: types engine, §15) |

### typescript/eslint

ESLint 9 and later (flat config) with typescript-eslint.

Detected by: a dependency on `eslint >=9.0.0` (npm); or a file matching `eslint.config.js`, `eslint.config.mjs`, `eslint.config.cjs`, `eslint.config.ts`, `eslint.config.mts`, `eslint.config.cts`.
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- ESLint 9+ reads only the flat config `eslint.config.*`: ignores go in its `ignores`, globals in `languageOptions.globals`; `.eslintrc*`, `.eslintignore` and `/* eslint-env */` comments are rejected.
- ESLint warnings on added lines are fixed like errors. The type-aware typescript-eslint rules (`no-unsafe-*`, `no-unnecessary-condition`, …) need `parserOptions.projectService`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/eslint/flat-config-only` | No .eslintrc* or .eslintignore files; ESLint 9 reads only eslint.config.*, so they configure nothing (move ignores to the config's ignores). | any | check lint: a shell script of 2 lines (when `.eslintrc`, `.eslintrc.*`, `**/.eslintrc`, `**/.eslintrc.*`, `.eslintignore` or `**/.eslintignore` changes) |
| `typescript/eslint/lint` | ESLint passes on the added lines with no error and no warning (eslint --max-warnings=0 on the changed files; the whole project in CI). | typescript | check lint: `forge_do eslint lint .` (fails on warnings) |
| `typescript/eslint/no-eslint-env` | No /* eslint-env */ comments; flat config ignores them (ESLint 10 reports them as errors). Declare globals in the config's languageOptions.globals. | typescript | grep forbid `/\*[[:space:]]*eslint-env` |
| `typescript/eslint/ts-version` | The typescript package that typescript-eslint resolves has the compiler API (TypeScript below 6.1); TypeScript 7 ships none, so typescript-eslint cannot parse with it. | any | check lint: a shell script of 17 lines (when `**/package.json`, `pnpm-lock.yaml`, `package-lock.json`, `yarn.lock` or `bun.lock` changes) |
| `typescript/eslint/type-checked` | typescript-eslint's type-aware rules that Biome has no equivalent for are errors: no any flowing through assignments, calls, member access, returns and arguments; return await in try; no unbound methods, misused spreads, array delete or for-in over arrays; template expressions, promise rejections and catch callbacks typed; no unnecessary assertions or conditions; no object literal assertions. They need parserOptions.projectService and TypeScript 6 or older. | typescript | tool eslint `@typescript-eslint/no-unsafe-assignment`<br>tool eslint `@typescript-eslint/no-unsafe-call`<br>tool eslint `@typescript-eslint/no-unsafe-member-access`<br>tool eslint `@typescript-eslint/no-unsafe-return`<br>tool eslint `@typescript-eslint/no-unsafe-argument`<br>tool eslint `@typescript-eslint/return-await`<br>tool eslint `@typescript-eslint/unbound-method`<br>tool eslint `@typescript-eslint/no-misused-spread`<br>tool eslint `@typescript-eslint/no-array-delete`<br>tool eslint `@typescript-eslint/no-for-in-array`<br>tool eslint `@typescript-eslint/restrict-template-expressions`<br>tool eslint `@typescript-eslint/prefer-promise-reject-errors`<br>tool eslint `@typescript-eslint/use-unknown-in-catch-callback-variable`<br>tool eslint `@typescript-eslint/no-unnecessary-type-assertion`<br>tool eslint `@typescript-eslint/no-unnecessary-condition`<br>tool eslint `@typescript-eslint/consistent-type-assertions` |

### typescript/express

Express applications (sessions).

Detected by: a dependency on `express` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Express sessions: `express-session` cookie options and `cookie-session` options never set `secure: false` or `httpOnly: false` literally (an environment-dependent value is fine).
- Behind a proxy, `trust proxy` (Express) is a hop count, the proxy subnets or a function, not `true`.
- (when `express >=5.0.0` (npm) declared) Express 5 (path-to-regexp 8): a wildcard is named (`/*splat`, `/{*splat}`), an optional segment is `{...}`; `?`, `+`, `(…)` in paths are rejected.
- (when `express >=5.0.0` (npm) declared) Express 5: `req.query` is a getter (keep parsed input in `res.locals`); async handlers need no wrapper (`express-async-errors` is rejected); `res.sendStatus(n)`, not `res.send(n)`; no `res.redirect("back")`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/express/no-async-errors` | No express-async-errors with Express 5: it patches Express 4's router, which Express 5 no longer has, and Express 5 already passes a rejected promise of a handler to next(err). | typescript | ast-grep forbid<br>applies where `express >=5.0.0` (npm) declared |
| `typescript/express/no-async-handler` | express-async-handler and express-promise-router are redundant with Express 5, which passes a rejected promise of a handler to next(err); use plain async handlers and express.Router(). | typescript | ast-grep advise<br>applies where `express >=5.0.0` (npm) declared<br>advice, never blocks |
| `typescript/express/no-permissive-trust-proxy` | app.set("trust proxy", true) on an Express app trusts every X-Forwarded-For hop, so the client sets req.ip (IP rate limits and logs are spoofable); give the hop count, the proxy subnets or a function. | typescript | ast-grep advise (where `$A`: types engine, §15)<br>advice, never blocks |
| `typescript/express/no-req-query-assign` | No assignment to req.query or to a property of it on an Express 5 request: req.query is a getter that parses the query string again on each read, so the assignment is lost (or throws). Keep parsed input in res.locals or a typed variable. | typescript | ast-grep forbid (where `$R`: types engine, §15)<br>applies where `express >=5.0.0` (npm) declared |
| `typescript/express/no-v4-response-signatures` | No Express 4 response signatures that Express 5 changed: res.send(<number>) sends the number as a JSON body with status 200 (use res.sendStatus(n) or res.status(n).send(...)), and res.redirect("back") redirects to a path named back (use req.get("Referrer") \|\| "/"). | typescript | ast-grep forbid (where `$R`: types engine, §15)<br>applies where `express >=5.0.0` (npm) declared |
| `typescript/express/path-syntax-v5` | Express 5 route paths (path-to-regexp 8) have no unnamed * (write /*splat or /{*splat}) and no ?, +, !, (, ), [ or ] (an optional segment is {...}, e.g. /file{.:ext}): Express 5 throws on them when the route is registered. | typescript | ast-grep forbid (where `$R`: types engine, §15)<br>applies where `express >=5.0.0` (npm) declared |
| `typescript/express/session-cookie-flags` | No literal secure: false or httpOnly: false in the cookie options of express-session's session({ cookie: {...} }) or in cookie-session's cookieSession({...}) (default imports, any local name): a literal false turns the protection off in every environment; set it from the environment if needed. | typescript | ast-grep forbid |
| `typescript/express/types-match` | @types/express (and @types/express-serve-static-core) has the major version of express in the same package.json: v4 types on Express 5 hide every Express 5 change from the typecheck. | any | check lint: `forge_dep_major_match express @types/express @types/express-serve-static-core` (when `**/package.json` changes) |

### typescript/fastify

Fastify 5 plugins and route schemas.

Detected by: a dependency on `fastify >=5.0.0` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Fastify 5: a plugin is `async (instance, opts)` or `(instance, opts, done)`, never async with `done`.
- Route schemas (`body`, `querystring`, `params`, `headers`, `response.<code>`) are full JSON Schemas (`{ type: "object", properties: … }`) or a schema builder's output; the v4 shorthand property map is removed.
- `trustProxy` is a hop count, the proxy addresses or a function, not `true`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/fastify/full-json-schema` | In a Fastify route schema, body, querystring, params, headers and each response status are a full JSON Schema ({type: "object", properties: …}) or a schema builder's result, not a v4 shorthand map of properties: Fastify 5 removed the shorthand, and the route fails at ready() (FST_ERR_SCH_VALIDATION_BUILD … unknown keyword), which tsc does not see. | typescript | ast-grep forbid (where `$F`: types engine, §15) |
| `typescript/fastify/no-permissive-trust-proxy` | trustProxy: true in Fastify({...}) (fastify's default or named import, any local name) trusts every X-Forwarded-For hop, so the client sets request.ip (IP rate limits and logs are spoofable); give the hop count, the proxy addresses or a function. | typescript | ast-grep advise<br>advice, never blocks |
| `typescript/fastify/plugin-style` | A Fastify plugin is either async (instance, opts) or (instance, opts, done), never an async function with a third parameter: Fastify 5 rejects it at ready() (FST_ERR_PLUGIN_INVALID_ASYNC_HANDLER), which tsc does not see. | typescript | ast-grep forbid (where `$F`: types engine, §15) |

### typescript/graphql-codegen

GraphQL Code Generator (generated types up to date).

Detected by: a dependency on `@graphql-codegen/cli` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- GraphQL Code Generator writes the generated types: after changing the schema, a `.graphql` document or a `gql` operation, run `graphql-codegen` and commit its output; generated files are never edited by hand.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/graphql-codegen/up-to-date` | The files GraphQL Code Generator writes match the schema and the operations (graphql-codegen --check): regenerate after a change, never edit them by hand. | any: `**/*.graphql`, `**/*.gql`, `**/*.ts`, `**/*.tsx`, `**/*.mts`, `**/*.cts`, `**/*.js`, `**/*.jsx`, `**/*.mjs`, `**/*.cjs`, `codegen.yml`, `codegen.yaml`, `codegen.json`, `.graphqlrc*` | check codegen: a shell script calling `forge_do graphql-codegen codegen-check` |

### typescript/graphql-yoga

GraphQL Yoga servers (error masking).

Detected by: a dependency on `graphql-yoga` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- GraphQL Yoga: error masking stays on (no `maskedErrors: false`); errors meant for clients are thrown as `GraphQLError`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/graphql-yoga/masked-errors` | No maskedErrors: false in createYoga({...}) (graphql-yoga, also under an import alias): unexpected resolver errors then reach clients with their messages and internals; throw GraphQLError for errors meant for clients. | typescript | ast-grep forbid |

### typescript/hono

Hono routes (request validation, CORS).

Detected by: a dependency on `hono` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Hono: request data is validated by middleware on the route, `zValidator("json" | "query" | "form" | "param", schema)` from `@hono/zod-validator` or `validator()` from `hono/validator`, and read back with `c.req.valid("json")` etc., which is typed.
- In files that import from `hono` or `@hono/*`, `c.req.json()`, `c.req.parseBody()`, `c.req.formData()`, `c.req.query()` and `c.req.queries()` (also on a destructured `req`) are rejected unless passed straight to a schema (`Schema.parse(await c.req.json())`).
- `cors()` with `credentials: true` names its origins; `origin: "*"` or a function returning the origin it receives (`(origin) => origin`, `o => o ?? ""`) with credentials is rejected.
- `raw()` from `hono/html` built from a template or concatenation (or a variable built from one) is rejected; the `html` tag escapes values.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/hono/cors-credentials-wildcard` | cors() with credentials true names its allowed origins. A function that returns the origin it is given (alone or with a ?? / \|\| fallback, as an expression or a return) lets any site make credentialed requests; origin "*" with credentials is refused by browsers. | typescript | ast-grep forbid<br>deprecated: replaced by `typescript/cors-credentials-wildcard` |
| `typescript/hono/no-raw-interpolation` | No raw() from hono/html (also under an import alias) given a template with ${}, a concatenation or a variable declared from one in the same function or module: raw() inserts its text unescaped (XSS); the html`...` tag escapes values. | typescript | ast-grep forbid |
| `typescript/hono/validate-input` | Hono handlers read the body and query through a validator middleware (zValidator, validator) and c.req.valid(), or pass the read straight to a schema (Schema.parse(await c.req.json()), safeParse, parseAsync, valibot's parse(Schema, …)); not c.req.json(), parseBody(), formData(), query() or queries() whose result is used unvalidated. The request may come from c.req or a destructured ({ req }) parameter. | typescript | ast-grep forbid |

### typescript/jest

Jest 30 and later.

Detected by: a dependency on `jest >=30.0.0` (npm).
Child of `typescript`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/jest/no-test-path-pattern` | Jest 30 filters test files with --testPathPatterns; the old --testPathPattern flag is rejected by it, in package.json scripts and in commands. | any: `**/package.json` | grep forbid `--testPathPattern([^s]\|$)`<br>guard deny: command `{name: jest, flags_any: [--testPathPattern]}` |

### typescript/jsonwebtoken

jsonwebtoken (verification options).

Detected by: a dependency on `jsonwebtoken` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- jsonwebtoken: `jwt.verify(token, key, { algorithms: ["HS256"] })` names its algorithms (never `none`) and keeps expiry checks (no `ignoreExpiration: true`); `decode()` does not verify and is not used for trust decisions.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/jsonwebtoken/no-decode-trust` | jsonwebtoken's decode() does not verify the signature; use verify() whenever the claims decide anything (identity, permissions, expiry). | typescript | ast-grep advise<br>advice, never blocks |
| `typescript/jsonwebtoken/verify-algorithms` | jsonwebtoken's verify(token, key, options) gets an options object with an algorithms list that does not contain 'none' (or an options variable), and no ignoreExpiration: true; sign() gets no algorithm: 'none'. Without a fixed list the token's own header picks the algorithm (algorithm confusion). | typescript | ast-grep forbid |

### typescript/kysely

Kysely query builder (raw SQL, whole-table writes).

Detected by: a dependency on `kysely` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Kysely: values reach SQL through the sql`...` tag, which binds them, and names through `sql.ref()` / `sql.id()` / `sql.table()`; `sql.raw()` and `sql.lit()` built from a template or concatenation are rejected (unescaped).
- `deleteFrom()` and `updateTable()` chains have a `where…()` before `execute()`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/kysely/no-query-in-loop` | A Kysely query's execute() / executeTakeFirst() / executeTakeFirstOrThrow() is not awaited inside a loop body (for, for…of, for…in, while, do) or an async .map / .forEach / .flatMap callback; one query per item is N round trips. Fetch the rows in one query (where … in, a join) or insert an array of values. | typescript | ast-grep advise (where `$B`: types engine, §15)<br>advice, never blocks |
| `typescript/kysely/no-sql-raw` | No Kysely sql.raw() / sql.lit() built from a template with ${} or a concatenation, directly or through a variable declared from one in the same function or module (Kysely documents both as unescaped: SQL injection); use the sql`...` tag, which binds values, and sql.ref() / sql.id() for names. | typescript | ast-grep forbid (where `$CALL`: types engine, §15) |
| `typescript/kysely/write-with-where` | A Kysely deleteFrom(table) / updateTable(table) chain executed with execute*() has a where…(); without it the whole table is deleted or updated. | typescript | ast-grep advise (where `$DB`: types engine, §15)<br>advice, never blocks |

### typescript/langchain

LangChain.js (legacy chains and agents).

Detected by: a dependency on one of `langchain` (npm), `@langchain/core` (npm), `@langchain/classic` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- `langchain-1` (when `langchain >=1.0.0` (npm) declared) LangChain 1: agents are built with `createAgent` from `langchain` (and LangGraph); `LLMChain`, `ConversationChain`, `RetrievalQAChain`, `AgentExecutor` and the like are legacy (`@langchain/classic`).
- `langchain-1` (when `@langchain/core >=1.0.0` (npm) declared) LangChain 1: agents are built with `createAgent` from `langchain` (and LangGraph); `LLMChain`, `ConversationChain`, `RetrievalQAChain`, `AgentExecutor` and the like are legacy (`@langchain/classic`).

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/langchain/no-legacy-chains` | No new imports of LLMChain, ConversationChain, RetrievalQAChain, ConversationalRetrievalQAChain, initializeAgentExecutorWithOptions or AgentExecutor from @langchain/classic or langchain/chains / langchain/agents: LangChain 1 moved them to @langchain/classic as legacy; build agents with createAgent from langchain. | typescript | ast-grep advise<br>advice, never blocks |

### typescript/library

An npm library's public exports (deprecations).

Detected by: the project publishes a library to npm.
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- This project is an npm library: its public API is what the entry points in `package.json` `exports` (and `main`, `module`, `types`) export.
- An exported name is not removed or renamed outright: it stays exported as a `/** @deprecated Use <new name> */` alias; removing it, even in a major version, is the user's decision: ask them. Removing an export, or turning it into an alias of a renamed export without that JSDoc tag, is rejected.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/library/deprecation-shim` | A name exported by a package entry point (package.json exports, main, module, types, mapped from dist/ to src/; export * followed through relative modules) in the base is still exported, and a name that became an alias of another export (export const old = renamed, export { renamed as old }) carries a /** @deprecated */ JSDoc comment on that export. A major version bump does not allow a removal by itself: removing a name is the user's decision: ask them. | typescript: `!@ts-tests`, `!@e2e-tests` | check lint: `forge_helper bun public_api.mjs removed` (when `**/*.ts`, `**/*.tsx`, `**/*.mts`, `**/*.js`, `**/*.mjs` or `package.json` changes) |
| `typescript/library/exports-map` | A published package (not private) declares its entry points in package.json exports, not only main, module or types; reported where main, module or types is added. | any: `**/package.json` | check lint: a shell script of 25 lines |

### typescript/mcp

Model Context Protocol servers (TypeScript SDK).

Detected by: a dependency on one of `@modelcontextprotocol/sdk` (npm), `@modelcontextprotocol/server` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- MCP: a server on the stdio transport writes only JSON-RPC to stdout; logs go to stderr (`console.error`), and `console.log` / `info` / `debug` or `process.stdout.write` in a file importing `StdioServerTransport` are rejected.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/mcp/stdio-no-stdout` | A file that imports StdioServerTransport (@modelcontextprotocol/sdk/server/stdio.js or @modelcontextprotocol/server) writes nothing to stdout: no console.log / info / debug, no process.stdout.write; stdout carries the JSON-RPC stream and any print corrupts it. Log with console.error (stderr). | typescript | ast-grep forbid |

### typescript/mikro-orm

MikroORM configuration and schema commands.

Detected by: a dependency on `@mikro-orm/core` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- MikroORM: each request works in its own context (`RequestContext` middleware or `em.fork()`); `allowGlobalContext: true` is rejected outside tests.
- Schema changes go through migrations (`migration:create`, `migration:up`); `schema:drop --run` / `schema:fresh --run` are refused, `schema:update --run` and `migration:fresh` need confirmation.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/mikro-orm/migration-present` | Entity changes come with a migration: once the migrations have run on the local database (mikro-orm migration:up), mikro-orm migration:check finds no schema change. | typescript | check migration: `forge_do mikro-orm migrate && forge_do mikro-orm migrations-check` (needs a local database; timeout 180 s) |
| `typescript/mikro-orm/no-global-context` | No allowGlobalContext: true in the options of MikroORM.init(…), new MikroORM(…) or defineConfig(…) (bindings imported from @mikro-orm/*) outside test files: one identity map is then shared across requests (data leaks, stale entities); use RequestContext or em.fork(). | typescript: `!**/*.test.*`, `!**/*.spec.*`, `!**/__tests__/**`, `!e2e/**` | ast-grep forbid |
| `typescript/mikro-orm/schema-guard` | No mikro-orm schema:drop --run or schema:fresh --run (also -r, and the mikro-orm-esm bin); they wipe the database schema. | any | guard deny: command `{name: [mikro-orm, mikro-orm-esm], sub: ['schema:drop\|schema:fresh'], flags_any: [--run, -r]}` |
| `typescript/mikro-orm/schema-update` | mikro-orm schema:update --run (changes the schema without a migration) and migration:fresh (drops the database and reruns every migration) need confirmation. | any | guard ask: command `{name: [mikro-orm, mikro-orm-esm], sub: ['schema:update'], flags_any: [--run, -r]}` or `{name: [mikro-orm, mikro-orm-esm], sub: ['migration:fresh']}` |

### typescript/mongoose

Mongoose models and queries (operator injection, server-side JavaScript).

Detected by: a dependency on `mongoose` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Mongoose: request input (`req.body`, `req.query`, `req.params`) is validated or passed through `sanitizeFilter()` before it reaches a filter or update; passing it whole is rejected (operator injection).
- No `$where` (server-side JavaScript); use query operators or `$expr`.
- (when `mongoose >=9.0.0` (npm) declared) Mongoose 9: an update pipeline (an array) needs `{ updatePipeline: true }` on the call or `mongoose.set("updatePipeline", true)`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/mongoose/no-query-in-loop` | A Mongoose query or aggregation (Model.find, findOne, findById, countDocuments, updateOne, aggregate, …) is not awaited inside a loop body (for, for…of, for…in, while, do) or an async .map / .forEach / .flatMap callback; one query per item is N round trips. Fetch the documents in one query ($in, populate, $lookup) or use bulkWrite. | typescript | ast-grep advise (where `$Q`: types engine, §15)<br>advice, never blocks |
| `typescript/mongoose/no-request-filter` | A request's body, query or params (Express or Fastify) is not passed whole as the filter or update of a Mongoose find / findOne / findOneAndUpdate / findOneAndDelete / findOneAndReplace / updateOne / updateMany / deleteOne / deleteMany / countDocuments / replaceOne: a client sends {"password": {"$ne": null}} (operator injection). Validate it first, or wrap it in sanitizeFilter(). | typescript | ast-grep forbid (where `$M`, `$R`: types engine, §15) |
| `typescript/mongoose/no-where-operator` | No $where in a query passed to a Mongoose model or query, and no .$where(): it runs JavaScript on the database server (injection, denial of service) and is deprecated in MongoDB 8; use query operators or $expr. | typescript | ast-grep forbid (where `$M`: types engine, §15) |
| `typescript/mongoose/update-pipeline` | Mongoose 9 rejects an update pipeline (an array) in updateOne, updateMany, findOneAndUpdate or findByIdAndUpdate unless the call passes { updatePipeline: true } or the app sets mongoose.set("updatePipeline", true); pass the option, or check the global setting. | typescript | ast-grep advise (where `$M`: types engine, §15)<br>applies where `mongoose >=9.0.0` (npm) declared<br>advice, never blocks |

### typescript/msw

Mock Service Worker (msw) in tests.

Detected by: a dependency on `msw` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- The msw server of the tests starts with `server.listen({ onUnhandledRequest: "error" })`, so a request no handler covers fails the test instead of reaching the network.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/msw/unhandled-error` | An msw server is started with onUnhandledRequest: "error" (or a function deciding it), so a request no handler covers fails the test instead of passing through to the network. | typescript | ast-grep advise (where `$S`: types engine, §15)<br>advice, never blocks |

### typescript/nestjs

NestJS controllers, validation pipes and DTOs.

Detected by: a dependency on one of `@nestjs/core` (npm), `@nestjs/common` (npm).
Child of `typescript`: active only where it is.
Switches off biome `lint/style/useImportType` where active: a DTO imported with import type erases design:paramtypes, so ValidationPipe stops validating the body; Biome's own docs advise turning useImportType off with decorators.

Context (in `.claude/rules/forge-*.md`, §7):

- NestJS: the global `ValidationPipe` is created with `whitelist: true` (and `forbidNonWhitelisted: true`), so undeclared body fields never reach a handler.
- `@Body()`, `@Query()` and `@Param()` without a pipe argument are typed with a DTO class (class-validator decorators); `any`, `unknown`, `object`, inline object types and `Record<…>` are rejected, since only classes are validated.
- A nested DTO property has `@ValidateNested()` together with `@Type(() => Child)` from class-transformer.
- Controllers throw `HttpException` subclasses (`NotFoundException`, `BadRequestException`, …), not a plain `Error`.
- On the Express platform (Nest 11), route paths use path-to-regexp 8 syntax: `*splat` / `{*splat}`, optional segments `{...}`.
- A DTO used as a `@Body()`, `@Query()` or `@Param()` type is imported as a value, never with `import type` / `{ type X }` (that erases it from the decorator metadata and turns validation off).

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/nestjs/dto-is-class` | A parameter decorated @Body(), @Query() or @Param() (from @nestjs/common, without a pipe argument) is typed with a DTO class: no type, any, unknown, object, an inline object type or Record<…> reaches the handler unvalidated, since ValidationPipe validates class types only. | typescript | ast-grep forbid |
| `typescript/nestjs/dto-not-type-import` | The type of a @Body(), @Query() or @Param() parameter (from @nestjs/common) is not imported with import type / { type X }: a type-only import erases the class from design:paramtypes, and ValidationPipe then validates nothing. Import the DTO as a value. | typescript | ast-grep forbid |
| `typescript/nestjs/express-path-syntax` | On Nest 11's Express platform (path-to-regexp 8), route paths in @Get/@Post/.../@All() and in forRoutes() have no unnamed * (write *splat or {*splat}) and no ?, +, !, (, ), [ or ] (an optional segment is {...}): Nest converts the old syntax with a warning, Express 5 itself rejects it. | typescript | ast-grep advise (where `$D`, `$C`: types engine, §15)<br>applies where `@nestjs/platform-express >=11.0.0` (npm) declared<br>advice, never blocks |
| `typescript/nestjs/http-exception` | A method of a @Controller() class (from @nestjs/common) throws an HttpException subclass (NotFoundException, BadRequestException, …), not a plain Error, which becomes a generic 500. An Error thrown inside a try block is not concerned: it is caught there. | typescript | ast-grep advise<br>advice, never blocks |
| `typescript/nestjs/validate-nested-type` | A property decorated @ValidateNested() (class-validator) also has @Type(() => X) (class-transformer); without it the nested value stays a plain object and is not validated. | typescript | ast-grep forbid |
| `typescript/nestjs/validation-pipe-forbid-non-whitelisted` | new ValidationPipe({...}) sets forbidNonWhitelisted: true, so a request with undeclared fields is rejected (400) instead of silently stripped. | typescript | ast-grep advise (where `$N`: types engine, §15)<br>advice, never blocks |
| `typescript/nestjs/validation-pipe-whitelist` | new ValidationPipe({...}) sets whitelist: true; without it, body fields the DTO does not declare (role, isAdmin) reach the handler (mass assignment). | typescript | ast-grep forbid (where `$N`: types engine, §15) |

### typescript/nextjs

Next.js 16 App Router.

Detected by: a dependency on `next >=16.0.0` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Next.js 16 App Router. Request APIs (`params`, `searchParams`, `cookies()`, `headers()`, `draftMode()`) are async: await them.
- Components are Server Components unless the file starts with `'use client'`.
- Request interception lives in `proxy.ts` (not `middleware.ts`).
- `revalidateTag(tag, "max")` takes a cacheLife profile; `updateTag(tag)` in a Server Action gives read-your-writes.
- Remote images are allowed with `images.remotePatterns` naming the hosts (no `*` / `**` hostname); `dangerouslyAllowSVG` needs a `contentSecurityPolicy`.
- `next lint` no longer exists and `next build` does not lint: run the project's own linter.
- Server Actions and route handlers (`app/**/route.ts`) are public endpoints: validate their input with a schema and check who is calling.
- `NEXT_PUBLIC_` variables are inlined into the browser bundle; one named like `*SECRET*`, `*PRIVATE*`, `*PASSWORD*`, `*SERVICE_ROLE*` or `*DATABASE_URL*` is rejected.
- `'use cache'` code never calls `cookies()`, `headers()` or `draftMode()`; `redirect()`, `notFound()` and friends are not called inside `try` unless the catch calls `unstable_rethrow(e)`.
- `typescript.ignoreBuildErrors` is never set; every parallel route `@slot` has a `default.tsx`; metadata uses the Metadata API, not `next/head`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/nextjs/cookies-set-in-render` | cookies().set() and .delete() are not called while rendering a page, layout, template or default (Next.js throws: cookies can only be modified in a Server Action or route handler); move them into a 'use server' function or a route handler. | typescript: `app/**/page.*`, `app/**/layout.*`, `app/**/template.*`, `app/**/default.*`, `src/app/**/page.*`, `src/app/**/layout.*`, `src/app/**/template.*`, `src/app/**/default.*` | ast-grep forbid |
| `typescript/nextjs/image-remote-patterns-strict` | images.remotePatterns has no hostname "*" or "**": the image optimizer would fetch from any host (an open proxy). Name the hosts. | typescript: `next.config.*` | ast-grep forbid |
| `typescript/nextjs/image-unsafe-options` | images.dangerouslyAllowSVG: true only with a contentSecurityPolicy next to it (SVG can carry scripts), and no images.dangerouslyAllowLocalIP: true (the optimizer would fetch from the internal network). | typescript: `next.config.*` | ast-grep forbid |
| `typescript/nextjs/no-ignore-build-errors` | next.config does not set typescript.ignoreBuildErrors: true; next build would ship code that does not type-check. Fix the errors. | typescript: `next.config.*` | ast-grep forbid |
| `typescript/nextjs/no-legacy-image-config` | No next/legacy/image and no images.domains. Use next/image and images.remotePatterns. | typescript | ast-grep forbid |
| `typescript/nextjs/no-next-head-in-app` | No next/head or next/document under app/; they do nothing in the App Router. Export metadata or generateMetadata instead. | typescript: `app/**`, `src/app/**` | ast-grep forbid |
| `typescript/nextjs/no-next-router` | No next/router. Use next/navigation. | typescript | ast-grep forbid |
| `typescript/nextjs/no-pages-data` | No getServerSideProps or getStaticProps in the App Router. Fetch in Server Components. | typescript: `app/**`, `src/app/**` | ast-grep forbid |
| `typescript/nextjs/no-public-secret-env` | Secrets are never NEXT_PUBLIC_ variables, which are inlined into the browser bundle. | typescript | ast-grep forbid |
| `typescript/nextjs/no-redirect-in-try` | redirect(), permanentRedirect(), notFound(), forbidden() and unauthorized() from next/navigation are not called in a try block whose catch does not call unstable_rethrow(error): they throw, and the catch would swallow the navigation. Call them after the try. | typescript | ast-grep forbid |
| `typescript/nextjs/no-request-apis-in-use-cache` | cookies(), headers() and draftMode() from next/headers are not called inside a 'use cache' function or file (an error at run time); read them outside and pass the values in. | typescript | ast-grep forbid |
| `typescript/nextjs/no-sync-scripts` | No synchronous <script src> in JSX; it blocks rendering. Use next/script (or async / defer). | tsx | tool biome `lint/performance/noSyncScripts` |
| `typescript/nextjs/parallel-route-default` | Every parallel route slot (an @folder under app/) has a default.tsx (or .ts, .jsx, .js); next build fails without it in Next.js 16. | any: `app/**`, `src/app/**` | check lint: a shell script of 4 lines |
| `typescript/nextjs/proxy-exports` | proxy.ts exports proxy (or a default export), not middleware, and no runtime segment config: Next.js 16 looks for the proxy export, and a proxy always runs on Node.js. | typescript: `proxy.ts`, `src/proxy.ts` | ast-grep forbid |
| `typescript/nextjs/proxy-not-middleware` | middleware.ts is renamed to proxy.ts in Next.js 16. | typescript: `middleware.ts`, `src/middleware.ts` | ast-grep forbid |
| `typescript/nextjs/revalidate-tag-profile` | revalidateTag takes a cacheLife profile (revalidateTag(tag, "max")); use updateTag(tag) in Server Actions for read-your-writes. | typescript | ast-grep forbid |
| `typescript/nextjs/stable-cache-imports` | Import cacheLife and cacheTag from next/cache, not unstable_cacheLife or unstable_cacheTag (to be removed; no @deprecated tag shows it). | typescript | ast-grep forbid |

### typescript/nextjs/cache-components

Next.js with cacheComponents enabled ('use cache', cacheLife, cacheTag).

Detected by: a line matching `^\s*cacheComponents\s*:\s*true\b` in a file matching `next.config.{js,mjs,cjs,ts,mts}`.
Child of `typescript/nextjs`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Next.js runs with `cacheComponents: true`. Data is cached with the `'use cache'` directive plus `cacheLife()` and `cacheTag()` from `next/cache`, not with `unstable_cache`.
- Route segment config exports (`dynamic`, `revalidate`, `fetchCache`, `experimental_ppr`) fail `next build` in this mode: use `'use cache'` and `cacheLife()` instead, and `<Suspense>` around what reads request data.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/nextjs/cache-components/no-segment-config` | With cacheComponents, route files under app/ export no dynamic, revalidate, fetchCache or experimental_ppr (next build rejects them); use 'use cache' with cacheLife(). | typescript: `app/**`, `src/app/**` | ast-grep forbid |
| `typescript/nextjs/cache-components/no-unstable-cache` | With cacheComponents, cache with the 'use cache' directive and cacheTag() / cacheLife(), not unstable_cache from next/cache. | typescript | ast-grep advise<br>advice, never blocks |

### typescript/nextjs/react

React Server and Client Components in the App Router.

Detected by: a dependency on `react >=19.0.0` (npm).
Child of `typescript/nextjs`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- State, effects, context, refs and event handlers need a Client Component; keep `'use client'` at the leaves.
- Server Actions are async functions in a file or function marked `'use server'`.
- A `'use client'` file reads only `NEXT_PUBLIC_*` and `NODE_ENV` from `process.env` and exports no async component; a `'use server'` file exports only async functions.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/nextjs/react/client-env-public-only` | A 'use client' file reads only NEXT_PUBLIC_ variables and NODE_ENV from process.env; any other variable is undefined in the browser. Read it in a Server Component and pass the value down. | typescript | ast-grep forbid |
| `typescript/nextjs/react/client-hooks` | Client-only React hooks in app/ need a 'use client' file (custom hooks named use… are exempt). | typescript: `app/**`, `src/app/**` | ast-grep forbid |
| `typescript/nextjs/react/no-async-client-component` | No async component in a 'use client' file; Client Components cannot be async. Fetch in a Server Component, or use a hook. | tsx | tool biome `lint/correctness/noNextAsyncClientComponent` |
| `typescript/nextjs/react/no-server-in-client` | Client Components do not import server-only modules (server-only, node built-ins, next/headers, next/root-params, the Prisma client). | tsx: `app/**`, `src/app/**`, `components/**`, `src/components/**` | ast-grep forbid |
| `typescript/nextjs/react/no-ssr-false-in-server` | dynamic(..., { ssr: false }) from next/dynamic is used only in a 'use client' file; in a Server Component it is a build error. Wrap it in a Client Component. | typescript: `app/**`, `src/app/**` | ast-grep forbid |
| `typescript/nextjs/react/server-functions-async` | A 'use server' file exports only async functions (and types): no sync function, class, or literal value such as export const LIMIT = 10 (a build error). Move constants to another module. | typescript | ast-grep forbid |

### typescript/node

Node.js runtime APIs and how TypeScript runs on Node.

Detected by: a dependency on `@types/node` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- TypeScript runs through the project's own runner (its package.json scripts), not `ts-node`, which is unmaintained and rejected in package.json.
- `node:vm` is not a security boundary: never use it to run untrusted code.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/node/esm-globals` | An ES module (.mts / .mjs, or a .ts / .js file under a package.json with "type": "module") uses no CommonJS globals: no __dirname / __filename (import.meta.dirname / import.meta.filename, Node 20.11+), no require() (import, or createRequire), no module.exports or exports.x (export). @types/node declares them, so tsc accepts them, but they throw ReferenceError when the module runs. | typescript: `@esm-modules` | ast-grep forbid (where `$G`: types engine, §15) |
| `typescript/node/fetch-timeout` | fetch() gets a signal (signal AbortSignal.timeout(ms)); the platform fetch has no overall timeout, so a stalled server hangs the request forever. | typescript | ast-grep advise (where `$F`: types engine, §15)<br>advice, never blocks |
| `typescript/node/no-ecb-cipher` | No ECB mode in createCipheriv/createDecipheriv (equal plaintext blocks give equal ciphertext blocks); use an authenticated mode such as aes-256-gcm. | typescript | ast-grep forbid (where `$F`: types engine, §15) |
| `typescript/node/no-shell-interpolation` | No shell command built from values. exec/execSync get a constant command; pass values as an argv array to execFile or spawn, without shell true. | typescript | ast-grep forbid (where `$EXEC`, `$SPAWN`: types engine, §15) |
| `typescript/node/no-sync-fs-in-async` | No node:fs *Sync call inside an async function; await node:fs/promises instead (a sync call blocks the event loop for every request). | typescript | ast-grep advise (where `$F`: types engine, §15)<br>advice, never blocks |
| `typescript/node/no-ts-node` | No ts-node in package.json (scripts or dependencies), and running ts-node asks; it is unmaintained since 2023 and breaks on modern ESM. Run TypeScript with node file.ts (type stripping) or tsx. | any: `**/package.json` | grep forbid `(^\|[^A-Za-z0-9_@/-])ts-node([^A-Za-z0-9_-]\|$)`<br>guard ask: command `{name: [ts-node, ts-node-esm, ts-node-script]}` |
| `typescript/node/no-vm-sandbox` | Importing node:vm is reported for review; it is not a security boundary, so code from users or the network must not run in it. | typescript | ast-grep advise<br>advice, never blocks |
| `typescript/node/weak-crypto` | createHash/createHmac with md5 or sha1 only for checksums, never for passwords, tokens or signatures: use sha256 or better (and scrypt or argon2 for passwords). | typescript | ast-grep advise (where `$F`: types engine, §15)<br>advice, never blocks |

### typescript/npm

npm is the package manager.

Detected by: a file matching `package-lock.json`, `npm-shrinkwrap.json`.
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- npm is the package manager: `npm install`, `npm ci`; pnpm, yarn and bun installs ask.
- Peer dependency conflicts are resolved, not hidden: `--legacy-peer-deps` and `--force` (flags or `.npmrc`) are rejected, and `npm audit fix --force` asks.
- Dependencies come from the registry: git, GitHub and tarball URL specs, and `allow-git=all` / `allow-remote=all`, are rejected.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/npm/allow-scripts-command` | Claude asks before approving dependency install scripts (npm approve-scripts, npm install-scripts approve, --allow-scripts, --dangerously-allow-all-scripts); npm 12 blocks them by default, and a failed build is not a reason to run them. | any | guard ask: command `{name: npm, sub: [approve-scripts, '*']}` or `{name: npm, sub: [approve-scripts], flags_any: [--all, --no-allow-scripts-pin]}` or `{name: npm, sub: [install-scripts, approve]}` or `{name: [npm, pnpm, yarn, bun, npx], flags_any: [--dangerously-allow-all-scripts]}` or `{name: [npm, pnpm, yarn, bun, npx], option_values: {--allow-scripts: '^[^-]'}}` |
| `typescript/npm/allow-scripts` | No new install-script approvals: package.json allowScripts gains no entry that is not false, and .npmrc gains no ignore-scripts=false, allow-scripts entry or dangerously-allow-all-scripts=true. npm 12 blocks dependency install scripts by default; the user approves them, not Claude. | any | check lint: `forge_helper bun pm_policy.mjs npm-scripts \|\| forge_accept_hint` (when `package.json` or `.npmrc` changes) |
| `typescript/npm/lock-sync` | package-lock.json matches the package.json files (npm ci --dry-run --ignore-scripts from npm 10.5.1, which leaves node_modules alone; before, npm ls --package-lock-only --all, since an earlier ci --dry-run deletes node_modules). | any | check lint: `forge_do npm lock-check` (when `**/package.json`, `package-lock.json` or `npm-shrinkwrap.json` changes) |
| `typescript/npm/lockfile-lint` | package-lock.json resolves every package from registry.npmjs.org, over https, with a sha512 integrity (pinned lockfile-lint); a project on a private registry replaces the rule locally with its hosts. | any: `@lockfiles` | check lint: `forge_lockfile_lint` (when `@lockfiles` changes) |
| `typescript/npm/no-audit-fix-force` | npm audit fix --force asks; it installs versions outside the declared ranges, major upgrades included. | any | guard ask: command `{name: npm, sub: [audit, fix], flags_any: [--force]}` |
| `typescript/npm/no-exotic-deps` | No dependency added (or changed) with a git, GitHub, GitLab, Bitbucket or tarball URL spec (git+…, github:…, user/repo, https://…): they bypass the registry, its release-age checks and --ignore-scripts. Publish to a registry or vendor the code. | any: `**/package.json` | check lint: a shell script calling `forge_pkg_deps` |
| `typescript/npm/no-exotic-sources` | npm is not allowed to install from any git or remote source (allow-git=all, allow-remote=all, in .npmrc or as a flag); such dependencies bypass the registry, its release-age checks and --ignore-scripts. | any: `**/.npmrc` | grep forbid `allow-(git\|remote)[[:space:]]*=[[:space:]]*all`<br>guard deny: command `{flags_any: [--allow-git=all, --allow-remote=all], value_flags: [--allow-git, --allow-remote]}` |
| `typescript/npm/no-legacy-peer-deps-config` | No legacy-peer-deps=true or force=true in .npmrc; they hide every peer dependency conflict of every install. | any: `**/.npmrc` | grep forbid `^[[:space:]]*(legacy-peer-deps\|force)[[:space:]]*=[[:space:]]*true` |
| `typescript/npm/no-legacy-peer-deps` | npm install, ci and add run without --legacy-peer-deps or --force; resolve the peer dependency conflict instead of hiding it. | any | guard deny: command `{name: npm, sub: [install\|i\|ci\|add], flags_any: [--legacy-peer-deps, --force]}` |
| `typescript/npm/no-other-pm` | Dependencies are managed with npm; installing with pnpm, yarn or bun asks. | any | guard ask: command `{name: pnpm, sub: [add\|install\|i\|update\|up\|remove\|rm]}` or `{name: yarn, sub: [add\|install\|up\|upgrade\|remove]}` or `{name: yarn, words: ^yarn( --(immutable\|frozen-lockfile))?$}` or `{name: bun, sub: [install\|i\|add]}` |
| `typescript/npm/release-age-command` | Claude asks before turning npm's release-age cooldown off for a command (--min-release-age=0); a package that must update sooner goes in min-release-age-exclude. | any | guard ask: command `{option_values: {--min-release-age: ^0*$}}` |
| `typescript/npm/release-age` | npm installs only versions published at least 3 days ago (.npmrc min-release-age=3 or more), and package.json requires npm 11.10 or later (devEngines.packageManager or packageManager), which is the first npm to read it; a compromised release is caught before it lands. Packages that must update sooner go in min-release-age-exclude. | any | check lint: `forge_helper bun pm_policy.mjs npm-cooldown $FORGE_COOLDOWN_SECONDS` (when `.npmrc`, `package.json` or `package-lock.json` changes) |

### typescript/openapi

Clients generated from an OpenAPI document (openapi-typescript, orval, @hey-api/openapi-ts) stay in sync with it.

Detected by: a dependency on one of `openapi-typescript` (npm), `orval` (npm), `@hey-api/openapi-ts` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- The API client is generated from the OpenAPI document by a package.json script (openapi-typescript, orval or @hey-api/openapi-ts); generated files say so in their header.
- A change to a tracked OpenAPI document without a change to a generated file is rejected: run the generator script and keep both changes together.
- A change to a generated file without a change to the document or the generator config is rejected: change the document (or the server it comes from) and regenerate.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/openapi/no-generated-edits` | Generated client files (their header names openapi-typescript, orval or @hey-api/openapi-ts) change only with the OpenAPI document or the generator config (orval.config.*, openapi-ts.config.*, redocly.*, or a package.json line naming the generator: its script or dependency version; any other package.json change does not count), never by hand; checked at Stop when the repository tracks the document. | any | check lint: `forge_openapi_sync hand-edits` |
| `typescript/openapi/regenerate` | A change to a tracked OpenAPI document comes with the regenerated client (a file whose header names openapi-typescript, orval or @hey-api/openapi-ts), when a package.json script runs one of them; checked at Stop. | any | check lint: `forge_openapi_sync regenerate` |

### typescript/opentelemetry

OpenTelemetry for Node.js (SDK start-up, span attributes).

Detected by: a dependency on one of `@opentelemetry/api` (npm), `@opentelemetry/sdk-node` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- OpenTelemetry (Node): the SDK starts in its own file, loaded before the app (`node --import ./instrumentation.mjs`); importing instrumented libraries in that file is advised against (ES imports load them before the SDK patches them).
- Span attributes use the stable semantic conventions (`http.request.method`, `http.response.status_code`, `url.full`, `server.address`, `db.query.text`, `db.system.name`); the old keys are rejected.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/opentelemetry/init-before-imports` | A file that imports @opentelemetry/sdk-node does not also import a library the Node auto-instrumentations patch (express, fastify, koa, http, pg, mysql2, ioredis, redis, mongodb, mongoose, graphql, …): ES imports are hoisted, so the library loads before the SDK starts and its spans go missing. Start the SDK in its own file loaded with node --import ./instrumentation.mjs. | typescript | ast-grep advise<br>advice, never blocks |
| `typescript/opentelemetry/semconv-stable-keys` | span.setAttribute(s) uses the stable semantic convention keys, not the old ones: http.request.method (not http.method), http.response.status_code (http.status_code), url.full (http.url), url.path / url.query (http.target), url.scheme (http.scheme), user_agent.original (http.user_agent), server.address / server.port (net.peer.name / net.peer.port, net.host.name / net.host.port), db.query.text (db.statement), db.system.name (db.system). Keeping the old keys for a backend that needs them is the user's decision: ask them. | typescript | ast-grep forbid (where `$S`: types engine, §15) |

### typescript/oxlint

oxlint with its type-aware rules (oxlint-tsgolint).

Detected by: a dependency on `oxlint` (npm); or a file matching `.oxlintrc.json`, `oxlint.config.ts`.
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- oxlint warnings on added lines are fixed like errors. Its type-aware rules (such as `typescript/no-floating-promises`) need the `oxlint-tsgolint` package.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/oxlint/lint` | oxlint passes on the added lines with its type-aware rules and no warning (oxlint --type-aware --deny-warnings); without oxlint-tsgolint installed the type-aware run fails, so those rules are never skipped silently. | typescript | check lint: `forge_do oxlint lint` (fails on warnings) |

### typescript/playwright

Playwright end-to-end tests.

Detected by: a dependency on `@playwright/test` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Playwright. Locate elements with `getByRole`, `getByLabel`, `getByText` or `getByTestId`; web-first assertions (`await expect(locator)...`) wait on their own.
- `waitForTimeout` (a fixed wait) is rejected in end-to-end tests.
- Accessibility: `@axe-core/playwright` (`new AxeBuilder({ page }).analyze()`) checks pages for WCAG violations in end-to-end tests.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/playwright/axe` | When @axe-core/playwright is a dependency, at least one end-to-end test scans pages with AxeBuilder. | any | check lint: a shell script of 1 lines (when `package.json`, `@e2e-tests` or `**/*.spec.*` changes) |
| `typescript/playwright/config-file` | Claude asks you before editing the Playwright config (test paths, retries, projects). | any | permission ask `Edit(**/playwright.config.*)` |
| `typescript/playwright/locators` | Use locators (getByRole, getByLabel, locator()), not element handles or waitForSelector/waitForNavigation. | typescript: `@e2e-tests`, `**/*.spec.*` | ast-grep forbid |
| `typescript/playwright/no-new-skip` | End-to-end tests are not skipped, marked fixme or fail, or focused. | typescript: `@e2e-tests` | ast-grep no-increase |
| `typescript/playwright/no-snapshot-update` | Updating Playwright snapshots needs confirmation; the snapshots are part of the reviewed expectations. | any | guard ask: command `{name: playwright, sub: [test], flags_any: [--update-snapshots, -u]}` |
| `typescript/playwright/no-test-edits` | Changing an end-to-end test that existed at the start of the turn (anything in it, formatting aside, the pages it visits and its control flow included), or removing it, needs approval; adding tests is fine. Edits ask when made, unapproved changes are blocked at Stop. | typescript: `@e2e-tests` | check lint: `forge_test_edits` |
| `typescript/playwright/no-wait-for-timeout` | No fixed waits (waitForTimeout). Wait for a condition with a web-first assertion or waitForURL. | typescript: `@e2e-tests`, `**/*.spec.*` | ast-grep forbid |
| `typescript/playwright/run` | End-to-end tests pass. | any | check test: `forge_do playwright test` (when `@e2e-tests` or `playwright.config.*` changes) |

### typescript/pnpm

pnpm is the package manager.

Detected by: a file matching `pnpm-lock.yaml`, `pnpm-workspace.yaml`.
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- pnpm is the package manager: `pnpm add`, `pnpm install`; npm, yarn and bun installs ask.
- pnpm settings (overrides, allowBuilds, minimumReleaseAge, nodeLinker, …) go in `pnpm-workspace.yaml`; a `pnpm` field in package.json and pnpm settings in `.npmrc` (registry and auth only) are rejected.
- (when `pnpm >=11.0.0` (npm) declared) pnpm 11: `allowBuilds` replaces `onlyBuiltDependencies` / `neverBuiltDependencies` / `ignoredBuiltDependencies` / `ignoreDepScripts`, `allowUnusedPatches` replaces `allowNonAppliedPatches`, `auditConfig.ignoreGhsas` replaces `ignoreCves`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/pnpm/allow-builds` | No new build approvals in pnpm-workspace.yaml: no allowBuilds entry set to true (pnpm 11) or onlyBuiltDependencies entry (pnpm 10) is added, dangerouslyAllowAllBuilds is not turned on, and strictDepBuilds and blockExoticSubdeps are not turned off. Install scripts are the main npm malware vector; the user approves them, not Claude. | any | check lint: `forge_helper bun pm_policy.mjs pnpm-builds \|\| forge_accept_hint` (when `pnpm-workspace.yaml` changes) |
| `typescript/pnpm/catalog` | A dependency added or changed in a package.json whose name is in the pnpm catalog (catalog or catalogs in pnpm-workspace.yaml) uses catalog: (or catalog:<name>), so every package gets the same version; not reported when catalogMode is strict (pnpm enforces it). | any: `**/package.json` | check lint: `forge_advise forge_helper bun pm_policy.mjs pnpm-catalog` (when `**/package.json` changes)<br>advice, never blocks |
| `typescript/pnpm/lock-sync` | pnpm-lock.yaml matches the package.json files (pnpm install --frozen-lockfile --lockfile-only, which writes nothing; pnpm 8.6.12 and later: before, it rewrites a stale lock and passes). | any | check lint: `forge_do pnpm lock-check` (when `**/package.json`, `pnpm-lock.yaml` or `pnpm-workspace.yaml` changes) |
| `typescript/pnpm/no-other-pm` | Dependencies are managed with pnpm; installing with npm, yarn or bun asks. | any | guard ask: command `{name: npm, sub: [install\|i\|ci\|add]}` or `{name: yarn, sub: [add\|install\|up\|upgrade\|remove]}` or `{name: yarn, words: ^yarn( --(immutable\|frozen-lockfile))?$}` or `{name: bun, sub: [install\|i\|add]}` |
| `typescript/pnpm/no-removed-settings` | No pnpm-workspace.yaml settings removed in pnpm 11: onlyBuiltDependencies, neverBuiltDependencies, ignoredBuiltDependencies, ignoreDepScripts (use allowBuilds), allowNonAppliedPatches (use allowUnusedPatches), auditConfig.ignoreCves (use auditConfig.ignoreGhsas). | any: `**/pnpm-workspace.yaml` | grep forbid `^["']?(onlyBuiltDependencies\|neverBuiltDependencies\|ignoredBuiltDependencies\|ignoreDepScripts\|allowNonAppliedPatches)["']?[[:space:]]*:\|^[[:space:]]+["']?ignoreCves["']?[[:space:]]*:`<br>applies where `pnpm >=11.0.0` (npm) declared |
| `typescript/pnpm/overrides` | Each overrides entry added or changed in pnpm-workspace.yaml is reported once, so Claude says why (a security fix, an incompatible transitive version): an override can hide a vulnerability or force a version its dependents do not support. | any | check lint: `forge_advise forge_helper bun pm_policy.mjs pnpm-overrides` (when `pnpm-workspace.yaml` changes)<br>advice, never blocks |
| `typescript/pnpm/settings-location` | pnpm settings are in pnpm-workspace.yaml: no pnpm field in package.json, and no pnpm settings in .npmrc (it keeps registry and auth only); pnpm 11 reads neither. | any: `**/package.json`, `**/.npmrc` | check lint: a shell script of 28 lines |
| `typescript/pnpm/workspace-protocol` | A dependency on a package of the pnpm workspace (pnpm-workspace.yaml packages) uses the workspace: protocol; since pnpm 9 a plain version range can install a registry copy instead of linking the local package (linkWorkspacePackages is false by default). | any: `**/package.json` | check lint: `forge_helper bun pm_policy.mjs pnpm-workspace` (when `**/package.json` or `pnpm-workspace.yaml` changes) |

### typescript/prettier

Prettier formats the project (prettier --check).

Detected by: a dependency on `prettier` (npm); or a file matching `.prettierrc`, `.prettierrc.json`, `.prettierrc.json5`, `.prettierrc.yaml`, `.prettierrc.yml`, `.prettierrc.toml`, `.prettierrc.js`, `.prettierrc.mjs`, `.prettierrc.cjs`, `.prettierrc.ts`, `.prettierrc.mts`, `.prettierrc.cts`, `prettier.config.js`, `prettier.config.mjs`, `prettier.config.cjs`, `prettier.config.ts`, `prettier.config.mts`, `prettier.config.cts`.
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Prettier formats the project; its options live in the Prettier config, never in a file's own comments.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/prettier/format` | Files are formatted with the project's Prettier (prettier --check; a changed file is written with prettier --write). | any | check format: `forge_do prettier format-check .` |

### typescript/prisma

Prisma 7 schema, migrations and client.

Detected by: a dependency on `prisma >=7.0.0` (npm); or a file matching `prisma/schema.prisma`.
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Prisma 7. Generator `prisma-client` with an explicit `output`; the client is imported from that path.
- SQL databases connect through a driver adapter (e.g. `@prisma/adapter-pg`) passed to `new PrismaClient({ adapter })`.
- CLI configuration and the connection URL live in `prisma.config.ts`; `.env` is not loaded automatically.
- `prisma migrate dev` does not seed: run `prisma db seed`. Query hooks use Client Extensions (`$extends`; `$use` is removed).
- Raw SQL uses the tagged `$queryRaw` / `$executeRaw` forms; the `Unsafe` variants, and `Prisma.raw()` built from a template or concatenation, are rejected.
- `$transaction(async (tx) => …)` callbacks make no network calls (SDKs included): call them before or after the transaction; `fetch` and calls through an imported axios, got, ky, ofetch, node-fetch or undici there are rejected.
- A schema change comes with a migration (`prisma migrate dev`), except in a db push workflow (no `prisma/migrations`, a `prisma db push` script), where none is required.
- Dropping a column or table in a new migration is the user's decision: ask them first, and drop only after a deploy in which no code reads it (expand and contract).

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/prisma/applied-migrations` | Prisma migrations already in the base (file role @prisma-migrations) are not modified or deleted; prisma migrate checksums applied ones, so a change goes in a new migration (prisma migrate dev). | any | check migration: `forge_applied_migrations @prisma-migrations` |
| `typescript/prisma/client-generator` | The Prisma schema uses the prisma-client generator (Prisma 7), not prisma-client-js. | any | check lint: a shell script of 1 lines (when `**/*.prisma` changes) |
| `typescript/prisma/db-push` | Commands that change a database outside migrations need confirmation. | any | guard ask: command `{name: prisma, sub: [db, push\|execute]}` or `{name: prisma, sub: [migrate, deploy]}` |
| `typescript/prisma/destructive-migration` | A new Prisma migration does not drop a column or table (a SQLite table redefinition that leaves columns out included) unless the user approved that drop: ask them first, and drop only after a deploy in which no code reads it (expand and contract). | any: `prisma/migrations/**/migration.sql` | check migration: a shell script calling `forge_prisma_drops` (when `prisma/migrations/**` changes) |
| `typescript/prisma/generated-client-import` | Import the Prisma client from the generator output path, not @prisma/client. | typescript | ast-grep forbid |
| `typescript/prisma/migration-lint` | New migrations do not break the running app during a deploy (no column or table renames, type changes, new required columns or DROP DATABASE); Squawk lints the SQL. | any: `prisma/migrations/**/migration.sql` | check migration: `forge_squawk_files $(git ls-files 'prisma/migrations/*/migration.sql' \| sort \| tail -n 1)` (when `prisma/migrations/**` changes) |
| `typescript/prisma/migration-present` | Schema changes come with a migration (not in a db push workflow - no prisma/migrations and a package.json script running prisma db push). | any | check migration: a shell script calling `forge_do prisma migrations-check` (when `prisma/schema.prisma` changes; needs a local database) |
| `typescript/prisma/no-middleware` | No Prisma middleware ($use). Use Client Extensions. | typescript | ast-grep forbid |
| `typescript/prisma/no-network-in-transaction` | An interactive transaction callback ($transaction(async (tx) => ...)) makes no HTTP calls: the global fetch / $fetch, or a function or method of axios, got, ky, ofetch, node-fetch or undici imported in the file; call them before or after the transaction. | typescript | ast-grep forbid |
| `typescript/prisma/no-query-in-loop` | A Prisma query whose result type is PrismaPromise (findMany, count, aggregate, groupBy, the *Many methods) is not awaited inside a loop body (for, for…of, for…in, while, do) or an async .map / .forEach / .flatMap callback; one query per item is N round trips. Fetch the rows in one query (include, in, a relation filter) or batch them. | typescript | ast-grep advise (where `$Q`: types engine, §15)<br>advice, never blocks |
| `typescript/prisma/no-raw-interpolation` | No Prisma.raw() built from a template with ${} or a concatenation, directly or through a variable declared from one in the same function or module; raw() inserts its text unescaped. Use the tagged $queryRaw`...` / Prisma.sql`...` forms, which bind values. | typescript | ast-grep forbid (where `$CALL`: types engine, §15) |
| `typescript/prisma/no-raw-unsafe` | No $queryRawUnsafe / $executeRawUnsafe. Use the tagged $queryRaw`...` form, which binds parameters. | typescript | ast-grep forbid |
| `typescript/prisma/no-reset` | Prisma commands that wipe the database are not run by Claude. | any | guard deny: command `{name: prisma, sub: [migrate, reset]}` or `{name: prisma, sub: [db, push], flags_any: [--force-reset, --accept-data-loss]}` |
| `typescript/prisma/no-unscoped-bulk-write` | A Prisma deleteMany() / updateMany() without a where: it deletes or rewrites the whole table (seeds and test resets do it on purpose). | typescript | ast-grep advise (where `$CALL`: types engine, §15)<br>advice, never blocks |
| `typescript/prisma/validate` | The Prisma schema is valid. | any | check migration: `forge_do prisma validate` (when `prisma/schema.prisma` changes) |

### typescript/pulumi

Pulumi programs in TypeScript.

Detected by: a dependency on `@pulumi/pulumi` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Pulumi: `protect: true` resource options are kept (a changed file may not have fewer); unprotecting or deleting protected resources is the user's call.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/pulumi/no-output-in-string` | An Output<T> is not put in a template literal or concatenated with a string: it is a promise-like value, so the text gets "Calling [toString] on an [Output<T>] is not supported" instead of the value. Use pulumi.interpolate`…`, pulumi.concat(…) or output.apply((v) => …). | typescript | ast-grep forbid (where `$E`: types engine, §15) |
| `typescript/pulumi/protect-ratchet` | A changed file keeps its protect: true resource options (their count does not decrease): removing protection is the step before deleting a resource; that change is the user's to make. | typescript | grep no-decrease `(^\|[^A-Za-z0-9_$])protect[[:space:]]*:[[:space:]]*true([^A-Za-z0-9_$]\|$)` |

### typescript/react

React 19 components, shared by every React setup (Next.js, Astro islands, SPAs).

Detected by: a dependency on `react >=19.0.0` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- React 19: `ref` is a regular prop of function components; `forwardRef` is not needed.
- Form actions use `useActionState` from `react` (not `useFormState` from `react-dom`); `act` comes from `react`, not `react-dom/test-utils`.
- Function components have no `propTypes` or `defaultProps`: use TypeScript types and default parameter values; an object, array or function default is hoisted to a module constant.
- `ReactDOM.render` is removed: use `createRoot` from `react-dom/client`.
- Components follow the React Compiler's rules: no `setState` called synchronously in an effect, no ref read or written while rendering, no `Date.now()` / `Math.random()` while rendering; props are never assigned to.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/react/a11y` | Biome's accessibility rules are errors (the European Accessibility Act applies to e-commerce since June 2025). | tsx | tool biome `lint/a11y/useAltText`<br>tool biome `lint/a11y/useButtonType`<br>deprecated: replaced by `typescript/a11y` |
| `typescript/react/act-from-react` | act comes from react, not react-dom/test-utils (deprecated in React 19). | typescript | ast-grep forbid |
| `typescript/react/compiler-rules` | Components and hooks pass React Compiler's checks: no setState called synchronously in an effect (derive the value while rendering), no ref read or written while rendering, no impure call (Date.now(), Math.random()) while rendering. | tsx | tool biome `lint/nursery/useReactCompiler` |
| `typescript/react/hooks-rules` | Hooks run at the top level of a component or hook, and effects list every dependency they read. | tsx | tool biome `lint/correctness/useHookAtTopLevel`<br>tool biome `lint/correctness/useExhaustiveDependencies` |
| `typescript/react/no-dangerous-html` | No dangerouslySetInnerHTML. Render text as children, or a sanitizing Markdown component. | tsx | tool biome `lint/security/noDangerouslySetInnerHtml`<br>ast-grep forbid |
| `typescript/react/no-deprecated-types` | No imports React's types mark @deprecated (ElementRef, MutableRefObject, LegacyRef, ...). Use the replacement the deprecation names. Biome reads the tags from node_modules, and only from real directories (not pnpm's symlinks). | tsx | tool biome `lint/suspicious/noDeprecatedImports`<br>deprecated: replaced by `typescript/no-deprecated` |
| `typescript/react/no-forward-ref` | No forwardRef. In React 19, ref is a regular prop. | tsx | tool biome `lint/suspicious/noReactForwardRef`<br>ast-grep forbid |
| `typescript/react/no-nested-components` | No component defined inside another component, and no function that defines and returns a component or hook (a factory): each render or call makes a new component type, which remounts. | tsx | tool biome `lint/correctness/noNestedComponentDefinitions`<br>tool biome `lint/nursery/noComponentHookFactories` |
| `typescript/react/no-prop-assign` | Props are never assigned to (props.x = ...): they are read-only, and a mutation breaks rendering and memoization. Derive a local value instead. | tsx | tool biome `lint/correctness/noReactPropAssignments` |
| `typescript/react/no-prop-types` | No propTypes or defaultProps assignments. React 19 ignores them on function components; use types and default parameters. | tsx | ast-grep forbid |
| `typescript/react/stable-default-props` | Destructured props do not default to an object, array or function literal ({ items = [] }): it is a new value on every render, which defeats memo and re-runs effects. Hoist the default to a module constant. | tsx | tool biome `lint/nursery/noReactObjectTypeAsDefaultProp` |
| `typescript/react/stable-keys` | List keys are stable ids, never the array index. | tsx | tool biome `lint/suspicious/noArrayIndexKey` |
| `typescript/react/use-action-state` | Form action state comes from useActionState in react, not useFormState from react-dom. | tsx | ast-grep forbid |

### typescript/react/react-hook-form

React Hook Form.

Detected by: a dependency on `react-hook-form >=7.0.0` (npm).
Child of `typescript/react`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/react/react-hook-form/watch-all` | Watch named fields (useWatch({ control, name: "email" }) or watch("email")), not watch() with no argument: it subscribes to the whole form, so every keystroke re-renders the component, and it defeats React Compiler's memoization. | typescript | ast-grep advise (where `$W`: types engine, §15)<br>advice, never blocks |

### typescript/react/zustand

zustand stores in React.

Detected by: a dependency on `zustand >=5.0.0` (npm).
Child of `typescript/react`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- zustand 5: a store hook compares selector results with `Object.is`; a selector returning a new object or array (`useStore((s) => ({ a: s.a }))`) re-renders forever ("Maximum update depth"). Select one value per call, or wrap the selector in `useShallow` from `zustand/react/shallow`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/react/zustand/stable-selector` | A zustand store hook's selector does not return a new object or array literal (useStore((s) => ({ a: s.a })), (s) => [s.a, s.b]): zustand 5 compares with Object.is, so every render gets a new value and the component re-renders until React gives up (Maximum update depth exceeded). Select one value per call or wrap the selector in useShallow(). | typescript | ast-grep forbid (where `$S`: types engine, §15) |

### typescript/react-router

React Router framework mode (route module actions).

Detected by: a dependency on `@react-router/dev >=7.0.0` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- React Router framework mode: route modules export `loader` (GET data) and `action` (mutations), typed with `Route.LoaderArgs` / `Route.ActionArgs` from `./+types/<route>`.
- An `action` is a public POST endpoint: validate what it reads from `request.formData()` or `request.json()` with a schema and check who is calling.
- Responses use `data()` and `redirect()`; `json()` and `defer()` are gone.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/react-router/route-types` | Route modules type their exports with the generated Route types (Route.LoaderArgs, Route.ActionArgs, Route.MetaArgs, ... from ./+types/<route>), not the generic LoaderFunctionArgs, ActionFunctionArgs, MetaFunction or LinksFunction, which lose the route's params and loader data. | typescript: `app/routes/**`, `app/root.tsx` | ast-grep advise<br>advice, never blocks |

### typescript/redis

Redis clients (ioredis, node-redis).

Detected by: a dependency on one of `ioredis` (npm), `redis` (npm), `@redis/client` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Redis: no `KEYS` (`client.keys(pattern)`) in application code; iterate with `scanStream()` (ioredis), `scanIterator()` (node-redis) or `SCAN`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/redis/no-keys` | No KEYS command on a Redis client (client.keys(pattern) / client.KEYS(…)): it walks the whole keyspace in O(N) and blocks the server; iterate with scanStream() / scanIterator() / SCAN. | typescript | ast-grep forbid (where `$C`: types engine, §15) |

### typescript/resend

Resend email SDK.

Detected by: a dependency on `resend` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Resend: `send()` returns `{ data, error }` instead of throwing; destructure it and handle `error` (a discarded result is rejected).

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/resend/check-error` | The result of resend.emails.send(…) / resend.batch.send(…) is used (const { data, error } = await …, then error handled), not discarded: Resend returns errors instead of throwing, so a failed send is otherwise silent. | typescript | ast-grep forbid (where `$R`: types engine, §15) |

### typescript/solid

SolidJS components (fine-grained reactivity).

Detected by: a dependency on `solid-js` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- SolidJS: a component runs once; props are reactive getters, so they are read as `props.x` (or split with `splitProps`), never destructured.
- Lists render with `<For each={...}>` (or `<Index>`), not `.map()` in JSX.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/solid/for-component` | Lists in JSX render with <For each={...}>, not .map(): .map() recreates every node when the array changes. | tsx | tool biome `lint/performance/useSolidForComponent` |
| `typescript/solid/no-destructured-props` | Solid components do not destructure props: the value is read once and stops updating. Read props.x, or use splitProps / mergeProps. | tsx | tool biome `lint/correctness/noSolidDestructuredProps` |

### typescript/storybook

Storybook 9 and later.

Detected by: a dependency on `storybook >=9.0.0` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Storybook 9+ ships its test, actions, viewport, manager and preview APIs and theming in the `storybook` package: import `storybook/test`, `storybook/actions`, `storybook/viewport`, `storybook/manager-api`, `storybook/preview-api`, `storybook/theming`, and doc blocks from `@storybook/addon-docs/blocks`. The old `@storybook/*` packages for them, and `@storybook/addon-essentials` / `addon-interactions`, are rejected.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/storybook/consolidated-deps` | package.json does not depend on the packages Storybook 9 folded into storybook (@storybook/test, addon-actions, addon-viewport, addon-essentials, addon-interactions, manager-api, preview-api, theming, blocks): remove them; storybook provides those APIs. | any: `**/package.json` | grep forbid `"@storybook/(test\|addon-actions\|addon-viewport\|addon-essentials\|addon-interactions\|manager-api\|preview-api\|theming\|blocks)"[[:space:]]*:` |
| `typescript/storybook/consolidated-imports` | No imports from the packages Storybook 9 folded into storybook (@storybook/test, addon-actions, addon-viewport, addon-essentials, addon-interactions, manager-api, preview-api, theming, blocks): import storybook/test, storybook/actions, storybook/viewport, storybook/manager-api, storybook/preview-api, storybook/theming, @storybook/addon-docs/blocks. | typescript | ast-grep forbid |

### typescript/stripe

Stripe Node SDK (webhook signature verification).

Detected by: a dependency on `stripe` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Stripe webhooks: `stripe.webhooks.constructEvent` / `constructEventAsync` get the raw body (`await req.text()`, `express.raw()`'s Buffer), never `JSON.stringify(…)` or a parsed body; the signature covers the exact bytes.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/stripe/webhook-raw-body` | The payload given to stripe.webhooks.constructEvent / constructEventAsync is the raw request body (text or Buffer), not JSON.stringify(…), await req.json() or a variable declared from one in the same function: the signature is computed over the exact bytes Stripe sent, so a re-serialised body never verifies. | typescript | ast-grep forbid (where `$S`: types engine, §15) |

### typescript/svelte

Svelte 5 components (runes).

Detected by: a dependency on `svelte >=5.0.0` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Svelte 5 components use runes: `$props()`, `$state()`, `$derived()`, `$effect()`, event attributes (`onclick`), snippets and `{@render}`.
- Legacy syntax (`export let`, `$:`, `on:click`, `<slot>`, `$$props`, `createEventDispatcher`, `beforeUpdate` / `afterUpdate`, `svelte/legacy`) is rejected: it compiles silently in legacy mode.
- `{@html}` renders unescaped HTML and is rejected; render text, or sanitise in one component.
- `{#each}` blocks have a key (`{#each items as item (item.id)}`); no `{@debug}` tag is left in.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/svelte/each-key` | An {#each} block has a key ({#each items as item (item.id)}), so Svelte can track each item instead of patching in place. | any: `**/*.svelte` | tool biome `lint/nursery/useSvelteRequireEachKey` |
| `typescript/svelte/no-at-html` | No {@html ...} in .svelte files; it renders unescaped HTML (XSS). Render the content as text, or sanitise it in one component. | any: `**/*.svelte` | grep forbid `\{@html[[:space:]]` |
| `typescript/svelte/no-debug-tags` | No {@debug} tag is left in a .svelte file: it pauses the browser's devtools on every update. | any: `**/*.svelte` | tool biome `lint/nursery/noSvelteAtDebugTags` |
| `typescript/svelte/runes-only` | Svelte components use runes, not legacy syntax that silently switches the component to legacy mode: no export let ($props()), $: ($derived / $effect), on:event (onevent), <slot> or $$slots ({@render}), $$props / $$restProps, createEventDispatcher (callback props), beforeUpdate / afterUpdate ($effect.pre / $effect), svelte/legacy. | any: `**/*.svelte` | grep forbid `^[[:space:]]*\$:[[:space:]]\|^[[:space:]]*export[[:space:]]+let[[:space:]]\|[[:space:]]on:[a-zA-Z]+[\|=>[:space:]]\|<slot([[:space:]/>]\|$)\|\$\$(slots\|props\|restProps)\|createEventDispatcher\|(beforeUpdate\|afterUpdate)[[:space:]]*[,}(]\|['"]svelte/legacy['"]` |

### typescript/svelte/kit

SvelteKit configuration.

Detected by: a dependency on `@sveltejs/kit` (npm).
Child of `typescript/svelte`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/svelte/kit/external-redirect` | SvelteKit 3: redirect() to a literal http(s):// URL passes a third argument ({ external: true } or an allowlist of origins); without it SvelteKit 3 throws on an external location. | typescript | ast-grep forbid (where `$F`: types engine, §15)<br>applies where `@sveltejs/kit >=3.0.0` (npm) declared |
| `typescript/svelte/kit/no-app-stores` | No imports of $app/stores (deprecated since SvelteKit 2.12, removed in 3): use page, navigating and updated from $app/state. | any: `**/*.svelte`, `**/*.ts`, `**/*.js`, `**/*.mts`, `**/*.mjs` | grep forbid `(from\|import)[[:space:]]*\(?[[:space:]]*["']\$app/stores["']`<br>applies where `@sveltejs/kit >=2.12.0` (npm) declared |
| `typescript/svelte/kit/no-csrf-off` | svelte.config does not turn off SvelteKit's CSRF protection: no csrf.checkOrigin: false, no "*" in csrf.trustedOrigins. Name the trusted origins. | typescript: `svelte.config.*` | ast-grep forbid |
| `typescript/svelte/kit/no-env-modules` | No imports of $env/static/private, $env/static/public, $env/dynamic/private or $env/dynamic/public on SvelteKit 3 (deprecated): import from $app/env/private or $app/env/public. | any: `**/*.svelte`, `**/*.ts`, `**/*.js`, `**/*.mts`, `**/*.mjs` | grep forbid `(from\|import)[[:space:]]*\(?[[:space:]]*["']\$env/(static\|dynamic)/(private\|public)["']`<br>applies where `@sveltejs/kit >=3.0.0` (npm) declared |

### typescript/t3-env

Environment variables validated with t3-env (createEnv).

Detected by: a dependency on one of `@t3-oss/env-core` (npm), `@t3-oss/env-nextjs` (npm), `@t3-oss/env-nuxt` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Environment variables are read through the validated `env` object of the env module (`createEnv`), never `process.env` elsewhere; tool config files (`*.config.*`) are exempt.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/t3-env/no-process-env` | No process.env outside the env module (env.ts, env.mjs) and tool config files; import the validated env instead. | typescript | tool biome `lint/style/noProcessEnv` |

### typescript/tailwind

Tailwind CSS 4 utilities and configuration.

Detected by: a dependency on `tailwindcss >=4.0.0` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Tailwind CSS 4: configuration lives in CSS (`@import "tailwindcss"`, `@theme`); a `tailwind.config.*` is read only when a CSS file loads it with `@config`.
- Utilities removed in v4 generate nothing and are rejected: `bg-opacity-*` and the other `*-opacity-*` (use `bg-black/50`), `flex-shrink-*` / `flex-grow-*` (`shrink-*` / `grow-*`), `overflow-ellipsis` (`text-ellipsis`), `decoration-slice` / `decoration-clone` (`box-decoration-*`).
- A `<style>` block in a `.vue`, `.svelte` or `.astro` file, or a CSS module, that uses `@apply` starts with `@reference "<path to the main CSS>"`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/tailwind/apply-reference` | A .vue, .svelte or .astro file, or a CSS module, that uses @apply also has @reference (or @import "tailwindcss"): Tailwind 4 builds these stylesheets on their own, and @apply fails with 'Cannot apply unknown utility class'. | any: `**/*.vue`, `**/*.svelte`, `**/*.astro`, `**/*.module.css` | check lint: a shell script of 8 lines |
| `typescript/tailwind/config-referenced` | A tailwind.config.js, .ts, .mjs or .cjs at the root is loaded by a CSS file with @config: Tailwind 4 ignores it otherwise, without a word. Add @config, or move the theme to @theme and delete the file. | any: `tailwind.config.*`, `**/*.css` | check lint: a shell script of 7 lines (when `tailwind.config.*` or `**/*.css` changes) |
| `typescript/tailwind/no-removed-utilities` | No Tailwind 3 utilities removed in v4, which generate nothing: *-opacity-N (bg-black/50), flex-shrink-* and flex-grow-* (shrink-*, grow-*), overflow-ellipsis (text-ellipsis), decoration-slice and decoration-clone (box-decoration-*). | any: `**/*.tsx`, `**/*.jsx`, `**/*.vue`, `**/*.svelte`, `**/*.astro`, `**/*.html`, `**/*.css` | grep forbid `(^\|[^a-zA-Z0-9-])((bg\|text\|border\|divide\|ring\|placeholder)-opacity-[0-9]+\|flex-(shrink\|grow)(-[0-9]+)?\|overflow-ellipsis\|decoration-(slice\|clone))([^a-zA-Z0-9:-]\|$)` |

### typescript/tanstack-router

TanStack Router's generated route tree.

Detected by: a dependency on one of `@tanstack/react-router` (npm), `@tanstack/solid-router` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- TanStack Router: `routeTree.gen.ts` is generated from the routes directory (by the bundler plugin or `tsr generate`) and is never edited by hand; change the route files instead.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/tanstack-router/no-generated-edits` | routeTree.gen.ts is generated from the route files and never edited by hand (TanStack Router's default generatedRouteTree name); edit the routes and regenerate. | any | permission deny `Edit(**/routeTree.gen.ts)` |

### typescript/temporal

Temporal TypeScript SDK workflows (deterministic sandbox).

Detected by: a dependency on `@temporalio/workflow` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Temporal: workflow code runs in a deterministic sandbox. A file importing `@temporalio/workflow` imports no Node built-ins and no `@temporalio/activity` / `client` / `worker`; activities are reached through `proxyActivities<typeof activities>()` with `import type * as activities`.
- In workflows, `uuid4()` from `@temporalio/workflow` replaces `crypto.randomUUID()`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/temporal/workflow-determinism` | A workflow file (one with a value import of @temporalio/workflow) imports no Node built-in (fs, path, os, crypto, http, https, net, tls, dns, child_process, worker_threads, cluster, node:*) nor @temporalio/activity, client or worker, by import or require(), and calls no crypto.randomUUID() / getRandomValues(): workflow code runs in a deterministic sandbox bundle, where they fail to bundle or break replay. Activities are called through proxyActivities<typeof activities>() with an import type; uuid4() comes from @temporalio/workflow. | typescript | ast-grep forbid |

### typescript/testing-library

React Testing Library.

Detected by: a dependency on `@testing-library/react` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Hooks are tested with `renderHook` from `@testing-library/react` (`@testing-library/react-hooks` is for React 16 and 17, and is rejected).
- Async queries are awaited, `findBy*` replaces `waitFor` + `getBy*`, queries go through `screen`, `waitFor` callbacks have no side effects, no needless `act()` and no DOM node access.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/testing-library/lint` | In ESLint projects, eslint-plugin-testing-library's rules are errors: async queries are awaited, waitFor callbacks have no side effects, findBy instead of waitFor + getBy, queries through screen, no unnecessary act(), no DOM node access. | typescript: `@ts-tests` | tool eslint `testing-library/await-async-queries`<br>tool eslint `testing-library/no-wait-for-side-effects`<br>tool eslint `testing-library/prefer-find-by`<br>tool eslint `testing-library/prefer-screen-queries`<br>tool eslint `testing-library/no-unnecessary-act`<br>tool eslint `testing-library/no-node-access` |
| `typescript/testing-library/no-container-queries` | A test queries what the user sees through screen (getByRole, getByLabelText, getByText), not render()'s container with querySelector / querySelectorAll: a CSS selector ties the test to the markup and passes for elements no user can reach. | typescript: `@ts-tests` | ast-grep advise (where `$R`: types engine, §15)<br>advice, never blocks |
| `typescript/testing-library/no-hooks-lib` | No imports from @testing-library/react-hooks (React 16 and 17 only); renderHook comes from @testing-library/react. | typescript | ast-grep forbid |

### typescript/trpc

tRPC procedures (input validation, authorization, errors).

Detected by: a dependency on `@trpc/server` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- tRPC: a procedure that takes input declares `.input(<schema>)` (Zod or another validator) and reads it from `input`; `.input(z.any())`, `.input(z.unknown())`, a check-less `z.custom()`, a `z.record()` of `z.any()` and a cast-only function are rejected.
- Errors thrown in a procedure are `TRPCError` with a `code` (`NOT_FOUND`, `FORBIDDEN`, `BAD_REQUEST`, …); a plain `Error` is sent as INTERNAL_SERVER_ERROR, and `throw new Error()` / `throw Error()` in a resolver (outside a `try` block) is rejected.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/trpc/input-validate` | A procedure's .input() validates. No .input(z.any()), .input(z.unknown()), .input(z.custom<T>()) without a check, .input(z.record(..., z.any())) or a function that only casts its argument. | typescript | ast-grep forbid |
| `typescript/trpc/trpc-error` | A procedure resolver (the function passed to .query(), .mutation() or .subscription(), in a file importing @trpc/server or taking tRPC's { ctx, input } options) throws TRPCError with a code, not a plain Error, with or without new (it reaches the client as INTERNAL_SERVER_ERROR). An Error thrown inside a try block is not concerned: it is caught there. | typescript | ast-grep forbid |

### typescript/turbo

Turborepo (2 and later) runs the workspace's tasks.

Detected by: a file matching `turbo.json`.
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Turborepo 2: tasks are under `tasks` in `turbo.json` (not `pipeline`), and env mode stays strict.
- Every environment variable the code reads is declared in `turbo.json` `env` (per task) or `globalEnv`, so it is part of the cache key.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/turbo/declared-env` | Every environment variable read through process.env is declared in turbo.json (env or globalEnv); an undeclared one is not part of the cache key, so a cached build can carry another environment's value. | typescript | tool biome `lint/suspicious/noUndeclaredEnvVars` |
| `typescript/turbo/no-env-loose-flag` | turbo runs without --env-mode=loose; declare the variables a task needs (env, passThroughEnv) instead. | any | guard deny: command `{name: turbo, flags_any: [--env-mode=loose], value_flags: [--env-mode]}` |
| `typescript/turbo/no-env-loose` | turbo.json keeps the strict env mode (no "envMode": "loose"): in loose mode tasks see every variable and the cache ignores the undeclared ones. | any: `**/turbo.json` | grep forbid `"envMode"[[:space:]]*:[[:space:]]*"loose"` |
| `typescript/turbo/tasks-key` | turbo.json declares its tasks under tasks; Turborepo 2 rejects the old pipeline key. | any: `**/turbo.json` | grep forbid `"pipeline"[[:space:]]*:` |

### typescript/typeorm

TypeORM data sources, raw queries, query builders and schema commands.

Detected by: a dependency on `typeorm` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- TypeORM: schema changes go through migrations (`migration:generate`, `migration:run`); `synchronize: true` and `dropSchema: true` are rejected, `schema:sync` / `schema:drop` are refused and `migration:revert` needs confirmation.
- Raw `.query()` takes its values as parameters (`query(sql, [values])`); query builder conditions and orderings use `:name` parameters, never `${}` or concatenation.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/typeorm/migration-present` | Entity changes come with a migration: once the migrations have run on the local database, TypeORM's migration:generate --check finds no schema change. | typescript | check migration: `forge_typeorm_pending @typeorm-datasource @esm-modules` (needs a local database; timeout 180 s) |
| `typescript/typeorm/migration-revert` | typeorm migration:revert runs the last migration's down() against the database; it needs confirmation. | any | guard ask: command `{name: [typeorm, typeorm-ts-node-esm, typeorm-ts-node-commonjs], sub: ['migration:revert']}` |
| `typescript/typeorm/no-qb-where-interpolation` | No TypeORM query builder where / andWhere / orWhere / having / andHaving / orHaving / orderBy / addOrderBy whose condition is a template with ${}, a concatenation or a variable declared from one; use :name parameters with their values. | typescript | ast-grep forbid (where `$QB`: types engine, §15) |
| `typescript/typeorm/no-query-in-loop` | A TypeORM Repository or EntityManager call (find, findOne, findOneBy, count, save, update, delete, query, …) is not awaited inside a loop body (for, for…of, for…in, while, do) or an async .map / .forEach / .flatMap callback; one query per item is N round trips. Fetch the rows in one query (In([...]), relations, a join) or save an array. | typescript | ast-grep advise (where `$R`: types engine, §15)<br>advice, never blocks |
| `typescript/typeorm/no-raw-query-interpolation` | No TypeORM .query() (DataSource, EntityManager, QueryRunner, Repository) whose SQL is a template with ${}, a concatenation or a variable declared from one in the same function or module; pass the values as parameters (query(sql, [values])). | typescript | ast-grep forbid (where `$R`: types engine, §15) |
| `typescript/typeorm/no-synchronize` | No literal synchronize: true or dropSchema: true in new DataSource({...}), TypeOrmModule.forRoot({...}) or the object a TypeOrmModule.forRootAsync useFactory returns: schema auto-sync alters and drops columns, dropSchema wipes the database at every start; schema changes go through migrations. | typescript | ast-grep forbid (where `$D`: types engine, §15) |
| `typescript/typeorm/schema-sync-guard` | No typeorm schema:sync or schema:drop (any typeorm bin, typeorm-ts-node-esm / -commonjs included); they change or wipe the schema without a migration. | any | guard deny: command `{name: [typeorm, typeorm-ts-node-esm, typeorm-ts-node-commonjs], sub: ['schema:sync\|schema:drop']}` |
| `typescript/typeorm/synchronize-expression` | synchronize or dropSchema set from an expression in new DataSource({...}), TypeOrmModule.forRoot({...}) or a forRootAsync useFactory result is on whenever that expression is true, possibly in production; prefer false and migrations everywhere. | typescript | ast-grep advise (where `$D`: types engine, §15)<br>advice, never blocks |

### typescript/upstash-ratelimit

Upstash rate limiting (@upstash/ratelimit).

Detected by: a dependency on `@upstash/ratelimit` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Upstash Ratelimit: `limit()` never throws; its result is checked (`const { success } = await ratelimit.limit(id)`), never discarded.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/upstash-ratelimit/check-limit` | The result of ratelimit.limit(id) is used and its success checked, not discarded: limit() never throws, so a bare call (awaited or not) rate-limits nothing. | typescript | ast-grep forbid (where `$RL`: types engine, §15) |

### typescript/vite

Vite builds and serves the app.

Detected by: a dependency on `vite` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- `VITE_` variables are inlined into the client bundle: secrets never get the prefix (names with SECRET, PRIVATE, PASSWORD, SERVICE_ROLE or DATABASE_URL are rejected in code and `.env` files).
- `define` in `vite.config.*` never inlines the whole `process.env`, and `server.allowedHosts` / `preview.allowedHosts` list hosts instead of `true`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/vite/no-define-process-env` | define in vite.config.* does not inline the whole process.env ("process.env": process.env or JSON.stringify(process.env)): every server variable, secrets included, would land in the client bundle. Define the variables you need, or use import.meta.env. | typescript: `**/vite.config.*` | ast-grep forbid |
| `typescript/vite/no-open-dev-host` | No server.allowedHosts: true or preview.allowedHosts: true in vite.config.*; it lets any site reach the dev server through DNS rebinding. List the hosts. | typescript: `**/vite.config.*` | ast-grep forbid |
| `typescript/vite/no-public-secret-env-file` | No VITE_ variable named as a secret (SECRET, PRIVATE, PASSWORD, SERVICE_ROLE, DATABASE_URL) in .env files; Vite inlines VITE_ variables into the client bundle. | any: `.env`, `.env.*`, `**/.env`, `**/.env.*` | grep forbid `^[[:space:]]*(export[[:space:]]+)?VITE_[A-Z0-9_]*(SECRET\|PRIVATE\|PASSWORD\|SERVICE_ROLE\|DATABASE_URL)` |
| `typescript/vite/no-public-secret-env` | Secrets are never VITE_ variables, which Vite inlines into the client bundle (import.meta.env.VITE_… named with SECRET, PRIVATE, PASSWORD, SERVICE_ROLE or DATABASE_URL). | typescript | ast-grep forbid |

### typescript/vitest

Vitest runs and test integrity.

Detected by: a dependency on `vitest` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- (when `vitest >=4.0.0` (npm) declared) Vitest 4. Several test setups are `test.projects` in `vitest.config.*`; a `vitest.workspace.*` file is ignored. Parallelism is set with `maxWorkers`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/vitest/config-file` | Claude asks you before editing the Vitest config (test paths, coverage thresholds). | any | permission ask `Edit(**/vitest.config.*)` |
| `typescript/vitest/no-exit-zero` | Test, setup and mock files never call process.exit(0): the run reports success whatever the tests did. | typescript: `**/*.test.*`, `**/*.spec.*`, `**/__tests__/**`, `**/__mocks__/**`, `**/test-utils.*`, `**/test-helpers.*`, `**/vitest.setup.*`, `!e2e/**` | ast-grep forbid |
| `typescript/vitest/no-fewer-expects` | Assertions are not removed from tests. | typescript: `@ts-tests`, `!@e2e-tests` | ast-grep no-decrease |
| `typescript/vitest/no-matcher-override` | expect.extend does not redefine a built-in matcher (toBe, toEqual, toStrictEqual, toMatchObject, toThrow, …): every assertion using it would then pass. Give a custom matcher its own name. The receiver is Vitest's expect by its type, whatever it is called. | typescript: `@ts-tests`, `**/vitest.setup.*`, `**/__mocks__/**` | ast-grep forbid (where `$E`: types engine, §15) |
| `typescript/vitest/no-mock-increase` | A test file may not gain mocks: vi.mock, vi.doMock, vi.spyOn, vi.fn, also through an import alias (import { vi as v }); test through the real code or a fake at the boundary. vi.stubEnv and vi.stubGlobal are not counted: they set configuration for a test, like pytest's monkeypatch. | typescript: `@ts-tests`, `!@e2e-tests` | ast-grep no-increase |
| `typescript/vitest/no-new-skip` | Tests are not skipped, made conditional, left as todo or focused with .only. | typescript: `@ts-tests`, `!@e2e-tests` | ast-grep no-increase |
| `typescript/vitest/no-pass-with-no-tests` | Running Vitest with --passWithNoTests needs confirmation; it turns a missing or mistyped test file into a pass. | any | guard ask: command `{name: vitest, flags_any: [--passWithNoTests]}` |
| `typescript/vitest/no-snapshot-update` | Updating Vitest snapshots needs confirmation; the snapshots are part of the reviewed expectations. | any | guard ask: command `{name: vitest, flags_any: [-u, --update]}` |
| `typescript/vitest/no-tautological-expect` | Assertions test something. No expect() on a literal, such as expect(true).toBe(true). | typescript: `@ts-tests`, `!@e2e-tests` | ast-grep forbid |
| `typescript/vitest/no-test-edits` | Changing a test that existed at the start of the turn (anything in it, formatting aside, input data, .each tables and control flow included), or removing it, needs approval; adding tests is fine. Edits ask when made, unapproved changes are blocked at Stop. Changed test support code (helpers, vi.mock factories, __mocks__, test utils) is reported once. | typescript: `@ts-tests`, `**/__mocks__/**`, `**/test-utils.*`, `**/test-helpers.*`, `**/vitest.setup.*`, `!@e2e-tests` | check lint: `forge_test_edits` |
| `typescript/vitest/no-test-env-branch` | Application code does not branch on whether it runs under Vitest. | typescript: `**/*`, `!@ts-tests`, `!@e2e-tests`, `!vitest.config.*`, `!vitest.setup.*` | ast-grep forbid |
| `typescript/vitest/no-workspace-file` | No vitest.workspace file. Vitest 4 ignores it silently; declare test.projects in vitest.config instead. | any | check lint: `! git ls-files 'vitest.workspace.*' \| grep .` (when `vitest.*` changes) |
| `typescript/vitest/run` | Tests pass. | typescript | check test: `forge_do vitest test` (reruns failing tests; timeout 300 s) |
| `typescript/vitest/snapshot-stale` | Output-producing code changed but its snapshots did not: a source file whose added lines hold a string literal in an expression or an object literal key (not an import, a type annotation, an interface or a type alias), imported by a test that uses toMatchSnapshot, toMatchFileSnapshot, toMatchInlineSnapshot or golden files (testdata/), while no .snap, __snapshots__ or testdata file changed in the turn. Run the snapshot tests with inputs that exercise the new output and update them. Checked at Stop; it blocks once per turn, then accepts. | typescript: `!@ts-tests`, `!@e2e-tests`, `!**/*.d.ts` | check lint: `forge_advise forge_snapshot_stale`<br>advice, never blocks |
| `typescript/vitest/untested-change` | A turn that changes application code also adds or updates a test, or says why none is needed; advisory, it blocks Stop once per turn. | typescript: `!@ts-tests`, `!@e2e-tests`, `!**/*.d.ts`, `!**/*.config.*` | check lint: `forge_advise forge_untested_change`<br>advice, never blocks |

### typescript/vue

Vue 3 single-file components (vue-tsc, v-for keys, v-html).

Detected by: a dependency on `vue >=3.0.0` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Vue 3 with `<script setup lang="ts">` single-file components; `vue-tsc` type-checks `.vue` files (tsc does not).
- Templates: an element with `v-for` binds a `:key` that uses its item, no `v-if` with `v-for` on one element, directives have their value.
- Refs are read with `.value` in script code (never as a condition or operand), and `setup()` does not destructure its props (`toRefs(props)` instead).
- `v-html` and `:innerHTML` in `.vue` files are rejected: they render unescaped HTML. Render text with `{{ }}`, or sanitise in one dedicated component.
- Vue 2 template syntax is rejected (Vue 3 ignores it): `v-model:prop` instead of `.sync`, `v-slot` / `#name` instead of `slot-scope`, no `.native`, key names instead of key codes.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/vue/no-setup-props-destructure` | setup() does not destructure its props parameter: the destructured values are read once and lose reactivity (use props.x, toRefs(props) or toRef(props, "x")). | any: `**/*.vue`, `**/*.ts`, `**/*.mts`, `**/*.cts` | tool biome `lint/correctness/noVueSetupPropsReactivityLoss` |
| `typescript/vue/no-v-html` | No v-html or :innerHTML in .vue files; they render unescaped HTML (XSS). Render text with {{ }}, or sanitise in one dedicated component. | any: `**/*.vue` | grep forbid `(^\|[[:space:]])(v-html\|:innerHTML\|v-bind:innerHTML)[[:space:]]*=` |
| `typescript/vue/no-vue2-template-syntax` | No Vue 2 template syntax, which Vue 3 compiles without a word and ignores: the .sync modifier (v-model:prop), slot-scope (v-slot / #name), the .native modifier, numeric key codes (@keyup.enter). | any: `**/*.vue` | grep forbid `[[:space:]](:\|v-bind:)[a-zA-Z-]+\.sync[[:space:]]*=\|[[:space:]]slot-scope[[:space:]]*=\|[[:space:]](@\|v-on:)[a-zA-Z-]+(\.[a-z]+)*\.native([.=[:space:]]\|$)\|[[:space:]](@\|v-on:)key(up\|down\|press)\.[0-9]+` |
| `typescript/vue/ref-value` | A ref is read through .value in script code: a ref used as an operand, a condition or a template-literal value is the Ref object itself (always truthy), which vue-tsc reports only for arithmetic. | any: `**/*.vue`, `**/*.ts`, `**/*.mts`, `**/*.cts` | tool biome `lint/nursery/noVueRefAsOperand` |
| `typescript/vue/typecheck` | The project passes vue-tsc, which type-checks .vue files (vue-tsc --build for a tsconfig with references). Skipped until nuxt prepare has written .nuxt/ in a Nuxt project. | any: `**/*.vue`, `**/*.ts`, `**/*.tsx`, `**/*.mts` | check typecheck: a shell script calling `forge_do vue-tsc typecheck-build`, `forge_do vue-tsc typecheck` |
| `typescript/vue/v-for-key` | An element with v-for in a .vue template binds :key, so Vue can track each item instead of patching in place. | any: `**/*.vue` | tool biome `lint/correctness/useVueVForKey`<br>deprecated: replaced by `typescript/vue/valid-templates` |
| `typescript/vue/valid-templates` | Vue templates are valid, as Biome checks them with html.experimentalFullSupportEnabled: v-for binds :key and has a valid expression, no v-if with v-for on one element, v-bind / v-on / v-if / v-else / v-else-if / v-html / v-text with what they need, and a valid root <template>. | any: `**/*.vue` | tool biome `lint/correctness/useVueVForKey`<br>tool biome `lint/correctness/noVueVIfWithVFor`<br>tool biome `lint/correctness/useVueValidVBind`<br>tool biome `lint/correctness/useVueValidVOn`<br>tool biome `lint/correctness/useVueValidVIf`<br>tool biome `lint/correctness/useVueValidVElse`<br>tool biome `lint/correctness/useVueValidVElseIf`<br>tool biome `lint/correctness/useVueValidVHtml`<br>tool biome `lint/correctness/useVueValidVText`<br>tool biome `lint/correctness/useVueValidTemplateRoot`<br>tool biome `lint/nursery/useVueValidVFor` |

### typescript/vue/nuxt

Nuxt server routes and runtime config.

Detected by: a dependency on `nuxt >=3.0.0` (npm).
Child of `typescript/vue`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Nuxt: server routes in `server/api/` and `server/routes/` read input with h3's validated helpers, `readValidatedBody(event, schema.parse)`, `getValidatedQuery(event, schema.parse)`, `getValidatedRouterParams(event, schema.parse)`; `readBody`, `readFormData`, `readMultipartFormData`, `getQuery`, `getRouterParams` and `getRouterParam` under `server/` are rejected unless passed straight to a schema (`Schema.parse(await readBody(event))`).
- `runtimeConfig.public` is sent to the browser: keys naming a secret, private key, password, credential, token, API or access key, service role, DSN or database URL there are rejected (unless they say publishable, public, anon or site key); secrets stay at the top level of `runtimeConfig`.
- In `.vue` files, `import.meta.client` / `.server` / `.dev`, never `process.client` / `.server` / `.dev`.
- No module-level `ref()` / `reactive()` in `composables/` or `utils/` (shared by every SSR request): `useState("key", …)` instead.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/vue/nuxt/no-process-flags` | In .vue files, use import.meta.client, .server, .browser and .dev, not process.client, process.server, process.browser or process.dev (deprecated in Nuxt; .ts files get the deprecation from tsc). | any: `**/*.vue` | grep forbid `(^\|[^.a-zA-Z0-9_$])process\.(client\|server\|browser\|dev)([^a-zA-Z0-9_$]\|$)` |
| `typescript/vue/nuxt/no-public-secret` | runtimeConfig.public is sent to the browser; no key there names a secret, private key, password, credential, token, API key, access key, service role, DSN or database URL (keys that say publishable, public, anon or site are allowed: those values are meant for the browser). | typescript: `nuxt.config.*` | ast-grep forbid |
| `typescript/vue/nuxt/no-shared-module-state` | No ref(), reactive(), shallowRef() or shallowReactive() at module level in composables/ or utils/: on the server it is shared by every request (one user's data leaks to another). Use useState('key', ...) or create it inside the composable. | typescript: `composables/**`, `app/composables/**`, `utils/**`, `app/utils/**` | ast-grep forbid |
| `typescript/vue/nuxt/server-validate` | Nuxt server handlers read input with readValidatedBody, getValidatedQuery or getValidatedRouterParams and a schema, or pass the read straight to a schema (Schema.parse(await readBody(event)), safeParse, parseAsync); not readBody, readFormData, readMultipartFormData, getQuery, getRouterParams or getRouterParam whose result is used unvalidated. | typescript: `server/**` | ast-grep forbid |

### typescript/vue/pinia

Pinia stores in Vue.

Detected by: a dependency on `pinia` (npm).
Child of `typescript/vue`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Pinia: state and getters destructured from a store lose reactivity; destructure `storeToRefs(useStore())` (actions can be destructured from the store itself).

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/vue/pinia/store-to-refs` | State and getters destructured from a Pinia store go through storeToRefs(); a plain destructure copies the values and loses reactivity (actions may be destructured directly, so this is advice). | typescript | ast-grep advise (where `$F`: types engine, §15)<br>advice, never blocks |

### typescript/yarn

Yarn (Berry, 2 and later) is the package manager.

Detected by: a file matching `.yarnrc.yml`.
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Yarn (Berry) is the package manager: `yarn add`, `yarn install`; npm, pnpm and bun installs ask.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/yarn/lock-sync` | yarn.lock matches the package.json files (yarn install --immutable --mode=skip-build, Yarn 2 and later; it fails instead of changing the lock file). | any | check lint: `forge_do yarn lock-check` (when `**/package.json`, `yarn.lock` or `.yarnrc.yml` changes) |
| `typescript/yarn/no-other-pm` | Dependencies are managed with Yarn; installing with npm, pnpm or bun asks. | any | guard ask: command `{name: npm, sub: [install\|i\|ci\|add]}` or `{name: pnpm, sub: [add\|install\|i\|update\|up\|remove\|rm]}` or `{name: bun, sub: [install\|i\|add]}` |
| `typescript/yarn/scripts-ratchet` | Install scripts stay a decision of the user: .yarnrc.yml enableScripts is not turned on (Yarn 4 defaults it to false), and no package.json dependenciesMeta entry gains built: true. | any | check lint: `forge_helper bun pm_policy.mjs yarn-scripts \|\| forge_accept_hint` (when `.yarnrc.yml` or `**/package.json` changes) |

### typescript/zod

Zod validation at boundaries.

Detected by: a dependency on `zod` (npm).
Child of `typescript`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Zod validates untrusted input at boundaries; types derive from schemas with `z.infer<typeof Schema>`.
- (when `zod >=4.0.0` (npm) declared) Zod 4: string formats are top-level (`z.email()`, `z.uuid()`, `z.url()`, `z.iso.datetime()`); `z.string().email()` and the like are rejected.
- (when `zod >=4.0.0` (npm) declared) Zod 4: custom messages use the single `error` parameter (`message`, `required_error`, `invalid_type_error` and `errorMap` are rejected).
- (when `zod >=4.0.0` (npm) declared) Zod 4: object strictness is `z.strictObject()` and `z.looseObject()`; errors are read with `error.issues`, `z.treeifyError()`, `z.flattenError()`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `typescript/zod/error-param` | Zod 4 custom messages use the single error parameter ({ error }), not message, required_error, invalid_type_error or errorMap. | typescript | ast-grep forbid<br>applies where `zod >=4.0.0` (npm) declared |
| `typescript/zod/v4-string-formats` | Zod 4 string formats are top-level (z.email(), z.uuid(), z.iso.datetime()), not z.string().email(). | typescript | ast-grep forbid (where `$S`: types engine, §15) |
