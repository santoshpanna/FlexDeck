import { useCallback } from "react";

import { useApi } from "../../../../hooks/useApi/useApi.hook.ts";
import type { ArgumentChoices } from "../../../../services/api/api.ts";

export type LoadChoices = (plugin: string, action: string, args: Record<string, string>) => Promise<ArgumentChoices>;

/**
 * Ask this desktop's plugins what an action's arguments can be set to.
 *
 * A function rather than state: the form asks when the action or its other
 * arguments change, and owns what comes back, so two widgets edited one after
 * the other never see each other's answers.
 */
export function useArgumentChoices(desktopId: string | null): LoadChoices {
  const api = useApi();
  return useCallback(
    (plugin, action, args) =>
      desktopId ? api.argumentChoices(desktopId, plugin, action, args) : Promise.resolve({ arguments: {} }),
    [api, desktopId],
  );
}
