import { useCallback, useState } from "react";

import { useApi } from "../../../../hooks/useApi/useApi.hook.ts";
import type { Device, Status } from "../../../../services/api/api.ts";

export interface AgentFactsValue {
  status: Status | null;
  devices: Device[];
  reload: () => Promise<void>;
  revoke: (deviceId: string) => Promise<void>;
}

/**
 * What the agent reports about itself: its address and revision, and the
 * tablets paired to it.
 *
 * Both are fetched together because they are always shown together, and one
 * `Promise.all` cannot show a status from one moment beside a device list from
 * another.
 */
export function useAgentFacts(desktopId: string | null): AgentFactsValue {
  const api = useApi();
  const [status, setStatus] = useState<Status | null>(null);
  const [devices, setDevices] = useState<Device[]>([]);

  const reload = useCallback(async () => {
    if (!desktopId) {
      setStatus(null);
      setDevices([]);
      return;
    }

    const [nextStatus, nextDevices] = await Promise.all([
      api.status(desktopId),
      api.devices(desktopId),
    ]);
    setStatus(nextStatus);
    setDevices(nextDevices.devices);
  }, [api, desktopId]);

  const revoke = useCallback(
    async (deviceId: string) => {
      if (desktopId) {
        await api.revokeDevice(desktopId, deviceId);
      }
    },
    [api, desktopId],
  );

  return { status, devices, reload, revoke };
}
