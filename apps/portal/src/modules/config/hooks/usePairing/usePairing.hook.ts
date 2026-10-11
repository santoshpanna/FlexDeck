import { useCallback, useState } from "react";

import { useApi } from "../../../../hooks/useApi/useApi.hook.ts";
import type { PairingBegin } from "../../../../services/api/api.ts";

export interface PairingValue {
  /** The open window, if one is showing. A window is single-use. */
  pairing: PairingBegin | null;
  begin: () => Promise<void>;
  /** Close the open window early, so its code stops working at once. */
  cancel: () => Promise<void>;
}

/** Opening a pairing window for a new tablet. */
export function usePairing(desktopId: string | null): PairingValue {
  const api = useApi();
  const [pairing, setPairing] = useState<PairingBegin | null>(null);

  const begin = useCallback(async () => {
    if (desktopId) {
      setPairing(await api.beginPairing(desktopId));
    }
  }, [api, desktopId]);

  const cancel = useCallback(async () => {
    if (desktopId) {
      await api.cancelPairing(desktopId);
    }
    setPairing(null);
  }, [api, desktopId]);

  return { pairing, begin, cancel };
}
