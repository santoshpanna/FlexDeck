import { Navigate, Route, Routes } from "react-router-dom";

import { EDITOR_ONLY } from "./common/site/site.ts";
import { ChangelogPage } from "./modules/changelog/changelog.page.tsx";
import { ConfigPage } from "./modules/config/config.page.tsx";
import { DocsPage } from "./modules/docs/docs.page.tsx";
import { DownloadPage } from "./modules/download/download.page.tsx";
import { LandingPage } from "./modules/landing/landing.page.tsx";

/**
 * The route table, and nothing else.
 *
 * Each route maps to a *page*, which is that module's controller and view
 * composed together. Keeping this file free of anything but that mapping is what
 * lets a module change without touching routing, and routing change without
 * touching a module.
 *
 * The editor-only build (app.flex-deck.com — see `common/site`) is the editor at
 * `/` and nothing else; the public pages live on the public site.
 *
 * `*` redirects rather than 404s: a stray URL is far more likely to be a typo or
 * a stale link than an intent to see an error.
 */
export function App() {
  if (EDITOR_ONLY) {
    return (
      <Routes>
        <Route path="/" element={<ConfigPage />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    );
  }
  return (
    <Routes>
      <Route path="/" element={<LandingPage />} />
      <Route path="/config" element={<ConfigPage />} />
      <Route path="/download" element={<DownloadPage />} />
      <Route path="/changelog" element={<ChangelogPage />} />
      <Route path="/docs" element={<DocsPage />} />
      <Route path="/docs/:slug" element={<DocsPage />} />
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}
