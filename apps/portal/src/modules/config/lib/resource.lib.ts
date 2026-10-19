import type { WidgetPreview } from "../../../services/api/api.ts";

export type ReadingStatus = "idle" | "loading" | "loaded" | "empty" | "failed";

export interface ReadSnapshot<T> {
  value: T | undefined;
  error: Error | null;
  busy: boolean;
}

export function readingOf<T>(desktopId: string | null, snapshot: ReadSnapshot<T>, isEmpty: (value: T) => boolean): ReadingStatus {
  if (desktopId === null) return "idle";
  if (snapshot.busy && (snapshot.value === undefined || snapshot.error !== null)) return "loading";
  if (snapshot.error !== null) return "failed";
  if (snapshot.value === undefined) return "loading";
  return isEmpty(snapshot.value) ? "empty" : "loaded";
}

export function needsReread(shownRev: number | undefined, rev: number): boolean {
  return rev !== Number(shownRev ?? -1);
}

export function overlayPreviews(
  loaded: Record<string, WidgetPreview> | undefined,
  pushed: Record<string, NonNullable<WidgetPreview["state"]>>,
  widgetIds: readonly string[],
): Record<string, WidgetPreview> {
  const merged: Record<string, WidgetPreview> = { ...loaded };
  for (const id of widgetIds) {
    const state = pushed[id];
    if (state) merged[id] = { ...merged[id], state };
  }
  return merged;
}

const REREAD_ON_OPEN = new Set(["config", "facts", "previews"]);

export function rereadsOnOpen(
  ids: readonly (readonly unknown[])[],
  desktopId: string,
  first: boolean,
  busy: (id: readonly unknown[]) => boolean,
): (readonly unknown[])[] {
  return ids.filter((id) => id[0] === desktopId && REREAD_ON_OPEN.has(String(id[1])) && !(first && busy(id)));
}

interface Snapshots {
  snapshot(id: readonly unknown[]): { busy: boolean; error: Error | null };
}

export async function current(store: Snapshots, id: readonly unknown[], read: Promise<unknown>): Promise<void> {
  try {
    await read;
  } catch (cause) {
    const now = store.snapshot(id);
    if (!now.busy && now.error !== null && (now.error === cause || !(cause instanceof Error))) throw cause;
  }
}

export const CONFIRM_WINDOW_MS = 120_000;

export type Watched = "deck" | "automations";

export interface Unconfirmed {
  desktopId: string;
  watch: Watched | null;
  rev: number | undefined;
  until: number;
}

export function isConfirmed(pending: Unconfirmed, rev: number | undefined): boolean {
  return pending.watch !== null && rev !== undefined && rev !== pending.rev;
}
