import type { GridSpec } from "@flexdeck/grid";

import { Stepper } from "../../../../common/components/Stepper/Stepper.component.tsx";
import type { GridSizePatch } from "../../lib/grid.lib.ts";

export interface GridSizePanelProps {
  spec: GridSpec;
  onChange: (patch: GridSizePatch) => void;
}

/// Big enough for any tablet deck, small enough that a cell is still tappable.
const MAX_COLS = 24;
const MAX_ROWS = 24;
const MAX_GAP = 200;

/**
 * The deck's shape: how many cells, and how far apart. The panel's pixel size
 * is not shown — it comes from the device, nobody edits it here, and a number
 * that cannot be changed and disagrees with the tablet is worse than none.
 */
export function GridSizePanel({ spec, onChange }: GridSizePanelProps) {
  return (
    <div className="grid grid-cols-2 gap-3">
      <Stepper label="Columns" value={spec.cols} min={1} max={MAX_COLS} onChange={(cols) => onChange({ cols })} />
      <Stepper label="Rows" value={spec.rows} min={1} max={MAX_ROWS} onChange={(rows) => onChange({ rows })} />
      <Stepper label="Gap across" value={spec.gapX} min={0} max={MAX_GAP} onChange={(gapX) => onChange({ gapX })} />
      <Stepper label="Gap down" value={spec.gapY} min={0} max={MAX_GAP} onChange={(gapY) => onChange({ gapY })} />
    </div>
  );
}
