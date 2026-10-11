/**
 * The bundled line-icon set.
 *
 * Deliberately small, monochrome and stroke-only, to match the deck's own look —
 * and held as DATA rather than components so the picker and the renderer read the
 * same list. An icon is stored on a widget as its NAME (in the existing
 * `props.icon`), so adding one here makes it selectable with no other change.
 *
 * Geometry is on a 24×24 grid. The `paths` are stroke paths, several per icon
 * where a shape needs more than one.
 */

export interface IconDefinition {
  name: string;
  title: string;
  paths: readonly string[];
}

export const ICONS: readonly IconDefinition[] = [
  { name: "play", title: "Play", paths: ["M8 5l11 7-11 7z"] },
  { name: "pause", title: "Pause", paths: ["M9 5v14", "M15 5v14"] },
  { name: "stop", title: "Stop", paths: ["M7 7h10v10H7z"] },
  { name: "record", title: "Record", paths: ["M8.5 12a3.5 3.5 0 1 0 7 0a3.5 3.5 0 1 0 -7 0"] },
  { name: "power", title: "Power", paths: ["M12 3v9", "M7.5 6.5a7 7 0 1 0 9 0"] },
  { name: "check", title: "Check", paths: ["M5 13l4 4L19 7"] },
  { name: "close", title: "Close", paths: ["M6 6l12 12", "M18 6L6 18"] },
  { name: "plus", title: "Add", paths: ["M12 5v14", "M5 12h14"] },
  { name: "minus", title: "Remove", paths: ["M5 12h14"] },
  { name: "pencil", title: "Edit", paths: ["M3 21l5-1L20 8l-4-4L5 17z", "M13 9l3 3"] },
  {
    name: "trash",
    title: "Delete",
    paths: ["M4 7h16", "M9 7V4h6v3", "M6.5 7l1 13h9l1-13", "M10 11v5", "M14 11v5"],
  },
  { name: "volume", title: "Volume", paths: ["M4 9h3l4-4v14l-4-4H4z", "M14.5 9.5a4 4 0 0 1 0 5"] },
  { name: "mute", title: "Mute", paths: ["M4 9h3l4-4v14l-4-4H4z", "M15 10l5 5", "M20 10l-5 5"] },
  {
    name: "mic",
    title: "Microphone",
    paths: [
      "M12 3a3 3 0 0 1 3 3v6a3 3 0 0 1-6 0V6a3 3 0 0 1 3-3z",
      "M5 11a7 7 0 0 0 14 0",
      "M12 18v3",
    ],
  },
  {
    name: "mic-off",
    title: "Microphone off",
    paths: [
      "M12 3a3 3 0 0 1 3 3v6a3 3 0 0 1-6 0V6a3 3 0 0 1 3-3z",
      "M5 11a7 7 0 0 0 14 0",
      "M12 18v3",
      "M4 4l16 16",
    ],
  },
  {
    name: "camera",
    title: "Camera",
    paths: ["M3 8h4l2-2h6l2 2h4v11H3z", "M12 16.5a3 3 0 1 0 0-6a3 3 0 1 0 0 6"],
  },
  { name: "monitor", title: "Display", paths: ["M3 5h18v11H3z", "M8 20h8"] },
  {
    name: "grid",
    title: "Grid",
    paths: ["M4 4h7v7H4z", "M13 4h7v7h-7z", "M4 13h7v7H4z", "M13 13h7v7h-7z"],
  },
  {
    name: "settings",
    title: "Settings",
    paths: [
      "M12 15.5a3.5 3.5 0 1 0 0-7a3.5 3.5 0 1 0 0 7",
      "M12 3v3",
      "M12 18v3",
      "M3 12h3",
      "M18 12h3",
    ],
  },
  { name: "home", title: "Home", paths: ["M4 11l8-7 8 7", "M6 10v10h12V10"] },
  { name: "folder", title: "Folder", paths: ["M3 6h6l2 2h10v11H3z"] },
  { name: "search", title: "Search", paths: ["M10.5 17a6.5 6.5 0 1 0 0-13a6.5 6.5 0 0 0 0 13", "M15.5 15.5L21 21"] },
  { name: "refresh", title: "Refresh", paths: ["M20 12a8 8 0 1 1-3-6", "M20 4v5h-5"] },
  { name: "undo", title: "Undo", paths: ["M4 12h11a5 5 0 0 1 0 10h-3", "M8 8l-4 4 4 4"] },
  { name: "redo", title: "Redo", paths: ["M20 12H9a5 5 0 0 0 0 10h3", "M16 8l4 4-4 4"] },
  { name: "arrow-up", title: "Up", paths: ["M12 19V5", "M6 11l6-6 6 6"] },
  { name: "arrow-down", title: "Down", paths: ["M12 5v14", "M6 13l6 6 6-6"] },
  { name: "arrow-left", title: "Left", paths: ["M19 12H5", "M11 6l-6 6 6 6"] },
  { name: "arrow-right", title: "Right", paths: ["M5 12h14", "M13 6l6 6-6 6"] },
  { name: "chevron-up", title: "Previous", paths: ["M6 15l6-6 6 6"] },
  { name: "chevron-down", title: "Next", paths: ["M6 9l6 6 6-6"] },
  { name: "chevron-left", title: "Back", paths: ["M15 6l-6 6 6 6"] },
  { name: "chevron-right", title: "Forward", paths: ["M9 6l6 6-6 6"] },
  { name: "star", title: "Star", paths: ["M12 4l2.4 5 5.6.8-4 3.9 1 5.5-5-2.7-5 2.7 1-5.5-4-3.9 5.6-.8z"] },
  { name: "heart", title: "Heart", paths: ["M12 20s-7-4.5-7-9a4 4 0 0 1 7-2.5A4 4 0 0 1 19 11c0 4.5-7 9-7 9z"] },
  { name: "bell", title: "Alert", paths: ["M6 16V11a6 6 0 0 1 12 0v5l2 3H4z", "M10 21h4"] },
  { name: "lock", title: "Lock", paths: ["M6 11h12v9H6z", "M8.5 11V8a3.5 3.5 0 0 1 7 0v3"] },
  { name: "user", title: "User", paths: ["M12 12a4 4 0 1 0 0-8a4 4 0 0 0 0 8", "M4 20a8 8 0 0 1 16 0"] },
  {
    name: "globe",
    title: "Globe",
    paths: ["M12 21a9 9 0 1 0 0-18a9 9 0 0 0 0 18", "M3 12h18", "M12 3a14 14 0 0 1 0 18", "M12 3a14 14 0 0 0 0 18"],
  },
  {
    name: "link",
    title: "Link",
    paths: ["M10 14a4 4 0 0 1 0-6l3-3a4 4 0 1 1 6 6l-2 2", "M14 10a4 4 0 0 1 0 6l-3 3a4 4 0 1 1-6-6l2-2"],
  },
  { name: "sun", title: "Light", paths: ["M12 17a5 5 0 1 0 0-10a5 5 0 0 0 0 10", "M12 2v3", "M12 19v3", "M2 12h3", "M19 12h3"] },
  { name: "moon", title: "Dark", paths: ["M20 14a8 8 0 1 1-10-10a7 7 0 0 0 10 10z"] },
  { name: "bolt", title: "Action", paths: ["M13 3L5 14h6l-1 7 8-11h-6z"] },
  // Weather, time, the machine, media: what the gallery's keys show.
  { name: "cloud", title: "Cloudy", paths: ["M7 18h10a4 4 0 0 0 .5-7.97A6 6 0 0 0 6.1 9.5A4.5 4.5 0 0 0 7 18z"] },
  { name: "cloud-rain", title: "Rain", paths: ["M7 15h10a4 4 0 0 0 .5-7.97A6 6 0 0 0 6.1 6.5A4.5 4.5 0 0 0 7 15z", "M8 18l-1 3", "M12 18l-1 3", "M16 18l-1 3"] },
  { name: "cloud-snow", title: "Snow", paths: ["M7 15h10a4 4 0 0 0 .5-7.97A6 6 0 0 0 6.1 6.5A4.5 4.5 0 0 0 7 15z", "M8 19v.01", "M12 21v.01", "M16 19v.01"] },
  { name: "thermometer", title: "Temperature", paths: ["M10 14.5V5a2 2 0 1 1 4 0v9.5a4 4 0 1 1-4 0z"] },
  { name: "clock", title: "Clock", paths: ["M12 3a9 9 0 1 0 0 18a9 9 0 0 0 0-18z", "M12 7v5l3 2"] },
  { name: "calendar", title: "Calendar", paths: ["M5 5h14a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2z", "M3 10h18", "M8 3v4", "M16 3v4"] },
  { name: "cpu", title: "Processor", paths: ["M7 7h10v10H7z", "M9 3v4", "M15 3v4", "M9 17v4", "M15 17v4", "M3 9h4", "M3 15h4", "M17 9h4", "M17 15h4"] },
  { name: "drive", title: "Disk", paths: ["M4 6h16l2 8v4a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1v-4z", "M2 14h20", "M17 17v.01"] },
  { name: "wifi", title: "Network", paths: ["M2 9a15 15 0 0 1 20 0", "M5.5 12.5a10 10 0 0 1 13 0", "M9 16a5 5 0 0 1 6 0", "M12 19.5v.01"] },
  { name: "battery", title: "Battery", paths: ["M3 7h15a1 1 0 0 1 1 1v8a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V8a1 1 0 0 1 1-1z", "M22 11v2"] },
  { name: "music", title: "Music", paths: ["M9 18V5l11-2v13", "M6 21a3 3 0 1 0 0-6a3 3 0 0 0 0 6z", "M17 19a3 3 0 1 0 0-6a3 3 0 0 0 0 6z"] },
  { name: "skip-back", title: "Previous", paths: ["M19 20L9 12l10-8z", "M5 19V5"] },
  { name: "skip-forward", title: "Next", paths: ["M5 4l10 8-10 8z", "M19 5v14"] },
];

const BY_NAME = new Map(ICONS.map((icon) => [icon.name, icon] as const));

/// The icon with this name, or `null`. Callers treat `null` as "render a
/// placeholder", never as "render nothing".
export function iconByName(name: string): IconDefinition | null {
  return BY_NAME.get(name) ?? null;
}

/// Icons whose name or title contains the query. An empty query returns them all.
export function searchIcons(query: string): IconDefinition[] {
  const needle = query.trim().toLowerCase();
  if (!needle) {
    return [...ICONS];
  }
  return ICONS.filter(
    (icon) => icon.name.includes(needle) || icon.title.toLowerCase().includes(needle),
  );
}
