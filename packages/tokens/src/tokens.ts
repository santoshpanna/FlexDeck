/**
 * The same tokens as `tokens.css`, for code that needs a colour as a string.
 *
 * Kept deliberately small, and deliberately in step: anything a stylesheet can
 * read should read the CSS custom property instead, so there is one definition
 * of each value. This exists for the few places that must pass a colour to a
 * canvas or an inline style.
 *
 * Keys are the custom-property names WITHOUT the leading `--`, so a value here
 * and a value in the stylesheet are checked against each other by
 * `scripts/check-contrast.mjs`. `darkTokens` holds only what dark overrides —
 * exactly like the `:root.dark` block — so nothing is stated twice and drifts.
 */
export const tokens: Record<string, string> = {
  paper: "#fafaf0",
  surface: "#f4f2e8",
  "surface-sunken": "#ece9dc",
  ink: "#111111",
  "ink-2": "#333333",
  muted: "#777777",
  faint: "#9a958a",
  rule: "#cdc8bb",
  "rule-strong": "#111111",
  accent: "#8c3b2e",
  "accent-soft": "#f0e2dd",
  "accent-ink": "#fafaf0",
  ok: "#3f6b4a",
  warn: "#8a6a1f",
  bad: "#8c2f2a",
  off: "#9a958a",
  "ok-soft": "#e4ece5",
  "warn-soft": "#f1ebda",
  "bad-soft": "#f3e4e2",
  deck: "#14161a",
  "deck-widget": "#2a3140",
  "deck-widget-line": "#3d465a",
  "deck-ink": "#ffffff",
  "deck-muted": "#98a0b0",
  "deck-control": "#fafaf0",
  "deck-control-ink": "#111111",
  "deck-bezel": "#111111",
};

/** Only what dark changes. The deck paint is absent on purpose: the tablet on
 *  the desk does not change colour because the editor did. */
export const darkTokens: Record<string, string> = {
  paper: "#151410",
  surface: "#201e19",
  "surface-sunken": "#0d0c09",
  ink: "#f3f0e7",
  "ink-2": "#d4cfc2",
  muted: "#bcb5a5",
  faint: "#8b8677",
  rule: "#403d34",
  "rule-strong": "#f3f0e7",
  accent: "#f4a894",
  "accent-soft": "#3e1f17",
  "accent-ink": "#171312",
  ok: "#9dd4a7",
  warn: "#efc876",
  bad: "#faa39c",
  off: "#8b8677",
  "ok-soft": "#132416",
  "warn-soft": "#36290e",
  "bad-soft": "#3b1c1a",
  "deck-bezel": "#313942",
};

export type Appearance = "light" | "dark";

/** Every token as it resolves in one appearance — the cascade, in one function. */
export function tokensFor(appearance: Appearance): Record<string, string> {
  return appearance === "dark" ? { ...tokens, ...darkTokens } : tokens;
}
