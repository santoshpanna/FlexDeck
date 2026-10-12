/**
 * The interface's own glyphs: navigation, actions, chrome.
 *
 * Not the deck's icon set — that one is data a widget stores by name and the
 * tablet draws, and it lives with the editor. These never leave the interface,
 * so they are a plain map from name to stroke paths on a 24×24 grid.
 */
const GLYPHS = {
  home: ["M3 11l9-7 9 7v9a1 1 0 0 1-1 1h-5v-6H9v6H4a1 1 0 0 1-1-1z"],
  tablet: ["M6 2h12a1 1 0 0 1 1 1v18a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V3a1 1 0 0 1 1-1z", "M11 18h2"],
  plug: ["M9 3v4", "M15 3v4", "M6 7h12v4a6 6 0 0 1-12 0z", "M12 17v4"],
  sliders: ["M4 6h10", "M18 6h2", "M4 12h4", "M12 12h8", "M4 18h12", "M14 4v4", "M8 10v4", "M16 16v4"],
  arrowRight: ["M5 12h14", "M13 6l6 6-6 6"],
  close: ["M6 6l12 12", "M18 6L6 18"],
  check: ["M5 13l4 4L19 7"],
  refresh: ["M20 11a8 8 0 1 0-2.3 5.7", "M20 4v7h-7"],
  chevronDown: ["M6 9l6 6 6-6"],
  chevronLeft: ["M15 6l-6 6 6 6"],
  chevronRight: ["M9 6l6 6-6 6"],
  copy: ["M9 9h11v11H9z", "M5 15H4V4h11v1"],
  file: ["M6 2h8l4 4v16H6z", "M14 2v4h4"],
  folder: ["M3 6h6l2 2h10v11H3z"],
  log: ["M5 4h14v16H5z", "M8 8h8", "M8 12h8", "M8 16h5"],
  power: ["M12 3v9", "M7.5 6.5a7 7 0 1 0 9 0"],
  globe: ["M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18z", "M3 12h18", "M12 3c2.5 3 2.5 15 0 18", "M12 3c-2.5 3-2.5 15 0 18"],
  monitor: ["M3 4h18v12H3z", "M8 20h8", "M12 16v4"],
  plus: ["M12 5v14", "M5 12h14"],
  search: ["M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14z", "M20 20l-3.5-3.5"],
  bolt: ["M13 2L4 14h7l-1 8 9-12h-7z"],
  shield: ["M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6z"],
} as const;

export type GlyphName = keyof typeof GLYPHS;

export function Glyph({
  name,
  size = 16,
  strokeWidth = 1.7,
  className,
  label,
}: {
  name: GlyphName;
  size?: number;
  strokeWidth?: number;
  className?: string;
  /** Name it only when nothing beside it does; otherwise it is decoration. */
  label?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      {...(label ? { role: "img", "aria-label": label } : { "aria-hidden": true })}
    >
      {GLYPHS[name].map((d) => (
        <path key={d} d={d} />
      ))}
    </svg>
  );
}
