//! The hook entries Forge writes into the settings file. Each runs
//! [`forge_launcher::hook_wrapper`] for its event: one process per event, which reads the hook
//! input (tool name included) on stdin; the wrapper keeps quiet when Forge is not installed.
//!
//! Project level:
//!
//! | Claude Code event  | Matcher                                      | `forge hook` event        | Timeout | Extra            |
//! |--------------------|----------------------------------------------|---------------------------|---------|------------------|
//! | SessionStart       | `startup\|resume\|clear\|compact`            | `session-start`           | 10 s    |                  |
//! | UserPromptSubmit   |                                              | `user-prompt-submit`      | 10 s    |                  |
//! | PreToolUse         | `Bash`                                       | `pre-tool-use`            | 180 s   | `onFailure: block` |
//! | PreToolUse         | [`FILE_TOOLS`]                               | `pre-tool-use`            | 180 s   |                  |
//! | PostToolUse        | [`EDITS_AND_BASH`]                           | `post-tool-use`           | 60 s    |                  |
//! |                    |                                              | `post-tool-use-typecheck` | 300 s   | `asyncRewake`    |
//! | PostToolUseFailure | `Bash`                                       | `post-tool-use-failure`   | 60 s    |                  |
//! |                    |                                              | `post-tool-use-typecheck` | 300 s   | `asyncRewake`    |
//! | Stop               |                                              | `stop`                    | 600 s   |                  |
//! | PermissionRequest  |                                              | `permission-request`      | 180 s   |                  |
//! | SessionEnd         |                                              | `session-end`             | 10 s    |                  |
//! | PreToolUse         | `*`                                          | `agents`                  | 30 s    |                  |
//! | PostToolUse        | [`AGENT_TOOL`]                               | `agents`                  | 30 s    |                  |
//! | SubagentStart      | [`FORGE_AGENTS`]                             | `agents`                  | 30 s    |                  |
//! | SubagentStop       | [`FORGE_AGENTS`]                             | `agents`                  | 600 s   |                  |
//!
//! The `agents` entries run Forge's own agents (the workflow steps and the reviewer, native
//! subagents): the gate while a step is pending matches every tool, the others the Agent call and
//! the two agents' start and end.
//!
//! User level: PreToolUse `Bash` → `user-pre-tool-use` (180 s, `onFailure: block`), PreToolUse
//! [`FILE_TOOLS`] → `user-pre-tool-use` (180 s: the internals refusal), PermissionRequest → `user-permission-request` (180 s), Notification
//! `permission_prompt|idle_prompt` → `user-notification` (5 s), and the reviewer agent's entries
//! (PreToolUse [`REVIEW_TOOLS`], PostToolUse [`AGENT_TOOL`], SubagentStart and SubagentStop
//! `forge-reviewer` → `user-agents`). The `user-` events are the
//! user-level setup's own (its rules, not the project's), so a project with both runs each once.
//!
//! The 180 s timeouts leave room for a question nobody is there to answer, which a reviewer
//! answers; the usual answer takes milliseconds.

use forge_packs::model::Level;
use serde_json::{Map, Value, json};

use crate::settings::{DesiredEntry, SettingsEntry};

/// One command of a matcher group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookCommand {
    /// The event `forge hook` is called with (lower-case kebab).
    pub event: &'static str,
    pub timeout: u32,
    /// A crash blocks the tool call (the guard fails closed).
    pub block_on_failure: bool,
    /// Runs in the background; exit 2 wakes Claude with its stderr.
    pub async_rewake: bool,
}

/// A matcher group of a Claude Code event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookGroup {
    pub event: &'static str,
    pub matcher: Option<&'static str>,
    pub commands: &'static [HookCommand],
}

const fn cmd(event: &'static str, timeout: u32) -> HookCommand {
    HookCommand {
        event,
        timeout,
        block_on_failure: false,
        async_rewake: false,
    }
}

