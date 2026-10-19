import { useResource, useStore, type UseResourceResult } from "@questorylabs/qhttp/react";
import { useCallback } from "react";

import { current } from "../../lib/resource.lib.ts";

export interface DesktopResourceValue<T> {
  resource: UseResourceResult<T>;
  reload: () => Promise<void>;
  ensure: () => Promise<void>;
}

export function useDesktopResource<T>(desktopId: string | null, name: string, load: () => Promise<T>): DesktopResourceValue<T> {
  const store = useStore();
  const resource = useResource<T>({ id: [desktopId, name], load, when: desktopId !== null });

  const reload = useCallback(async () => {
    if (desktopId === null) return;
    await current(store, [desktopId, name], store.reload([desktopId, name], load));
  }, [store, desktopId, name, load]);

  const ensure = useCallback(async () => {
    if (desktopId === null) return;
    await current(store, [desktopId, name], store.ensure([desktopId, name], load));
  }, [store, desktopId, name, load]);

  return { resource, reload, ensure };
}
