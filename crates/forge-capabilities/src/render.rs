//! What the agent reads (§20.5): the selected capabilities, in the UserPromptSubmit hook's
//! `additionalContext`. Every word is the capabilities' own.

/// A prompt's capabilities stay under this many characters (Claude Code cuts hook context at
/// 10,000).
pub const PROMPT_MAX: usize = 8000;

/// A capability to give, in the order given (pack-id order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Give<'a> {
    pub id: &'a str,
    pub text: &'a str,
}

/// The text for one prompt, and the capabilities that did not fit (given another time).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rendered {
    pub text: String,
    /// The ids given.
    pub given: Vec<String>,
    /// The ids that did not fit under [`PROMPT_MAX`].
    pub left_out: Vec<String>,
}

/// The injected capabilities' texts, separated by a blank line, while they fit.
pub fn render(inject: &[Give]) -> Rendered {
    let mut out = Rendered::default();
    for g in inject {
        let text = g.text.trim_end();
        let sep = if out.text.is_empty() { 0 } else { 2 };
        if out.text.chars().count() + sep + text.chars().count() > PROMPT_MAX {
            out.left_out.push(g.id.to_string());
            continue;
        }
        if sep > 0 {
            out.text.push_str("\n\n");
        }
        out.text.push_str(text);
        out.given.push(g.id.to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // The texts in order, a blank line between them; one that would pass the limit is left out.
    #[test]
    fn texts_in_order_under_the_limit() {
        let big = "b".repeat(PROMPT_MAX - 5);
        let r = render(&[
            Give {
                id: "a/one",
                text: "First.\n",
            },
            Give {
                id: "a/big",
                text: &big,
            },
            Give {
                id: "b/two",
                text: "Second.",
            },
        ]);
        assert_eq!(r.text, "First.\n\nSecond.");
        assert_eq!(r.given, ["a/one", "b/two"]);
        assert_eq!(r.left_out, ["a/big"]);
    }
}
