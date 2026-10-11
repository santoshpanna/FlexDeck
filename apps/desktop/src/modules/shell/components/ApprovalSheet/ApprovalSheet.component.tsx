import { Key, Label, Led, Sheet } from "@flexdeck/ui";

import type { ApprovalRequest } from "../../lib/types.ts";

/**
 * A browser elsewhere asking to do something that runs new code here, or lets
 * another device in. It drops in over everything because it is time-boxed — an
 * unanswered request is refused after a few seconds — and it cannot be swiped
 * away: it is answered, or it times out.
 */
export function ApprovalSheet({
  approvals,
  busy,
  onAnswer,
}: {
  approvals: ApprovalRequest[];
  busy: boolean;
  onAnswer: (id: number, allow: boolean) => void;
}) {
  const request = approvals[0] ?? null;
  return (
    <Sheet open={request !== null} onClose={() => {}} dismissable={false} side="top" label="Approval needed">
      {request ? (
        <div className="flex flex-col gap-3" role="alert">
          <div className="flex items-center justify-between">
            <Label className="flex items-center gap-2 text-signal">
              <Led state="signal" /> Approval needed
            </Label>
            {approvals.length > 1 ? <Label>+{approvals.length - 1} more</Label> : null}
          </div>
          <p className="m-0 text-[15px] leading-snug">
            <strong className="font-semibold">{request.requester}</strong> wants to {request.action.charAt(0).toLowerCase() + request.action.slice(1)}.
          </p>
          {request.detail ? <p className="m-0 text-[13px] text-muted">{request.detail}</p> : null}
          <div className="flex gap-2.5">
            <Key size="lg" className="flex-1" disabled={busy} onClick={() => onAnswer(request.id, false)}>
              Deny
            </Key>
            <Key tone="signal" size="lg" className="flex-1" disabled={busy} onClick={() => onAnswer(request.id, true)}>
              Allow
            </Key>
          </div>
        </div>
      ) : null}
    </Sheet>
  );
}
