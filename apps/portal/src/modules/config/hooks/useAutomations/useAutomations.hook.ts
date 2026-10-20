import { useStore } from "@questorylabs/qhttp/react";
import { useCallback, useMemo } from "react";

import { useApi } from "../../../../hooks/useApi/useApi.hook.ts";
import { ApiError, type Automation, type AutomationDocument, type Run } from "../../../../services/api/api.ts";
import { readingOf } from "../../lib/resource.lib.ts";
import { useDesktopResource } from "../useDesktopResource/useDesktopResource.hook.ts";

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
  ensure: () => Promise<void>;
  /// Replace the entire document. The agent stamps its own `rev`; the client
  /// never writes the document's revision number itself.
  put: (next: AutomationDocument) => Promise<number>;
  /// Dry-run one automation. The trace comes back as the same wire shape the
  /// editor renders, so no translation is needed in the UI.
  dryRun: (automationId: string) => Promise<Run>;
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
  const store = useStore();

  const load = useCallback(async (): Promise<AutomationDocument | null> => {
    try {
      return await api.automations(desktopId ?? "");
    } catch (cause) {
      // 404 means "no document yet" — treat as an empty stub, not a failure.
      // The agent's storage layer returns this on first run.
      if (cause instanceof ApiError && cause.status === 404) return null;
      throw cause;
    }
  }, [api, desktopId]);

  const { resource, reload, ensure } = useDesktopResource<AutomationDocument | null>(desktopId, "automations", load);
  const status = readingOf(desktopId, resource, (document) => (document?.automations ?? []).length === 0);
  const value = resource.value;
  const document = useMemo(() => {
    if (status === "loaded") return value ?? null;
    if (status === "empty") return value ? { ...EMPTY, rev: value.rev } : EMPTY;
    return null;
  }, [status, value]);

  const put = useCallback(
    async (next: AutomationDocument) => {
      if (desktopId === null) throw new Error("no desktop selected");
      const answer = await api.putAutomations(desktopId, next);
      const id = [desktopId, "automations"];
      const shown = store.snapshot<AutomationDocument | null>(id);
      if (shown.busy) {
        await store.reload(id, load).catch(() => undefined);
        return answer.rev;
      }
      if (shown.value === undefined || shown.error !== null) return answer.rev;
      store.update<AutomationDocument | null>(id, (previous) =>
        previous ? { ...previous, rev: answer.rev } : { ...EMPTY, rev: answer.rev },
      );
      return answer.rev;
    },
    [api, store, desktopId, load],
  );

  const dryRun = useCallback(
    async (automationId: string): Promise<Run> => {
      if (desktopId === null) throw new Error("no desktop selected");
      return api.dryRun(desktopId, automationId);
    },
    [api, desktopId],
  );

  return {
    document,
    status,
    reload,
    ensure,
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
