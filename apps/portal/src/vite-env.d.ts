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
  /** Unused: the portal has no build-time settings. Kept so the type exists. */
  readonly MODE: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
