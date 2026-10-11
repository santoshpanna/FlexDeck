import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter } from "react-router-dom";

import { startTheme } from "@flexdeck/tokens/theme.ts";
import { App } from "./App.tsx";
import { ApiProvider } from "./providers/ApiProvider/ApiProvider.provider.tsx";
import { DesktopProvider } from "./providers/DesktopProvider/DesktopProvider.provider.tsx";
import "./app.css";

// Before React, and before the first paint the bootstrap script could not cover:
// this is what makes a later change to the system preference take effect.
startTheme();

const container = document.getElementById("root");
if (!container) {
  throw new Error("the page has no #root element");
}

createRoot(container).render(
  <StrictMode>
    {/*
      The nesting is a dependency order: the desktop list needs the API client.
      There is no session provider — there are no accounts to sign in to.
    */}
    <BrowserRouter>
      <ApiProvider>
        <DesktopProvider>
          <App />
        </DesktopProvider>
      </ApiProvider>
    </BrowserRouter>
  </StrictMode>,
);
