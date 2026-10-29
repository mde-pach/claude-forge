//! Strict YAML (the format packs, rules and forge.yaml are written in), as `src/yaml.ts`
//! reads it: one document, YAML 1.2 core schema, no duplicate keys, anchors, aliases or
//! explicit tags. The document becomes a JSON value, as the JS `yaml` package's `toJS()`
//! makes it; the scalars are kept with their place in the source for the lints.

use saphyr_parser::{Event, Parser, ScalarStyle, Span};
use serde_json::{Map, Number, Value};

/// A parsed document and where its scalars are.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub value: Value,
    pub scalars: Vec<Scalar>,
}

/// A scalar used as a value (never a key), with its JSON pointer and source span.
#[derive(Debug, Clone, PartialEq)]
pub struct Scalar {
    /// JSON pointer of the value ("/description", "/enforce/0/run").
    pub pointer: String,
    /// Written plain (unquoted, not a block scalar).
    pub plain: bool,
    /// The scalar's text after YAML folding.
    pub value: String,
    /// First line (1-based).
    pub line: usize,
    /// Last line (1-based) and the column (0-based, in characters) just after the scalar.
    pub end_line: usize,
    pub end_col: usize,
}

/// Every problem found in a document, joined with "; " (the TS message's shape).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YamlError {
    pub problems: Vec<String>,
}

impl std::fmt::Display for YamlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.problems.join("; "))
    }
}

enum Frame {
    Seq {
        items: Vec<Value>,
        pointer: String,
    },
    Map {
        map: Map<String, Value>,
        /// The pending key: its JS property name and its typed value (for duplicate checks).
        key: Option<String>,
        typed_keys: Vec<Value>,
        pointer: String,
    },
}

/// Parses `text` as strict YAML.
pub fn parse_strict(text: &str) -> Result<Parsed, YamlError> {
    let mut problems = Vec::new();
    let mut scalars = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    let mut root: Option<Value> = None;
    let mut documents = 0usize;

    for item in Parser::new_from_str(text) {
        let (event, span) = match item {
            Ok(e) => e,
            Err(e) => {
                let m = e.marker();
                problems.push(format!(
                    "{} at line {}, column {}",
                    e.info(),
                    m.line(),
                    m.col() + 1
                ));
                return Err(YamlError { problems });
            }
        };
        match event {
            Event::StreamStart | Event::StreamEnd | Event::DocumentEnd | Event::Nothing => {}
            Event::DocumentStart(_) => {
                documents += 1;
                if documents > 1 {
                    problems.push(format!(
                        "Source contains multiple documents; please use YAML.parseAllDocuments() (line {})",
                        span.start.line()
                    ));
                }
            }
            Event::Alias(_) => {
                problems.push(format!(
                    "aliases are not allowed (line {})",
                    span.start.line()
                ));
                attach(&mut stack, &mut root, &mut problems, Value::Null);
            }
            Event::Scalar(value, style, anchor, tag) => {
                if anchor != 0 {
                    problems.push(format!(
                        "anchors are not allowed (line {})",
                        span.start.line()
                    ));
                }
                if let Some(tag) = tag {
                    problems.push(format!(
                        "tag {} is not allowed (line {})",
                        tag_name(&tag),
                        span.start.line()
                    ));
                }
                let typed = if style == ScalarStyle::Plain {
                    resolve_plain(&value)
                } else {
                    Value::String(value.to_string())
                };
                let at = attach(&mut stack, &mut root, &mut problems, typed);
                if let Some(pointer) = at {
                    scalars.push(Scalar {
                        pointer,
                        plain: style == ScalarStyle::Plain,
                        value: value.to_string(),
                        line: span.start.line(),
                        end_line: span.end.line(),
                        end_col: span.end.col(),
                    });
                }
            }
            Event::SequenceStart(anchor, ref tag) | Event::MappingStart(anchor, ref tag) => {
                if anchor != 0 {
                    problems.push(format!(
                        "anchors are not allowed (line {})",
                        span.start.line()
                    ));
                }
                if let Some(tag) = tag {
                    problems.push(format!(
                        "tag {} is not allowed (line {})",
                        tag_name(tag),
                        span.start.line()
                    ));
                }
                let pointer = child_pointer(&stack, &mut problems, span);
                let is_seq = matches!(event, Event::SequenceStart(..));
                stack.push(if is_seq {
                    Frame::Seq {
                        items: Vec::new(),
                        pointer,
                    }
                } else {
                    Frame::Map {
                        map: Map::new(),
                        key: None,
                        typed_keys: Vec::new(),
                        pointer,
                    }
                });
            }
            Event::SequenceEnd | Event::MappingEnd => {
                let done = match stack.pop() {
                    Some(Frame::Seq { items, .. }) => Value::Array(items),
                    Some(Frame::Map { map, .. }) => Value::Object(map),
                    None => Value::Null,
                };
                attach(&mut stack, &mut root, &mut problems, done);
            }
        }
    }
    if problems.is_empty() {
        Ok(Parsed {
            value: root.unwrap_or(Value::Null),
            scalars,
        })
    } else {
        Err(YamlError { problems })
    }
}

