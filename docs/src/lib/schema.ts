// The lifecycle schema: Claude Code's own lifecycle (all hook events from
// https://code.claude.com/docs/en/hooks), with the pieces Forge attaches to it.
// The backbone never changes; a rule or pack only lights up the pieces it uses.
// Events a page does not involve are grouped into collapsible sections that open in place.
import type { Effect, LifecycleEvent } from "./lifecycle";

export type Piece =
  | "apply" | "context" | "session-start" | "turn-start" | "guard" | "permission" | "sandbox"
  | "post-edit" | "typecheck" | "stop" | "notify" | "commit" | "ci";

export const PIECES: Record<Piece, { label: string; at: string; infra?: true }> = {
  apply: { label: "forge apply", at: "Forge CLI" },
  "session-start": { label: "session-start.sh", at: "SessionStart", infra: true },
  context: { label: "forge-<pack>.md", at: "InstructionsLoaded" },
  "turn-start": { label: "turn-start.sh", at: "UserPromptSubmit", infra: true },
  guard: { label: "guard.sh", at: "PreToolUse" },
  permission: { label: "permission rules", at: "PermissionRequest" },
  sandbox: { label: "sandbox", at: "tool executes" },
  "post-edit": { label: "post-edit.sh", at: "PostToolUse" },
  typecheck: { label: "typecheck.sh (async)", at: "PostToolUse" },
  stop: { label: "stop.sh → check.sh", at: "Stop" },
  notify: { label: "notify.sh", at: "Notification", infra: true },
  commit: { label: "pre-commit → check.sh", at: "git commit" },
  ci: { label: "check.sh --all", at: "CI" },
};

/** The Forge piece an event of the docs lifecycle model happens in. */
export function pieceOf(e: LifecycleEvent): Piece {
  switch (e.step) {
    case "apply": return "apply";
    case "session": return "context";
    case "tool": return e.via ?? "guard";
    case "edit": return e.effect === "wakes" ? "typecheck" : "post-edit";
    default: return e.step;
  }
}

type Row =
  | { zone: string; core?: true }
  | { ev: string; sub: string; kind?: "native" | "forge" | "outside"; pieces?: Piece[]; core?: true }
  | { pair: [string, string][] }
  | { open: Lane; label?: string }
  | { close: Lane };

/** Return arrows, drawn as brackets on the part of the flow they repeat. */
export type Lane = "turn" | "retry" | "loop";
export const LANES: Record<Lane, { key: string; text: string }> = {
  loop: { key: "A", text: "next tool call" },
  retry: { key: "B", text: "Stop blocked: Claude keeps working" },
  turn: { key: "C", text: "next prompt" },
};

