//! The hook input Claude Code writes on stdin. Every field is optional: a field Claude Code
//! leaves out (or a newer one) never makes a hook fail.

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct HookInput {
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    pub hook_event_name: Option<String>,
    pub permission_mode: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input: Value,
    pub tool_use_id: Option<String>,
    /// UserPromptSubmit.
    pub prompt: Option<String>,
    /// Stop: Claude is already continuing because of a Stop hook.
    pub stop_hook_active: bool,
    /// Notification.
    pub message: Option<String>,
    /// SessionStart: startup, resume, clear, compact.
    pub source: Option<String>,
}

impl HookInput {
    /// The input, or an empty one when it is not JSON.
    pub fn parse(bytes: &[u8]) -> HookInput {
        serde_json::from_slice(bytes).unwrap_or_default()
    }

    pub fn tool(&self) -> &str {
        self.tool_name.as_deref().unwrap_or("")
    }

    /// A string field of `tool_input`.
    pub fn input_str(&self, key: &str) -> Option<&str> {
        self.tool_input.get(key).and_then(Value::as_str)
    }

    pub fn command(&self) -> Option<&str> {
        self.input_str("command")
    }

    /// The id safe as a file name (`[A-Za-z0-9_-]`, others replaced by `_`); "default" when absent.
    pub fn session_key(&self) -> String {
        safe_key(self.session_id.as_deref().unwrap_or("")).unwrap_or_else(|| "default".into())
    }

    /// The tool call's id, safe as a file name.
    pub fn tool_key(&self) -> Option<String> {
        safe_key(self.tool_use_id.as_deref()?)
    }
}

/// `s` with characters other than `[A-Za-z0-9_-]` replaced, and a leading `-`/`_` prefixed, so it
/// is one safe path component; None when empty.
pub fn safe_key(s: &str) -> Option<String> {
    if s.is_empty() {
        return None;
    }
    let mut out: String = s
        .chars()
        .take(120)
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if !out.starts_with(|c: char| c.is_ascii_alphanumeric()) {
        out.insert(0, 'k');
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lenient() {
        let i = HookInput::parse(br#"{"session_id":"a/b","tool_input":{"command":"ls"},"new":1}"#);
        assert_eq!(i.session_key(), "a_b");
        assert_eq!(i.command(), Some("ls"));
        assert_eq!(HookInput::parse(b"nope").session_key(), "default");
        assert_eq!(safe_key("-x").unwrap(), "k-x");
    }
}