/// A tag as it was written: `!!str` for the YAML core tags, else handle and suffix.
fn tag_name(tag: &saphyr_parser::Tag) -> String {
    match tag.handle.as_str() {
        "tag:yaml.org,2002:" => format!("!!{}", tag.suffix),
        handle => format!("{handle}{}", tag.suffix),
    }
}

/// The pointer a node starting now gets; a collection used as a key is reported.
fn child_pointer(stack: &[Frame], problems: &mut Vec<String>, span: Span) -> String {
    match stack.last() {
        None => String::new(),
        Some(Frame::Seq { items, pointer }) => format!("{pointer}/{}", items.len()),
        Some(Frame::Map {
            key: Some(k),
            pointer,
            ..
        }) => format!("{pointer}/{}", escape_pointer(k)),
        Some(Frame::Map {
            key: None, pointer, ..
        }) => {
            problems.push(format!(
                "collection keys are not allowed (line {})",
                span.start.line()
            ));
            format!("{pointer}/?")
        }
    }
}

fn escape_pointer(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

/// Adds a finished node to its parent: an item, a key, or a key's value. Returns the
/// node's pointer, or None when it is a key.
fn attach(
    stack: &mut [Frame],
    root: &mut Option<Value>,
    problems: &mut Vec<String>,
    node: Value,
) -> Option<String> {
    match stack.last_mut() {
        None => {
            *root = Some(node);
            Some(String::new())
        }
        Some(Frame::Seq { items, pointer }) => {
            let at = format!("{pointer}/{}", items.len());
            items.push(node);
            Some(at)
        }
        Some(Frame::Map {
            map,
            key,
            typed_keys,
            pointer,
        }) => match key.take() {
            Some(k) => {
                let at = format!("{pointer}/{}", escape_pointer(&k));
                map.insert(k, node);
                Some(at)
            }
            None => {
                let name = match &node {
                    Value::Null => String::new(),
                    Value::String(s) => s.clone(),
                    Value::Bool(b) => b.to_string(),
                    Value::Number(n) => n.to_string(),
                    // reported when the collection started (child_pointer)
                    Value::Array(_) | Value::Object(_) => "?".to_string(),
                };
                // the yaml package compares scalar keys by value (uniqueKeys); two keys
                // that differ in type but name the same property are refused too
                if typed_keys.contains(&node) || map.contains_key(&name) {
                    problems.push(format!("Map keys must be unique (\"{name}\")"));
                }
                typed_keys.push(node);
                *key = Some(name);
                None
            }
        },
    }
}

/// A plain scalar's value under the YAML 1.2 core schema, as the `yaml` package resolves it
/// (`schema: "core"`): null, booleans, integers (decimal, 0o, 0x), floats, else a string.
pub fn resolve_plain(s: &str) -> Value {
    match s {
        "" | "~" | "null" | "Null" | "NULL" => return Value::Null,
        "true" | "True" | "TRUE" => return Value::Bool(true),
        "false" | "False" | "FALSE" => return Value::Bool(false),
        ".nan" | ".NaN" | ".NAN" => return Value::Null, // NaN: JSON has none (JSON.stringify gives null)
        _ => {}
    }
    let unsigned = s.strip_prefix(['-', '+']).unwrap_or(s);
    if matches!(unsigned, ".inf" | ".Inf" | ".INF") {
        return Value::Null; // ±Infinity: JSON.stringify gives null
    }
    if let Some(oct) = s.strip_prefix("0o")
        && !oct.is_empty()
        && oct.bytes().all(|b| (b'0'..=b'7').contains(&b))
    {
        return radix(oct, 8);
    }
    if let Some(hex) = s.strip_prefix("0x")
        && !hex.is_empty()
        && hex.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return radix(hex, 16);
    }
    if !unsigned.is_empty() && unsigned.bytes().all(|b| b.is_ascii_digit()) {
        return match s.parse::<i64>() {
            Ok(i) => js_number(i as f64, Some(i)),
            Err(_) => js_number(s.parse::<f64>().unwrap_or(f64::NAN), None),
        };
    }
    if is_core_float(unsigned) {
        let mut norm = s.to_string();
        if norm.ends_with('.') {
            norm.push('0');
        }
        let norm = norm.replace("-.", "-0.").replace("+.", "+0.");
        let norm = if norm.starts_with('.') {
            format!("0{norm}")
        } else {
            norm
        };
        let norm = norm.replace(".e", ".0e").replace(".E", ".0E");
        if let Ok(f) = norm.parse::<f64>() {
            return js_number(f, None);
        }
    }
    Value::String(s.to_string())
}

