import { useCallback, useEffect, useRef, useState } from "react";

import { useApi } from "../../../../hooks/useApi/useApi.hook.ts";
import { ApiError, type Automation, type AutomationDocument, type Run } from "../../../../services/api/api.ts";

/**
 * What the document read is doing.
 *
 * Empty and failed are distinct on purpose: a desktop with no automations yet
 * is a valid state to author from, while a read that failed is a page that
 * should not let the user edit a copy of last week's document and call it
 * authoritative.
 */
export type AutomationsStatus = "idle" | "loading" | "loaded" | "empty" | "failed";

export interface AutomationsValue {
  /// `null` when the document is not loaded (status gives the reason). The
  /// stub schema is returned for an empty document; the editor treats that as
  /// "no automations".
  document: AutomationDocument | null;
  status: AutomationsStatus;
  /// Re-read the document. Throws on failure; the controller already has the
  /// page-wide error surface.
  reload: () => Promise<void>;
  /// Replace the entire document. The agent stamps its own `rev`; the client
  /// never writes the document's revision number itself.
  put: (next: AutomationDocument) => Promise<void>;
  /// Dry-run one automation. The trace comes back as the same wire shape the
  /// editor renders, so no translation is needed in the UI.
  dryRun: (automationId: string) => Promise<Run>;
}

interface Reading {
  desktopId: string | null;
  status: AutomationsStatus;
  document: AutomationDocument | null;
}

/// An empty-but-valid stub. The editor renders this as "no automations yet".
const EMPTY: AutomationDocument = { schema: 1, rev: 0, automations: [] };

/**
 * Reactive automations document for one desktop.
 *
 * The read drops stale responses (desktop switch race) the same way
 * `useConfigDocument` does. There is no portal-side undo/redo: the document is
 * small, and a `Cmd-Z` that hits the agent's server side is the right
 * place for it.
 */
export function useAutomations(desktopId: string | null): AutomationsValue {
  const api = useApi();
  const [reading, setReading] = useState<Reading>({
    desktopId: null,
    status: "idle",
    document: null,
  });

  /// Which read is the current one. A response that is no longer the latest is
  /// dropped so a slow read of the previous desktop does not clobber the new
  /// desktop's state.
  const latest = useRef(0);

  const reload = useCallback(async () => {
    if (desktopId === null) {
      setReading({ desktopId: null, status: "idle", document: null });
      return;
    }
    const ticket = ++latest.current;
    // A re-read of what is already shown keeps showing it, rather than blanking
    // the automations (and anything drawn from them) until the answer lands.
    setReading((previous) =>
      previous.desktopId === desktopId && previous.document !== null ? previous : { desktopId, status: "loading", document: null },
    );
    try {
      const document = await api.automations(desktopId);
      if (ticket !== latest.current) return;
      const isEmpty = (document.automations ?? []).length === 0;
      setReading({
        desktopId,
        status: isEmpty ? "empty" : "loaded",
        document: isEmpty ? EMPTY : document,
      });
    } catch (cause) {
      if (ticket !== latest.current) return;
      // 404 means "no document yet" — treat as an empty stub, not a failure.
      // The agent's storage layer returns this on first run.
      if (cause instanceof ApiError && cause.status === 404) {
        setReading({ desktopId, status: "empty", document: EMPTY });
        return;
      }
      setReading({ desktopId, status: "failed", document: null });
      throw cause;
    }
  }, [api, desktopId]);

  const put = useCallback(
    async (next: AutomationDocument) => {
      if (desktopId === null) throw new Error("no desktop selected");
      const answer = await api.putAutomations(desktopId, next);
      // Re-read so the response's `rev` wins; the client's input was advisory.
      setReading((prev) =>
        prev.document === null
          ? prev
          : { ...prev, document: { ...prev.document, rev: answer.rev } },
      );
    },
    [api, desktopId],
  );

  const dryRun = useCallback(
    async (automationId: string): Promise<Run> => {
      if (desktopId === null) throw new Error("no desktop selected");
      return api.dryRun(desktopId, automationId);
    },
    [api, desktopId],
  );

  useEffect(() => {
    void reload().catch(() => {
      // The error is already on the reading's status; nothing else to do.
    });
  }, [reload]);

  return {
    document: reading.document,
    status: reading.status,
    reload,
    put,
    dryRun,
  };
}

/// Pick an automation by id from a loaded document. Returns `null` for an
/// empty document or a missing id (e.g. one that was deleted in another
/// session).
export function findAutomation(
  document: AutomationDocument | null,
  id: string,
): Automation | null {
  if (document === null) return null;
  return (document.automations ?? []).find((a) => a.id === id) ?? null;
}
