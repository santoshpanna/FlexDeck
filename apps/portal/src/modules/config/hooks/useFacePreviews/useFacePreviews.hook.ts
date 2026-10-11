import { useContext, useEffect, useState } from "react";

import { ApiContext } from "../../../../providers/ApiProvider/ApiProvider.provider.tsx";
import type { WidgetPreview, WireWidget } from "../../../../services/api/api.ts";

/** How often live values are asked for again while the page shows. */
const REFRESH_MS = 2_000;

/** Whether anything on a widget follows a live value, so it is worth asking again. */
function follows(widget: WireWidget): boolean {
  const text = JSON.stringify([widget.props, widget.face, widget.states]);
  return text.includes("{{") || text.includes('"rules"') || text.includes('"graph"');
}

/**
 * What each widget on a page shows now, as the agent works it out — so the
 * editor's preview fills a template exactly as the tablet will, and a
 * template that does not parse says why.
 *
 * Asked once when the widgets change (a moment after, so typing is not a
 * request per key), then again every couple of seconds while any of them
 * follows a live value and the window is in front.
 */
export function useFacePreviews(desktopId: string | null, widgets: readonly WireWidget[]): Record<string, WidgetPreview> {
  // Read without the throwing hook: a view drawn outside the providers simply
  // has no previews.
  const api = useContext(ApiContext);
  const [previews, setPreviews] = useState<Record<string, WidgetPreview>>({});
  const key = JSON.stringify(widgets);

  useEffect(() => {
    if (!api || !desktopId || widgets.length === 0) {
      setPreviews({});
      return;
    }
    let live = true;
    const ask = () => {
      if (typeof document !== "undefined" && document.visibilityState === "hidden") return;
      api
        .facePreviews(desktopId, [...widgets])
        .then((answer) => live && setPreviews(answer))
        // An agent that predates previews, or a dropped call: draw from the config.
        .catch(() => live && setPreviews({}));
    };
    const first = setTimeout(ask, 250);
    const again = widgets.some(follows) ? setInterval(ask, REFRESH_MS) : null;
    return () => {
      live = false;
      clearTimeout(first);
      if (again) clearInterval(again);
    };
    // `key` stands for `widgets`: a new array with the same widgets is no change.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [api, desktopId, key]);

  return previews;
}
