//! Reading a tool's output: which diagnostics name the change (`forge_diagnostics`), their
//! keys for comparing a typecheck with the base (`forge_diag_keys`), the excerpt of a failed run
//! (`forge_excerpt`), and the paths a workspace package's run prints (`forge_prefix_paths`).

use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

use forge_core::Finding;
use regex::Regex;

use crate::adapters::strip_ansi;
use crate::change::{Added, ChangeSet};

macro_rules! re {
    ($name:ident, $src:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($src).expect("regex"));
    };
}

re!(ABS_HDR, r"^/\S*$");
re!(STYLISH, r"^\s+[0-9]+:[0-9]+\s+(error|warning)\s");
re!(
    WARN_LOC,
    r"^\s*(-->\s*)?[^\s:(]+(\([0-9]+,[0-9]+\)|:[0-9]+)"
);
re!(WARN_WORD, r"(^|[^A-Za-z])[Ww]arning([^A-Za-z]|$)");
re!(ZERO, r"(^|[^0-9])0 (errors?|warnings?)([^A-Za-z]|$)");
re!(LEAD, r"^\s*(-->\s*)?");
re!(LOC, r"^[^\s:(]+(\([0-9]+,[0-9]+\)|:[0-9]+(:[0-9]+)?)");
re!(LOC_TAIL, r"(\([0-9]+,[0-9]+\)|:[0-9]+(:[0-9]+)?)$");
re!(REFORMAT, r"^Would reformat: (\S+)");
re!(BIOME_FMT, r"^([^\s:(]+) format ");
re!(PRETTIER_WARN, r"^\[warn\] (\S+)$");
re!(PRETTIER_ERR, r"^\[error\] (\S+): ");

/// How a failed run's diagnostics are attributed outside `--all`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// On the added lines (lint).
    Line,
    /// In the changed files (typecheck, a whole-project format run).
    File,
}

/// What the diagnostics of a failed run name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attribution {
    /// A diagnostic that counts.
    Changed,
    /// Only other files or lines.
    Other,
    /// Only Forge's own files.
    Forge,
    /// Warnings and no error.
    Warnings,
    /// No line names a file.
    None,
}

/// `forge_attribute_level`: lint by added line, typecheck and a whole-project format run by
/// changed file; anything else is not attributed.
pub fn level_of(stage: forge_packs::model::Stage, whole: bool) -> Option<Level> {
    use forge_packs::model::Stage;
    match stage {
        Stage::Lint => Some(Level::Line),
        Stage::Typecheck => Some(Level::File),
        Stage::Format if whole => Some(Level::File),
        _ => None,
    }
}

fn relative(p: &str, root: &Path) -> String {
    let mut p = p.to_string();
    for r in [
        root.to_path_buf(),
        std::fs::canonicalize(root).unwrap_or_default(),
    ] {
        let prefix = format!("{}/", r.display());
        if r.as_os_str().len() > 1
            && let Some(rest) = p.strip_prefix(&prefix)
        {
            p = rest.to_string();
            break;
        }
    }
    p.strip_prefix("./").map(str::to_string).unwrap_or(p)
}

/// The changed files and added lines a diagnostic is attributed to.
pub struct Target<'a> {
    pub changed: &'a [String],
    pub added: &'a std::collections::BTreeMap<String, Added>,
}

