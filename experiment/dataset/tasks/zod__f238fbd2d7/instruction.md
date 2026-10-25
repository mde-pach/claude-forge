You are working in the git repository in the current directory. Make the change described below. Work autonomously: nobody will answer questions.

# Emoji validation is vulnerable to catastrophic backtracking (ReDoS)

## Problem

The emoji check (`z.string().emoji()` and `z.emoji()` in v4, `z.string().emoji()` in v3) uses a regular expression that backtracks exponentially when a match fails.

The pattern is a repeated alternation of `\p{Extended_Pictographic}` and `\p{Emoji_Component}`. These two Unicode properties overlap on U+1F9B0–U+1F9B3 (🦰🦱🦲🦳). On a failed match, the engine tries every way of assigning each overlapping character to one branch or the other. For example, 26 copies of `🦰` followed by a space takes about 2 seconds to reject, and each extra character roughly doubles the time. A string of about 100 bytes can block the event loop for seconds.

## Required behaviour

- The emoji check must accept and reject exactly the same strings as before. In particular:
  - `"🦰🦱🦲🦳"` must still parse successfully.
  - Mixed strings such as `"😀 is an emoji"`, `"😀stuff"` and `"stuff😀"` must still be rejected.
- Rejecting an input such as `"🦰".repeat(26) + " "` must take well under 100 ms. This applies to both v3 (`z.string().emoji()`) and v4 (`z.emoji()` / `z.string().emoji()`).
- In v4, `z.core.regexes.emoji()` must return a `RegExp` with flags `"u"` and exactly this source:

  ```
  ^[\p{Extended_Pictographic}\p{Emoji_Component}]+$
  ```

  This changes user-visible output:
  - The `pattern` in an invalid-emoji issue becomes `"/^[\\p{Extended_Pictographic}\\p{Emoji_Component}]+$/u"`.
  - `z.toJSONSchema(z.emoji())` emits `"pattern": "^[\\p{Extended_Pictographic}\\p{Emoji_Component}]+$"` together with `"format": "emoji"`.
- The v3 emoji regex must use the same single-character-class form.

## Broader guarantee

Every pattern exported from v4's `z.core.regexes` must be free of super-linear backtracking. This covers:

- every exported `RegExp` value;
- the output of the factory functions across their argument ranges:
  - `emoji()`
  - `mac()` and `mac("-")`
  - `uuid()` and `uuid(1..8)`
  - `string()` and `string({ minimum, maximum })`
  - `time({ precision })` for `precision` in `null, -1, 0, 3, 6`
  - `datetime({ precision, local, offset })` for all combinations of those precisions with `local` and `offset` set to `true` or `false`

Safety is judged by the `recheck` ReDoS checker, called with each pattern's real `source` and `flags`. The flags matter: without `u`, `\p{...}` is read as a literal and a vulnerable pattern can look safe. The current emoji pattern fails this check; every pattern must pass it after the change.
