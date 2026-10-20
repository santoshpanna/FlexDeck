import { useCallback, useMemo, useState } from "react";

import { useApi } from "../../../../hooks/useApi/useApi.hook.ts";
import { ApiError, type WireConfig } from "../../../../services/api/api.ts";
import { readingOf } from "../../lib/resource.lib.ts";
import { WriteQueue } from "../../lib/write.queue.lib.ts";
import { useDesktopResource } from "../useDesktopResource/useDesktopResource.hook.ts";

/**
 * What the document read is doing.
 *
 * The first three of these are all "no document", and they need DIFFERENT
 * screens: still reading is not the same as a desktop that has none yet, and
 * neither is a read that failed. Inferring them from a nullable config plus the
 * page's shared error cannot tell them apart — which is how a deck that does not
 * exist came to be drawn as an editable 0 x 0 one.
 */
export type DocumentStatus = "idle" | "loading" | "loaded" | "empty" | "failed";

export interface ConfigDocumentValue {
  /** `null` when there is nothing to show — `status` says which kind of nothing. */
  config: WireConfig | null;
  status: DocumentStatus;
  /**
   * Re-read the document.
   *
   * Every method here *throws* on failure rather than holding its own error
   * state: the controller already has one error to show, and a second one would
   * give the page two places to disagree about what went wrong.
   */
  reload: () => Promise<void>;
  ensure: () => Promise<void>;
  put: (next: WireConfig) => Promise<void>;
  undo: () => Promise<void>;
  redo: () => Promise<void>;
}

/**
 * The configuration document, and the writes that replace it.
 *
 * Undo and redo are the *agent's* revisions rather than a portal-side stack, so
 * they survive a reload and the tablet sees the same history the editor does.
 */
export function useConfigDocument(desktopId: string | null): ConfigDocumentValue {
  const api = useApi();

  const load = useCallback(async (): Promise<WireConfig | null> => {
    try {
      return await api.config(desktopId ?? "");
    } catch (cause) {
      // A brand-new agent has no document until the first edit, and that is a
      // 404. A state, not a failure.
      if (cause instanceof ApiError && cause.status === 404) {
        return null;
      }
      throw cause;
    }
  }, [api, desktopId]);

  const { resource, reload, ensure } = useDesktopResource<WireConfig | null>(desktopId, "config", load);
  const status = readingOf(desktopId, resource, (value) => value === null);

  const queue = useMemo(
    () => (desktopId ? new WriteQueue<WireConfig>((next) => api.putConfig(desktopId, next)) : null),
    [api, desktopId],
  );
  const [pending, setPending] = useState<{ queue: WriteQueue<WireConfig>; config: WireConfig } | null>(null);

  const put = useCallback(
    async (next: WireConfig) => {
      if (!queue) return;
      setPending({ queue, config: next });
      try {
        await queue.push(next);
      } catch (cause) {
        setPending((shown) => (shown?.queue === queue ? null : shown));
        throw cause;
      }
      if (queue.busy) return;
      await reload().catch(() => undefined);
      if (queue.busy) return;
      queue.settle();
      setPending((shown) => (shown?.queue === queue ? null : shown));
    },
    [queue, reload],
  );

  const undo = useCallback(async () => {
    if (desktopId) {
      await queue?.idle();
      await api.undo(desktopId);
    }
  }, [api, desktopId, queue]);

  const redo = useCallback(async () => {
    if (desktopId) {
      await queue?.idle();
      await api.redo(desktopId);
    }
  }, [api, desktopId, queue]);

  const loaded = status === "loaded" ? (resource.value ?? null) : null;
  return {
    config: pending?.queue === queue ? pending.config : loaded,
    status,
    reload,
    ensure,
    put,
    undo,
    redo,
  };
}
