//! Documentation (`docs.sh`): immutable ADRs and docs that mention changed identifiers.

use std::collections::{BTreeSet, HashSet};
use std::process::{Command, Stdio};
use std::sync::LazyLock;

use forge_core::Report;

use crate::Run;
use crate::util::{has_word, js_word, occurrences, re};

/// Test files: their identifiers are not documented.
const TEST_PATH: &str = r"(^|/)(tests?|__tests__|e2e)/|(^|/)(test_[^/]*|[^/]*_test|conftest)\.py$|\.(test|spec)\.[^/]*$";

/// Documentation: Markdown, MDX and reStructuredText under a docs/ or doc/ directory, and
/// README files.
const DOC: &str = r"(^|/)(docs?/(.*/)?[^/]*\.(md|mdx|rst)|README[^/]*)$";

static STOP: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    "self cls this args kwargs props options opts config settings params param value values data name type kind keys item items result results error errors message path file files input output default index count size text body request response context event callback handler children model state status client server string number boolean object array true false none null undefined return async await const function class export import from init main test start stop update create delete call load save open close read write parse format info debug warn logger user node tree root parent child source target mode level headers method query schema field fields label title description content length list dict tuple bool float bytes json None True False Exception ValueError TypeError KeyError RuntimeError else elif while with pass yield raise assert except finally lambda global nonlocal case break continue Args Returns Raises Yields Example Examples Note Notes Attributes"
        .split(' ')
        .collect()
});

/// `-<text>` / `+<text>` for each line removed or added since the base (a file new or deleted
/// in the change counts fully).
fn diff_lines(run: &Run<'_>, path: &str) -> Vec<String> {
    let null = std::path::PathBuf::from("/dev/null");
    let old = run.base_path(path).unwrap_or_else(|| null.clone());
    let new = if run.exists(path) { run.path(path) } else { null };
    let out = Command::new("git")
        .args(["diff", "--no-index", "--no-ext-diff", "--no-textconv", "--no-color", "-U0", "--"])
        .arg(&old)
        .arg(&new)
        .current_dir(run.root())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    let Ok(out) = out else { return vec![] };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut h = false;
    let mut v = vec![];
    for l in text.lines() {
        if l.starts_with("@@") {
            h = true;
            continue;
        }
        if h && (l.starts_with('-') || l.starts_with('+')) {
            v.push(l.to_owned());
        }
    }
    v
}

fn ident(s: &str) -> String {
    let s = s.trim_start_matches(|c: char| !(c.is_ascii_alphabetic() || c == '_' || c == '$'));
    s.chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$')
        .collect()
}

/// The identifiers the change removed, renamed or changed the existing definition of, and the
/// ones of them that are parameters only.
struct ChangedIds {
    ids: BTreeSet<String>,
    params: HashSet<String>,
}

