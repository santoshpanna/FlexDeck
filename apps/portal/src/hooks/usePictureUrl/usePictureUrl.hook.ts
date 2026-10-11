import { useContext, useEffect, useState } from "react";

import { ApiContext } from "../../providers/ApiProvider/ApiProvider.provider.tsx";
import { DesktopContext } from "../../providers/DesktopProvider/DesktopProvider.provider.tsx";

/**
 * A picture from the selected desktop (`img:<sha256>`, or the bare sha) as a
 * URL an `<img>` or a CSS background can use; `null` until it arrives, and
 * wherever it cannot (through the cloud relay, outside the providers).
 *
 * Both providers are read without their throwing hooks, so a component using
 * this still renders in a test or a story.
 */
export function usePictureUrl(value: string | null | undefined): string | null {
  const api = useContext(ApiContext);
  const desktopId = useContext(DesktopContext)?.selected?.id ?? null;
  const sha = value?.startsWith("img:") ? value.slice(4) : (value ?? "");
  const [url, setUrl] = useState<string | null>(null);

  useEffect(() => {
    setUrl(null);
    if (!api || !desktopId || !/^[0-9a-f]{64}$/.test(sha)) {
      return;
    }
    let live = true;
    void api.imageUrl(desktopId, sha).then((found) => {
      if (live) {
        setUrl(found);
      }
    });
    return () => {
      live = false;
    };
  }, [api, desktopId, sha]);

  return url;
}
