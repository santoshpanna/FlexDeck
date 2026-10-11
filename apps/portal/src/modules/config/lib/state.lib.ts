/**
 * What a widget's state may be.
 *
 * Mirrors `StateEvent.state` in `events.proto` — the same five words, so the
 * portal cannot offer a state the tablet has never heard of. Selecting one in the
 * sidebar switches how the PREVIEW draws the widget; it is not sent anywhere.
 */
export const WIDGET_STATES = ["idle", "pressed", "on", "off", "disabled"] as const;

export type WidgetState = (typeof WIDGET_STATES)[number];
