//! Forge's internals, which no agent-facing text may name: its files, commands, variables and state.
//! Pack lints use it on rule descriptions and contexts; tests use it on every rendered agent text.

use std::sync::LazyLock;

use regex::Regex;

/// The term is group 1 or 2 (`.forge` as a name of its own: not `x.forge`, not `.forgejo`).
static INTERNALS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?:^|[^\w.-])(\.forge)(?:$|[^\w-])",
        r"|(?-u:\b)(check\.sh(?-u:\b)|forge\.yaml(?-u:\b)|forge\.lock(?-u:\b)|forge (?:apply|diff|check|status|hook)(?-u:\b)|claude-forge(?-u:\b)|FORGE_[A-Z0-9_]+|forge-state(?-u:\b)|models\.tsv(?-u:\b)|accept:)",
    ))
    .expect("regex")
});

/// The one command of Forge's the agent reads and runs (§1.6, §20.6): `claude-forge use`.
pub const AGENT_COMMAND: &str = "claude-forge use";

/// Each internal term `text` names, with its byte offset, in order. `claude-forge use` (followed
/// by a space, a line's end or the text's end) is the agent's command, not an internal; nothing
/// else of Forge's is.
pub fn find(text: &str) -> Vec<(usize, &str)> {
    INTERNALS
        .captures_iter(text)
        .filter_map(|c| c.get(1).or_else(|| c.get(2)))
        .filter(|m| {
            let rest = &text[m.start()..];
            !(m.as_str() == "claude-forge"
                && rest.strip_prefix(AGENT_COMMAND).is_some_and(|after| {
                    after.is_empty() || after.starts_with([' ', '\n', '`', '.', ','])
                }))
        })
        .map(|m| (m.start(), m.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::find;

    #[test]
    fn names_internals_not_lookalikes() {
        assert_eq!(
            find("run sh .forge/check.sh --all")
                .iter()
                .map(|t| t.1)
                .collect::<Vec<_>>(),
            [".forge", "check.sh"]
        );
        assert!(find("a .forgejo dir, x.forge, forged").is_empty());
        assert_eq!(find("set FORGE_STOP_BUDGET")[0].1, "FORGE_STOP_BUDGET");
        // the agent's one command, and nothing else of Forge's command line
        assert!(find("To use it, run: claude-forge use web-browser").is_empty());
        assert!(find("run `claude-forge use`.").is_empty());
        assert_eq!(find("claude-forge user")[0].1, "claude-forge");
        assert_eq!(find("claude-forge apply")[0].1, "claude-forge");
        assert_eq!(find("claude-forge use x; claude-forge check")[0].0, 20);
    }
}
