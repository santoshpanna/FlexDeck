import { LandingController } from "./controller/landing.controller.ts";
import { LandingView } from "./view/landing.view.tsx";

/**
 * The landing page — the router's entry point for `/`.
 *
 * Wiring only. The controller holds the data and the view holds the markup, so
 * neither of them has to know this route exists.
 */
export function LandingPage() {
  return (
    <LandingController>
      <LandingView />
    </LandingController>
  );
}
