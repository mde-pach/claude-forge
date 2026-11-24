//! What `forge status` and `forge explain` say about capabilities (§20): each active one with
//! its delivery, whether it is available on this machine, and how often it was selected and
//! loaded; the ones whose `when` does not hold.

use forge_capabilities::log::{self, Counts};
use forge_capabilities::{Availability, available};
use forge_packs::model::{Capability, Origin};
use serde::Serialize;

use super::Ctx;

#[derive(Debug, Serialize)]
pub struct CapabilityStatus {
    pub id: String,
    pub deliver: &'static str,
    pub local: bool,
    pub available: bool,
    /// Why it is not available here.
    pub unavailable: Vec<String>,
    #[serde(flatten)]
    pub counts: Counts,
}

/// This machine's provisioner, which knows the installed pinned tools.
fn provisioner() -> Option<forge_tools::Provisioner> {
    forge_home::Home::from_env()
        .ok()
        .map(forge_tools::Provisioner::new)
}

/// The repository's log of decisions and loads (empty when there is none).
fn counts(ctx: &Ctx) -> std::collections::BTreeMap<String, Counts> {
    let (Ok(home), Ok(id)) = (
        forge_home::Home::from_env(),
        forge_home::RepoId::for_root(&ctx.base),
    ) else {
        return Default::default();
    };
    log::counts(&log::log_path(&home.repo_state_dir(&id)))
}

pub fn status_of(
    cap: &Capability,
    counts: Counts,
    p: Option<&forge_tools::Provisioner>,
) -> CapabilityStatus {
    let unavailable = match available(cap, p) {
        Availability::Available => Vec::new(),
        Availability::Unavailable(why) => why,
    };
    CapabilityStatus {
        id: cap.id.clone(),
        deliver: cap.deliver.as_str(),
        local: cap.origin == Origin::Local,
        available: unavailable.is_empty(),
        unavailable,
        counts,
    }
}

/// Every active capability's status, by id.
pub fn statuses(ctx: &Ctx) -> Vec<CapabilityStatus> {
    let p = provisioner();
    let counts = counts(ctx);
    ctx.plan
        .capabilities
        .iter()
        .map(|c| {
            status_of(
                c,
                counts.get(&c.id).copied().unwrap_or_default(),
                p.as_ref(),
            )
        })
        .collect()
}

/// One status line, after the id.
pub fn line(s: &CapabilityStatus) -> String {
    let here = if s.available {
        "available here".to_string()
    } else {
        format!("not available here: {}", s.unavailable.join("; "))
    };
    let local = if s.local { " (local)" } else { "" };
    format!(
        "{}{local}: {}, {here}; selected for {} of {} prompts, loaded {} times",
        s.id, s.deliver, s.counts.selected, s.counts.decided, s.counts.loaded
    )
}
