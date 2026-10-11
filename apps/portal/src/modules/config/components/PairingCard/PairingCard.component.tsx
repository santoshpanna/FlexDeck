import { Digits, Key, Led } from "@flexdeck/ui";

import { Card } from "../../../../common/components/Card/Card.component.tsx";
import type { PairingBegin } from "../../../../services/api/api.ts";

export interface PairingCardProps {
  pairing: PairingBegin | null;
  onBegin: () => void;
  onCancel: () => void;
}

/**
 * Pairing a new tablet. The window is single-use, which is why the copy says
 * so: someone pairing two tablets from one window would otherwise watch the
 * second fail for no visible reason.
 */
export function PairingCard({ pairing, onBegin, onCancel }: PairingCardProps) {
  return (
    <Card title="Pair a tablet">
      {pairing ? (
        <div className="flex flex-col items-start gap-4">
          <Digits value={pairing.pin} />
          <p className="m-0 flex items-center gap-2 font-mono text-[11px] tracking-[0.12em] text-muted">
            <Led state="wait" /> TYPE IT ON THE TABLET · VALID UNTIL {new Date(pairing.expiresAt * 1000).toLocaleTimeString()}
          </p>
          <p className="m-0 text-[13px] text-muted">It works once: open a new code for each tablet.</p>
          <div className="flex gap-2">
            <Key size="sm" onClick={onBegin}>
              New code
            </Key>
            <Key size="sm" onClick={onCancel}>
              Cancel
            </Key>
          </div>
        </div>
      ) : (
        <div className="flex flex-col items-start gap-3">
          <p className="m-0 text-sm text-muted">Show a code here, then type it into FlexDeck on the tablet.</p>
          <Key tone="signal" onClick={onBegin}>
            Show a code
          </Key>
        </div>
      )}
    </Card>
  );
}
