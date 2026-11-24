//! Gate 2 (§20.2): a capability is available on this machine when every pinned tool it needs
//! is installed in Forge's cache. An unavailable one is never offered here; `forge status` says
//! why. Never downloads: a couple of `stat` calls per tool.

use forge_packs::model::Capability;
use forge_tools::Provisioner;

/// Whether a capability can be offered here, and why not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    Available,
    /// Each unmet need, as `forge status` shows it.
    Unavailable(Vec<String>),
}

impl Availability {
    pub fn is_available(&self) -> bool {
        matches!(self, Availability::Available)
    }
}

/// Whether `cap`'s needs are met on this machine (`provisioner`: Forge's cache; `None` when the
/// cache cannot be found, so no pinned tool is installed).
pub fn available(cap: &Capability, provisioner: Option<&Provisioner>) -> Availability {
    let mut unmet = Vec::new();
    for tool in cap.tools() {
        match provisioner {
            Some(p) if p.catalog().get(tool).is_none() => {
                unmet.push(format!("{tool} is not a tool this Forge pins"));
            }
            Some(p) if p.resolve(tool).is_some() => {}
            _ => unmet.push(format!("{tool} is not installed on this machine")),
        }
    }
    if unmet.is_empty() {
        Availability::Available
    } else {
        Availability::Unavailable(unmet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap(tools: &[&str]) -> Capability {
        let yaml = serde_json::json!({
            "id": "acme/browser", "description": "d", "relevant": ["The task needs a page."],
            "deliver": "on-demand", "text": "t",
            "needs": {"tools": tools},
        });
        serde_json::from_value(yaml).unwrap()
    }

    // No needs: available everywhere. A tool not installed in the cache, or one Forge does not
    // pin: unavailable, saying which.
    #[test]
    fn needs_decide_availability() {
        let dir = tempfile::tempdir().unwrap();
        let home = forge_home::Home::new(dir.path().join("cache"), dir.path().join("state"));
        let p = Provisioner::new(home);
        assert!(available(&cap(&[]), Some(&p)).is_available());
        assert_eq!(
            available(&cap(&["ruff", "nothing-such"]), Some(&p)),
            Availability::Unavailable(vec![
                "ruff is not installed on this machine".into(),
                "nothing-such is not a tool this Forge pins".into(),
            ])
        );
        assert!(!available(&cap(&["ruff"]), None).is_available());
    }
}
