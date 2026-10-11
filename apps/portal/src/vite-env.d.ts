/// <reference types="vite/client" />

/**
 * Ambient types for the Vite-provided globals the portal reads.
 *
 * `tsconfig.json` sets `"types": []`, which switches off the automatic
 * inclusion of `@types/*`. A triple-slash reference is the one way to opt back
 * in for a single package, so `import.meta.env` is typed here without loosening
 * the rest of the project.
 */

/** The environment variables the portal reads. */
interface ImportMetaEnv {
  readonly MODE: string;
  /**
   * The editor's origin, when it is deployed apart from the public pages
   * (e.g. `https://app.flex-deck.com`). Unset: the editor is on this origin.
   */
  readonly VITE_EDITOR_URL?: string;
  /** `"true"`: this build is the editor alone, at `/` (the cloud's own build). */
  readonly VITE_EDITOR_ONLY?: string;
  /** The public site's origin, for the editor's wordmark (e.g. `https://flex-deck.com`). */
  readonly VITE_SITE_URL?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
