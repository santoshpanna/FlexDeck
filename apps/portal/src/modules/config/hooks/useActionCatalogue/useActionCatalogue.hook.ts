import { useCallback } from "react";

import { useApi } from "../../../../hooks/useApi/useApi.hook.ts";
import type { ActionDefinition, PluginEventChoice, VariableChoice } from "../../../../services/api/api.ts";
import { useDesktopResource } from "../useDesktopResource/useDesktopResource.hook.ts";

export interface ActionCatalogueValue {
  catalogue: ActionDefinition[];
  /** Every event a plugin on this desktop can announce. */
  events: PluginEventChoice[];
  /** Every live value a plugin on this desktop owns. */
  variables: VariableChoice[];
  reload: () => Promise<void>;
  ensure: () => Promise<void>;
}

interface Answer {
  actions: ActionDefinition[];
  events: PluginEventChoice[];
  variables: VariableChoice[];
}

const NONE: never[] = [];

/**
 * The actions every plugin on this desktop exposes.
 *
 * Backed by `GET /api/plugins` rather than the legacy `GET /api/actions`: the
 * plugin endpoint carries every action of every plugin (built-in and
 * third-party) with its typed arguments and choice lists. The hook flattens
 * that into the `ActionDefinition[]` shape the editor's `ActionForm` already
 * understands, so the dropdown shows the `system` plugin's
 * `set_volume`/`set_muted`/`set_output_device` and `notifications`'s
 * `notify` without any further change to the form.
 */
export function useActionCatalogue(desktopId: string | null): ActionCatalogueValue {
  const api = useApi();

  const load = useCallback(() => api.plugins(desktopId ?? ""), [api, desktopId]);
  const { resource, reload, ensure } = useDesktopResource<Answer>(desktopId, "catalogue", load);
  const answer = desktopId === null ? undefined : resource.value;

  return {
    catalogue: answer?.actions ?? NONE,
    events: answer?.events ?? NONE,
    variables: answer?.variables ?? NONE,
    reload,
    ensure,
  };
}
