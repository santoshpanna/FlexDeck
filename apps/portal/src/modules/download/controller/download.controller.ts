import { Children, cloneElement, type ReactElement } from "react";

import { latest } from "../../../common/release/release.ts";
import { ASSETS, RELEASES_URL, detectPlatform } from "../../../common/release/release.lib.ts";
import type { DownloadViewProps } from "../view/download.view.tsx";

export interface DownloadControllerProps {
  children: ReactElement<Partial<DownloadViewProps>>;
}

/**
 * The download page's controller: which release is current, which file each
 * button fetches, and which platform this visitor is probably on.
 *
 * Before the first release there is no `latest/download` to point at, so every
 * button goes to the releases list instead of a 404.
 */
export function DownloadController({ children }: DownloadControllerProps): ReactElement {
  const released = latest !== null;
  const link = (url: string) => (released ? url : RELEASES_URL);
  const props: DownloadViewProps = {
    version: latest?.version ?? null,
    date: latest?.date ?? null,
    platform: typeof navigator === "undefined" ? "other" : detectPlatform(navigator.userAgent),
    windows: { ...ASSETS.windows, url: link(ASSETS.windows.url) },
    android: { ...ASSETS.android, url: link(ASSETS.android.url) },
    checksums: { ...ASSETS.checksums, url: link(ASSETS.checksums.url) },
  };
  return cloneElement(Children.only(children), props);
}
