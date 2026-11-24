## 20. Capabilities

A **capability** is something the agent is able to use or should use: a tool on this machine, a way of working the project expects. Context does not enforce anything, and generic text can push the agent the wrong way, so nothing generic is placed in the agent's context: a capability reaches the agent only when the task at hand needs it. A capability enforces nothing and grants nothing (§1): whatever must hold stays a rule. A capability is not a Claude Code skill: Forge writes no skill and never touches `.claude/skills`.

Pack `context` items and the rules files (§5.4, §7.1) are not capabilities: they depend on what is active in the project and are given to every session.

### 20.1 The capability file

One YAML file per capability, `capabilities/<name>.yaml` in a pack, whose id is `<pack id>/<name>`. `capabilities` is a reserved directory name of a pack, next to `rules` and `init` (§5.2). Local capabilities live in a local pack's `capabilities/` (`.forge/packs/<id>/capabilities/`) and as standalone files, `.forge/capabilities/<name>.yaml`. A standalone file whose `id` equals a pack capability's replaces it, as a local rule replaces a rule (§5.6): it is kept when its pack is active or is not a known pack at all, and one for a known pack that is not active is ignored with the warning `<file>: pack "<pack>" is not active here; capability <id> is ignored`. Capabilities are project level only.

They go through the loader of packs and rules (§5.1): the strict YAML reader, the JSON Schema generated from the Rust model (`schemas/capability.schema.json`), the parsed cache (the local tree's key covers `.forge/capabilities/`) and the content lints (§6.6). A file that fails stops the command, naming the file.

```yaml
id: typescript/nextjs/web-browser
description: A persistent web browser, helpful to simulate a real user or do testing.
relevant:                  # required; read by the capability engine; any one true selects it
  - The task may involve something a person sees or does in a web page.
threshold: 0.3             # optional, 0 to 1; 0.3 when absent
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

### 20.7 State

The capability log, `<state>/<repo-id>/capabilities.log`, holds one tab-separated line per decision and per load (`forge_capabilities::log`):

```text
<unix time>	<session>	decision	<capability id>	<value>,<value>…	selected|not-selected
<unix time>	<session>	load	<capability id>
```

A decision's values are the engine's, one per `relevant` statement in its order (`-` for one the answer left out). The file is created with mode 0600 in the state directory (§8.3). A line that cannot be written is lost, never the prompt.

### Tests

- §20.1 the file, the schema, local capabilities, the lints: `crates/forge-packs/tests/capabilities.rs`; `crates/forge-packs/tests/schemas.rs` (the committed schema).
- §20.2 availability: `crates/forge-capabilities/src/available.rs` (unit); the active gate, `disable`, status, explain, diff, the lock and the dump: `crates/forge-cli/tests/spec_cli.rs::capabilities_in_status_explain_diff_and_dump`.
- §20.7 the log and its counts: `crates/forge-capabilities/src/log.rs` (unit).
