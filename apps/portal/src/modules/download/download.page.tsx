import { DownloadController } from "./controller/download.controller.ts";
import { DownloadView } from "./view/download.view.tsx";

/** The download page — the router's entry point for `/download`. Wiring only. */
export function DownloadPage() {
  return (
    <DownloadController>
      <DownloadView />
    </DownloadController>
  );
}
