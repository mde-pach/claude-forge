//! Globs as packs, rules and roles write them (`src/core/globs.ts`): `**` spans directories,
//! `*` and `?` stay within one segment, `[0-9]` / `[!x]` one character, `{a,b}` alternatives.

use fancy_regex::Regex;

/// The glob as an anchored regular expression on repository paths.
pub fn glob_to_regex(glob: &str) -> Regex {
    Regex::new(&format!("^{}$", glob_source(glob)))
        .unwrap_or_else(|_| Regex::new("$.^").expect("never-matching regex"))
}

pub fn glob_match(glob: &str, path: &str) -> bool {
    glob_to_regex(glob).is_match(path).unwrap_or(false)
}

fn glob_source(glob: &str) -> String {
    let g: Vec<char> = glob.chars().collect();
    let find = |c: char, from: usize| g.iter().skip(from).position(|&x| x == c).map(|p| p + from);
    let mut out = String::new();
    let mut i = 0;
    while i < g.len() {
        let c = g[i];
        if c == '*' {
            if g.get(i + 1) == Some(&'*') {
                i += 1;
                if g.get(i + 1) == Some(&'/') {
                    i += 1;
                    out.push_str("(?:.*/)?");
                } else {
                    out.push_str(".*");
                }
            } else {
                out.push_str("[^/]*");
            }
        } else if c == '?' {
            out.push_str("[^/]");
        } else if c == '[' && find(']', i + 2).is_some() {
            // a bracket expression: [0-9], [!._] (one character, never "/")
            let end = find(']', i + 2).unwrap_or(g.len());
            let mut body: String = g[i + 1..end].iter().collect();
            let neg = body.starts_with('!') || body.starts_with('^');
            if neg {
                body.remove(0);
            }
            let body: String = body
                .chars()
                .map(|ch| {
                    if matches!(ch, '\\' | ']' | '^' | '[' | '&' | '~') {
                        format!("\\{ch}")
                    } else {
                        ch.to_string()
                    }
                })
                .collect();
            out.push_str(&format!("(?!/)[{}{body}]", if neg { "^" } else { "" }));
            i = end;
        } else if c == '{' {
            // the matching brace, and the alternatives split at depth 0
            let mut alts = Vec::new();
            let mut depth = 0;
            let mut start = i + 1;
            let mut end = None;
            let mut k = i + 1;
            while k < g.len() && end.is_none() {
                match g[k] {
                    '{' => depth += 1,
                    '}' if depth > 0 => depth -= 1,
                    '}' => end = Some(k),
                    ',' if depth == 0 => {
                        alts.push(g[start..k].iter().collect::<String>());
                        start = k + 1;
                    }
                    _ => {}
                }
                k += 1;
            }
            match end {
                None => out.push_str("\\{"),
                Some(end) => {
                    alts.push(g[start..end].iter().collect());
                    let parts: Vec<String> = alts.iter().map(|a| glob_source(a)).collect();
                    out.push_str(&format!("(?:{})", parts.join("|")));
                    i = end;
                }
            }
        } else if c.is_alphanumeric() || c == '/' || c == '_' {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            out.push_str(&fancy_regex::escape(c.encode_utf8(&mut buf)));
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::glob_match;

    #[test]
    fn globs() {
        for (glob, path, want) in [
            ("**/*.md", "README.md", true),
            ("**/*.md", "a/b/c.md", true),
            ("*.md", "a/b.md", false),
            ("docs/**", "docs/a/b", true),
            ("docs/**/*.{md,mdx,rst}", "docs/x.mdx", true),
            ("README*", "README.md", true),
            ("**/migrations/[0-9]*.py", "app/migrations/0001_x.py", true),
            ("**/migrations/[0-9]*.py", "app/migrations/x.py", false),
            ("[!.]*", ".x", false),
            ("{,src/}a.ts", "src/a.ts", true),
            ("{,src/}a.ts", "a.ts", true),
            ("a+b(c).ts", "a+b(c).ts", true),
            ("x?.ts", "x/.ts", false),
        ] {
            assert_eq!(glob_match(glob, path), want, "{glob} ~ {path}");
        }
    }
}
