---
title: Configuration
description: Adjust detected packs and rules with forge.yaml and project-local rules.
---

Most projects need no configuration: Forge activates packs from your manifests and lock files. Two optional inputs adjust the result. Forge never rewrites them, and Claude must ask before editing them.

## `forge.yaml`

```yaml
forge: "0.5"
packs:
  add: [typescript/zod]          # activate a pack (and its ancestors)
  remove: [typescript/playwright] # deactivate a pack and its sub-packs
disable:
  - id: python/pytest/run
    reason: The suite needs a GPU; it runs in CI only.
```

Every rule blocks, so a rule you don't want is disabled with a reason, which `forge status` shows. The exact behaviour — ancestors, subtrees, warnings, invalid files — is specified in [5 · Packs and project inputs](../../spec/05-inputs/), and the file format in [Schemas](../../reference/schemas/#forge).

## Project-local rules

Rule files in `.forge/rules/` use the same format as pack rules. A local rule whose `id` equals a pack rule's id replaces it; a new id adds a rule. For example, a stricter version of `typescript/no-ts-ignore`:

```yaml
# .forge/rules/no-ts-ignore.yaml
id: typescript/no-ts-ignore
description: No @ts-ignore or @ts-nocheck.
scope: {language: typescript}
enforce:
  - target: ast-grep
    mode: forbid
    rule: {kind: comment, regex: "^//[[:space:]]*@ts-(ignore|nocheck)"}
examples:
  match: ["// @ts-nocheck\nconst a = 1;\n"]
  no_match: ["// ok\nconst a = 1;\n"]
```

`forge test`, run inside the repository, tests local rules' examples too.
