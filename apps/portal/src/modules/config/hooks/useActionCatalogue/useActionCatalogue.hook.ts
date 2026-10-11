import { useCallback, useState } from "react";

import { useApi } from "../../../../hooks/useApi/useApi.hook.ts";
import type { ActionDefinition, PluginEventChoice, VariableChoice } from "../../../../services/api/api.ts";

export interface ActionCatalogueValue {
  catalogue: ActionDefinition[];
  /** Every event a plugin on this desktop can announce. */
  events: PluginEventChoice[];
  /** Every live value a plugin on this desktop owns. */
  variables: VariableChoice[];
  reload: () => Promise<void>;
}

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
  const [catalogue, setCatalogue] = useState<ActionDefinition[]>([]);
  const [events, setEvents] = useState<PluginEventChoice[]>([]);
  const [variables, setVariables] = useState<VariableChoice[]>([]);

  const reload = useCallback(async () => {
    if (!desktopId) {
      setCatalogue([]);
      setEvents([]);
      setVariables([]);
      return;
    }

    const answer = await api.plugins(desktopId);
    setCatalogue(answer.actions);
    setEvents(answer.events);
    setVariables(answer.variables);
  }, [api, desktopId]);

  return { catalogue, events, variables, reload };
}