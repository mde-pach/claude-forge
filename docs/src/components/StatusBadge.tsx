export type Status = "passed" | "failed" | "skipped" | "not run";

const ICON: Record<Status, string> = { passed: "✓", failed: "✗", skipped: "–", "not run": "○" };

/** A result pill. `detail` (e.g. "linux, darwin") is shown after the status. */
export default function StatusBadge({ status, detail }: { status: Status; detail?: string }) {
  return (
    <span className="forge-status" data-status={status} title={detail ? `${status} on ${detail}` : status}>
      <span aria-hidden="true">{ICON[status]}</span>
      {status}
      {detail ? <span style={{ fontWeight: 400 }}>· {detail}</span> : null}
    </span>
  );
}
