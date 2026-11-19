//! Dependabot's cooldown, as `forge apply` offers it (§3.5) where zizmor's `dependabot-cooldown`
//! audit is active (`github-actions/zizmor`): each update in `.github/dependabot.yml` waits
//! `default-days` before proposing a new version, as the package managers' cooldowns do.

use saphyr::{LoadableYamlNode, Yaml};

pub const FILE: &str = ".github/dependabot.yml";

/// The cooldown Forge adds, in days.
pub const DAYS: u32 = 7;

fn indent(l: &str) -> usize {
    l.len() - l.trim_start().len()
}

/// `dependabot.yml` with `cooldown: {default-days: 7}` added to each update that has none,
/// after its last line; `None`: every update has one, or a file whose shape this edit does not
/// understand (no block list under a top-level `updates:`, or a result that does not read back
/// with a cooldown on every update).
pub fn with_cooldown(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let head = lines.iter().position(|l| l.trim_end() == "updates:")?;
    // the updates: from the first item to the first line back at the top level
    let end = lines[head + 1..]
        .iter()
        .position(|l| {
            !l.trim().is_empty() && indent(l) == 0 && !l.starts_with('#') && !l.starts_with('-')
        })
        .map_or(lines.len(), |i| head + 1 + i);
    let first = (head + 1..end).find(|&i| lines[i].trim_start().starts_with("- "))?;
    let item = indent(lines[first]);
    let starts: Vec<usize> = (first..end)
        .filter(|&i| indent(lines[i]) == item && lines[i].trim_start().starts_with("- "))
        .collect();
    let key = item + 2;
    let mut inserts: Vec<(usize, String)> = Vec::new();
    for (n, &s) in starts.iter().enumerate() {
        let stop = starts.get(n + 1).copied().unwrap_or(end);
        let block = &lines[s..stop];
        let has = block.iter().enumerate().any(|(j, l)| {
            let k = if j == 0 {
                &l[item + 2..]
            } else {
                l.get(key..).unwrap_or("")
            };
            (j == 0 || indent(l) == key) && k.starts_with("cooldown:")
        });
        if has {
            continue;
        }
        let last = (s..stop).rev().find(|&i| !lines[i].trim().is_empty())?;
        let pad = " ".repeat(key);
        inserts.push((last, format!("{pad}cooldown:\n{pad}  default-days: {DAYS}")));
    }
    if inserts.is_empty() {
        return None;
    }
    let mut out = String::new();
    for (i, l) in lines.iter().enumerate() {
        out.push_str(l);
        out.push('\n');
        if let Some((_, t)) = inserts.iter().find(|(at, _)| *at == i) {
            out.push_str(t);
            out.push('\n');
        }
    }
    // it reads back with a cooldown on every update
    let docs = Yaml::load_from_str(&out).ok()?;
    let doc = docs.first()?;
    let updates = doc.as_mapping_get("updates")?.as_sequence()?;
    updates
        .iter()
        .all(|u| u.as_mapping_get("cooldown").is_some())
        .then_some(out)
}

#[cfg(test)]
mod tests {
    use super::with_cooldown;

    #[test]
    fn each_update_gets_a_cooldown() {
        let text = "version: 2\nupdates:\n  - package-ecosystem: \"npm\"\n    directory: \"/\"\n    schedule:\n      interval: \"weekly\"\n\n  - package-ecosystem: \"github-actions\"\n    directory: \"/\"\n    cooldown:\n      default-days: 3\n    schedule:\n      interval: \"weekly\"\n";
        assert_eq!(
            with_cooldown(text).unwrap(),
            "version: 2\nupdates:\n  - package-ecosystem: \"npm\"\n    directory: \"/\"\n    schedule:\n      interval: \"weekly\"\n    cooldown:\n      default-days: 7\n\n  - package-ecosystem: \"github-actions\"\n    directory: \"/\"\n    cooldown:\n      default-days: 3\n    schedule:\n      interval: \"weekly\"\n"
        );
        let flush = "version: 2\nupdates:\n- package-ecosystem: pip\n  directory: /\n  schedule: {interval: daily}\nregistries: {}\n";
        assert_eq!(
            with_cooldown(flush).unwrap(),
            "version: 2\nupdates:\n- package-ecosystem: pip\n  directory: /\n  schedule: {interval: daily}\n  cooldown:\n    default-days: 7\nregistries: {}\n"
        );
        assert!(with_cooldown("version: 2\nupdates: []\n").is_none());
        assert!(with_cooldown("version: 2\nupdates:\n  - package-ecosystem: pip\n    cooldown: {default-days: 5}\n").is_none());
    }
}
