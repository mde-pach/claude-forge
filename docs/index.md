---
layout: home
hero:
  name: Forge
  text: Guardrails for Claude Code, generated from rule packs
  tagline: One command writes a standalone setup — hooks, permissions, checks — that runs with plain Claude Code. Nothing to install, nothing that calls Forge.
  image:
    src: /logo.svg
    alt: Forge
  actions:
    - theme: brand
      text: Get started
      link: /guide/getting-started
    - theme: alt
      text: Browse the packs
      link: /packs/
    - theme: alt
      text: GitHub
      link: https://github.com/mde-pach/claude-forge
features:
  - icon: 🧱
    title: Standalone output
    details: The generated files are POSIX sh, git, grep, sed and awk, plus your project's own tools. Uninstall Forge and everything keeps working.
  - icon: 🎯
    title: Enforced in the closest native place
    details: Your linter and type checker first, then ast-grep, then Claude Code permissions and the sandbox, then hooks — only for what concerns Claude's actions.
  - icon: 🔒
    title: Claude cannot change the setup unnoticed
    details: Forge-owned files sit behind a permission prompt and are hash-checked on every Stop, pre-commit and CI run.
  - icon: 📦
    title: Rules are data
    details: One YAML file per rule, with examples that double as tests. Adding a rule means adding a file — and these docs update themselves.
  - icon: 🩺
    title: Audits, never edits
    details: Forge tests your Biome, ruff, basedpyright and tsc configs against each rule's examples and tells you the exact snippet to add. It never writes your config.
  - icon: 🤫
    title: Success is silent
    details: Hooks print nothing when all is well. Every message starts with forge:&lt;rule-id&gt;, so you always know which rule spoke and why.
---