/// `[0-9]+.[0-9]*`, `.[0-9]+`, either with an exponent, or `[0-9]+` with an exponent (no sign).
fn is_core_float(s: &str) -> bool {
    let (mantissa, exp) = match s.find(['e', 'E']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    if let Some(e) = exp {
        let digits = e.strip_prefix(['-', '+']).unwrap_or(e);
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
    }
    let digits = |t: &str| t.bytes().all(|b| b.is_ascii_digit());
    match mantissa.split_once('.') {
        Some(("", frac)) => !frac.is_empty() && digits(frac),
        Some((int, frac)) => digits(int) && digits(frac),
        // an integer mantissa is a float only with an exponent
        None => exp.is_some() && !mantissa.is_empty() && digits(mantissa),
    }
}

fn radix(digits: &str, base: u32) -> Value {
    match u64::from_str_radix(digits, base) {
        Ok(u) => js_number(u as f64, i64::try_from(u).ok()),
        Err(_) => {
            let f = digits.chars().fold(0f64, |acc, c| {
                acc * f64::from(base) + f64::from(c.to_digit(base).unwrap_or(0))
            });
            js_number(f, None)
        }
    }
}

/// A JS number as JSON.stringify writes it: integral values within 2^53 as integers.
fn js_number(f: f64, exact: Option<i64>) -> Value {
    const SAFE: f64 = 9_007_199_254_740_992.0;
    if let Some(i) = exact
        && (i as f64).abs() <= SAFE
    {
        return Value::Number(i.into());
    }
    if !f.is_finite() {
        return Value::Null;
    }
    if f.fract() == 0.0 && f.abs() <= SAFE {
        return Value::Number((f as i64).into());
    }
    Number::from_f64(f).map_or(Value::Null, Value::Number)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn core_schema_scalars() {
        let cases = [
            ("~", json!(null)),
            ("", json!(null)),
            ("Null", json!(null)),
            ("nUll", json!("nUll")),
            ("True", json!(true)),
            ("yes", json!("yes")),
            ("on", json!("on")),
            ("tRue", json!("tRue")),
            ("12", json!(12)),
            ("+12", json!(12)),
            ("-12", json!(-12)),
            ("012", json!(12)),
            ("0o17", json!(15)),
            ("0x1F", json!(31)),
            ("0b11", json!("0b11")),
            ("1_000", json!("1_000")),
            ("1.5", json!(1.5)),
            ("1.", json!(1)),
            (".5", json!(0.5)),
            ("-.5", json!(-0.5)),
            ("1e3", json!(1000)),
            ("1.5E-1", json!(0.15)),
            (".inf", json!(null)),
            ("-.Inf", json!(null)),
            (".nan", json!(null)),
            ("1.2.3", json!("1.2.3")),
            ("12:30", json!("12:30")),
            ("2026-10-29", json!("2026-10-29")),
            (".", json!(".")),
            ("e3", json!("e3")),
        ];
        for (s, want) in cases {
            assert_eq!(resolve_plain(s), want, "{s:?}");
        }
    }

    #[test]
    fn quoted_scalars_are_strings() {
        let p = parse_strict("a: '12'\nb: \"true\"\nc: |\n  null\n").unwrap();
        assert_eq!(p.value, json!({"a": "12", "b": "true", "c": "null\n"}));
        assert!(p.scalars.iter().all(|s| !s.plain));
    }

    #[test]
    fn keys_are_property_names() {
        let p = parse_strict("1: a\ntrue: b\n~: c\n").unwrap();
        assert_eq!(p.value, json!({"1": "a", "true": "b", "": "c"}));
    }

    #[test]
    fn rejects_duplicates_anchors_aliases_tags() {
        for (text, want) in [
            ("a: 1\na: 2\n", "Map keys must be unique"),
            ("a: 1\n'a': 2\n", "Map keys must be unique"),
            ("a: &x 1\nb: 2\n", "anchors are not allowed"),
            ("a: &x [1]\nb: *x\n", "aliases are not allowed"),
            ("a: !!str 1\n", "tag !!str is not allowed"),
            ("a: !custom x\n", "is not allowed"),
            ("--- 1\n--- 2\n", "multiple documents"),
            ("a: [1\n", "line"),
        ] {
            let err = parse_strict(text).unwrap_err().to_string();
            assert!(err.contains(want), "{text:?}: {err}");
        }
    }

    #[test]
    fn plain_scalars_have_pointers_and_spans() {
        let p =
            parse_strict("description: cut here # a comment\nlist:\n  - one\n  - 'two'\n").unwrap();
        let ptrs: Vec<_> = p
            .scalars
            .iter()
            .filter(|s| s.plain)
            .map(|s| (s.pointer.as_str(), s.value.as_str(), s.end_line))
            .collect();
        assert_eq!(
            ptrs,
            [("/description", "cut here", 1), ("/list/0", "one", 3)]
        );
    }
}