/** Claude Code's lifecycle in documented order. `core` rows are always shown. */
export const FLOW: Row[] = [
  { zone: "Forge CLI · run once, writes everything below" },
  { ev: "forge apply", sub: "writes hooks, rules, check.sh; audits configs", kind: "forge", pieces: ["apply"] },
  { zone: "Claude Code · per session", core: true },
  { ev: "Setup", sub: "optional · --init / --maintenance" },
  { ev: "SessionStart", sub: "startup · resume · clear · compact · fork", pieces: ["session-start"], core: true },
  { ev: "InstructionsLoaded", sub: "CLAUDE.md and .claude/rules loaded", pieces: ["context"] },
  { open: "turn", label: "per turn · each prompt" },
  { ev: "UserPromptSubmit", sub: "your prompt, before Claude sees it", pieces: ["turn-start"], core: true },
  { ev: "UserPromptExpansion", sub: "slash command expands to a prompt" },
  { open: "retry" },
  { open: "loop", label: "agentic loop · each tool call" },
  { ev: "PreToolUse", sub: "before the tool runs · can block", pieces: ["guard"], core: true },
  { ev: "PermissionRequest", sub: "a permission decision is needed", pieces: ["permission"] },
  { ev: "PermissionDenied", sub: "auto mode denied the call" },
  { ev: "tool executes", sub: "built in · Bash runs in the sandbox", kind: "native", pieces: ["sandbox"], core: true },
  { pair: [["Elicitation", "MCP asks you"], ["ElicitationResult", "you answered"]] },
  { pair: [["SubagentStart", "subagent spawned"], ["SubagentStop", "subagent done"]] },
  { pair: [["TaskCreated", "task created"], ["TaskCompleted", "task completed"]] },
  { ev: "PostToolUse", sub: "after the tool succeeds", pieces: ["post-edit", "typecheck"], core: true },
  { ev: "PostToolUseFailure", sub: "after the tool fails" },
  { ev: "PostToolBatch", sub: "parallel calls resolved" },
  { close: "loop" },
  { ev: "Stop", sub: "Claude finishes responding", pieces: ["stop"], core: true },
  { close: "retry" },
  { ev: "StopFailure", sub: "turn ended by an API error" },
  { ev: "TeammateIdle", sub: "agent-team teammate goes idle" },
  { close: "turn" },
  { pair: [["PreCompact", "before compaction"], ["PostCompact", "then SessionStart"]] },
  { ev: "SessionEnd", sub: "clear · resume · logout · exit", core: true },
  { zone: "Any time · outside the flow" },
  { ev: "Notification", sub: "Claude needs your attention", pieces: ["notify"] },
  { pair: [["MessageDisplay", "assistant text streams"], ["ConfigChange", "settings changed"]] },
  { pair: [["PreModelSwitch", "before a model switch"], ["PostModelSwitch", "after a model switch"]] },
  { pair: [["CwdChanged", "working dir changed"], ["DirectoryAdded", "directory added"]] },
  { pair: [["FileChanged", "watched file changed"], ["WorktreeCreate", "worktree created"]] },
  { pair: [["WorktreeRemove", "worktree removed"]] },
  { zone: "Outside Claude Code · git and CI" },
  { ev: "git commit", sub: "pre-commit hook", kind: "outside", pieces: ["commit"] },
  { ev: "CI", sub: "your pipeline", kind: "outside", pieces: ["ci"] },
];

/** Every hook event name in FLOW (the Forge CLI, built-in steps, git and CI excluded). */
export const HOOK_EVENTS: string[] = FLOW.flatMap((r) =>
  "ev" in r ? (r.kind ? [] : [r.ev]) : "pair" in r ? r.pair.map((p) => p[0]) : [],
);

const RANK: Effect[] = ["blocks", "asks", "wakes", "feedback", "fixes", "audits", "informs", "inactive"];
const SHORT: Record<Effect, string> = {
  blocks: "BLOCKS", asks: "ASKS", wakes: "WAKES", feedback: "TELLS", fixes: "FIXES", audits: "AUDITS", informs: "INFORMS", inactive: "SKIPPED",
};

export interface Lit {
  effect: Effect;
  /** How many rules (or the context) act in this piece. */
  count: number;
  /** Tooltip lines. */
  lines: string[];
}

