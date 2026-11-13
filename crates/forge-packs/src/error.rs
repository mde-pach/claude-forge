/// What went wrong, for callers that branch on it (and for tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// A file could not be read (or is not UTF-8).
    Io,
    /// Not strict YAML: a syntax error, duplicate keys, anchors, aliases, tags, documents.
    Yaml,
    /// The file does not match its JSON Schema (or a pattern does not compile).
    Schema,
    /// The pack tree's layout: ids and paths, levels, reserved and stray directories,
    /// requires, local packs colliding with built-in ones.
    Layout,
    /// A rule's target-kind checks or file roles.
    Rule,
}

/// A file Forge cannot load, and why (`<file>: <message>`, as the TypeScript errors read).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{file}: {message}")]
pub struct LoadError {
    pub file: String,
    pub kind: ErrorKind,
    pub message: String,
}