/// `forge_changed_ids`: Python def, class, annotated names and module-level assignments;
/// TypeScript and JavaScript exported declarations, optional properties, typed members and
/// methods with a modifier. A name counts when a definition line of it was removed and not
/// added back as it was; a parameter only when it is gone from the function. Private (_x),
/// short (< 4) and common names are dropped.
fn changed_ids(run: &Run<'_>) -> ChangedIds {
    let keep = |id: &str| id.len() >= 4 && !id.starts_with('_') && !STOP.contains(id);
    let mut minus: HashSet<(String, String)> = HashSet::new();
    let mut plus: HashSet<(String, String)> = HashSet::new();
    let mut pminus: HashSet<String> = HashSet::new();
    let mut pplus: HashSet<String> = HashSet::new();
    let files: Vec<String> = run.files().iter().cloned().chain(run.deleted()).collect();
    for f in files {
        if re(TEST_PATH).is_match(&f) {
            continue;
        }
        let py = if f.ends_with(".py") || f.ends_with(".pyi") {
            true
        } else if [".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs"]
            .iter()
            .any(|e| f.ends_with(e))
        {
            false
        } else {
            continue;
        };
        for dl in diff_lines(run, &f) {
            let minus_sign = dl.starts_with('-');
            let line = &dl[1..];
            let t = line.trim_start_matches([' ', '\t']).trim_end_matches([' ', '\t', '\r']);
            if t.is_empty() {
                continue;
            }
            let mut def = |id: String| {
                if keep(&id) {
                    let k = (id, t.to_owned());
                    if minus_sign {
                        minus.insert(k);
                    } else {
                        plus.insert(k);
                    }
                }
            };
            if py {
                if t.starts_with('#') {
                    continue;
                }
                if let Some(m) = re(r"^(?:async[ \t]+)?def[ \t]+").find(t) {
                    let s = &t[m.end()..];
                    def(ident(s));
                    if let Some(i) = s.find('(') {
                        for p in s[i + 1..].split(',') {
                            let p = p.trim_start_matches([' ', '\t', '*']);
                            if re(r"^[A-Za-z_]").is_match(p) {
                                let id = ident(p);
                                if keep(&id) {
                                    if minus_sign {
                                        pminus.insert(id);
                                    } else {
                                        pplus.insert(id);
                                    }
                                }
                            }
                        }
                    }
                } else if let Some(s) = t.strip_prefix("class") {
                    if s.starts_with([' ', '\t']) {
                        def(ident(s));
                    }
                } else if let Some(c) =
                    re(r"^[A-Za-z_][A-Za-z0-9_]*[ \t]*:[ \t]*([^ \t=].*)$").captures(t)
                {
                    let r = &c[1];
                    if r.contains('=')
                        || re(r"^[A-Za-z_][A-Za-z0-9_.]*(\[.*)?([ \t]*\|[ \t]*[^ \t]+)*[ \t]*,?$")
                            .is_match(r)
                    {
                        def(ident(t));
                    }
                } else if re(r"^[A-Za-z_][A-Za-z0-9_]*[ \t]*=[^=]").is_match(line) {
                    def(ident(line));
                }
            } else {
                if re(r"^(//|/\*|\*)").is_match(t) {
                    continue;
                }
                if re(r"^export[ \t]").is_match(t) {
                    let s = re(r"^export[ \t]+((default|declare|abstract|async)[ \t]+)*").replace(t, "");
                    if re(r"^(function\*?|class|const|let|var|type|interface|enum|namespace)[ \t]").is_match(&s) {
                        def(ident(&re(r"^[a-z]+\*?[ \t]+").replace(&s, "")));
                    }
                } else if re(r"^(readonly[ \t]+)?[A-Za-z_$][A-Za-z0-9_$]*\?[ \t]*:").is_match(t)
                    || re(r"^(readonly[ \t]+)?[A-Za-z_$][A-Za-z0-9_$]*[ \t]*:.*;$").is_match(t)
                {
                    def(ident(&re(r"^readonly[ \t]+").replace(t, "")));
                } else if re(r"^((public|static|async|get|set|override)[ \t]+)+[A-Za-z_$][A-Za-z0-9_$]*[ \t]*[(<]").is_match(t) {
                    def(ident(&re(r"^((public|static|async|get|set|override)[ \t]+)+").replace(t, "")));
                }
            }
        }
    }
    let mut ids: BTreeSet<String> = minus
        .iter()
        .filter(|k| !plus.contains(*k))
        .map(|(id, _)| id.clone())
        .collect();
    let params: HashSet<String> = pminus
        .iter()
        .filter(|p| !pplus.contains(*p) && !ids.contains(*p))
        .cloned()
        .collect();
    ids.extend(params.iter().cloned());
    ChangedIds { ids, params }
}

