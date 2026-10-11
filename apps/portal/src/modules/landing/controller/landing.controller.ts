import { Children, cloneElement, type ReactElement } from "react";

import { EDITOR_HREF } from "../../../common/site/site.ts";
import type { LandingViewProps } from "../view/landing.view.tsx";

export interface LandingControllerProps {
  /** The single view element this controller injects into. */
  children: ReactElement<Partial<LandingViewProps>>;
}

/**
 * The landing page's controller: it owns the page's content and hands it to
 * the view, so the view is a pure function of its props and this is the only
 * place that knows where the words come from.
 */
export function LandingController({ children }: LandingControllerProps): ReactElement {
  const props: LandingViewProps = {
    headline: "Every key,\nexactly yours.",
    blurb:
      "Turn the tablet in your drawer into a hardware-grade control surface. Keys, faders, live states and automations — on your own Wi-Fi, with no accounts.",
    callToAction: { label: "Open the editor", to: EDITOR_HREF },
    facts: ["LAN only", "TLS pinned", "No accounts", "Open plugins"],
    anatomy: [
      { tag: "01 · Keycap", title: "Four gestures per key", body: "Tap, double tap, long press, hold and release — each runs its own flow." },
      { tag: "02 · Looks", title: "Keys that react", body: "States follow live values: recording, muted, now playing." },
      { tag: "03 · Plate", title: "Any grid, any tablet", body: "Layouts reflow to the screen, portrait or landscape." },
      { tag: "04 · Link", title: "Your Wi-Fi only", body: "Pinned TLS to your PC. No cloud in the loop." },
    ],
    features: [
      { field: "pair", tag: "01 · Pair", title: "Pair once, with six digits", body: "Your PC shows a code for two minutes. Type it on the tablet; it pins the PC's certificate and remembers it." },
      { field: "faders", tag: "02 · Faders", title: "Faders that feel physical", body: "Paced so the thumb never fights the echo, with grace on release." },
      { field: "rail", tag: "03 · Flows", title: "Automations without code", body: "When, if, repeat, do — in plain sentences, dry-run before anything moves." },
      { field: "install", tag: "04 · Plugins", title: "An open plugin system", body: "Install from reviewed repositories or link a folder. Every permission is shown first." },
    ],
    plugins: ["OBS Studio", "Spotify", "Discord", "Voicemeeter", "Twitch", "Philips Hue", "Home Assistant"],
    steps: [
      { title: "Install the desktop app", body: "It sits in the tray and runs your plugins locally." },
      { title: "Type a six-digit code", body: "Enter the PC's address and the code it shows. The tablet pins its certificate." },
      { title: "Drop actions on keys", body: "Edit in the browser; the deck updates live as you go." },
    ],
  };

  // `Children.only`: the page must compose exactly one view, and this says so
  // with an error that names the mistake.
  return cloneElement(Children.only(children), props);
}
