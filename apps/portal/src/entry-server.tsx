import { ResourceProvider } from "@questorylabs/qhttp/react";
import { renderToString } from "react-dom/server";
import { StaticRouter } from "react-router-dom";

import { App } from "./App.tsx";
import { latest } from "./common/release/release.ts";
import { DOCS } from "./modules/docs/lib/docs.content.ts";
import { ApiProvider } from "./providers/ApiProvider/ApiProvider.provider.tsx";
import { DesktopProvider } from "./providers/DesktopProvider/DesktopProvider.provider.tsx";

export { DOCS, latest };

const RESOURCES = { freshFor: Infinity, retries: false } as const;

export function render(path: string): string {
  return renderToString(
    <StaticRouter location={path}>
      <ApiProvider>
        <ResourceProvider defaults={RESOURCES}>
          <DesktopProvider>
            <App />
          </DesktopProvider>
        </ResourceProvider>
      </ApiProvider>
    </StaticRouter>,
  );
}
