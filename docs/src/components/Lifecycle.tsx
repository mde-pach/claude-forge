import type { Effect, LifecycleEvent, StepId } from "../lib/lifecycle";

const STEPS: { id: StepId; label: string }[] = [
  { id: "apply", label: "forge apply" },
  { id: "session", label: "Session start" },
  { id: "tool", label: "Tool call" },
  { id: "edit", label: "After an edit" },
  { id: "stop", label: "Claude stops" },
  { id: "commit", label: "git commit" },
  { id: "ci", label: "CI" },
];

export const EFFECT_LABEL: Record<Effect, string> = {
  blocks: "blocks",
  asks: "asks",
  feedback: "tells Claude",
  wakes: "wakes Claude",
  fixes: "fixes",
  audits: "audits config",
  informs: "informs Claude",
  inactive: "not checked",
};

/** Strongest effect first, for the rail colour. */
const ORDER: Effect[] = ["blocks", "asks", "wakes", "feedback", "fixes", "audits", "informs", "inactive"];

function Inline({ text }: { text: string }) {
  return (
    <>
      {text.split(/(`[^`]+`)/).map((part, i) =>
        part.startsWith("`") && part.endsWith("`") && part.length > 1 ? <code key={i}>{part.slice(1, -1)}</code> : <span key={i}>{part}</span>,
      )}
    </>
  );
}

export function EffectPill({ effect }: { effect: Effect }) {
  return (
    <span className="forge-effect" data-effect={effect}>
      {EFFECT_LABEL[effect]}
    </span>
  );
}

/** What a rule does at each step: a rail of the seven steps, then one line per event. */
export default function Lifecycle({ events, title = "When it acts" }: { events: LifecycleEvent[]; title?: string }) {
  const byStep = new Map<StepId, LifecycleEvent[]>();
  for (const e of events) byStep.set(e.step, [...(byStep.get(e.step) ?? []), e]);
  const strongest = (step: StepId) => ORDER.find((o) => byStep.get(step)?.some((e) => e.effect === o));
  return (
    <div className="forge-lifecycle">
      <p className="forge-label">{title}</p>
      <ol className="forge-rail" aria-label="Lifecycle steps">
        {STEPS.map((s) => {
          const effect = strongest(s.id);
          return (
            <li key={s.id} data-effect={effect ?? "none"} title={effect ? EFFECT_LABEL[effect] : "not involved"}>
              {s.label}
            </li>
          );
        })}
      </ol>
      <dl className="forge-events">
        {STEPS.filter((s) => byStep.has(s.id)).map((s) => (
          <div key={s.id}>
            <dt>{s.label}</dt>
            <dd>
              {byStep.get(s.id)!.map((e, i) => (
                <p key={i}>
                  <EffectPill effect={e.effect} />
                  {e.condition ? <span className="forge-condition">{e.condition}</span> : null}
                  <Inline text={e.text} />
                </p>
              ))}
            </dd>
          </div>
        ))}
      </dl>
    </div>
  );
}
