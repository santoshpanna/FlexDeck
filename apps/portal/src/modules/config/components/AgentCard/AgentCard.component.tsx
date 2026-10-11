import { Card } from "../../../../common/components/Card/Card.component.tsx";
import type { Status } from "../../../../services/api/api.ts";

/** The agent's own report: where it is and what revision it holds. */
export function AgentCard({ status }: { status: Status | null }) {
  const rows: [string, string][] = status
    ? [
        ["ADDRESS", `${status.host}:${status.port}`],
        ["REVISION", status.rev === null ? "none yet" : String(status.rev).padStart(3, "0")],
        ["DEVICES", String(status.deviceCount)],
        ["FINGERPRINT", status.fingerprint],
      ]
    : [];
  return (
    <Card title="This desktop">
      {status ? (
        <dl className="m-0 grid grid-cols-[auto_minmax(0,1fr)] gap-x-5 gap-y-2 rounded-xl border border-line bg-well p-3 font-mono text-[12px] shadow-well">
          {rows.map(([label, value]) => (
            <div key={label} className="contents">
              <dt className="text-faint">{label}</dt>
              <dd className="m-0 break-all text-ink-2">{value}</dd>
            </div>
          ))}
        </dl>
      ) : (
        <p className="m-0 text-sm text-muted">Reading…</p>
      )}
    </Card>
  );
}
