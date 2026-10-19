import { useCallback } from "react";

import { useApi } from "../../../../hooks/useApi/useApi.hook.ts";
import type { Device, Status } from "../../../../services/api/api.ts";
import { useDesktopResource } from "../useDesktopResource/useDesktopResource.hook.ts";

export interface AgentFactsValue {
  status: Status | null;
  devices: Device[];
  reload: () => Promise<void>;
  ensure: () => Promise<void>;
  revoke: (deviceId: string) => Promise<void>;
  revokeAll: () => Promise<void>;
}

interface Facts {
  status: Status;
  devices: Device[];
}

const NO_DEVICES: Device[] = [];

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

  const load = useCallback(async (): Promise<Facts> => {
    const [status, devices] = await Promise.all([
      api.status(desktopId ?? ""),
      api.devices(desktopId ?? ""),
    ]);
    return { status, devices: devices.devices };
  }, [api, desktopId]);

  const { resource, reload, ensure } = useDesktopResource<Facts>(desktopId, "facts", load);
  const facts = desktopId === null ? undefined : resource.value;

  const revoke = useCallback(
    async (deviceId: string) => {
      if (desktopId) {
        await api.revokeDevice(desktopId, deviceId);
      }
    },
    [api, desktopId],
  );

  const revokeAll = useCallback(async () => {
    if (desktopId) {
      await api.revokeAllDevices(desktopId);
    }
  }, [api, desktopId]);

  return { status: facts?.status ?? null, devices: facts?.devices ?? NO_DEVICES, reload, ensure, revoke, revokeAll };
}