/// `id` joined to another name by a dot: module.id, Class.id, id.attr.
fn qualified(s: &str, id: &str) -> bool {
    occurrences(s, id, js_word).any(|i| {
        let before: Vec<char> = s[..i].chars().rev().take(2).collect();
        let after: Vec<char> = s[i + id.len()..].chars().take(2).collect();
        (before.first() == Some(&'.')
            && before.get(1).is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_'))
            || (after.first() == Some(&'.')
                && after.get(1).is_some_and(|c| c.is_ascii_alphabetic() || *c == '_'))
    })
}

/// A parameter name as code uses it: `name=`, `name(`, `.name`.
fn in_code(s: &str, id: &str) -> bool {
    occurrences(s, id, js_word).any(|i| {
        s[..i].ends_with('.')
            || s[i + id.len()..]
                .trim_start_matches([' ', '\t'])
                .starts_with(['=', '('])
    })
}

/// The parts of a line inside backticks (`spans`) or outside them (prose).
fn split_ticks(s: &str, inside: bool) -> String {
    s.split('`')
        .enumerate()
        .filter(|(i, _)| (i % 2 == 1) == inside)
        .map(|(_, p)| format!(" {p}"))
        .collect()
}

/// `forge_docs_mentions`: when no documentation file changed, the lines of the documentation
/// that mention an identifier whose definition the change removed or changed, where docs
/// reference code: an inline code span, a fenced code block, or a qualified name in the text;
/// never a word of the prose. A removed parameter counts in a fenced block only where code
/// uses it (`name=`, `name(`, `.name`): common words fill configuration and shell examples.
pub fn docs_mentions(run: &Run<'_>, _args: &[String]) -> Report {
    let mut r = Report::default();
    if run.changed().iter().any(|f| re(DOC).is_match(f)) {
        return r;
    }
    let ChangedIds { ids, params } = changed_ids(run);
    if ids.is_empty() {
        return r;
    }
    let mut hits = vec![];
    for f in run.ls_files(&["--cached", "--others", "--exclude-standard"]) {
        if !re(DOC).is_match(&f) {
            continue;
        }
        let Some(text) = run.read(&f) else { continue };
        if !ids.iter().any(|id| text.contains(id.as_str())) {
            continue;
        }
        let mut fence = false;
        for (i, l) in text.lines().enumerate() {
            if re(r"^[ \t]*(```|~~~)").is_match(l) {
                fence = !fence;
                continue;
            }
            let hit = ids.iter().find(|id| {
                if fence {
                    if params.contains(*id) {
                        in_code(l, id)
                    } else {
                        has_word(l, id, js_word)
                    }
                } else {
                    has_word(&split_ticks(l, true), id, js_word)
                        || qualified(&split_ticks(l, false), id)
                }
            });
            if let Some(id) = hit {
                hits.push(format!("{f}:{} {id}", i + 1));
            }
        }
    }
    if hits.is_empty() {
        return r;
    }
    let mut msg = String::from(
        "These docs mention identifiers you changed; check they still describe the behaviour:",
    );
    for h in hits.iter().take(5) {
        msg.push_str(&format!("\n  - {h}"));
    }
    if hits.len() > 5 {
        msg.push_str(&format!("\n  (and {} more)", hits.len() - 5));
    }
    r.findings.push(run.finding(msg));
    r
}

/// `forge_adr_accepted`: MADR front matter `status: accepted`, or a Status heading whose first
/// non-empty line starts with Accepted.
fn adr_accepted(text: &str) -> bool {
    let (mut fm, mut st) = (false, false);
    for (i, l) in text.lines().enumerate() {
        if i == 0 && re(r"^---[ \t]*$").is_match(l) {
            fm = true;
            continue;
        }
        if fm {
            if re(r"^---[ \t]*$").is_match(l) {
                fm = false;
            } else if re(r#"^status:[ \t]*["']?accepted["']?[ \t]*$"#).is_match(&l.to_lowercase()) {
                return true;
            }
            continue;
        }
        if re(r"^#+[ \t]+(?i:status)[ \t]*$").is_match(l) {
            st = true;
            continue;
        }
        if st && l.starts_with('#') {
            st = false;
        }
        if st {
            if let Some(w) = l.split_whitespace().next() {
                let w: String = w.to_lowercase().chars().filter(|c| c.is_ascii_lowercase()).collect();
                if w == "accepted" {
                    return true;
                }
                st = false;
            }
        }
    }
    false
}

/// `forge_adr_body`: the ADR without its status (the front matter status line, the content of
/// the Status section).
fn adr_body(text: &str) -> Vec<&str> {
    let (mut fm, mut st) = (false, false);
    let mut out = vec![];
    for (i, l) in text.lines().enumerate() {
        if i == 0 && re(r"^---[ \t]*$").is_match(l) {
            fm = true;
            out.push(l);
            continue;
        }
        if fm {
            if re(r"^---[ \t]*$").is_match(l) {
                fm = false;
            } else if l.to_lowercase().starts_with("status:") {
                continue;
            }
            out.push(l);
            continue;
        }
        if re(r"^#+[ \t]+(?i:status)[ \t]*$").is_match(l) {
            st = true;
            out.push(l);
            continue;
        }
        if st && l.starts_with('#') {
            st = false;
        }
        if st {
            continue;
        }
        out.push(l);
    }
    out
}

/// `forge_adr_edits`: the changed ADRs that were accepted in the base and whose change touches
/// more than their status.
pub fn adr_edits(run: &Run<'_>, _args: &[String]) -> Report {
    let mut r = Report::default();
    for f in run.files() {
        if ![".md", ".mdx", ".markdown", ".rst"].iter().any(|e| f.ends_with(e)) {
            continue;
        }
        let (Some(base), Some(now)) = (run.base(f), run.read(f)) else {
            continue;
        };
        if !adr_accepted(&base) || adr_body(&base) == adr_body(&now) {
            continue;
        }
        r.findings.push(run.at(
            f,
            None,
            "an accepted ADR was rewritten; an ADR is a record of a decision: only its status may change (superseded, deprecated), and the new decision goes in a new ADR that supersedes it",
        ));
    }
    r
}
