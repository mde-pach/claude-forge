//! What the agent reads (§20.5): the selected capabilities, in the UserPromptSubmit hook's
//! `additionalContext`. Injected ones are their text; on-demand ones are one line each under a
//! heading, naming the command that prints the text. The heading and the line's "To use it, run:"
//! are the only words Forge writes itself.

use forge_packs::model::Capability;

/// A prompt's capabilities stay within this many characters (Claude Code cuts hook context at
/// 10,000).
pub const PROMPT_MAX: usize = 8000;
/// The heading of the on-demand lines.
pub const ON_DEMAND_HEADING: &str = "Available if the task turns out to need them:";
/// The command the agent runs to load an on-demand capability (§20.6).
pub const COMMAND: &str = "claude-forge use";

/// The name the agent uses for `cap` among `all` (the project's active capabilities): the id's
/// last segment when no other active capability has it, else the whole id.
pub fn display_name<'a>(cap: &'a Capability, all: &[Capability]) -> &'a str {
    let unique = all.iter().filter(|c| c.name() == cap.name()).count() <= 1;
    if unique { cap.name() } else { &cap.id }
}

/// The active capability `name` names: its whole id, or its last segment when that is unique.
pub fn resolve<'a>(name: &str, all: &'a [Capability]) -> Option<&'a Capability> {
    if let Some(c) = all.iter().find(|c| c.id == name) {
        return Some(c);
    }
    let mut by_name = all.iter().filter(|c| c.name() == name);
    match (by_name.next(), by_name.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    }
}

/// A capability to give, in the order given (pack-id order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Give<'a> {
    pub id: &'a str,
    /// The name the agent uses (on-demand line).
    pub name: &'a str,
    pub description: &'a str,
    pub text: &'a str,
}

/// The text for one prompt.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rendered {
    pub text: String,
    /// The ids given (as text or as a line).
    pub given: Vec<String>,
    /// Injected capabilities given as on-demand lines, their texts not fitting.
    pub as_lines: Vec<String>,
    /// Capabilities not given at all, not even their line fitting.
    pub left_out: Vec<String>,
}

fn line(g: &Give) -> String {
    format!(
        "- {}: {} To use it, run: {COMMAND} {}",
        g.name,
        g.description.trim(),
        g.name
    )
}

fn len(s: &str) -> usize {
    s.chars().count()
}

type Parts<'a> = Vec<(&'a str, String)>;

fn section(lines: &Parts) -> usize {
    if lines.is_empty() {
        0
    } else {
        len(ON_DEMAND_HEADING) + lines.iter().map(|(_, l)| 1 + len(l)).sum::<usize>()
    }
}

fn body(texts: &Parts) -> usize {
    texts.iter().map(|(_, t)| len(t)).sum::<usize>() + 2 * texts.len().saturating_sub(1)
}

fn total(texts: &Parts, lines: &Parts) -> usize {
    let (b, s) = (body(texts), section(lines));
    b + s + if b > 0 && s > 0 { 2 } else { 0 }
}