const GUARD: HookCommand = HookCommand {
    block_on_failure: true,
    ..cmd("pre-tool-use", 180)
};
const TYPECHECK: HookCommand = HookCommand {
    async_rewake: true,
    ..cmd("post-tool-use-typecheck", 300)
};
const USER_GUARD: HookCommand = HookCommand {
    block_on_failure: true,
    ..cmd("user-pre-tool-use", 180)
};

/// Claude Code's tools that read, search or edit files (every one taking a path), and Serena's.
pub const FILE_TOOLS: &str = "Read|Edit|Write|MultiEdit|NotebookEdit|Grep|Glob|mcp__serena__.*";

/// Claude Code's tools that edit files, Bash, and Serena's (its edits are checked like them).
pub const EDITS_AND_BASH: &str = "Edit|Write|MultiEdit|NotebookEdit|Bash|mcp__serena__.*";

/// The Agent tool (`Task` in older Claude Code).
pub const AGENT_TOOL: &str = "Agent|Task";

/// Forge's own agents (`forge_hooks::agents`).
pub const FORGE_AGENTS: &str = "forge-workflow-step|forge-reviewer";

/// The reviewer's calls at user level: its start (the Agent call) and its own read-only tools.
pub const REVIEW_TOOLS: &str = "Agent|Task|Read|Grep|Glob|mcp__serena__.*";

pub const PROJECT_HOOKS: &[HookGroup] = &[
    HookGroup {
        event: "SessionStart",
        matcher: Some("startup|resume|clear|compact"),
        commands: &[cmd("session-start", 10)],
    },
    HookGroup {
        event: "UserPromptSubmit",
        matcher: None,
        // the first-prompt workflows, the spec step among them (session_workflows.rs: 1740 s at
        // most)
        commands: &[cmd("user-prompt-submit", 1800)],
    },
    HookGroup {
        event: "PreToolUse",
        matcher: Some("Bash"),
        commands: &[GUARD],
    },
    HookGroup {
        event: "PreToolUse",
        matcher: Some(FILE_TOOLS),
        commands: &[cmd("pre-tool-use", 180)],
    },
    HookGroup {
        event: "PostToolUse",
        matcher: Some(EDITS_AND_BASH),
        commands: &[cmd("post-tool-use", 60), TYPECHECK],
    },
    HookGroup {
        event: "PostToolUseFailure",
        matcher: Some("Bash"),
        commands: &[cmd("post-tool-use-failure", 60), TYPECHECK],
    },
    HookGroup {
        event: "Stop",
        matcher: None,
        commands: &[cmd("stop", 600)],
    },
    HookGroup {
        event: "PermissionRequest",
        matcher: None,
        commands: &[cmd("permission-request", 180)],
    },
    HookGroup {
        event: "SessionEnd",
        matcher: None,
        commands: &[cmd("session-end", 10)],
    },
    HookGroup {
        event: "PreToolUse",
        matcher: Some("*"),
        commands: &[cmd("agents", 30)],
    },
    HookGroup {
        event: "PostToolUse",
        matcher: Some(AGENT_TOOL),
        commands: &[cmd("agents", 30)],
    },
    HookGroup {
        event: "SubagentStart",
        matcher: Some(FORGE_AGENTS),
        commands: &[cmd("agents", 30)],
    },
    HookGroup {
        event: "SubagentStop",
        matcher: Some(FORGE_AGENTS),
        // putting the repository back and starting the next step
        commands: &[cmd("agents", 600)],
    },
];

