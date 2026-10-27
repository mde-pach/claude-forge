// What a rule or pack does at each step of a project's life. Each target kind describes
// its own events (src/targets/<kind>.ts, next to how the generator compiles it), so the
// site and the generator cannot drift apart; this module only re-exports them for the pages.
export { type Effect, type LifecycleEvent, packLifecycle, ruleLifecycle, STEPS, type StepId } from "../../../src/targets/index";
