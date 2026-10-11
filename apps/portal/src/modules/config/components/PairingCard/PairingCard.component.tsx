import { Digits, Endpoint, Key, Label, Led } from "@flexdeck/ui";

import { Card } from "../../../../common/components/Card/Card.component.tsx";
import type { PairingBegin } from "../../../../services/api/api.ts";

export interface PairingCardProps {
  pairing: PairingBegin | null;
  /** Where the tablet finds the desktop, from its status; `null` until it has answered. */
  host: string | null;
  port: number | null;
  onBegin: () => void;
  onCancel: () => void;
}

/**
 * Pairing a new tablet. The window is single-use, which is why the copy says
 * so: someone pairing two tablets from one window would otherwise watch the
 * second fail for no visible reason.
 */
export function PairingCard({ pairing, host, port, onBegin, onCancel }: PairingCardProps) {
  // The code's own copy is the authority while one is open: it is what the
  // tablet will be checked against.
  const endpoint = <Endpoint host={pairing?.qr.host ?? host ?? ""} port={pairing?.qr.port ?? port} />;
  return (
    <Card title="Pair a tablet">
      {pairing ? (
        <div className="flex flex-col items-start gap-4">
          <Label>1 · Enter on the tablet</Label>
          {endpoint}
          <Label>2 · Then type this code</Label>
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
          <p className="m-0 text-sm text-muted">
            Show a code here, then on the tablet open FlexDeck and enter the address, port and code it shows. The
            tablet must be on the same Wi-Fi as the desktop.
          </p>
          <Key tone="signal" onClick={onBegin}>
            Show a code
          </Key>
        </div>
      )}
    </Card>
  );
}
