import { createContext, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";

import { useApi } from "../../hooks/useApi/useApi.hook.ts";
import { ApiError, type DesktopSummary } from "../../services/api/api.ts";
import { describeError } from "../../services/api/errors.ts";
import {
  forget,
  forgetVia,
  loadConnections,
  route,
  saveConnections,
  upsert,
  type Connection,
} from "../../services/connections/connections.lib.ts";

export interface DesktopValue {
  /** `null` until the first look has finished. `[]` is "none connected". */
  desktops: DesktopSummary[] | null;
  selected: DesktopSummary | null;
  select: (desktopId: string) => void;
  refresh: (askAgain?: boolean) => Promise<void>;
  /** Pair with the desktop showing `code`. Resolves to its name; throws on refusal. */
  pair: (code: string) => Promise<string>;
  /** Stop reaching a desktop from this browser. */
  forgetDesktop: (desktopId: string) => void;
  /** Why the last look failed, already phrased for a person. */
  error: string | null;
  waitingForDesktop: boolean;
  /**
   * Bumped each time this browser reconnects to a desktop that had refused it,
   * so whatever failed against the old token knows to read again.
   */
  reconnects: number;
}

export const DesktopContext = createContext<DesktopValue | null>(null);

/** Browser storage, or nothing where it is blocked. */
function storage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/** A name for this browser in a desktop's device list. */
function browserName(): string {
  const agent = navigator.userAgent;
  const browser = /Edg\//.test(agent)
    ? "Edge"
    : /Firefox\//.test(agent)
      ? "Firefox"
      : /Chrome\//.test(agent)
        ? "Chrome"
        : /Safari\//.test(agent)
          ? "Safari"
          : "Browser";
  const platform = /Android/.test(agent)
    ? "Android"
    : /iPhone|iPad/.test(agent)
      ? "iOS"
      : /Mac OS X/.test(agent)
        ? "Mac"
        : /Windows/.test(agent)
          ? "Windows"
          : /Linux/.test(agent)
            ? "Linux"
            : "";
  return platform ? `${browser} on ${platform}` : browser;
}

/**
 * The desktops this browser can reach, and which one is being configured.
 *
 * No sign-in. On load it looks for the desktop app on this computer and, if it
 * is there, connects to it with no questions — the user is sitting at it. Other
 * desktops are ones this browser was paired with by typing a code they showed.
 */
export function DesktopProvider({ children }: { children: ReactNode }) {
  const api = useApi();

  const [connections, setConnections] = useState<Connection[]>(() => loadConnections(storage()));
  const [localHere, setLocalHere] = useState<string | null>(null);
  const [online, setOnline] = useState<Record<string, boolean>>({});
  const [looked, setLooked] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reconnects, setReconnects] = useState(0);
  const [waitingForDesktop, setWaitingForDesktop] = useState(false);
  const localDeclined = useRef<string | null>(null);
  /** When each desktop last refused us, so a token that fails at once is not retried in a loop. */
  const lastRefusal = useRef(new Map<string, number>());

  const remember = useCallback((next: (current: Connection[]) => Connection[]) => {
    setConnections((current) => {
      const updated = next(current);
      saveConnections(storage(), updated);
      return updated;
    });
  }, []);

  const refresh = useCallback(async (askAgain = false) => {
    if (askAgain) localDeclined.current = null;
    try {
      // The desktop app on this computer, if any — connecting on first sight.
      const hello = await api.localHello();
      setLocalHere(hello?.fingerprint ?? null);
      let current = loadConnections(storage());
      if (hello && !current.some((c) => c.fingerprint === hello.fingerprint && c.via === "local")) {
        if (localDeclined.current !== null) throw new Error(localDeclined.current);
        setWaitingForDesktop(true);
        const issued = await api.localConnect(browserName()).catch((cause: unknown) => {
          if (cause instanceof ApiError && cause.status === 403) localDeclined.current = describeError(cause);
          throw cause;
        }).finally(() => setWaitingForDesktop(false));
        current = upsert(current, {
          fingerprint: issued.fingerprint,
          name: issued.name ?? hello.name,
          token: issued.token,
          via: "local",
        });
        remember(() => current);
      }

      // Which of the others the cloud can reach right now.
      const remote = [...new Set(current.filter((c) => c.via === "relay").map((c) => c.fingerprint))];
      const reachable: Record<string, boolean> = {};
      if (remote.length > 0) {
        const answer = await api.presence(remote);
        for (const desktop of answer.desktops) {
          reachable[desktop.fingerprint] = desktop.online;
        }
      }
      setOnline(reachable);
      setError(null);
    } catch (cause) {
      setError(describeError(cause));
    } finally {
      setLooked(true);
    }
  }, [api, remember]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // One row per desktop, reached the best available way.
  const desktops = useMemo<DesktopSummary[] | null>(() => {
    if (!looked) {
      return null;
    }
    const fingerprints = [...new Set(connections.map((connection) => connection.fingerprint))];
    return fingerprints.flatMap((fingerprint) => {
      const best = route(connections, fingerprint, localHere === fingerprint);
      if (!best) {
        return [];
      }
      return [
        {
          id: fingerprint,
          fingerprint,
          name: best.name,
          via: best.via,
          online: best.via === "local" ? true : (online[fingerprint] ?? false),
        },
      ];
    });
  }, [connections, localHere, online, looked]);

  // The API addresses desktops by fingerprint; tell it how to reach each.
  //
  // During render rather than in an effect: children's effects run BEFORE this
  // provider's, so a hook that loads the selected desktop's config would
  // otherwise call the API before it knew the route, and fail.
  useMemo(() => {
    const routes = new Map<string, Connection>();
    for (const desktop of desktops ?? []) {
      const best = route(connections, desktop.fingerprint, localHere === desktop.fingerprint);
      if (best) {
        routes.set(desktop.fingerprint, best);
      }
    }
    api.setRoutes(routes);
  }, [api, desktops, connections, localHere]);

  const selectedVia = desktops?.find((desktop) => desktop.id === selectedId)?.via;
  useEffect(() => {
    if (!selectedId || selectedVia !== "relay") return;
    return api.live(selectedId, (message) => {
      if (message.kind === "open" || message.kind === "offline") {
        setOnline((current) => ({ ...current, [selectedId]: message.kind === "open" }));
      }
    });
  }, [api, selectedId, selectedVia]);

  // Keep the current choice if it still exists; otherwise the first, preferring
  // this computer's own desktop.
  useEffect(() => {
    setSelectedId((current) =>
      current && desktops?.some((desktop) => desktop.id === current)
        ? current
        : (desktops?.find((desktop) => desktop.via === "local")?.id ?? desktops?.[0]?.id ?? null),
    );
  }, [desktops]);

  const pair = useCallback(
    async (code: string) => {
      const issued = await api.pair(code, browserName());
      const name = issued.name ?? "Desktop";
      remember((current) =>
        upsert(current, { fingerprint: issued.fingerprint, name, token: issued.token, via: "relay" }),
      );
      setSelectedId(issued.fingerprint);
      await refresh();
      return name;
    },
    [api, remember, refresh],
  );

  const forgetDesktop = useCallback(
    (desktopId: string) => remember((current) => forget(current, desktopId)),
    [remember],
  );

  // A local token the desktop app no longer honours (it was removed there) is
  // dropped and replaced, so the page connects afresh instead of failing
  // forever. Written to storage directly, because `refresh` reads it from there
  // and a state updater may not have run yet.
  useEffect(() => {
    api.setOnLocalRefused((desktopId) => {
      const now = Date.now();
      if (now - (lastRefusal.current.get(desktopId) ?? 0) < 10_000) {
        return;
      }
      lastRefusal.current.set(desktopId, now);

      const next = forgetVia(loadConnections(storage()), desktopId, "local");
      saveConnections(storage(), next);
      setConnections(next);
      void refresh().then(() => setReconnects((count) => count + 1));
    });
    return () => api.setOnLocalRefused(null);
  }, [api, refresh]);

  const selected = useMemo(
    () => desktops?.find((desktop) => desktop.id === selectedId) ?? null,
    [desktops, selectedId],
  );

  const value = useMemo<DesktopValue>(
    () => ({
      desktops,
      selected,
      select: (desktopId: string) => setSelectedId(desktopId),
      refresh,
      pair,
      forgetDesktop,
      error,
      waitingForDesktop,
      reconnects,
    }),
    [desktops, selected, refresh, pair, forgetDesktop, error, waitingForDesktop, reconnects],
  );

  return <DesktopContext.Provider value={value}>{children}</DesktopContext.Provider>;
}