pub const USER_HOOKS: &[HookGroup] = &[
    HookGroup {
        event: "PreToolUse",
        matcher: Some("Bash"),
        commands: &[USER_GUARD],
    },
    HookGroup {
        event: "PreToolUse",
        matcher: Some(FILE_TOOLS),
        commands: &[cmd("user-pre-tool-use", 180)],
    },
    HookGroup {
        event: "PermissionRequest",
        matcher: None,
        commands: &[cmd("user-permission-request", 180)],
    },
    HookGroup {
        event: "Notification",
        matcher: Some("permission_prompt|idle_prompt"),
        commands: &[cmd("user-notification", 5)],
    },
    HookGroup {
        event: "PreToolUse",
        matcher: Some(REVIEW_TOOLS),
        commands: &[cmd("user-agents", 30)],
    },
    HookGroup {
        event: "PostToolUse",
        matcher: Some(AGENT_TOOL),
        commands: &[cmd("user-agents", 30)],
    },
    HookGroup {
        event: "SubagentStart",
        matcher: Some("forge-reviewer"),
        commands: &[cmd("user-agents", 30)],
    },
    HookGroup {
        event: "SubagentStop",
        matcher: Some("forge-reviewer"),
        commands: &[cmd("user-agents", 30)],
    },
];

pub fn groups(level: Level) -> &'static [HookGroup] {
    match level {
        Level::Project => PROJECT_HOOKS,
        Level::User => USER_HOOKS,
    }
}

/// The settings value of a matcher group.
pub fn group_value(g: &HookGroup) -> Value {
    let mut m = Map::new();
    if let Some(matcher) = g.matcher {
        m.insert("matcher".into(), json!(matcher));
    }
    let hooks: Vec<Value> = g
        .commands
        .iter()
        .map(|c| {
            let mut h = Map::new();
            h.insert("type".into(), json!("command"));
            h.insert(
                "command".into(),
                json!(forge_launcher::hook_wrapper(c.event)),
            );
            h.insert("timeout".into(), json!(c.timeout));
            if c.block_on_failure {
                h.insert("onFailure".into(), json!("block"));
            }
            if c.async_rewake {
                h.insert("asyncRewake".into(), json!(true));
            }
            Value::Object(h)
        })
        .collect();
    m.insert("hooks".into(), Value::Array(hooks));
    Value::Object(m)
}

/// The hook entries of a level, for `file`.
pub fn hook_entries(level: Level, file: &str) -> Vec<DesiredEntry> {
    groups(level)
        .iter()
        .map(|g| DesiredEntry {
            entry: SettingsEntry {
                file: file.to_string(),
                path: format!("hooks.{}", g.event),
                value: group_value(g),
            },
            append: true,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_entry() {
        let v = group_value(&PROJECT_HOOKS[2]);
        assert_eq!(v["matcher"], "Bash");
        assert_eq!(v["hooks"][0]["onFailure"], "block");
        assert_eq!(v["hooks"][0]["timeout"], 180);
        assert!(
            v["hooks"][0]["command"]
                .as_str()
                .unwrap()
                .contains("claude-forge hook pre-tool-use")
        );
        let pt = group_value(&PROJECT_HOOKS[4]);
        assert_eq!(pt["hooks"][1]["asyncRewake"], true);
    }

    #[test]
    fn forges_agents_have_entries_at_both_levels() {
        let at = |groups: &[HookGroup], event: &str, hook: &str| {
            groups
                .iter()
                .find(|g| g.event == event && g.commands.iter().any(|c| c.event == hook))
                .map(|g| g.matcher)
        };
        // the gate holds every tool of the main agent
        assert_eq!(at(PROJECT_HOOKS, "PreToolUse", "agents"), Some(Some("*")));
        assert_eq!(
            at(PROJECT_HOOKS, "PostToolUse", "agents"),
            Some(Some(AGENT_TOOL))
        );
        assert_eq!(
            at(PROJECT_HOOKS, "SubagentStart", "agents"),
            Some(Some(FORGE_AGENTS))
        );
        assert_eq!(
            at(PROJECT_HOOKS, "SubagentStop", "agents"),
            Some(Some(FORGE_AGENTS))
        );
        assert_eq!(
            at(USER_HOOKS, "SubagentStop", "user-agents"),
            Some(Some("forge-reviewer"))
        );
        assert_eq!(
            at(USER_HOOKS, "PreToolUse", "user-agents"),
            Some(Some(REVIEW_TOOLS))
        );
    }
}
