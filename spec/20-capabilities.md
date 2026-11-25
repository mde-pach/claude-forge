## 20. Capabilities

A **capability** is something the agent can do: a tool or a procedure it runs for some tasks, to find something out or to do the task, which no check decides. Knowledge and constraints are rules (§6), not capabilities. Context does not enforce anything, and generic text can push the agent the wrong way, so nothing generic is placed in the agent's context: a capability reaches the agent only when the task at hand needs it. A capability enforces nothing and grants nothing (§1): whatever must hold stays a rule. A capability is not a Claude Code skill: Forge writes no skill and never touches `.claude/skills`.

Pack `context` items and the rules files (§5.4, §7.1) are not capabilities: they depend on what is active in the project and are given to every session.

### 20.1 The capability file

One YAML file per capability, `capabilities/<name>.yaml` in a pack, whose id is `<pack id>/<name>`. `capabilities` is a reserved directory name of a pack, next to `rules` and `init` (§5.2). Local capabilities live in a local pack's `capabilities/` (`.forge/packs/<id>/capabilities/`) and as standalone files, `.forge/capabilities/<name>.yaml`. A standalone file whose `id` equals a pack capability's replaces it, as a local rule replaces a rule (§5.6): it is kept when its pack is active or is not a known pack at all, and one for a known pack that is not active is ignored with the warning `<file>: pack "<pack>" is not active here; capability <id> is ignored`. Capabilities are project level only.

