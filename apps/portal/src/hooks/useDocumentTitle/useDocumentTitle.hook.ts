import { useEffect } from "react";

import { pageTitle } from "../../common/site/seo.ts";

/** The tab's title while this page is shown: "<title> · flexdeck", or the home page's title. */
export function useDocumentTitle(title?: string) {
  useEffect(() => {
    document.title = pageTitle(title);
  }, [title]);
}
