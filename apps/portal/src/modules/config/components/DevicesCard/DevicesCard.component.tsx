import { AnimatePresence, motion } from "motion/react";
import { useMemo, useState } from "react";

import { Glyph, HoldToConfirm, Pager, Pill, item, paginate, useSteadyHeight } from "@flexdeck/ui";

import { Card } from "../../../../common/components/Card/Card.component.tsx";
import type { Device } from "../../../../services/api/api.ts";

export interface DevicesCardProps {
  devices: Device[];
  onRevoke: (deviceId: string) => void;
  /** Remove every device but this browser. */
  onRevokeAll: () => void;
}

const PAGE_SIZE = 8;

/** Tablet or browser. Only called when the desktop says which. */
function kindLabel(device: Device): string {
  return device.kind === "tablet" ? "Tablet" : "Browser";
}

/**
 * The tablets and browsers paired to this desktop — this browser first, then
 * newest first, a page at a time. Removing one — or all the others — is a hold, not a tap.
 */
export function DevicesCard({ devices, onRevoke, onRevokeAll }: DevicesCardProps) {
  const [page, setPage] = useState(0);
  const newest = useMemo(
    () => [...devices].sort((a, b) => Number(b.current ?? false) - Number(a.current ?? false) || b.created_at - a.created_at),
    [devices],
  );
  const paged = paginate(newest, page, PAGE_SIZE);
  const steady = useSteadyHeight<HTMLUListElement>(paged.pages);

  return (
    <Card
      title={`Devices · ${devices.length}`}
      actions={
        devices.length > 1 ? (
          <HoldToConfirm size="sm" tone="danger" onConfirm={onRevokeAll}>
            Remove all others
          </HoldToConfirm>
        ) : null
      }
    >
      {devices.length === 0 ? (
        <p className="m-0 text-sm text-muted">None yet.</p>
      ) : (
        <>
          {/* A page change swaps the rows outright: no fade, no rows animating in or
              out, so nothing moves. The steady height keeps a short last page from
              pulling the pager up. */}
          <ul
            key={paged.page}
            ref={steady.ref}
            style={steady.style}
            className="m-0 flex list-none flex-col gap-2 p-0"
          >
            <AnimatePresence initial={false}>
              {paged.items.map((device) => (
                <motion.li
                  key={device.id}
                  variants={item}
                  initial="initial"
                  animate="animate"
                  exit="exit"
                  className="flex items-center gap-3 rounded-[14px] border border-line bg-body px-3 py-2.5"
                >
                  <span className="flex size-9 items-center justify-center rounded-[10px] border border-line bg-plate text-ink-2">
                    <Glyph name={device.kind?.startsWith("browser") ? "globe" : "tablet"} size={16} />
                  </span>
                  <div className="min-w-0 flex-1">
                    <div className="flex min-w-0 items-center gap-2">
                      <span className="truncate text-sm">{device.name}</span>
                      {device.kind ? <Pill>{kindLabel(device)}</Pill> : null}
                    </div>
                    <div className="font-mono text-[10px] tracking-[0.08em] text-muted uppercase">
                      Added {new Date(device.created_at * 1000).toLocaleDateString()}
                      {device.current ? <span className="text-ink"> · this browser</span> : null}
                    </div>
                  </div>
                  <HoldToConfirm size="sm" tone="danger" onConfirm={() => onRevoke(device.id)}>
                    Remove
                  </HoldToConfirm>
                </motion.li>
              ))}
            </AnimatePresence>
          </ul>
          <Pager {...paged} total={devices.length} onPage={setPage} />
        </>
      )}
    </Card>
  );
}
