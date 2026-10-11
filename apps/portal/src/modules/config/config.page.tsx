import { ConfigController } from "./controller/config.controller.ts";
import { ConfigView } from "./view/config.view.tsx";

/**
 * The configurator — the router's entry point for `/config`.
 *
 * Wiring only. The controller holds the logic and the view holds the markup, so
 * neither of them has to know this route exists.
 */
export function ConfigPage() {
  return (
    <ConfigController>
      <ConfigView />
    </ConfigController>
  );
}
