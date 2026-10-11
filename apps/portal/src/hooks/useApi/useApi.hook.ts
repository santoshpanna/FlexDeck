import { useContext } from "react";

import { ApiContext } from "../../providers/ApiProvider/ApiProvider.provider.tsx";
import type { Api } from "../../services/api/api.ts";

/**
 * The shared API client.
 *
 * Throws rather than returning `null`: a missing provider is a wiring mistake,
 * and the alternative is an undefined-method failure at the first request —
 * far from the component that forgot the provider.
 */
export function useApi(): Api {
  const api = useContext(ApiContext);
  if (!api) {
    throw new Error("useApi must be used inside <ApiProvider>");
  }
  return api;
}
