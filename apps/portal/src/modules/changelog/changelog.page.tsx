import { ChangelogController } from "./controller/changelog.controller.ts";
import { ChangelogView } from "./view/changelog.view.tsx";

/** The changelog page — the router's entry point for `/changelog`. Wiring only. */
export function ChangelogPage() {
  return (
    <ChangelogController>
      <ChangelogView />
    </ChangelogController>
  );
}
