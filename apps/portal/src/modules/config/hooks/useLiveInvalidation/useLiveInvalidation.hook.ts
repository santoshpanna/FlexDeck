import { useStore } from "@questorylabs/qhttp/react";
import { useCallback, useRef } from "react";

import { useLive } from "../../../../hooks/useLive/useLive.hook.ts";
import type { WireConfig } from "../../../../services/api/api.ts";
import type { LiveMessage } from "../../../../services/api/live.ts";
import { needsReread, rereadsOnOpen } from "../../lib/resource.lib.ts";

export function useLiveInvalidation(desktopId: string | null): void {
  const store = useStore();
  const opened = useRef<string | null>(null);

  const hear = useCallback(
    (message: LiveMessage) => {
      if (desktopId === null) return;
      if (message.kind === "open") {
        const first = opened.current !== desktopId;
        opened.current = desktopId;
        for (const id of rereadsOnOpen(store.getAllIds(), desktopId, first, (candidate) => store.snapshot(candidate).busy)) {
          store.touch(id);
        }
        return;
      }
      if (message.kind === "revision") {
        const shown = store.snapshot<WireConfig | null>([desktopId, "config"]);
        if (shown.error !== null || needsReread(shown.value?.rev, message.rev)) {
          store.touch([desktopId, "config"]);
        }
        return;
      }
      if (message.kind === "devices") {
        store.touch([desktopId, "facts"]);
      }
    },
    [store, desktopId],
  );
  useLive(desktopId, hear);
}
