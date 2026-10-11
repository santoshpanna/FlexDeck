import { Navigate, Route, Routes } from "react-router-dom";

import { ConfigPage } from "./modules/config/config.page.tsx";
import { LandingPage } from "./modules/landing/landing.page.tsx";

/**
 * The route table, and nothing else.
 *
 * Each route maps to a *page*, which is that module's controller and view
 * composed together. Keeping this file free of anything but that mapping is what
 * lets a module change without touching routing, and routing change without
 * touching a module.
 *
 * `*` redirects rather than 404s: this app has two real screens, and a stray URL
 * is far more likely to be a typo or a stale link than an intent to see an error.
 */
export function App() {
  return (
    <Routes>
      <Route path="/" element={<LandingPage />} />
      <Route path="/config" element={<ConfigPage />} />
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}
