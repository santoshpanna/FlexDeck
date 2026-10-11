import type { WireConfig } from "../../../../services/api/api.ts";

/**
 * The document exactly as it travels, for when the grid is not enough. Folded
 * away: it exists for the moment the preview and the agent disagree.
 */
export function RawConfigCard({ config }: { config: WireConfig | null }) {
  return (
    <details className="rounded-plate border border-line bg-plate p-5">
      <summary className="lbl cursor-pointer hover:text-ink">Raw configuration</summary>
      <pre className="mt-4 mb-0 max-h-[70vh] overflow-auto rounded-xl border border-line bg-well p-3 font-mono text-[11px] leading-relaxed text-ink-2 shadow-well">
        {JSON.stringify(config, null, 2)}
      </pre>
    </details>
  );
}
