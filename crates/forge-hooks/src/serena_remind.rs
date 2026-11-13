//! The reminder to use Serena's code tools (§17.8): Serena's own `remind` hook, in Forge's
//! PreToolUse. Claude Code loads Serena's tools with the session, but agents still reach for
//! searches and whole-file reads. After a run of them on code without a Serena call (3 searches,
//! 3 reads of code files, or 4 of both), the call is refused once, saying which Serena tools do
//! it better and that the call may be made again; then the agent is left alone for two minutes.
//! A Serena call resets the counts. Per session and per agent (the main agent and each subagent
//! count apart), only in a project where Forge set Serena up.

use std::path::Path;

use forge_core::serena;
use forge_guard::parse::Parsed;
use serde::{Deserialize, Serialize};

use crate::input::HookInput;
use crate::{Output, Project, pre_decision, state};

const SEARCHES: u32 = 3;
const READS: u32 = 3;
const MIXED: u32 = 4;
/// A gap longer than this between two calls of a kind starts the count again.
const GAP_SECS: f64 = 1000.0;
/// After a reminder, nothing is counted for this long.
const QUIET_SECS: f64 = 120.0;

pub const REASON: &str = "Several searches and reads of code in a row: use Serena's code tools to find and read code by symbol (get_symbols_overview, find_symbol with include_body, find_referencing_symbols). You may make this call again if you still need it.";

#[derive(Default, Serialize, Deserialize)]
struct Counts {
    searches: u32,
    reads: u32,
    mixed: u32,
    last: f64,
    reminded: f64,
}

/// What a call is, for the counts.
#[derive(Debug, PartialEq, Eq)]
enum Call {
    Serena,
    Search,
    Read,
    Other,
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// The extensions of the project's Serena languages (`.serena/project.yml`); empty when Forge did
/// not set Serena up here.
fn code_exts(root: &Path) -> Vec<&'static str> {
    let Ok(text) = std::fs::read_to_string(root.join(serena::PROJECT_FILE)) else {
        return Vec::new();
    };
    if !text.starts_with("# Written by the project's tooling") {
        return Vec::new();
    }
    serena::LANGUAGES
        .iter()
        .filter(|l| {
            text.lines()
                .any(|line| line.trim() == format!("- {}", l.id))
        })
        .flat_map(|l| l.exts.iter().copied())
        .collect()
}

fn is_code(path: &str, exts: &[&str]) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(stem, ext)| !stem.is_empty() && !stem.ends_with('/') && exts.contains(&ext))
}

fn classify(input: &HookInput, parsed: Option<&Parsed>, exts: &[&str]) -> Call {
    let tool = input.tool();
    if serena::tool_of(tool).is_some() {
        return Call::Serena;
    }
    match tool {
        "Grep" => Call::Search,
        "Read" => match input.input_str("file_path") {
            Some(p) if is_code(p, exts) => Call::Read,
            _ => Call::Other,
        },
        "Bash" => {
            let Some(parsed) = parsed else {
                return Call::Other;
            };
            let mut read = false;
            for c in &parsed.commands {
                for &i in &c.starts {
                    match c.name_at(i) {
                        "grep" | "rg" | "ugrep" | "ag" | "egrep" | "fgrep" => return Call::Search,
                        "git" if c.words.get(i + 1).is_some_and(|w| w.text == "grep") => {
                            return Call::Search;
                        }
                        "cat" | "head" | "tail" | "sed" | "less" | "more" | "nl" | "bat" => {
                            read |= c.words[i + 1..].iter().any(|w| is_code(&w.text, exts));
                        }
                        _ => {}
                    }
                }
            }
            if read { Call::Read } else { Call::Other }
        }
        _ => Call::Other,
    }
}

/// The reminder for this call, when it ends a run; None otherwise. Updates the counts.
pub fn check(p: &Project, input: &HookInput, parsed: Option<&Parsed>) -> Option<Output> {
    let exts = code_exts(&p.root);
    if exts.is_empty() {
        return None;
    }
    let call = classify(input, parsed, &exts);
    if call == Call::Other {
        return None;
    }
    let dir = p.session_dir.join("serena");
    let file = dir.join(format!(
        "{}.json",
        input.agent_key().unwrap_or_else(|| "main".into())
    ));
    let mut c: Counts = std::fs::read(&file)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let t = now();
    let save = |c: &Counts| {
        if state::ensure_dir(&dir) {
            if let Ok(b) = serde_json::to_vec(c) {
                state::write_atomic(&file, &b);
            }
        }
    };
    if call == Call::Serena {
        if c.searches + c.reads + c.mixed > 0 {
            c.searches = 0;
            c.reads = 0;
            c.mixed = 0;
            save(&c);
        }
        return None;
    }
    if t - c.reminded < QUIET_SECS {
        return None;
    }
    if t - c.last > GAP_SECS {
        c.searches = 0;
        c.reads = 0;
        c.mixed = 0;
    }
    c.last = t;
    match call {
        Call::Search => c.searches += 1,
        Call::Read => c.reads += 1,
        _ => {}
    }
    c.mixed += 1;
    let remind = c.searches >= SEARCHES || c.reads >= READS || c.mixed >= MIXED;
    if remind {
        c.searches = 0;
        c.reads = 0;
        c.mixed = 0;
        c.reminded = t;
    }
    save(&c);
    remind.then(|| pre_decision("deny", REASON))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_files_are_those_of_the_project_s_languages() {
        let exts = ["py"];
        assert!(is_code("src/app/main.py", &exts));
        assert!(is_code("main.py", &exts));
        assert!(!is_code("README.md", &exts));
        assert!(!is_code(".py", &exts));
    }
}