impl<'a> Target<'a> {
    pub fn of(c: &'a ChangeSet) -> Target<'a> {
        Target {
            changed: &c.changed,
            added: &c.added,
        }
    }

    fn is_changed(&self, p: &str) -> bool {
        self.changed.iter().any(|c| c == p)
    }

    fn whole(&self, p: &str) -> bool {
        matches!(self.added.get(p), Some(Added::Whole))
    }

    fn on_added(&self, p: &str, ln: u32) -> bool {
        self.added.get(p).is_some_and(|a| a.contains(ln))
    }
}

/// `forge_diagnostics`: what a failed run's diagnostics name, and the lines that count.
pub fn attribute(
    output: &str,
    root: &Path,
    target: &Target<'_>,
    level: Level,
    warn: bool,
) -> (Attribution, Vec<String>) {
    let mut hdr = String::new();
    let (mut warned, mut erred, mut named, mut project, mut hit) =
        (false, false, false, false, false);
    let mut lines = Vec::new();
    for raw in output.lines() {
        let line = strip_ansi(raw);
        if ABS_HDR.is_match(&line) {
            hdr = line.clone();
            continue;
        }
        let sty = !hdr.is_empty() && STYLISH.is_match(&line);
        if !sty && line.starts_with(|c: char| !c.is_whitespace()) {
            hdr.clear();
        }
        let has_warning = line.contains("Warning") || line.contains("warning");
        let has_error = line.contains("Error") || line.contains("error");
        if !warn && has_warning && !has_error {
            if WARN_LOC.is_match(&line) && WARN_WORD.is_match(&line) {
                warned = true;
            }
            continue;
        }
        if (has_error || (warn && has_warning)) && !ZERO.is_match(&line) {
            erred = true;
        }
        let l = LEAD.replace(&line, "").into_owned();
        let mut ln: Option<u32> = None;
        let p: String;
        if sty {
            p = hdr.clone();
            ln = l.split(':').next().and_then(|n| n.trim().parse().ok());
        } else if let Some(m) = LOC.find(&l) {
            let loc = m.as_str();
            let tail = LOC_TAIL.find(loc).map(|t| t.start()).unwrap_or(loc.len());
            p = loc[..tail].to_string();
            let nums = &loc[tail + 1..];
            ln = nums
                .split(|c: char| !c.is_ascii_digit())
                .find(|s| !s.is_empty())
                .and_then(|s| s.parse().ok());
        } else if let Some(c) = REFORMAT.captures(&l) {
            p = c[1].to_string();
        } else if let Some(c) = BIOME_FMT.captures(&l) {
            p = c[1].to_string();
        } else if let Some(c) = PRETTIER_WARN.captures(&l) {
            p = c[1].to_string();
            erred = true;
        } else if let Some(c) = PRETTIER_ERR.captures(&l) {
            p = c[1].to_string();
        } else {
            continue;
        }
        let p = relative(&p, root);
        named = true;
        if !p.starts_with(".forge/") && !p.starts_with(".claude/skills/forge-project/") {
            project = true;
        }
        let counts = match level {
            Level::Line => {
                target.whole(&p)
                    || match ln {
                        None | Some(0) => target.is_changed(&p),
                        Some(n) => target.on_added(&p, n),
                    }
            }
            Level::File => target.is_changed(&p),
        };
        if counts {
            hit = true;
            lines.push(if sty {
                format!("{p}:{}", l.trim_start())
            } else {
                line.clone()
            });
        }
    }
    let what = if hit {
        Attribution::Changed
    } else if named && !project {
        Attribution::Forge
    } else if named {
        Attribution::Other
    } else if warned && !erred {
        Attribution::Warnings
    } else {
        Attribution::None
    };
    (what, lines)
}

/// One diagnostic line as a key (`forge_diag_keys`): on an added line, its path, its text after
/// the location ("path:12:5: error[x] msg" is ": error[x] msg"), the line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagKey {
    pub on_added: bool,
    pub path: String,
    pub text: String,
    pub line: String,
}

pub fn diag_keys(output: &str, root: &Path, target: &Target<'_>) -> Vec<DiagKey> {
    let mut out = Vec::new();
    for raw in output.lines() {
        let line = strip_ansi(raw);
        let l = LEAD.replace(&line, "").into_owned();
        let Some(m) = LOC.find(&l) else { continue };
        let loc = m.as_str();
        let tail = LOC_TAIL.find(loc).map(|t| t.start()).unwrap_or(loc.len());
        let p = relative(&loc[..tail], root);
        let ln: u32 = loc[tail + 1..]
            .split(|c: char| !c.is_ascii_digit())
            .find(|s| !s.is_empty())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let text = l[m.end()..].replace('\t', " ");
        out.push(DiagKey {
            on_added: target.whole(&p) || target.on_added(&p, ln),
            path: p,
            text,
            line: raw.to_string(),
        });
    }
    out
}

