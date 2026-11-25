<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `code`

### code

Norms for changing code, shared by the language packs.

Detected by: every repository.

Context (in `.claude/rules/forge-*.md`, §7):

- `reuse-helpers` (when pack `python` or `typescript` active) Before writing a utility, search the codebase for an existing helper (warn-once and logging helpers, `utils` functions such as an existing property setter) and reuse it instead of adding a near-duplicate.
- `test-integrity` (when pack `python/pytest`, `typescript/vitest`, `typescript/playwright`, `typescript/bun` or `typescript/jest` active) Tests assert behaviour: never weaken, remove or skip an assertion to make a test pass.
- `test-edits` (when pack `python/pytest`, `typescript/vitest`, `typescript/playwright`, `typescript/bun` or `typescript/jest` active) When the task changes behaviour an existing test asserts, update that test in place with Edit or Write (keep its name) and say so in your summary. Otherwise never edit an existing test to make the code pass.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `code/no-debug-tags-left` | Temporary debug instrumentation tagged [DEBUG-…] is removed before the work is done: grep for the tag and delete every tagged line. | any: `**/*.{py,pyi,ts,tsx,mts,cts,js,jsx,mjs,cjs,vue,svelte,astro,go,rs,rb,java,kt,swift,php,cs,c,h,cc,cpp,hpp,sh,ex,exs,scala,dart,lua}` | grep forbid `\[DEBUG-[A-Za-z0-9_-]+\]` |
| `code/no-ease-in-ui` | UI transitions do not use ease-in: it starts slowly, so the element lags at the moment the user watches it. Use ease-out or a custom cubic-bezier for elements entering, leaving or responding to input. | any: `**/*.{css,scss,sass,less,ts,tsx,js,jsx,vue,svelte,astro,html}` | grep advise `(^\|[^A-Za-z0-9_-])ease-in([^A-Za-z0-9_-]\|$)\|["']easeIn["']`<br>advice, never blocks |
| `code/no-scale-zero-entrance` | Animations do not start from scale(0): nothing in the real world appears from nothing. Start from scale(0.9) to scale(0.97) with opacity 0, and scale popovers from their trigger with transform-origin. | any: `**/*.{css,scss,sass,less,ts,tsx,js,jsx,vue,svelte,astro,html}` | grep advise `scale\([[:space:]]*0[[:space:]]*\)\|scale["']?[[:space:]]*:[[:space:]]*0([^.0-9]\|$)`<br>advice, never blocks |
| `code/no-transition-all` | Transitions name the properties they animate (transition: transform 200ms, opacity 200ms; Tailwind transition-transform, transition-opacity), not all: all animates every property that changes, layout ones included, off the GPU. | any: `**/*.{css,scss,sass,less,ts,tsx,js,jsx,vue,svelte,astro,html}` | grep advise ``transition(-property)?["']?[[:space:]]*:[[:space:]]*["'`]?all([^A-Za-z0-9_-]\|$)\|(^\|[^A-Za-z0-9_-])transition-all([^A-Za-z0-9_-]\|$)``<br>advice, never blocks |
| `code/no-versioned-duplicates` | Code is changed in place, not copied: no new foo-v2.ts, foo_new.py, foo-old.tsx, Button copy.tsx or foo.ts.bak beside the foo it copies, which leaves two versions to maintain and the old one still imported. Edit the original and delete the copy, unless the task asks for both side by side (an API v2). At Stop it blocks once per turn, then accepts. | any: `**/*.{ts,tsx,mts,cts,js,jsx,mjs,cjs,py,vue,svelte,astro}`, `**/*.bak`, `**/*.orig`, `!@build-artifacts`, `!**/migrations/**` | check lint: a shell script calling `forge_advise`, `forge_new_files`<br>advice, never blocks |
| `code/stryker-output-ignored` | A project with a StrykerJS config ignores its working directory and reports in .gitignore (.stryker-tmp/ and reports/mutation/): Stryker copies the whole project into .stryker-tmp on every run. | any: `stryker.conf.*`, `stryker.config.*`, `.gitignore` | check lint: a shell script of 12 lines (when `stryker.conf.*`, `stryker.config.*` or `.gitignore` changes) |

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `code/change-impact` | Weighing the risk of a diff by who calls the changed code, its history and what tests cover it. | on-demand | The task is to review a diff, a branch or a pull request for its risk or impact. | wherever its pack is |
| `code/duplicate-intent` | Finding functions that do the same thing under different names, which a copy-paste detector misses. | on-demand | The task is to find functions or helpers that duplicate each other's purpose.<br>The task is to consolidate duplicated helpers or utilities. | wherever its pack is |
| `code/flaky-test` | Finding why a test passes and fails on different runs, by rerunning it many ways and comparing. | on-demand | The task is about a test that fails only sometimes, or only in CI, or only with other tests. | wherever its pack is |
| `code/http-check` | Checking what a running site answers over HTTP (headers, redirects, status codes, metadata, TLS) with curl. | on-demand | The task changes or needs checked the HTTP headers, redirects, status codes or caching a site sends.<br>The task changes or needs checked a site's SEO metadata, robots.txt or sitemap. | wherever its pack is |
| `code/pr-review-comments` | Reading the unresolved review comments of the branch's GitHub pull request with gh, to address them. | on-demand | The task is to address review comments or feedback on a GitHub pull request. | wherever its pack is |
| `code/prototype` | Answering a design question with a throwaway prototype before implementing it in the real code. | on-demand | The task asks to prototype, sketch or compare options for a design before building it. | wherever its pack is |
| `code/reproduce-first` | Fixing a reported bug by reproducing it with a failing test first, then fixing its cause. | inject | The task is to fix a reported bug, an error or a crash.<br>The task reports code that gives a wrong result or behaves differently from what is expected. | wherever its pack is |
| `code/test-first` | Building a change test-first, one behaviour at a time, watching each test fail before writing the code. | on-demand | The task asks for the change to be developed test-first or with TDD.<br>The task asks to write the tests before the implementation. | wherever its pack is |
| `code/test-strength` | Finding out whether tests would catch a bug, by breaking the code on purpose or with a mutation tool. | on-demand | The task asks whether tests actually catch bugs, or to strengthen weak tests.<br>The task involves mutation testing or surviving mutants. | wherever its pack is |
| `code/variant-analysis` | Finding the other places where a known bug or vulnerability occurs, from one instance. | on-demand | The task is to find or fix other occurrences of a bug or vulnerability already found in one place. | wherever its pack is |

### code/jscpd

The project's own jscpd (duplicated blocks), as its config sets it up.

Detected by: a file matching `.jscpd.json`; or a line matching `^\s*"jscpd"\s*:\s*\{` in a file matching `package.json`.
Child of `code`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `code/jscpd/duplicates` | The change does not copy code the project's jscpd finds duplicated (with its own config): put the shared code in one function or module and use it from both places. A duplicate is advice, and fails when the project's jscpd config fails on it (the duplication over its `threshold`, or an `exitCode`). | any | check lint: `forge_jscpd` |
