//! Kept for callers of `converted::rule`: the packs carry the structural guard form themselves
//! now, so a rule's guard targets are its pack's ([`GuardRule::from_pack`]).

use crate::set::GuardRule;

/// The rule's guard targets; None when it has none.
pub fn rule(pack_rule: &forge_packs::Rule) -> Option<GuardRule> {
    GuardRule::from_pack(pack_rule)
}