/// `forge_typecheck_compare`'s count: of the errors in the changed files (`now`), only as many of
/// a text count as the run now reports more of it than the base did (`all_now` vs `base`, whole
/// project), those on added lines first. Returns the lines that count and how many others.
pub fn compare_with_base(
    now: &[DiagKey],
    all_now: &[DiagKey],
    base: &[DiagKey],
) -> (Vec<String>, usize) {
    let mut left: HashMap<&str, i64> = HashMap::new();
    for k in all_now {
        *left.entry(&k.text).or_default() += 1;
    }
    for k in base {
        *left.entry(&k.text).or_default() -= 1;
    }
    let mut keep = vec![false; now.len()];
    for pass in [true, false] {
        for (i, k) in now.iter().enumerate() {
            if k.on_added == pass
                && let Some(n) = left.get_mut(k.text.as_str())
                && *n > 0
            {
                *n -= 1;
                keep[i] = true;
            }
        }
    }
    let new: Vec<String> = now
        .iter()
        .zip(&keep)
        .filter(|(_, k)| **k)
        .map(|(d, _)| d.line.clone())
        .collect();
    let old = keep.iter().filter(|k| !**k).count();
    (new, old)
}

/// `forge_excerpt`: at most `n` lines of a failed run's output, the failure first.
pub fn excerpt(output: &str, n: usize) -> String {
    let lines: Vec<String> = output
        .lines()
        .map(|l| {
            let l = strip_ansi(l);
            if l.chars().count() > 300 {
                format!("{}...", l.chars().take(297).collect::<String>())
            } else {
                l
            }
        })
        .collect();
    if lines.len() <= n {
        return lines.join("\n");
    }
    re!(
        SUMMARY,
        r"^forge: timed out|^ *Test Files .*failed|^ *Tests .*failed|^(FAILED|ERROR) |^=+ .*([0-9]+ (failed|errors?)).* =+$"
    );
    re!(VFAIL, r"^ *FAIL  ");
    re!(BLK_SKIP, r"^ *[0-9]*\||^ *$");
    re!(BLK_KEEP, r"Error|Caused by|[Ee]xpected|Received|^ *[-+] ");
    re!(PY_HDR, r"^_{3,} .* _{3,}$");
    re!(PY_E, r"^E   ");
    re!(PY_LOC, r"^[^ :]+\.py:[0-9]+: [A-Za-z]*(Error|Exception)");
    re!(
        ANY_ERR,
        r"[Ee]rror|ERROR|FAIL|[Ff]ail(ed|ure)|[Pp]anic|Traceback|[Rr]efused"
    );
    re!(WARN, r"[Ww]arn");
    re!(ERR, r"[Ee]rror");
    let mut cls: Vec<Option<u8>> = vec![None; lines.len()];
    let (mut blk, mut nb, mut frame, mut e) = (false, 0, false, 0);
    let mut have = [false; 4];
    for (i, l) in lines.iter().enumerate() {
        if SUMMARY.is_match(l) {
            cls[i] = Some(1);
            have[1] = true;
            blk = false;
            continue;
        }
        if VFAIL.is_match(l) {
            cls[i] = Some(2);
            have[2] = true;
            blk = true;
            nb = 0;
            frame = false;
            continue;
        }
        if blk {
            if l.contains('⎯') {
                blk = false;
                continue;
            }
            if BLK_SKIP.is_match(l) {
                continue;
            }
            if nb < 5 && (nb == 0 || BLK_KEEP.is_match(l) || l.contains('→')) {
                cls[i] = Some(2);
                have[2] = true;
                nb += 1;
                continue;
            }
            if !frame && l.contains('❯') && !l.contains("node_modules") {
                cls[i] = Some(2);
                have[2] = true;
                frame = true;
            }
            continue;
        }
        if PY_HDR.is_match(l) {
            cls[i] = Some(2);
            have[2] = true;
            e = 0;
            continue;
        }
        if PY_E.is_match(l) && e < 4 {
            cls[i] = Some(2);
            have[2] = true;
            e += 1;
            continue;
        }
        if PY_LOC.is_match(l) {
            cls[i] = Some(2);
            have[2] = true;
        }
    }
    if !have[1] && !have[2] {
        for (i, l) in lines.iter().enumerate() {
            if ANY_ERR.is_match(l) && (ERR.is_match(l) || !WARN.is_match(l)) {
                cls[i] = Some(3);
            }
        }
    }
    let mut out = Vec::new();
    for c in 1..=3u8 {
        for (i, l) in lines.iter().enumerate() {
            if out.len() >= n - 1 {
                break;
            }
            if cls[i] == Some(c) {
                out.push(l.clone());
            }
        }
    }
    let shown_cls = out.len();
    let mut sep = 0;
    if shown_cls > 0 && shown_cls < n.saturating_sub(2) {
        out.push("--- output:".into());
        sep = 1;
    }
    let mut shown = shown_cls;
    for (i, l) in lines.iter().enumerate() {
        if shown + sep >= n - 1 {
            break;
        }
        if cls[i].is_none() {
            out.push(l.clone());
            shown += 1;
        }
    }
    if lines.len() > shown {
        out.push(format!("... ({} more lines)", lines.len() - shown));
    }
    out.join("\n")
}

