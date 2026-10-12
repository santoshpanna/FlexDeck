import { useCallback, useContext, useEffect, useMemo, useState } from "react";

import { useLive } from "../../../../hooks/useLive/useLive.hook.ts";
import { ApiContext } from "../../../../providers/ApiProvider/ApiProvider.provider.tsx";
import type { WidgetPreview, WireWidget } from "../../../../services/api/api.ts";
import type { LiveMessage } from "../../../../services/api/live.ts";

/**
 * What each widget on a page shows now, as the agent works it out — so the
 * editor's preview fills a template exactly as the tablet will, and a
 * template that does not parse says why.
 *
 * Asked once when the widgets change (a moment after, so typing is not a
 * request per key) and whenever the live socket (re)opens. After that, every
 * look the agent pushes — a scene going live, a timer ticking — lands the
 * moment it changes, exactly as it does on the tablet. Nothing is polled.
 */
export function useFacePreviews(desktopId: string | null, widgets: readonly WireWidget[]): Record<string, WidgetPreview> {
  // Read without the throwing hook: a view drawn outside the providers simply
  // has no previews.
  const api = useContext(ApiContext);
  const [previews, setPreviews] = useState<Record<string, WidgetPreview>>({});
  // What the agent pushed since the last ask. Kept apart, and laid over the
  // answer: a push can land before the answer does, and is the newer of the two.
  const [pushed, setPushed] = useState<Record<string, NonNullable<WidgetPreview["state"]>>>({});
  // Bumped by a (re)opened socket: what was read before it may be stale.
  const [opened, setOpened] = useState(0);
  const key = JSON.stringify(widgets);

  useEffect(() => {
    if (!api || !desktopId || widgets.length === 0) {
      setPreviews({});
      return;
    }
    let live = true;
    const first = setTimeout(() => {
      // From here on, only pushes newer than this ask count.
      setPushed({});
      api
        .facePreviews(desktopId, [...widgets])
        .then((answer) => live && setPreviews(answer))
        // An agent that predates previews, or a dropped call: draw from the config.
        .catch(() => live && setPreviews({}));
    }, 250);
    return () => {
      live = false;
      clearTimeout(first);
    };
    // `key` stands for `widgets`: a new array with the same widgets is no change.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [api, desktopId, key, opened]);

  const hear = useCallback((message: LiveMessage) => {
    if (message.kind === "open") {
      setOpened((count) => count + 1);
      return;
    }
    if (message.kind !== "state") return;
    setPushed((current) => ({ ...current, [message.widgetId]: message.state }));
  }, []);
  useLive(desktopId, hear);

  return useMemo(() => {
    const merged: Record<string, WidgetPreview> = { ...previews };
    // Only keys this page shows; the agent pushes every live key's look.
    for (const widget of widgets) {
      const state = pushed[widget.id];
      if (state) merged[widget.id] = { ...merged[widget.id], state };
    }
    return merged;
    // `key` stands for `widgets`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [previews, pushed, key]);
}