They go through the loader of packs and rules (§5.1): the strict YAML reader, the JSON Schema generated from the Rust model (`schemas/capability.schema.json`), the parsed cache (the local tree's key covers `.forge/capabilities/`) and the content lints (§6.6). A file that fails stops the command, naming the file.

```yaml
id: typescript/nextjs/web-browser
description: A persistent web browser, helpful to simulate a real user or do testing.
relevant:                  # required; read by the capability engine; any one true selects it
  - The task may involve something a person sees or does in a web page.
threshold: 0.7             # optional, 0 to 1; 0.7 when absent
deliver: on-demand         # inject | on-demand
text: |                    # what the agent gets
  ...
when:                      # optional; exactly a context item's `when` (deps, files, facts, packs)
needs:                     # optional
  tools: [<pinned tool>]
examples:                  # optional
  selected: ["Fix this issue in this nextjs project"]
  not_selected: ["Bump the version in package.json"]
```

- `id`: `<pack id>/<file name>` for a pack's file; loading refuses another (`id must be "<pack>/<name>"`). The agent names a capability by its id's last segment (§20.6).
- `description`: one line (no newline, no tab), agent-facing.
- `relevant`: at least one statement about "the task", each single-line and positive. They are read by the capability engine, never by the agent.
- `threshold`: the engine's value at or above which a statement holds; a number from 0 to 1.
- `deliver`: decided by the file, never by the engine: `inject` (the text goes in the prompt's context) or `on-demand` (one line naming the command that prints it). The list is open to later kinds.
- `text`: what the agent gets, agent-facing.
- `when`: exactly a context item's `when` (§5.4): `deps`, `files`, `facts`, `packs`, judged on the repository's facts in the places the capability's pack applies to. A `when.facts` naming an unknown fact or value, and a `when.packs` naming an unknown pack, refuse the file.
- `needs.tools`: tools Forge pins (§17.2), each named once.
- `examples`: prompts the capability engine selects it for (`selected`) and does not (`not_selected`).
- Unknown fields are refused.

`forge.yaml` `disable` (§5.5) takes a capability id with its reason, as it takes a rule id: the capability is not active here, and `forge status` lists it under `Disabled:`.

The content lints (§6.6) read `description` and `text` as agent-facing text (`cut-by-comment`, `forge-internals`, `enforcement-claim`, `enforcement-limit`), and add three of their own: `inject-size`, `relevant-statement` and `threshold-examples`.

### 20.2 The gates

Three gates, each with one owner. A capability reaches the agent only through all three.

1. **Active**: its pack is active (a standalone local one of no known pack: always) and its `when` holds, on repository facts only; `forge.yaml` does not disable it. Decided by the plan (`forge_render::capabilities`), as the rules are: what holds is the same on every machine. The lock records each active capability (§12).
2. **Available**: what it `needs` is on this machine: each pinned tool is installed in Forge's cache (`forge_capabilities::available`, a couple of `stat` calls, never a download). An unavailable capability is never offered here, and `forge status` says why: `<tool> is not installed on this machine`, or `<tool> is not a tool this Forge pins`. `forge apply` does not provision a capability's tools.
3. **Relevant**: the capability engine says so, per prompt (§20.3).

**What `forge status` says.** After the rules, `Capabilities:` lists each active capability, by id, `(local)` for a local file:

```text
Capabilities:
  acme/linted: inject, not available here: ruff is not installed on this machine; selected for 0 of 0 prompts, loaded 0 times
  acme/web: on-demand, available here; selected for 3 of 12 prompts, loaded 1 times
  acme/gated: not active (its when does not hold in this repository)
```

The counts are read from the capability log (§20.7): the prompts the engine decided on it, those that selected it, and the times the agent loaded it. The section is left out when the repository has no capability. `--json` adds `capabilities` (each with `id`, `deliver`, `local`, `available`, `unavailable`, `decided`, `selected`, `loaded`) and `inactive_capabilities` (`id`, `why`). Drift includes `capability <id> changed since the last apply`, which `forge diff` also prints.

**`forge explain <capability id>`** prints the file, then `Disabled here: <reason>`, `Not active here: <why>.`, `Not active here: pack <pack> is not active.`, or `Active here: ` and its status line. An id that is neither a rule nor a capability: `forge: unknown rule or capability <id>`, exit 1.

**`forge packs dump`** holds each pack's `capabilities`.

### 20.3 The capability engine

The capability engine decides gate 3, per prompt. It is behind a small trait (`forge_capabilities::Engine`): its input is the digest (§20.4) and the candidates' statements, its output the decision on each candidate. The one real implementation is TypeSafe's Jev (`forge_capabilities::jev`, https://docs.typesafe.ai).

On every UserPromptSubmit (§8.5), except Claude Code's own messages (a subagent's report, a background task's end):

1. The candidates are the active, available capabilities (§20.2), from the hooks' cache of the active rules (§8.3), less those already given in this session; in pack-id order. The cache is rebuilt as it is for the rules: a capability file added or changed is seen once the lock or the packaging files change (`forge apply`), and where the repository cannot be planned the lock's `[capabilities]` are the active ones.
2. With none left, no request is made.
3. The digest is built from the hook input's prompt and `transcript_path` (§20.4).
4. One `POST https://api.typesafe.ai/v1/systemone`, `Authorization: Bearer <key>`, with the pinned model (`jev-1.14.0`, recorded with the tool pins: `forge.lock`'s `[tools]` `jev`, §17.2), and one `noul` question per statement, keyed `<capability id>#<n>` (`n` from 0). The `state` is the digest with each part named for what it is, after the situation: `situation` ("A user is working with a coding agent in their repository. `user_message` is the user's latest message to the agent."), `user_message` (the request), `agent_previous_answer` and `agent_recent_actions` (when the digest has them). Each question's `instructions` is an object, `task` ("What the user wants the agent to do now, from `user_message` (read with `agent_previous_answer` and `agent_recent_actions` when they are there).") and `statement`; its `criteria` are the same for every statement: `true` "The statement holds for what the user asks the agent to do now.", `false` "What the user asks now is something else; at most it shares a topic with the statement." Naming the parts and saying that the statement must hold for what is asked now, not share its topic, is what TypeSafe's guidance asks of a state and a Noul; on 60 labelled prompts (half held out) it gave a third of the wrong selections of a bare digest with one framing sentence, for as many misses.
5. A Noul answer is the probability that the statement is true, from 0 to 1. A capability is selected when any of its statements gets a value of at least its threshold: 0.7 unless its file sets one, TypeSafe's starting point for a yes. A selection that was not needed puts text in Claude's context that can push it the wrong way, which costs more than a capability left out, so the default leans to no. A statement the answer leaves out (or answers with something that is not a noul between 0 and 1) counts as not holding, so an answer missing every question of a capability does not select it.

Each decision is logged (§20.7).

**Failures never reach the agent and never hold the prompt.** No retry:

| What happened | Selected | For the person |
|---|---|---|
| No consent or no key on this machine | nothing | no note per prompt; `forge status` says capabilities are off here and why |
| No answer within 1.5 s, no network, 429, 529, a cut answer | nothing, for this prompt | one `environment` note ("… so no capability was selected for this prompt.") |
| 401 | nothing | one `setup` note: give the engine another key with `forge apply --user` |
| Any other status, an answer that is not JSON | nothing | one `ForgeFailure` note |

**Consent and the key.** The digest leaves the machine, so `forge apply --user` asks for consent once per machine, after showing what is sent: "On each prompt that has capabilities to offer, Forge asks TypeSafe's capability engine (api.typesafe.ai) which ones the task needs. Each request sends the prompt's first 8,000 characters, Claude's last answer's first 2,000, the newest 30 tool calls of the last turn (each as the tool and its file, or a command's program and subcommand) and the capabilities' relevance statements; never a file's content, a command's arguments or a tool's output." A `y` at the terminal, or `--capability-engine`, consents; otherwise capabilities stay off here and apply goes on. A new text of the disclosure asks again. Then the key: from `TYPESAFE_API_KEY` in apply's own environment (the variable TypeSafe's documentation suggests), else typed at the terminal with its echo off; a later `forge apply --user` with another `TYPESAFE_API_KEY` replaces it. Both are stored in the user-level state, `<state>/user/capabilities/` (`consent`, `key`; the directory mode 0700, the files 0600), never in a repository or `~/.claude`. The hooks never read the key from the session's environment, and the agent cannot read the file (the state root is refused, §9.3). The key goes in the `Authorization` header only: never on a command line, in a log, a note or an error.

`forge status` says whether the engine is asked here, in the `Capabilities` heading: `capability engine: on, on this machine (jev-1.14.0)`, or why not (`capabilities are off on this machine: nobody consented to the capability engine (forge apply --user asks)`, `… has no key …`, `capabilities are off: FORGE_CAPABILITIES=off`); `--json`: `capability_engine`.

`FORGE_CAPABILITIES=off` turns the whole thing off: no candidate, no request. `FORGE_CAPABILITIES_URL` points the client at another base URL, for Forge's own tests. Selection has its own budget (§8.11).

### 20.4 The digest

What the capability engine reads about the task (`forge_capabilities::digest`), from the hook input's `prompt` and the session's transcript (Claude Code's JSON lines at `transcript_path`):

- `request`: the new prompt, its first 8,000 characters;
- `previous_answer`: the agent's last message, its first 2,000 characters;
- `recent_actions`: the previous turn's tool calls, the newest 30, each as `tool` and `target`. A target is a file path (`file_path`, `notebook_path`, Serena's `relative_path`, a `path`), relative to the repository inside it and `~/…` under the home directory; or, for Bash, each command the guard parses (§9.1), by its program and subcommand, `; `-separated, at most 5 (`git status && uv run pytest -q` reads `git status; pytest`). A subcommand is the word after the program when it is an unquoted lowercase word known before the command runs (letters, digits, `-`); a flag, a path, a file name or quoted text never is. Shell setup (`cd`, `export`, …) is left out. A call with neither has no target.
- Never an argument beyond that word, a file's content or a tool's output.

On a first prompt the transcript does not exist, and the digest is the request alone. The transcript is read thus: only its `user` and `assistant` records count (its other records, `api-request-blob` among them, repeat the calls and carry the session's context, and are never read); a `user` record whose content is text is a prompt, unless it is Claude Code's own (`<local-command…`, `<command-name>`, `<task-notification>`, `<agent-message`, `<system-reminder>`), and starts a turn; an `assistant` record's `text` blocks are the agent's message and its `tool_use` blocks its calls, each counted once by its `id` (the file repeats rows); records of subagents (`isSidechain`), meta records and the compaction summary are left out. When UserPromptSubmit runs, the transcript does not hold the new prompt yet, so the last turn in it is the previous one.

### 20.5 Delivery

Everything goes in the UserPromptSubmit hook's `additionalContext`:

- `inject`: the capability's `text`, the selected ones in pack-id order, separated by a blank line;
- `on-demand`: one line with its name, its description and the command, under a heading after the injected texts.

The exact shape of what the agent reads:

```text
<text of each injected capability>

Available if the task turns out to need them:
- web-browser: A persistent web browser, helpful to simulate a real user or do testing. To use it, run: claude-forge use web-browser
```

The heading and the "To use it, run:" suffix are the only words Forge writes itself. The name is the id's last segment when no other active capability has it, else the whole id (§20.6).

- **Once per session.** Each capability is given once per session (`<session>/capabilities/given`), as its text or as its line.
- **After a compaction.** Text a hook injected does not survive a compaction (§20.9), so SessionStart `compact` gives the current turn's capabilities again, as the agent read them (`<session>/capabilities/turn`, replaced when a new turn starts, added to by a prompt that continues the turn; §8.4).
- **Size.** A prompt's total stays within 8,000 characters (Claude Code cuts hook context at 10,000). Forge handles overflow itself: injected capabilities that do not fit, in pack-id order, are given as on-demand lines for that prompt, and the person gets an `environment` note naming them ("Capabilities given as a line to load rather than their text, …"). A line that does not fit either is left out, not marked given, with a note.

### 20.6 `claude-forge use <name>`

The one Forge command the agent reads and runs (§1.6).

- It prints the capability's `text` on stdout and nothing else (a newline added when the text has none).
- It is read-only and stateless: it finds the project from the working directory (the nearest `forge.lock`, else `forge.yaml`, up to the git root), loads the packs without the parsed cache, plans the repository as it is now, writes nothing and never downloads, so it runs inside Claude Code's sandbox.
- `<name>` is the id's last segment when it is unique among the project's active capabilities, else the whole id; both are accepted.
- It answers for any active, available capability, selected or not, injected or on-demand.
- Any other name (not active here, not available here, ambiguous, unknown, or no project): one neutral line on stderr, `No capability named "<name>" is available here.`, and exit 1.
- `forge apply` writes one `permissions.allow` entry for it, `Bash(claude-forge use *)`, at project level where the project has an active capability, recorded in the lock as its other settings entries are (§7.2, §12).
- The guard lets it through as it does `forge test` (§9.3), and the command line runs it for the agent as it does `mcp` and `lsp` (§3.1).
- The load is recorded by the PreToolUse hook on Bash, not by the command: a command line the guard lets through that runs `claude-forge use <name>` logs a `load` of the capability the name resolves to (§20.7).

### 20.7 State

The capability log, `<state>/<repo-id>/capabilities.log`, holds one tab-separated line per decision and per load (`forge_capabilities::log`):

```text
<unix time>	<session>	decision	<capability id>	<value>,<value>…	selected|not-selected
<unix time>	<session>	load	<capability id>
```

Every decision of the capability engine is a line, and so is every load of an on-demand capability (§20.6). A decision's values are the engine's, one per `relevant` statement in its order (`-` for one the answer left out). The file is created with mode 0600 in the state directory (§8.3). A line that cannot be written is lost, never the prompt.

### 20.8 Examples

A capability's `examples` are its tests: each `selected` prompt must select it, each `not_selected` one must not, each asked of the capability engine as a first prompt (the request alone) with the capability as the only candidate. They need the real engine. `forge test` runs the repository's local capabilities' examples (`--builtin`: the built-in packs') on this machine's engine (§3.2): `PASS`, `FAIL <label>: selected|not selected for "<prompt>" (values <v>, threshold <t>)`, or `SKIP <label>: <why>` when the engine cannot answer here (off, refused, unreachable). Forge's own suite runs them against the real engine only when opted in (`FORGE_TEST_JEV=1`, with `TYPESAFE_API_KEY`), and in the weekly upstream job.

### 20.9 Claude Code behaviour this chapter relies on

Verified on Claude Code 2.1.331 with `claude -p` (not on 2.1.323, the benchmark's version):

- The allow entry `Bash(claude-forge use *)` (and the older `Bash(claude-forge use:*)`) lets `claude-forge use <name>` run without a permission prompt in the `default`, `acceptEdits`, `dontAsk` and `bypassPermissions` modes, and with Claude Code's sandbox on (`default` and `dontAsk`; the real command, on macOS); without it the command is refused in `default`, `acceptEdits` and `dontAsk`. In `plan` mode nothing refused it, but the agent did not run it: plan mode tells it not to act. Project settings, this entry among them, load only in a trusted workspace (§8.12).

- When UserPromptSubmit runs, the transcript at `transcript_path` holds the previous prompts and the agent's messages and tool calls, with their inputs, as `user` and `assistant` records (`message.content` blocks of type `text`, `tool_use`, `tool_result`); it does not hold the new prompt yet. On a session's first prompt the file does not exist yet. Other record types (`attachment`, `api-request-blob`, `last-prompt`, …) share the file.
- Text a UserPromptSubmit hook injected does not survive a compaction (`/compact`): the summary dropped it, and nothing after the compaction boundary held it (the agent found it again only by searching the old transcript file with a tool).

### Tests

- §20.1 the file, the schema, local capabilities, the lints: `crates/forge-packs/tests/capabilities.rs`; `crates/forge-packs/tests/schemas.rs` (the committed schema).
- §20.2 availability: `crates/forge-capabilities/src/available.rs` (unit); the active gate, `disable`, status, explain, diff, the lock and the dump: `crates/forge-cli/tests/spec_cli.rs::capabilities_in_status_explain_diff_and_dump`, `::a_capabilitys_when_and_pack_decide_whether_it_is_active`; the former step agent's sandbox entry taken out: `::apply_user_takes_out_the_former_step_directory`.
- §20.3 the engine: thresholds (0.7 when a capability sets none: `crates/forge-packs/tests/capabilities.rs::the_default_threshold_is_seven_tenths`) and one request, `crates/forge-capabilities/src/engine.rs` (unit); the Jev client's request and each failure kind against a local server, `crates/forge-capabilities/src/jev.rs` (unit); consent and key, `crates/forge-capabilities/src/setup.rs` (unit), `crates/forge-cli/tests/spec_cli.rs::the_capability_engine_is_set_up_at_apply_user`; selection in the hook, once per session, no request without candidates, `FORGE_CAPABILITIES=off`, each failure's silence and note, a disabled one, an installed tool, Claude Code's own messages, the hooks' cache and its lock fallback: `crates/forge-hooks/tests/spec_hooks.rs` (`a_prompt_gets_the_selected_capabilities_once_per_session`, `a_middling_value_selects_nothing_by_default`, `no_request_without_candidates`, `each_failure_is_silence_and_a_note`, `a_disabled_capability_is_never_offered`, `a_capability_whose_tool_is_installed_is_offered`, `claude_codes_own_messages_ask_nothing`, `the_active_capabilities_follow_the_hooks_cache`); the consent and key prompts at the terminal: `crates/forge-cli/src/project/engine_setup.rs` (unit, a scripted person; the echo-off read itself needs a real terminal and has no test); the hook through the real client and the stored key: `crates/forge-cli/tests/hook.rs::capabilities_through_the_real_client`.
- §20.4 the digest from a transcript fixture: `crates/forge-capabilities/src/digest.rs` (unit, `tests/fixtures/transcript.jsonl`); from the hook input: `spec_hooks.rs::the_digest_comes_from_the_transcript`.
- §20.5 the shape, names, overflow: `crates/forge-capabilities/src/render.rs` (unit); in the hook: `spec_hooks.rs` (`a_prompt_gets_the_selected_capabilities_once_per_session`, `on_demand_capabilities_are_one_line_each`, `injected_texts_that_do_not_fit_become_lines`, `a_compaction_gives_the_turns_capabilities_again`, `a_prompt_continuing_the_turn_adds_to_its_capabilities`, `a_capability_that_cannot_fit_is_left_out_with_a_note`).
- §20.6 the command, its names, its refusal, writing nothing, the allow entry in the lock: `crates/forge-cli/tests/spec_cli.rs::the_use_command_prints_a_capabilitys_text`; the guard's exception and the load: `spec_hooks.rs::the_use_command_is_let_through_and_its_load_logged`, `::the_use_command_through_the_forge_name`; the internals check's one exception: `crates/forge-core/src/internals.rs` (unit).
- §20.7 the log and its counts: `crates/forge-capabilities/src/log.rs` (unit).
- §20.8 examples: `crates/forge-capabilities/src/examples.rs` (unit), `crates/forge-cli/tests/forge_test.rs::capability_examples_run_on_this_machines_engine` (a fake engine on localhost), `::capability_examples_against_the_real_engine` (opt-in).
- §20.9: verified by hand with `claude -p` (no test: needs Claude Code and an account).
