//! JSON with comments and trailing commas (`bun.lock`, `tsconfig.json`, Claude Code's settings),
//! parsed into a tree that keeps where each value is in the text, so a writer can change one
//! value and leave the rest of the file (comments, layout) as it was.

use serde_json::{Map, Value};

/// A value and the byte range `[start, end)` of its text.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Object {
        start: usize,
        end: usize,
        members: Vec<Member>,
    },
    Array {
        start: usize,
        end: usize,
        items: Vec<Node>,
    },
    Scalar {
        start: usize,
        end: usize,
        value: Value,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    pub key: String,
    /// Where the key's string starts (its opening quote).
    pub key_start: usize,
    pub value: Node,
}

impl Node {
    pub fn start(&self) -> usize {
        match self {
            Node::Object { start, .. } | Node::Array { start, .. } | Node::Scalar { start, .. } => {
                *start
            }
        }
    }

    pub fn end(&self) -> usize {
        match self {
            Node::Object { end, .. } | Node::Array { end, .. } | Node::Scalar { end, .. } => *end,
        }
    }

    pub fn to_value(&self) -> Value {
        match self {
            Node::Object { members, .. } => {
                let mut m = Map::new();
                for mem in members {
                    m.insert(mem.key.clone(), mem.value.to_value());
                }
                Value::Object(m)
            }
            Node::Array { items, .. } => Value::Array(items.iter().map(Node::to_value).collect()),
            Node::Scalar { value, .. } => value.clone(),
        }
    }

    /// The member `key` of an object.
    pub fn get(&self, key: &str) -> Option<&Node> {
        match self {
            Node::Object { members, .. } => members
                .iter()
                .rev()
                .find(|m| m.key == key)
                .map(|m| &m.value),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsoncError {
    pub offset: usize,
    pub message: &'static str,
}

impl std::fmt::Display for JsoncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at byte {}", self.message, self.offset)
    }
}

impl std::error::Error for JsoncError {}

/// Parses one JSONC value (the whole text, apart from whitespace and comments).
pub fn parse_tree(text: &str) -> Result<Node, JsoncError> {
    let mut p = P {
        b: text.as_bytes(),
        s: text,
        i: 0,
    };
    p.skip();
    let node = p.value(0)?;
    p.skip();
    if p.i != p.b.len() {
        return Err(p.err("unexpected text after the value"));
    }
    Ok(node)
}

/// Parses JSONC into a JSON value.
pub fn parse(text: &str) -> Result<Value, JsoncError> {
    parse_tree(text).map(|n| n.to_value())
}

struct P<'a> {
    b: &'a [u8],
    s: &'a str,
    i: usize,
}

impl P<'_> {
    fn err(&self, message: &'static str) -> JsoncError {
        JsoncError {
            offset: self.i,
            message,
        }
    }

    /// Skips whitespace and comments.
    fn skip(&mut self) {
        loop {
            while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
                self.i += 1;
            }
            if self.b[self.i..].starts_with(b"//") {
                while self.i < self.b.len() && self.b[self.i] != b'\n' {
                    self.i += 1;
                }
            } else if self.b[self.i..].starts_with(b"/*") {
                match self.s[self.i + 2..].find("*/") {
                    Some(k) => self.i += k + 4,
                    None => self.i = self.b.len(),
                }
            } else if self.b[self.i..].starts_with(b"\xEF\xBB\xBF") {
                self.i += 3;
            } else {
                return;
            }
        }
    }

    fn value(&mut self, depth: usize) -> Result<Node, JsoncError> {
        if depth > 256 {
            return Err(self.err("nested too deeply"));
        }
        let start = self.i;
        match self.b.get(self.i) {
            Some(b'{') => {
                self.i += 1;
                let mut members = Vec::new();
                loop {
                    self.skip();
                    match self.b.get(self.i) {
                        Some(b'}') => {
                            self.i += 1;
                            break;
                        }
                        Some(b'"') => {
                            let key_start = self.i;
                            let key = match self.string()? {
                                Value::String(s) => s,
                                _ => unreachable!(),
                            };
                            self.skip();
                            if self.b.get(self.i) != Some(&b':') {
                                return Err(self.err("expected ':'"));
                            }
                            self.i += 1;
                            self.skip();
                            let value = self.value(depth + 1)?;
                            members.push(Member {
                                key,
                                key_start,
                                value,
                            });
                            self.skip();
                            match self.b.get(self.i) {
                                Some(b',') => self.i += 1,
                                Some(b'}') => {}
                                _ => return Err(self.err("expected ',' or '}'")),
                            }
                        }
                        _ => return Err(self.err("expected a key or '}'")),
                    }
                }
                Ok(Node::Object {
                    start,
                    end: self.i,
                    members,
                })
            }
            Some(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                loop {
                    self.skip();
                    if self.b.get(self.i) == Some(&b']') {
                        self.i += 1;
                        break;
                    }
                    items.push(self.value(depth + 1)?);
                    self.skip();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {}
                        _ => return Err(self.err("expected ',' or ']'")),
                    }
                }
                Ok(Node::Array {
                    start,
                    end: self.i,
                    items,
                })
            }
            Some(b'"') => {
                let value = self.string()?;
                Ok(Node::Scalar {
                    start,
                    end: self.i,
                    value,
                })
            }
            Some(_) => {
                while self.i < self.b.len()
                    && !matches!(
                        self.b[self.i],
                        b',' | b']' | b'}' | b' ' | b'\t' | b'\n' | b'\r' | b'/'
                    )
                {
                    self.i += 1;
                }
                let tok = &self.s[start..self.i];
                let value: Value = serde_json::from_str(tok).map_err(|_| JsoncError {
                    offset: start,
                    message: "invalid value",
                })?;
                if value.is_object() || value.is_array() || value.is_string() {
                    return Err(JsoncError {
                        offset: start,
                        message: "invalid value",
                    });
                }
                Ok(Node::Scalar {
                    start,
                    end: self.i,
                    value,
                })
            }
            None => Err(self.err("unexpected end of text")),
        }
    }

    fn string(&mut self) -> Result<Value, JsoncError> {
        let start = self.i;
        self.i += 1;
        while self.i < self.b.len() {
            match self.b[self.i] {
                b'\\' => self.i += 2,
                b'"' => {
                    self.i += 1;
                    return serde_json::from_str(&self.s[start..self.i]).map_err(|_| JsoncError {
                        offset: start,
                        message: "invalid string",
                    });
                }
                b'\n' => break,
                _ => self.i += 1,
            }
        }
        Err(JsoncError {
            offset: start,
            message: "unterminated string",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn comments_and_trailing_commas() {
        let v =
            parse("// head\n{ \"a\": [1, 2,], /* x */ \"b\": {\"c\": \"d\\\"\",}, }\n").unwrap();
        assert_eq!(v, json!({"a": [1, 2], "b": {"c": "d\""}}));
        assert!(parse("{\"a\": }").is_err());
        assert!(parse("{} x").is_err());
    }

    #[test]
    fn spans() {
        let text = "{\"a\": [1, \"x\"]}";
        let t = parse_tree(text).unwrap();
        let a = t.get("a").unwrap();
        assert_eq!(&text[a.start()..a.end()], "[1, \"x\"]");
    }
}
