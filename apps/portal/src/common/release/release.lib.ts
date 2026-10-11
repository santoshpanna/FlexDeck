/**
 * Where the downloads live, and what the newest release is.
 *
 * The download links point at GitHub's `releases/latest/download/<name>`, which
 * always resolves to the newest release's file of that name — so the site never
 * needs a deploy to start serving a new version. That only works if every
 * release uploads its files under exactly these names.
 *
 * Pure: `release.ts` feeds it the generated changelog.
 */

export const REPO_URL = "https://github.com/santoshpanna/FlexDeck";
export const RELEASES_URL = `${REPO_URL}/releases`;

const latestFile = (name: string) => `${RELEASES_URL}/latest/download/${name}`;

export interface Asset {
  platform: string;
  file: string;
  url: string;
}

export const ASSETS = {
  windows: { platform: "Windows 10 / 11 · x64", file: "flexdeck-windows-x64-setup.exe", url: latestFile("flexdeck-windows-x64-setup.exe") },
  android: { platform: "Android 5.0+", file: "flexdeck-android.apk", url: latestFile("flexdeck-android.apk") },
  checksums: { platform: "Checksums", file: "SHA256SUMS.txt", url: latestFile("SHA256SUMS.txt") },
} as const satisfies Record<string, Asset>;

/** Mirrors `scripts/changelog/changelog.lib.ts` — the shape of `releases.json`. */
export type Area = "DESKTOP" | "ANDROID" | "EDITOR" | "PLUGINS" | "GENERAL";
export type Group = "Added" | "Improved" | "Fixed" | "Security";

export interface ReleaseGroup {
  name: Group;
  items: { area: Area; text: string }[];
}

export interface Release {
  version: string;
  /** What this release shipped; an app it skipped keeps its file from the release before. */
  platforms: ("desktop" | "android" | "server")[];
  hash: string;
  date: string;
  title: string;
  blurb: string;
  groups: ReleaseGroup[];
}

export interface Changelog {
  next: ReleaseGroup[];
  releases: Release[];
}

/** The newest release, or null before the first one. */
export function latestRelease(log: Changelog): Release | null {
  return log.releases[0] ?? null;
}

export type Platform = "windows" | "android" | "other";

/** Which download to put first. A guess from the user agent, so it only ever reorders — never hides. */
export function detectPlatform(userAgent: string): Platform {
  if (/android/i.test(userAgent)) return "android";
  if (/windows/i.test(userAgent)) return "windows";
  return "other";
}