/// The prompt's text: the injected capabilities' texts, in order and separated by a blank line,
/// then the on-demand lines under their heading. An injected text that would take the whole over
/// [`PROMPT_MAX`] is given as an on-demand line instead; a line that does not fit either is left
/// out.
pub fn render(inject: &[Give], on_demand: &[Give]) -> Rendered {
    let mut out = Rendered::default();
    let mut texts: Parts = Vec::new();
    let mut lines: Parts = Vec::new();
    // the on-demand lines first: one that cannot fit, even alone with the others, is left out
    for g in on_demand {
        let mut with = lines.clone();
        with.push((g.id, line(g)));
        if section(&with) <= PROMPT_MAX {
            lines = with;
        } else {
            out.left_out.push(g.id.to_string());
        }
    }
    for g in inject {
        let mut with = texts.clone();
        with.push((g.id, g.text.trim_end().to_string()));
        if total(&with, &lines) <= PROMPT_MAX {
            texts = with;
        } else {
            out.as_lines.push(g.id.to_string());
            lines.push((g.id, line(g)));
        }
    }
    let mut dropped = Vec::new();
    while !lines.is_empty() && total(&texts, &lines) > PROMPT_MAX {
        let (id, _) = lines.pop().expect("not empty");
        out.as_lines.retain(|a| a != id);
        dropped.push(id.to_string());
    }
    dropped.reverse();
    out.left_out.extend(dropped);
    let mut parts: Vec<String> = texts.iter().map(|(_, t)| t.clone()).collect();
    if !lines.is_empty() {
        let mut s = ON_DEMAND_HEADING.to_string();
        for (_, l) in &lines {
            s.push('\n');
            s.push_str(l);
        }
        parts.push(s);
    }
    out.text = parts.join("\n\n");
    out.given = texts
        .iter()
        .map(|(id, _)| (*id).to_string())
        .chain(lines.iter().map(|(id, _)| (*id).to_string()))
        .collect();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap(id: &str) -> Capability {
        serde_json::from_value(serde_json::json!({
            "id": id, "description": "d", "relevant": ["s"], "deliver": "on-demand", "text": "t",
        }))
        .unwrap()
    }

    fn give<'a>(id: &'a str, name: &'a str, text: &'a str) -> Give<'a> {
        Give {
            id,
            name,
            description: "A persistent web browser, helpful to simulate a real user or do testing.",
            text,
        }
    }

    // The exact shape the agent reads: injected texts, a blank line, the heading, one line each.
    #[test]
    fn injected_texts_then_on_demand_lines() {
        let r = render(
            &[
                give("a/one", "one", "First.\n"),
                give("b/two", "two", "Second."),
            ],
            &[give("t/web-browser", "web-browser", "unused")],
        );
        assert_eq!(
            r.text,
            "First.\n\nSecond.\n\nAvailable if the task turns out to need them:\n- web-browser: A persistent web browser, helpful to simulate a real user or do testing. To use it, run: claude-forge use web-browser"
        );
        assert_eq!(r.given, ["a/one", "b/two", "t/web-browser"]);
        let only = render(&[], &[give("t/web-browser", "web-browser", "unused")]);
        assert!(
            only.text
                .starts_with("Available if the task turns out to need them:\n- web-browser: ")
        );
    }

    // An injected text that does not fit is given as a line, in pack-id order; the whole stays
    // within the limit; a line that cannot fit either is left out.
    #[test]
    fn overflow_turns_injected_texts_into_lines() {
        let big = "b".repeat(PROMPT_MAX - 3);
        let r = render(
            &[
                give("a/one", "one", "First."),
                give("a/big", "big", &big),
                give("b/two", "two", "Second."),
            ],
            &[],
        );
        assert_eq!(r.as_lines, ["a/big"]);
        assert!(r.text.starts_with(
            "First.\n\nSecond.\n\nAvailable if the task turns out to need them:\n- big: "
        ));
        assert!(r.text.chars().count() <= PROMPT_MAX);
        assert_eq!(r.given, ["a/one", "b/two", "a/big"]);
        let r = render(&[give("a/big", "big", &big)], &[give("a/x", &big, "t")]);
        assert_eq!(r.left_out, ["a/x"]);
        assert!(r.text.chars().count() <= PROMPT_MAX);
    }

    // A capability's name is its id's last segment unless another active one has it; both
    // resolve.
    #[test]
    fn names_and_resolution() {
        let all = [cap("a/web"), cap("b/web"), cap("a/db")];
        assert_eq!(display_name(&all[2], &all), "db");
        assert_eq!(display_name(&all[0], &all), "a/web");
        assert_eq!(resolve("db", &all).map(|c| c.id.as_str()), Some("a/db"));
        assert_eq!(resolve("a/db", &all).map(|c| c.id.as_str()), Some("a/db"));
        assert!(resolve("web", &all).is_none());
        assert_eq!(resolve("b/web", &all).map(|c| c.id.as_str()), Some("b/web"));
        assert!(resolve("nope", &all).is_none());
    }
}
