import { createContext, useMemo, type ReactNode } from "react";

import { Api } from "../../services/api/api.ts";

/**
 * The portal's one API client.
 *
 * Held in context rather than constructed per component so that every caller
 * shares one instance — and, more importantly, so the session cookie collected
 * from one response is the same cookie every later call sends.
 */
export const ApiContext = createContext<Api | null>(null);

export function ApiProvider({ children }: { children: ReactNode }) {
  // Once per provider rather than per render. Nothing breaks today if this is
  // re-created (the cookie lives in the browser, not in the client), but it
  // would defeat any request cache added later and hand every consumer a new
  // object on every render.
  const api = useMemo(() => new Api(), []);

  return <ApiContext.Provider value={api}>{children}</ApiContext.Provider>;
}
