import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { startTheme } from "@flexdeck/tokens/theme.ts";
import { ShellPage } from "./modules/shell/shell.page.tsx";
import "./app.css";

// Before React, and before the first paint the bootstrap script could not cover:
// this is what makes a later change to the system preference take effect.
startTheme();

const container = document.getElementById("root");
if (container === null) {
  throw new Error("the window has no #root to mount into");
}

createRoot(container).render(
  <StrictMode>
    <ShellPage />
  </StrictMode>,
);
