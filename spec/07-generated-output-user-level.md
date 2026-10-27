## 7. Generated output — user level

```
~/.claude/
├── settings.json                         # appended entries only
├── skills/forge-user/
│   ├── .claude-plugin/plugin.json        # {"name":"forge-user", ...}
│   ├── hooks/hooks.json
│   └── scripts/{guard.sh, guard.rules, notify.sh, review.sh, lib.sh}
└── forge.lock.json
```

- **Hooks** (`hooks/hooks.json`, commands use `${CLAUDE_PLUGIN_ROOT}`):

| Event | Matcher | Script | Options |
|---|---|---|---|
| PreToolUse | `Bash` | `guard.sh` | timeout 180 s (an ask may wait for a review, §9.3), `onFailure: "block"` |
| PermissionRequest | — | `review.sh` | timeout 180 s (§9.3) |
| Notification | `permission_prompt\|idle_prompt` | `notify.sh` | prints `{"terminalSequence":"\u001b]777;notify;Claude Code;<msg>\u0007"}`; OSC 9 when `TERM_PROGRAM` is `iTerm.app` or `WezTerm` |

- **Appended to `~/.claude/settings.json`:** the `safety` pack's `permission` and `sandbox` targets, and `ask` on `Edit(~/.claude/skills/forge-user/**)` and `Edit(~/.claude/forge.lock.json)`.
- **A sandbox that cannot start.** On Linux, when the setup enables the sandbox, `apply --user` and `status --user` check that bubblewrap can run it (`bwrap` and `socat` on PATH; `bwrap --ro-bind / / --dev /dev --proc /proc --unshare-all --die-with-parent true` succeeds; `--proc` because Claude Code's sandbox mounts a new proc). Otherwise they print why (missing packages, or bubblewrap's error: in a container with Docker's default seccomp/AppArmor profile, "Creating new namespace failed: Operation not permitted"; with Docker's masked `/proc`, "Can't mount proc on /newroot/proc: Operation not permitted", which `--security-opt systempaths=unconfined` lifts) and the ways out: give the container what bubblewrap needs, or set `"sandbox": {"enabled": false}` in `~/.claude/settings.json` knowingly (Forge never changes a value the user set).
