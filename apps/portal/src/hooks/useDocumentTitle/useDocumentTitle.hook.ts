import { useEffect } from "react";

/** The tab's title while this page is shown: "<title> · flexdeck", or just "flexdeck". */
export function useDocumentTitle(title?: string) {
  useEffect(() => {
    document.title = title ? `${title} · flexdeck` : "flexdeck";
  }, [title]);
}