/// `forge_prefix_paths`: the output of a run in workspace package `dir`, with the relative paths
/// that start a diagnostic or name a failing test made repository paths.
pub fn prefix_paths(output: &str, dir: &str) -> String {
    re!(TEST_LEAD, r"^\s*FAIL +(\|[^|]*\| )?|^(FAILED|ERROR) ");
    re!(TEST_PATH, r"^[^\s]+\.[A-Za-z]+( |::|$)");
    re!(DIAG_LEAD, r"^\s*(-->\s*)?(Would reformat: )?");
    re!(DIAG_PATH, r"^[^\s:(]+(\([0-9]+,[0-9]+\)|:[0-9]+| format )");
    let join = |rest: &str| -> String {
        let mut p = format!("{dir}/{rest}");
        // "a/../b" → "b"
        re!(UP, r"(^|/)[^/.][^/]*/\.\./");
        while UP.is_match(&p) {
            p = UP.replace(&p, "/").into_owned();
            p = p.trim_start_matches('/').to_string();
        }
        p
    };
    let mut out = Vec::new();
    for raw in output.lines() {
        let line = strip_ansi(raw);
        if let Some(m) = TEST_LEAD.find(&line) {
            let rest = &line[m.end()..];
            if !rest.starts_with('/') && TEST_PATH.is_match(rest) {
                out.push(format!("{}{}", &line[..m.end()], join(rest)));
                continue;
            }
        }
        let m = DIAG_LEAD.find(&line).map(|m| m.end()).unwrap_or(0);
        let (lead, rest) = line.split_at(m);
        if !rest.starts_with('/')
            && (DIAG_PATH.is_match(rest) || lead.ends_with("Would reformat: "))
            && !rest.is_empty()
        {
            out.push(format!("{lead}{}", join(rest)));
        } else {
            out.push(raw.to_string());
        }
    }
    let mut s = out.join("\n");
    if output.ends_with('\n') {
        s.push('\n');
    }
    s
}

/// A diagnostic line's location: `path:line[:col][:] rest`, `path(line,col): rest`.
pub fn parse_location(line: &str) -> Option<(String, Option<u32>, String)> {
    re!(COLON, r"^([^\s:(]+):([0-9]+)(?::[0-9]+)?:?\s*(.*)$");
    re!(PAREN, r"^([^\s:(]+)\(([0-9]+),[0-9]+\):?\s*(.*)$");
    let l = LEAD.replace(line, "");
    if let Some(c) = COLON.captures(&l).or_else(|| PAREN.captures(&l)) {
        let ln: u32 = c[2].parse().ok()?;
        return Some((c[1].to_string(), (ln > 0).then_some(ln), c[3].to_string()));
    }
    None
}

/// Whether a line is an error diagnostic: it names a location and its text says `error`.
pub fn is_error_diagnostic(line: &str) -> bool {
    let line = strip_ansi(line);
    parse_location(&line).is_some_and(|(_, _, text)| text.to_lowercase().contains("error"))
}