/** Strongest effect and rule count per piece. */
export function litPieces(events: { event: LifecycleEvent; who: string }[]): Map<Piece, Lit> {
  const by = new Map<Piece, { event: LifecycleEvent; who: string }[]>();
  for (const x of events) {
    if (x.event.effect === "inactive") continue;
    const p = pieceOf(x.event);
    by.set(p, [...(by.get(p) ?? []), x]);
  }
  const out = new Map<Piece, Lit>();
  for (const [p, xs] of by) {
    const effect = RANK.find((r) => xs.some((x) => x.event.effect === r))!;
    out.set(p, {
      effect,
      count: new Set(xs.map((x) => x.who)).size,
      lines: xs.map((x) => `${x.who}: ${x.event.text.replace(/`/g, "")}`),
    });
  }
  return out;
}

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

export interface SchemaOptions {
  lit: Map<Piece, Lit>;
  /** Show unlit Forge pieces' events (the overview page) rather than only the lit ones. */
  showAllPieces?: boolean;
  /** Badge shows "BLOCKS 3" (pack pages) rather than "BLOCKS". */
  counts?: boolean;
}

/**
 * The schema as HTML. Events this page involves are shown; each run of other
 * events becomes a <details> section that opens in place. Lanes (return arrows)
 * wrap the rows they repeat, so they stretch with whatever is open.
 */
export function schemaHtml(o: SchemaOptions): { html: string; retry: boolean } {
  const isLit = (p?: Piece[]) => (p ?? []).some((x) => o.lit.has(x));
  const shown = (r: Row) => "ev" in r && (!!r.core || isLit(r.pieces) || (!!o.showAllPieces && !!r.pieces));
  const retry = o.lit.get("stop")?.effect === "blocks";

  const piece = (p: Piece) => {
    const l = o.lit.get(p);
    const cls = ["fs-piece", PIECES[p].infra ? "fs-infra" : "", l ? "fs-on" : ""].filter(Boolean).join(" ");
    const title = esc(`${PIECES[p].at} · ${PIECES[p].label}${l ? "\n" + l.lines.join("\n") : ""}`);
    const badge = l ? `<em>${o.counts && l.count > 1 ? `${SHORT[l.effect]} ${l.count}` : SHORT[l.effect]}</em>` : "";
    return `<span class="${cls}"${l ? ` data-effect="${l.effect}"` : ""} title="${title}"><code>${esc(PIECES[p].label)}</code>${badge}</span>`;
  };
  const row = (r: Row): string => {
    if ("pair" in r)
      return `<div class="fs-row fs-pair">${r.pair.map(([n, sub]) => `<div class="fs-ev fs-small"><b>${esc(n)}</b><span>${esc(sub)}</span></div>`).join("")}</div>`;
    if (!("ev" in r)) return "";
    const ps = (r.pieces ?? []).filter((x) => o.lit.has(x) || PIECES[x].infra || o.showAllPieces || !shown(r));
    return `<div class="fs-row"><div class="fs-ev fs-${r.kind ?? "hook"}"><b>${esc(r.ev)}</b><span>${esc(r.sub)}</span></div><div class="fs-pieces">${ps.map(piece).join("")}</div></div>`;
  };
  const names = (rows: Row[]) => rows.flatMap((r) => ("ev" in r ? [r.ev] : "pair" in r ? r.pair.map((p) => p[0]) : []));

  const out: string[] = [];
  let hidden: Row[] = [];
  const flush = () => {
    if (!hidden.length) return;
    const n = names(hidden);
    const list = n.length > 3 ? `${n.slice(0, 3).join(", ")}, …` : n.join(", ");
    out.push(`<details class="fs-more"><summary><span class="fs-open"><span class="fs-count">+${n.length}</span> ${esc(list)}</span><span class="fs-close">Hide ${n.length > 1 ? `these ${n.length} events` : "this event"}</span></summary>${hidden.map(row).join("")}</details>`);
    hidden = [];
  };
  for (const r of FLOW) {
    if ("zone" in r) { flush(); out.push(`<div class="fs-zone">${esc(r.zone)}</div>`); continue; }
    if ("open" in r) {
      flush();
      const lane = LANES[r.open];
      out.push(`<div class="fs-lane fs-lane-${r.open}${r.open === "retry" && retry ? " fs-lane-on" : ""}"><span class="fs-key" title="${esc(lane.text)}">${lane.key}</span>${r.label ? `<div class="fs-lane-t">${esc(r.label)}</div>` : ""}`);
      continue;
    }
    if ("close" in r) { flush(); out.push(`</div>`); continue; }
    if (shown(r)) { flush(); out.push(row(r)); } else hidden.push(r);
  }
  flush();
  return { html: `<div class="fs">${out.join("")}</div>`, retry };
}
