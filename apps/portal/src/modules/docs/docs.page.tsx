import { DocsController } from "./controller/docs.controller.ts";
import { DocsView } from "./view/docs.view.tsx";

/** The docs — the router's entry point for `/docs` and `/docs/:slug`. Wiring only. */
export function DocsPage() {
  return (
    <DocsController>
      <DocsView />
    </DocsController>
  );
}
