You are auditing the hidden tests of a benchmark task, before the task is used. The working directory holds the repository `{repo}` **after** the reference change. You may read any file in it with Read, Grep and Glob.

An engineer will receive only the instruction below and must implement it from the state before the change. Their work is then scored by running the hidden tests. The tests come from the upstream commit. Upstream tests often check *how* the code works: private helpers, internal state, exact wording nobody specified, mocks of internals. Such checks reject correct solutions that are implemented differently. Your job is to make the scoring check the **outcome** only, by applying this fixed rule.

## The rule
- Keep each test at its level: a unit test stays a unit test.
- Assert only what is observable at that level through the interface the instruction names, or through interfaces that already existed before the change.
- Allow test doubles only at the unit's boundary.
- Drop any test that cannot be checked without inspecting the implementation. Examples: private names (`_x`), internal state or attributes the instruction does not name, call counts or arguments on internal collaborators, exact wording or formatting the instruction does not specify, internal module paths a different correct implementation would not have, snapshot output of internals.
- When in doubt, drop.

## What you may do
- **Drop** a test. Give its exact ID from the lists below.
- **Relax** a test by deleting or loosening an assertion, as an exact search-and-replace edit on a hidden test file. `old` must occur exactly once in that file, and `new` replaces it. An edit may only remove or weaken checks, for example deleting an assert line, or replacing an exact-message match with a type-only check. It must never add a new check, change the test's level, or change what is being tested.
- **Keep** a test unchanged.

Judge every F2P test individually. For the P2P tests (existing tests that already passed before the change), drop only those that a correct implementation of the instruction, done differently from the reference, could plausibly break by inspecting internals. Otherwise keep them, without listing them.

## Task instruction (exactly what the engineer sees)
{instruction}

## Hidden test files (paths, repository-relative)
{test_files}

## Test diff (before → after the change)
```diff
{test_diff}
```

## Reference change (non-test diff; for your understanding only)
```diff
{ref_diff}
```

## F2P tests (fail before the change, pass after)
{f2p}

## P2P tests (pass before and after)
{p2p}

Return JSON that matches the schema:
- `verdicts`: one entry per F2P test, plus any P2P test you drop or relax. Each entry has `id`, `action` (`keep`, `drop` or `relax`) and `reason`, one sentence that cites the rule.
- `edits`: the search-and-replace edits for relaxed tests.
