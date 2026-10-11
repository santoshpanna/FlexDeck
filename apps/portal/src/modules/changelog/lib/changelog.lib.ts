import type { Area, ReleaseGroup } from "../../../common/release/release.lib.ts";

export type AreaFilter = "ALL" | Exclude<Area, "GENERAL">;

export const AREA_FILTERS: readonly { value: AreaFilter; label: string }[] = [
  { value: "ALL", label: "All" },
  { value: "DESKTOP", label: "Desktop" },
  { value: "ANDROID", label: "Android" },
  { value: "EDITOR", label: "Editor" },
  { value: "PLUGINS", label: "Plugins" },
];

/** Groups narrowed to one area. Groups left empty are dropped, so a release can come back with none. */
export function filterGroups(groups: ReleaseGroup[], area: AreaFilter): ReleaseGroup[] {
  if (area === "ALL") return groups;
  return groups.map((group) => ({ ...group, items: group.items.filter((item) => item.area === area) })).filter((group) => group.items.length > 0);
}

/** How many changes a set of groups holds. */
export function countChanges(groups: ReleaseGroup[]): number {
  return groups.reduce((sum, group) => sum + group.items.length, 0);
}
