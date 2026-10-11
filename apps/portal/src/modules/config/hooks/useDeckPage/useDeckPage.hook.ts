import { useCallback, useEffect, useRef, useState } from "react";

import { useApi } from "../../../../hooks/useApi/useApi.hook.ts";
import { useLive } from "../../../../hooks/useLive/useLive.hook.ts";
import type { LiveMessage } from "../../../../services/api/live.ts";

export interface DeckPageOptions {
  desktopId: string | null;
  /** The ids of the pages the loaded document has. */
  pageIds: string[];
  /** Show a page in the editor, without telling the tablets. */
  selectPage: (pageId: string) => void;
}

export interface DeckPageValue {
  /** Show a page in the editor AND on every tablet: what a person choosing one means. */
  choosePage: (pageId: string) => void;
}

/**
 * The editor and the tablets on the same page.
 *
 * A page turned on a tablet is followed here the moment the agent hears of
 * it (the live socket); a page chosen here is sent to the tablets. Only a CHOICE is sent — the editor landing on its first page
 * while loading is not, or opening the editor would yank every tablet home.
 *
 * A choice waits until the document has the page, so a page just created is
 * sent once its save is back rather than refused as unknown.
 */
export function useDeckPage({ desktopId, pageIds, selectPage }: DeckPageOptions): DeckPageValue {
  const api = useApi();
  const [wanted, setWanted] = useState<string | null>(null);
  /// The last page the agent was heard to be on, or told to be on.
  const remote = useRef<string | null>(null);
  /// Bumped on every send, so a read that left before it cannot undo it.
  const sends = useRef(0);
  /// Bumped when the live socket (re)opens: the page may have moved meanwhile.
  const [opened, setOpened] = useState(0);

  const follow = useCallback(
    (pageId: string | null) => {
      if (pageId === null || pageId === remote.current) return;
      remote.current = pageId;
      selectPage(pageId);
    },
    [selectPage],
  );

  // Where the tablets are now: once, and again after a reconnect.
  useEffect(() => {
    remote.current = null;
    setWanted(null);
  }, [desktopId]);
  useEffect(() => {
    if (!desktopId) return;
    let live = true;
    const before = sends.current;
    api
      .page(desktopId)
      .then(({ pageId }) => {
        if (live && before === sends.current) follow(pageId);
      })
      // An agent too old to know, or a moment offline: the editor still works.
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [api, desktopId, follow, opened]);

  // Then every turn, as it happens. A step names no page; the agent resolves
  // steps itself whenever it knows the page, so only a named page is followed.
  const hear = useCallback(
    (message: LiveMessage) => {
      if (message.kind === "open") setOpened((count) => count + 1);
      else if (message.kind === "page" && message.pageId) follow(message.pageId);
    },
    [follow],
  );
  useLive(desktopId, hear);

  const known = wanted !== null && pageIds.includes(wanted);
  useEffect(() => {
    if (!desktopId || wanted === null || !known) return;
    setWanted(null);
    if (wanted === remote.current) return;
    sends.current += 1;
    remote.current = wanted;
    api.showPage(desktopId, wanted).catch(() => {
      // Nobody to show it to is not worth an error: the editor has moved.
    });
  }, [api, desktopId, wanted, known]);

  const choosePage = useCallback(
    (pageId: string) => {
      selectPage(pageId);
      setWanted(pageId);
    },
    [selectPage],
  );

  return { choosePage };
}
