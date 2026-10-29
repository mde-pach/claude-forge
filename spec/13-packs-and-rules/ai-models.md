<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `ai-models`

### ai-models

Code that calls hosted AI models (Anthropic, OpenAI, and the SDKs and frameworks built on them).

Detected by: a dependency on one of `anthropic` (PyPI), `openai` (PyPI), `pydantic-ai` (PyPI), `litellm` (PyPI), `langchain-anthropic` (PyPI), `langchain-openai` (PyPI), `@anthropic-ai/sdk` (npm), `openai` (npm), `ai` (npm), `@ai-sdk/anthropic` (npm), `@ai-sdk/openai` (npm), `@langchain/anthropic` (npm), `@langchain/openai` (npm).

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `ai-models/deprecated-model-ids` | A quoted model id on an added line that its provider deprecated (still served until its retirement date: claude-sonnet-4-5, gpt-5, o3, gpt-image-1.5, ...) is reported with its date and replacement, and a dated Claude id that is no published model (claude-opus-5-5-20260401) with the advice to use the alias. Ids as of 2026-10-26, from the providers' deprecation pages; an id published later than that is reported too, hence advice. | any | check lint: a shell script calling `forge_advise`, `forge_model_ids`<br>advice, never blocks |
| `ai-models/model-ids` | No retired model id in a string literal: Anthropic's and OpenAI's retired models fail every call. Use a current model (claude-opus-5-5, claude-sonnet-5-5) or its alias. | any: `**`, `!**/*.{md,mdx,rst,adoc}`, `!**/README*`, `!**/CHANGELOG*`, `!**/docs/**`, `!**/doc/**` | grep forbid ``["'`]((anthropic\|openai)[:/])?(claude-opus-4-1-20250805\|claude-opus-4-1\|claude-opus-4-20250514\|claude-opus-4-0\|claude-sonnet-4-20250514\|claude-sonnet-4-0\|claude-3-7-sonnet-20250219\|claude-3-7-sonnet-latest\|claude-3-5-haiku-20241022\|claude-3-5-haiku-latest\|claude-3-haiku-20240307\|claude-3-opus-20240229\|claude-3-opus-latest\|claude-3-5-sonnet-20241022\|claude-3-5-sonnet-20240620\|claude-3-5-sonnet-latest\|claude-3-sonnet-20240229\|claude-2\.1\|claude-2\.0\|claude-1\.0\|claude-1\.1\|claude-1\.2\|claude-1\.3\|claude-instant-1\.0\|claude-instant-1\.1\|claude-instant-1\.2\|gpt-4\|gpt-4-0613\|gpt-4-1106-preview\|gpt-4o-2024-05-13\|o1\|o1-pro\|o3-mini\|gpt-4\.5-preview\|dall-e-2\|dall-e-3\|chatgpt-4o-latest\|codex-mini-latest\|gpt-5\.2-chat-latest\|gpt-5\.3-chat-latest\|gpt-image-1\|gpt-4-turbo(-[a-z0-9-]+)?\|gpt-3\.5-turbo(-[a-z0-9-]+)?\|gpt-4\.1-nano(-[a-z0-9-]+)?\|gpt-4-32k(-[a-z0-9-]+)?\|text-davinci(-[a-z0-9-]+)?)["'`]`` |
