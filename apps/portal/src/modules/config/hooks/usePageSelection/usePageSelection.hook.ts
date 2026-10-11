import { useState } from "react";

import type { WireProfile } from "../../../../services/api/api.ts";
import { pagesOf } from "../../lib/config.lib.ts";

export interface PageSelection {
  /** The page being edited, or `null` when the profile has none. */
  currentPageId: string | null;
  selectPage: (pageId: string) => void;
}

/**
 * Which page is being edited.
 *
 * The chosen id is only a PREFERENCE — the current page is derived from it on
 * every render. So a page deleted here, or in another tab, cannot leave the
 * editor pointing at something that no longer exists: it falls back to the first
 * page instead of showing a blank grid with no explanation.
 */
export function usePageSelection(profile: WireProfile | null): PageSelection {
  const [chosen, setChosen] = useState<string | null>(null);
  const pages = pagesOf(profile);

  const currentPageId =
    chosen !== null && pages.some((page) => page.id === chosen) ? chosen : (pages[0]?.id ?? null);

  return { currentPageId, selectPage: setChosen };
}
