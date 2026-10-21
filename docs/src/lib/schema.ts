// The lifecycle schema: Claude Code's own lifecycle (all hook events from
// https://code.claude.com/docs/en/hooks), with the pieces Forge attaches to it.
// The backbone never changes; a rule or pack only lights up the pieces it uses.
// Steps a page does not need are collapsed into one-line stubs.
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
  | { zone: string; core?: true; optional?: true }
  | { ev: string; sub: string; kind?: "native" | "forge" | "outside"; pieces?: Piece[]; core?: true; anchor?: string }
  | { pair: [string, string][] }
  | { open: "turn" | "loop"; label: string }
  | { close: "turn" | "loop" };

/** Claude Code's lifecycle in documented order. `core` rows are always shown. */
export const FLOW: Row[] = [
  { zone: "Forge CLI · run once, writes everything below", optional: true },
  { ev: "forge apply", sub: "writes hooks, rules, check.sh; audits configs", kind: "forge", pieces: ["apply"] },
  { zone: "Claude Code · per session", core: true },
  { ev: "Setup", sub: "optional · --init / --maintenance" },
  { ev: "SessionStart", sub: "startup · resume · clear · compact · fork", pieces: ["session-start"], core: true },
  { ev: "InstructionsLoaded", sub: "CLAUDE.md and .claude/rules loaded", pieces: ["context"] },
  { open: "turn", label: "per turn · each prompt" },
  { ev: "UserPromptSubmit", sub: "your prompt, before Claude sees it", pieces: ["turn-start"], core: true, anchor: "prompt" },
  { ev: "UserPromptExpansion", sub: "slash command expands to a prompt" },
  { open: "loop", label: "agentic loop · each tool call" },
  { ev: "PreToolUse", sub: "before the tool runs · can block", pieces: ["guard"], core: true, anchor: "pre" },
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
  { ev: "Stop", sub: "Claude finishes responding", pieces: ["stop"], core: true, anchor: "stop" },
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
  { zone: "Outside Claude Code · git and CI", optional: true },
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

/** Geometry (SVG user units). */
const LANES = 46, EX = LANES + 8, EW = 206, GAP = 22, PX = EX + EW + GAP, PW = 196, W = PX + PW + 8;

export interface SchemaOptions {
  lit: Map<Piece, Lit>;
  /** Show every event (true) or collapse the ones without a lit piece (false). */
  full: boolean;
  /** Show unlit Forge pieces (the overview page) rather than only the lit ones. */
  showAllPieces?: boolean;
  /** Badge shows "BLOCKS 3" (pack pages) rather than "BLOCKS". */
  counts?: boolean;
  /** Unique prefix for marker ids. */
  uid: string;
}

/** The schema as an SVG string. */
export function schemaSvg(o: SchemaOptions): { svg: string; retry: boolean } {
  const isLit = (p?: Piece[]) => (p ?? []).some((x) => o.lit.has(x));
  const shownPieces = (p?: Piece[]) => (p ?? []).filter((x) => o.showAllPieces || o.lit.has(x) || PIECES[x].infra);
  const visible = (r: Row): boolean => {
    if (o.full) return true;
    if ("ev" in r) return !!r.core || isLit(r.pieces) || (!!o.showAllPieces && !!r.pieces);
    return false;
  };
  let y = 6;
  const body: string[] = [];
  const boxes: string[] = [];
  const open: Record<string, { y: number; label: string }> = {};
  const anchors: Record<string, number> = {};
  const lastRow: Record<string, number> = {};
  let hidden: string[] = [];
  let section: { label: string; y: number; rows: number } | undefined;
  const inBox = () => Object.keys(open);
  const flush = () => {
    if (!hidden.length) return;
    const names = hidden.length > 3 ? `${hidden.slice(0, 3).join(", ")} …` : hidden.join(", ");
    body.push(`<g class="stub"><text x="${EX + 6}" y="${y + 11}">+ ${hidden.length} more: ${esc(names)}</text></g>`);
    for (const b of inBox()) lastRow[b] = y + 7;
    y += 18;
    hidden = [];
  };
  const pill = (p: Piece, py: number) => {
    const l = o.lit.get(p);
    const cls = ["pill", PIECES[p].infra ? "infra" : "", l ? "on" : ""].join(" ").trim();
    const title = l ? `<title>${esc(`${PIECES[p].at} · ${PIECES[p].label}\n${l.lines.join("\n")}`)}</title>` : `<title>${esc(`${PIECES[p].at} · ${PIECES[p].label}`)}</title>`;
    let badge = "";
    if (l) {
      const text = o.counts && l.count > 1 ? `${SHORT[l.effect]} ${l.count}` : SHORT[l.effect];
      const bw = text.length * 5.6 + 12;
      badge = `<rect class="badge" x="${PX + PW - 3 - bw}" y="${py + 3}" width="${bw}" height="14" rx="7"/><text class="badge-t" x="${PX + PW - 3 - bw / 2}" y="${py + 13}" text-anchor="middle">${text}</text>`;
    }
    return `<g class="${cls}"${l ? ` data-effect="${l.effect}"` : ""}>${title}<line class="tie" x1="${EX + EW}" y1="${py + 10}" x2="${PX}" y2="${py + 10}"/><rect class="pill-r" x="${PX}" y="${py}" width="${PW}" height="20" rx="10"/><text class="pill-t" x="${PX + 10}" y="${py + 14}">${esc(PIECES[p].label)}</text>${badge}</g>`;
  };
  let skipZone = false;
  for (let i = 0; i < FLOW.length; i++) {
    const r = FLOW[i];
    if ("zone" in r) {
      flush();
      const next = FLOW.slice(i + 1);
      const end = next.findIndex((x) => "zone" in x);
      const rows = end < 0 ? next : next.slice(0, end);
      const any = rows.some((x) => ("ev" in x || "pair" in x) && ("pair" in x ? o.full : visible(x)));
      // a zone with nothing to show: Forge and git/CI zones disappear, Claude Code zones keep a one-line stub
      skipZone = !any && !!r.optional;
      if (skipZone) continue;
      body.push(`<text class="zone" x="${EX}" y="${y + 13}">${esc(r.zone.toUpperCase())}</text>`);
      y += 19;
      continue;
    }
    if (skipZone) continue;
    if ("open" in r) {
      flush();
      open[r.open] = { y, label: r.label };
      y += 17;
      continue;
    }
    if ("close" in r) {
      flush();
      const s = open[r.close];
      const inset = r.close === "turn" ? 6 : 3;
      boxes.push(`<rect class="box box-${r.close}" x="${EX - inset}" y="${s.y}" width="${W - EX + inset - 4 - (r.close === "loop" ? 3 : 0)}" height="${y - s.y + 1}" rx="9"/><text class="box-t" x="${EX + 2}" y="${s.y + 12}">${esc(s.label.toUpperCase())}</text>`);
      delete open[r.close];
      y += 7;
      continue;
    }
    if ("pair" in r) {
      if (!o.full) { hidden.push(...r.pair.map((p) => p[0])); continue; }
      const w = (EW - 6) / 2;
      r.pair.forEach(([name, sub], i) => {
        const x = EX + i * (w + 6);
        body.push(`<g class="ev ev-small"><rect x="${x}" y="${y}" width="${w}" height="30" rx="6"/><text class="ev-n" x="${x + 7}" y="${y + 13}">${esc(name)}</text><text class="ev-s" x="${x + 7}" y="${y + 24}">${esc(sub)}</text></g>`);
      });
      for (const b of inBox()) lastRow[b] = y + 15;
      y += 35;
      continue;
    }
    if (!visible(r)) { hidden.push(r.ev); continue; }
    flush();
    const ps = shownPieces(r.pieces);
    const h = Math.max(32, ps.length * 24 + 8);
    body.push(`<g class="ev ev-${r.kind ?? "hook"}"><rect x="${EX}" y="${y}" width="${EW}" height="${h}" rx="7"/><text class="ev-n" x="${EX + 9}" y="${y + 14}">${esc(r.ev)}</text><text class="ev-s" x="${EX + 9}" y="${y + 26}">${esc(r.sub)}</text></g>`);
    ps.forEach((p, i) => body.push(pill(p, y + 6 + i * 24)));
    if (r.anchor) anchors[r.anchor] = y + 16;
    for (const b of inBox()) lastRow[b] = y + 16;
    y += h + 5;
  }
  flush();
  const H = y + 4;
  // Return arrows, each in its own lane left of the events; markers are letters explained in the legend.
  const retry = o.lit.get("stop")?.effect === "blocks";
  const lane = (cls: string, x: number, from: number, to: number, letter: string, red = false) =>
    `<g class="lane ${cls}${red ? " on" : ""}"><path d="M${EX} ${from} H${x + 5} Q${x} ${from} ${x} ${from - 5} V${to + 5} Q${x} ${to} ${x + 5} ${to} H${EX - 1}" marker-end="url(#${o.uid}-${red ? "r" : "a"})"/><circle cx="${x}" cy="${(from + to) / 2}" r="6.5"/><text x="${x}" y="${(from + to) / 2 + 3.2}" text-anchor="middle">${letter}</text></g>`;
  const arrows = [
    lane("tool", 38, lastRow.loop ?? anchors.pre + 20, anchors.pre + 3, "A"),
    lane("retry", 24, anchors.stop, anchors.pre - 3, "B", retry),
    lane("prompt", 10, lastRow.turn ?? anchors.stop, anchors.prompt, "C"),
  ].join("");
  const svg = `<svg class="forge-schema-svg" viewBox="0 0 ${W} ${H}" role="img" aria-label="Claude Code lifecycle, with the Forge pieces this page uses highlighted">
<defs><marker id="${o.uid}-a" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse"><path class="m" d="M0,0 L10,5 L0,10 z"/></marker><marker id="${o.uid}-r" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse"><path class="m-r" d="M0,0 L10,5 L0,10 z"/></marker></defs>
${boxes.join("")}${body.join("")}${arrows}</svg>`;
  return { svg, retry };
}
