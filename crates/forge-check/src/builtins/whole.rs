//! Whole-project tools a builtin attributes to the change itself (§10.4): condor, and the
//! project's own knip, dependency-cruiser, import-linter and jscpd. Such a tool reports on the
//! whole project, so the builtin keeps what counts, tells the checker its findings are already
//! the change's ([`CheckCtx::mark_attributed`]), and gives the rest as advice at Stop.
//!
//! - A finding counts when it is in a file the change touched, old or new, or about one (a
//!   cycle's members, a duplicate's other copy, the modules of an import chain).
//! - Elsewhere, a finding counts when the change caused it: a second run on the tree as it was
//!   at the base, and as many of each finding count as there are more of them now (matched by
//!   their file and [`Located::key`], never by line, which edits above it move). What is left of
//!   the base in a file the change deleted or renamed is matched by key alone, so moved code
//!   does not bring its findings along.

use std::collections::BTreeMap;

use forge_core::builtin::BuiltinCtx;
use forge_core::{Finding, Report, Severity};

use super::{CheckCtx, RunMode, not_reported};

/// One finding of a whole-project tool, as attribution sees it.
pub trait Located {
    /// The file it is reported in.
    fn path(&self) -> &str;
    /// The other files it is about.
    fn about(&self) -> Vec<&str> {
        Vec::new()
    }
    /// What identifies it in two runs, besides its file: never a line number.
    fn key(&self) -> String;
    /// Its line (0: none), for the order of the findings.
    fn line(&self) -> u32;
}

/// Whether a finding is in or about a file `changed` says the change touched.
pub fn touches<T: Located>(f: &T, changed: &dyn Fn(&str) -> bool) -> bool {
    changed(f.path()) || f.about().into_iter().any(changed)
}

/// The findings of `now` that count (module docs): every one in or about a touched file, and of
/// the others those the base run (`base`) does not have. `gone`: the change deleted the file.
pub fn counted<T: Located + Clone>(
    now: &[T],
    base: &[T],
    changed: &dyn Fn(&str) -> bool,
    gone: &dyn Fn(&str) -> bool,
) -> Vec<T> {
    let mut base_n: BTreeMap<(String, String), usize> = BTreeMap::new();
    for b in base.iter().filter(|b| !touches(*b, changed)) {
        *base_n.entry((b.path().to_owned(), b.key())).or_default() += 1;
    }
    let mut groups: BTreeMap<(String, String), Vec<&T>> = BTreeMap::new();
    let mut out: Vec<T> = Vec::new();
    for h in now {
        if touches(h, changed) {
            out.push(h.clone());
        } else {
            groups
                .entry((h.path().to_owned(), h.key()))
                .or_default()
                .push(h);
        }
    }
    let mut elsewhere: Vec<&T> = Vec::new();
    for (key, mut hs) in groups {
        let had = base_n.get(&key).copied().unwrap_or(0);
        if hs.len() <= had {
            if let Some(n) = base_n.get_mut(&key) {
                *n -= hs.len();
            }
            continue;
        }
        base_n.remove(&key);
        hs.sort_by_key(|h| h.line());
        let n = hs.len() - had;
        elsewhere.extend(hs.into_iter().take(n));
    }
    // moved code: what is left of the base in files the change deleted or renamed
    let mut moved: BTreeMap<String, usize> = BTreeMap::new();
    for ((path, key), n) in &base_n {
        if *n > 0 && gone(path) {
            *moved.entry(key.clone()).or_default() += n;
        }
    }
    for h in elsewhere {
        if let Some(n) = moved.get_mut(&h.key()).filter(|n| **n > 0) {
            *n -= 1;
            continue;
        }
        out.push(h.clone());
    }
    out.sort_by(|a, b| (a.path(), a.line()).cmp(&(b.path(), b.line())));
    out
}

/// The report of findings already attributed to the change: the errors fail the check, taken as
/// they are; without one, the advice is given at Stop, once per turn (§10.7), and neither the
/// commit-time check nor CI reports it.
pub fn attributed_report(
    ctx: &BuiltinCtx,
    check: &CheckCtx,
    errors: Vec<Finding>,
    advice: Vec<Finding>,
) -> Report {
    let mut r = Report::default();
    if !errors.is_empty() {
        check.mark_attributed();
        r.findings = errors;
        return r;
    }
    let Some(snap) = check
        .snapshot
        .as_ref()
        .filter(|_| check.mode == RunMode::Turn)
    else {
        return r;
    };
    let advice: Vec<Finding> = advice
        .into_iter()
        .map(|mut f| {
            f.severity = Severity::Advice;
            f
        })
        .collect();
    let texts: Vec<String> = advice.iter().map(Finding::agent_text).collect();
    let fresh = not_reported(snap, &ctx.rule, &texts);
    r.findings = advice
        .into_iter()
        .filter(|f| fresh.contains(&f.agent_text()))
        .collect();
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct F(&'static str, u32, &'static str, Vec<&'static str>);

    impl Located for F {
        fn path(&self) -> &str {
            self.0
        }
        fn about(&self) -> Vec<&str> {
            self.3.clone()
        }
        fn key(&self) -> String {
            self.2.to_owned()
        }
        fn line(&self) -> u32 {
            self.1
        }
    }

    #[test]
    fn a_finding_about_a_touched_file_counts_wherever_it_is() {
        let now = [
            F("a.ts", 1, "cycle", vec!["b.ts"]),
            F("c.ts", 1, "cycle", vec!["d.ts"]),
        ];
        let changed = |p: &str| p == "b.ts";
        // the base has both: only the one about the touched file counts
        let got = counted(&now, &now, &changed, &|_| false);
        assert_eq!(got, vec![F("a.ts", 1, "cycle", vec!["b.ts"])]);
    }

    #[test]
    fn elsewhere_only_what_the_base_did_not_have() {
        let base = [F("x.ts", 3, "unused f", vec![])];
        let now = [
            F("x.ts", 9, "unused f", vec![]),
            F("x.ts", 12, "unused g", vec![]),
        ];
        let got = counted(&now, &base, &|_| false, &|_| false);
        assert_eq!(got, vec![F("x.ts", 12, "unused g", vec![])]);
    }
}
