import log from "../../modules/changelog/releases.json";
import { type Changelog, latestRelease } from "./release.lib.ts";

/**
 * The generated changelog, typed. `releases.json` is rewritten by CI on every
 * push to main (see `scripts/changelog`), and read here at build time — the site
 * never fetches it.
 */
export const changelog = log as Changelog;
export const latest = latestRelease(changelog);
