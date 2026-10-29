## 13. Packs and rules

This chapter lists every built-in pack and rule. It is generated from the packs as Forge loads them, so it cannot drift from them: `crates/forge-packs/tests/spec_tables.rs` renders the files of this directory (the per-pack files and the index below) and fails when they differ. After a pack change, regenerate them:

```sh
FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables
```

Only the text outside the generated markers of this file is written by hand.

### 13.1 Reading the tables

- One file per root pack. Headings are pack ids, in tree order: a pack, then its children. A rule sits under the pack whose `rules/` directory holds it; its id is `<pack id>/<file name>`.
- **Detected by** renders the pack's `detect` (§5.1): any one of the listed facts detects it. A child pack is active only where its parent is active and it is detected itself. A pack with no `detect` is active only when `forge.yaml` adds it or an active pack requires it. User-level packs are not detected: `forge apply --user` installs them.
- **Context** lists the pack's context items (§5.1): the text the agent reads in `.claude/rules/forge-*.md` while the pack is active, each with its condition (`when`) when it has one. An item with an `id` is rendered once even when several active packs carry it.
- **Scope** is the rule's `scope`: its language and, when given, its file globs (`@role` names a file role, §15.4).
- **Enforced by** lists the rule's targets in order (§6.2):
  - `tool <tool> <code>`: an audit of the project's own tool config (§11).
  - `ast-grep <mode>`: an ast-grep rule run by the checker; `where` conditions are answered by the types engine (§15).
  - `grep <mode> <pattern>`: a regular expression matched line by line on the rule's scope.
  - `permission <decision> <rule>`: a Claude Code permission entry written to the settings (§7).
  - `guard <decision>`: the PreToolUse Bash guard (§9): a structural command shape (`command`), a built-in check (`check`), or both.
  - `sandbox <path> = <value>`: a user-level sandbox setting (§7).
  - `check <stage>`: a check command run by the checker (§10). A short command is shown as written. A long one is summarised by what it calls: `forge_do <tool> <operation>` (a tool adapter operation, §16) and the `forge_<name>` builtins (§6).
  - Then, when they apply: the rule's version gate (`when`, "applies where … declared"), "advice, never blocks" (every target is advisory: an `advise` mode or a `forge_advise` check), and a deprecation with its replacement.

### 13.2 Packs

<!-- generated:packs -->
| Root pack | Level | Packs | Rules |
|---|---|---|---|
| [ai-models](ai-models.md) | project | 1 | 2 |
| [code](code.md) | project | 1 | 0 |
| [docker](docker.md) | project | 2 | 23 |
| [docs](docs.md) | project | 1 | 2 |
| [git](git.md) | project | 1 | 5 |
| [github-actions](github-actions.md) | project | 1 | 5 |
| [gitlab-ci](gitlab-ci.md) | project | 1 | 5 |
| [kubernetes](kubernetes.md) | project | 3 | 7 |
| [openapi](openapi.md) | project | 1 | 3 |
| [pre-commit](pre-commit.md) | project | 1 | 1 |
| [python](python.md) | project | 52 | 255 |
| [release](release.md) | project | 1 | 3 |
| [safety](safety.md) | user | 1 | 35 |
| [secrets](secrets.md) | project | 1 | 3 |
| [sql](sql.md) | project | 1 | 3 |
| [supply-chain](supply-chain.md) | project | 1 | 9 |
| [terraform](terraform.md) | project | 1 | 8 |
| [toolchain](toolchain.md) | project | 1 | 2 |
| [typescript](typescript.md) | project | 75 | 315 |

147 packs, 686 rules.
<!-- /generated:packs -->

### Tests

- The tables: `crates/forge-packs/tests/spec_tables.rs` (`spec_13_is_generated_from_the_packs`, `every_rule_sits_under_its_own_pack`).
- Every rule has examples (§6.1): `crates/forge-packs/tests/spec_tables.rs::every_built_in_rule_has_examples`.
- Every rule's examples behave as declared: `crates/forge-check/tests/pack_examples.rs` (ast-grep and grep), `crates/forge-guard/tests/examples.rs` (guard), `crates/forge-cli/tests/check.rs::check_examples` (check targets whose tools are installed), `crates/forge-builtins-code/tests/pack_examples.rs`, `crates/forge-builtins-data/tests/examples.rs`, `crates/forge-builtins-infra/tests/examples.rs` (the builtins' rules).
- Permission and tool-target examples: (no test: the TypeScript Forge's `forge test` matched permission strings against examples and audited tool targets on them; `forge test` has no Rust counterpart yet, and tool-target audits run only in `forge status`, §11).
