import { useCallback, useRef, useState } from "react";

import { useApi } from "../../../../hooks/useApi/useApi.hook.ts";
import { ApiError, type WireConfig } from "../../../../services/api/api.ts";

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
  put: (next: WireConfig) => Promise<void>;
  undo: () => Promise<void>;
  redo: () => Promise<void>;
}

/// One desktop's outcome.
interface Reading {
  desktopId: string | null;
  status: DocumentStatus;
  config: WireConfig | null;
}

/**
 * The configuration document, and the writes that replace it.
 *
 * Undo and redo are the *agent's* revisions rather than a portal-side stack, so
 * they survive a reload and the tablet sees the same history the editor does.
 */
export function useConfigDocument(desktopId: string | null): ConfigDocumentValue {
  const api = useApi();
  const [reading, setReading] = useState<Reading>({
    desktopId: null,
    status: "idle",
    config: null,
  });

  /// Which read is the current one. A response that is no longer the latest is
  /// dropped: switching desktops with a read in flight would otherwise let the
  /// SLOWER answer land last and draw one machine's deck while the writes went to
  /// the other.
  const latest = useRef(0);

  const reload = useCallback(async () => {
    const request = ++latest.current;

    // No desktop chosen yet. There is nothing to read, and asking would be a
    // request for an id that does not exist.
    if (!desktopId) {
      setReading({ desktopId: null, status: "idle", config: null });
      return;
    }

    // A re-read of the deck already on screen (every edit ends in one) keeps it
    // on screen: dropping to "loading" would unmount the editor and redraw it
    // from nothing, which reads as the whole page refreshing.
    setReading((previous) =>
      previous.desktopId === desktopId && previous.status === "loaded" ? previous : { ...previous, status: "loading" },
    );

    try {
      const config = await api.config(desktopId);

      if (request !== latest.current) {
        return;
      }

      setReading({ desktopId, status: "loaded", config });
    } catch (cause) {
      if (request !== latest.current) {
        return;
      }

      // A brand-new agent has no document until the first edit, and that is a
      // 404. A state, not a failure.
      if (cause instanceof ApiError && cause.status === 404) {
        setReading({ desktopId, status: "empty", config: null });
        return;
      }

      // Drop the document rather than leaving the PREVIOUS desktop's on screen.
      // Keeping it would draw one machine's deck while every write targeted
      // another, and the only sign of that would be a deck that changes by itself.
      setReading({ desktopId, status: "failed", config: null });
      throw cause;
    }
  }, [api, desktopId]);

  // A document belongs to the desktop it was read FROM. Anything else is not this
  // desktop's deck, however recently it arrived; and a desktop we have no outcome
  // for yet is still being read, because the controller re-reads on every change
  // of selection.
  const current: Reading =
    reading.desktopId === desktopId
      ? reading
      : { desktopId, status: desktopId === null ? "idle" : "loading", config: null };

  const put = useCallback(
    async (next: WireConfig) => {
      if (desktopId) {
        await api.putConfig(desktopId, next);
      }
    },
    [api, desktopId],
  );

  const undo = useCallback(async () => {
    if (desktopId) {
      await api.undo(desktopId);
    }
  }, [api, desktopId]);

  const redo = useCallback(async () => {
    if (desktopId) {
      await api.redo(desktopId);
    }
  }, [api, desktopId]);

  return {
    config: current.config,
    status: current.status,
    reload,
    put,
    undo,
    redo,
  };
}
