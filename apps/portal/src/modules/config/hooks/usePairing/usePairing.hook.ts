import { useCallback, useEffect, useRef, useState } from "react";

import { useApi } from "../../../../hooks/useApi/useApi.hook.ts";
import { useLive } from "../../../../hooks/useLive/useLive.hook.ts";
import type { Device, PairingBegin } from "../../../../services/api/api.ts";
import type { LiveMessage } from "../../../../services/api/live.ts";

export interface PairingValue {
  /** The open window, if one is showing. A window is single-use. */
  pairing: PairingBegin | null;
  begin: () => Promise<void>;
  /** Close the open window early, so its code stops working at once. */
  cancel: () => Promise<void>;
}

/**
 * Opening a pairing window for a new tablet.
 *
 * The agent says when its devices change (the live socket), and the list is
 * read again then; a device that was not there when the code opened means it
 * was used, and the window closes on its own. It also closes when the code
 * expires, rather than showing a dead code.
 */
export function usePairing(
  desktopId: string | null,
  devices: Device[],
  reloadDevices: () => Promise<void>,
): PairingValue {
  const api = useApi();
  const [pairing, setPairing] = useState<PairingBegin | null>(null);
  // The devices paired before this code opened: anything else is the new one.
  const known = useRef<Set<string>>(new Set());

  const begin = useCallback(async () => {
    if (desktopId) {
      const opened = await api.beginPairing(desktopId);
      known.current = new Set(devices.map((device) => device.id));
      setPairing(opened);
    }
  }, [api, desktopId, devices]);

  const cancel = useCallback(async () => {
    if (desktopId) {
      await api.cancelPairing(desktopId);
    }
    setPairing(null);
  }, [api, desktopId]);

  useEffect(() => {
    if (!pairing) return;
    const expire = setTimeout(() => setPairing(null), Math.max(0, pairing.expiresAt * 1000 - Date.now()));
    return () => clearTimeout(expire);
  }, [pairing]);

  // Paired, revoked, connected, gone: the list everyone sees follows at once,
  // code or no code — which is also what closes the code once it is used.
  const hear = useCallback(
    (message: LiveMessage) => {
      if (message.kind === "devices" || message.kind === "open") reloadDevices().catch(() => {});
    },
    [reloadDevices],
  );
  useLive(desktopId, hear);

  useEffect(() => {
    if (pairing && devices.some((device) => !known.current.has(device.id))) {
      setPairing(null);
    }
  }, [pairing, devices]);

  // A code belongs to one desktop: switching away drops it from view.
  useEffect(() => setPairing(null), [desktopId]);

  return { pairing, begin, cancel };
}