/// A run's output as findings of `rule`: a line naming a location is a finding there, a line
/// starting with whitespace continues the previous one, any other line is a finding of its own.
pub fn findings_of(output: &str, rule: &str, root: &Path) -> Vec<Finding> {
    let mut out: Vec<Finding> = Vec::new();
    for l in output.lines() {
        let l = strip_ansi(l);
        if l.trim().is_empty() {
            continue;
        }
        if l.starts_with(char::is_whitespace)
            && let Some(last) = out.last_mut()
        {
            last.message.push('\n');
            last.message.push_str(&l);
            continue;
        }
        match parse_location(&l) {
            Some((p, ln, rest)) if !rest.is_empty() => {
                out.push(Finding::error(rule, rest).at(relative(&p, root), ln));
            }
            _ => out.push(Finding::error(rule, l)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::collections::BTreeSet;

    fn target<'a>(changed: &'a [String], added: &'a BTreeMap<String, Added>) -> Target<'a> {
        Target { changed, added }
    }

    #[test]
    fn attributes_by_line_and_file() {
        let changed = vec!["a.py".to_string()];
        let added = BTreeMap::from([("a.py".to_string(), Added::Lines(BTreeSet::from([3])))]);
        let t = target(&changed, &added);
        let root = Path::new("/r");
        let out = "a.py:3:1: F401 unused\na.py:9:1: E501 long\nb.py:1:1: F401 x\n";
        let (w, l) = attribute(out, root, &t, Level::Line, false);
        assert_eq!(w, Attribution::Changed);
        assert_eq!(l, ["a.py:3:1: F401 unused"]);
        let (w, _) = attribute("a.py:9:1: E501 long\n", root, &t, Level::Line, false);
        assert_eq!(w, Attribution::Other);
        let (w, l) = attribute("/r/a.py(9,1): error TS1: x\n", root, &t, Level::File, false);
        assert_eq!((w, l.len()), (Attribution::Changed, 1));
        let (w, _) = attribute("a.py:3:1: warning: w\n", root, &t, Level::Line, false);
        assert_eq!(w, Attribution::Warnings);
        let (w, _) = attribute("config error\n", root, &t, Level::Line, false);
        assert_eq!(w, Attribution::None);
        // ESLint's stylish output
        let (w, l) = attribute(
            "/r/a.py\n  3:5  error  bad  rule\n",
            root,
            &t,
            Level::Line,
            false,
        );
        assert_eq!(w, Attribution::Changed);
        assert_eq!(l, ["a.py:3:5  error  bad  rule"]);
    }

    #[test]
    fn compares_with_base() {
        let k = |p: &str, t: &str, a: bool| DiagKey {
            on_added: a,
            path: p.into(),
            text: t.into(),
            line: format!("{p}{t}"),
        };
        let now = vec![
            k("a", ": e1", false),
            k("a", ": e1", true),
            k("a", ": e2", false),
        ];
        let all = now.clone();
        let base = vec![k("a", ": e1", false), k("a", ": e2", false)];
        let (new, old) = compare_with_base(&now, &all, &base);
        assert_eq!(new, ["a: e1"]);
        assert_eq!(old, 2);
    }

    #[test]
    fn excerpt_keeps_failures_first() {
        let mut out: Vec<String> = (0..40).map(|i| format!("line {i}")).collect();
        out.push("FAILED tests/test_a.py::test_x - assert 1 == 2".into());
        let e = excerpt(&out.join("\n"), 15);
        assert!(e.starts_with("FAILED tests/test_a.py::test_x"));
        assert!(e.ends_with("more lines)"));
        assert!(e.lines().count() <= 15);
    }

    #[test]
    fn locations_and_prefixes() {
        assert_eq!(
            parse_location("src/a.ts(3,4): error TS2322: x"),
            Some(("src/a.ts".into(), Some(3), "error TS2322: x".into()))
        );
        assert_eq!(
            parse_location("a.py:0 is a credential file"),
            Some(("a.py".into(), None, "is a credential file".into()))
        );
        assert_eq!(
            prefix_paths("src/a.ts:1:2 x\nFAIL  t.test.ts > y\n", "pkg"),
            "pkg/src/a.ts:1:2 x\nFAIL  pkg/t.test.ts > y\n"
        );
        assert_eq!(
            prefix_paths("../b/c.py:1: x\n", "pkg/a"),
            "pkg/b/c.py:1: x\n"
        );
    }
}
