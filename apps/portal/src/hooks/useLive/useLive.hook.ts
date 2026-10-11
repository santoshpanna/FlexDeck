import { useContext, useEffect, useRef } from "react";

import { ApiContext } from "../../providers/ApiProvider/ApiProvider.provider.tsx";
import type { LiveListener } from "../../services/api/live.ts";

/**
 * Hear what happens on a desktop as it happens. One socket per desktop is
 * shared by every listener on the page.
 *
 * A listener joining a socket already open does not get its starting state
 * again, so read it once yourself, and again on `open` — a reconnect — rather
 * than relying on the first frames.
 */
export function useLive(desktopId: string | null, listener: LiveListener): void {
  // Read without the throwing hook: a view drawn outside the providers hears nothing.
  const api = useContext(ApiContext);
  const latest = useRef(listener);
  latest.current = listener;

  useEffect(() => {
    if (!api || !desktopId) return;
    return api.live(desktopId, (message) => latest.current(message));
  }, [api, desktopId]);
}
