import { JsonTree } from "../../../../common/components/JsonTree/JsonTree.component.tsx";
import type { Json } from "../../../../common/components/JsonTree/jsonTree.lib.ts";
import type { WireConfig } from "../../../../services/api/api.ts";

/**
 * The document exactly as it travels, for when the grid is not enough. Folded
 * away: it exists for the moment the preview and the agent disagree.
 */
export function RawConfigCard({ config }: { config: WireConfig | null }) {
  return (
    <details className="rounded-plate border border-line bg-plate p-5">
      <summary className="lbl cursor-pointer hover:text-ink">Raw configuration</summary>
      {config ? (
        // Not keyed by revision: what is unfolded stays unfolded across edits.
        <JsonTree value={config as unknown as Json} className="mt-4" />
      ) : (
        <p className="mt-4 mb-0 text-sm text-muted">Nothing read yet.</p>
      )}
    </details>
  );
}
