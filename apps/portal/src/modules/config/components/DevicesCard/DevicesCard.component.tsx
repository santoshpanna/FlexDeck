import { AnimatePresence, motion } from "motion/react";

import { Glyph, HoldToConfirm, item } from "@flexdeck/ui";

import { Card } from "../../../../common/components/Card/Card.component.tsx";
import type { Device } from "../../../../services/api/api.ts";

export interface DevicesCardProps {
  devices: Device[];
  onRevoke: (deviceId: string) => void;
}

/** The tablets and browsers paired to this desktop. Removing one is a hold, not a tap. */
export function DevicesCard({ devices, onRevoke }: DevicesCardProps) {
  return (
    <Card title={`Devices · ${devices.length}`}>
      {devices.length === 0 ? (
        <p className="m-0 text-sm text-muted">None yet.</p>
      ) : (
        <ul className="m-0 flex list-none flex-col gap-2 p-0">
          <AnimatePresence initial={false}>
            {devices.map((device) => (
              <motion.li
                key={device.id}
                variants={item}
                initial="initial"
                animate="animate"
                exit="exit"
                className="flex items-center gap-3 rounded-[14px] border border-line bg-body px-3 py-2.5"
              >
                <span className="flex size-9 items-center justify-center rounded-[10px] border border-line bg-plate text-ink-2">
                  <Glyph name="tablet" size={16} />
                </span>
                <div className="min-w-0 flex-1">
                  <div className="truncate text-sm">{device.name}</div>
                  <div className="font-mono text-[10px] tracking-[0.08em] text-muted">
                    ADDED {new Date(device.created_at * 1000).toLocaleDateString()}
                  </div>
                </div>
                <HoldToConfirm size="sm" tone="danger" onConfirm={() => onRevoke(device.id)}>
                  Remove
                </HoldToConfirm>
              </motion.li>
            ))}
          </AnimatePresence>
        </ul>
      )}
    </Card>
  );
}
