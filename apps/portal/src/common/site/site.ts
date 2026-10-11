/**
 * Which site this build is, and where the other one lives.
 *
 * One portal codebase is built twice:
 *
 *   flex-deck.com       the public pages, on a static host. `VITE_EDITOR_URL`
 *                       says where the editor is, so "Open the editor" crosses over.
 *   app.flex-deck.com   the editor alone, at `/`, served by the cloud with its
 *                       same-origin API. `VITE_EDITOR_ONLY` turns the public
 *                       pages off; `VITE_SITE_URL` points the logo back home.
 *
 * With neither set — local development, a self-hosted cloud — it is one site:
 * the public pages at `/` and the editor at `/config`.
 */
const env = import.meta.env;
const trim = (url: string | undefined) => (url ?? "").replace(/\/+$/, "");

/** This build is the editor alone, served at `/`. */
export const EDITOR_ONLY = env.VITE_EDITOR_ONLY === "true";

const editorOrigin = trim(env.VITE_EDITOR_URL);

/** Where "Open the editor" goes. */
export const EDITOR_HREF = editorOrigin ? `${editorOrigin}/` : EDITOR_ONLY ? "/" : "/config";

/** Where the wordmark goes: the public site's home. */
export const SITE_HREF = trim(env.VITE_SITE_URL) || "/";
