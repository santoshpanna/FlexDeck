import { Label } from "../Plate/Plate.component.tsx";
import { cn } from "../../lib/cn.ts";

export interface EndpointProps {
  host: string;
  port: number | null;
  className?: string;
}

/**
 * The address and port a tablet types to find this computer, laid out like the
 * two fields it types them into, so each value can be read straight across.
 */
export function Endpoint({ host, port, className }: EndpointProps) {
  return (
    <div className={cn("grid w-full grid-cols-[minmax(0,1fr)_auto] gap-2.5", className)}>
      <div className="flex min-w-0 flex-col gap-1.5">
        <Label>Address</Label>
        <span className="truncate rounded-well border border-line bg-well px-3 py-2.5 font-mono text-base text-ink select-all">
          {host || "—"}
        </span>
      </div>
      <div className="flex flex-col gap-1.5">
        <Label>Port</Label>
        <span className="rounded-well border border-line bg-well px-3 py-2.5 font-mono text-base text-ink select-all">
          {port ?? "—"}
        </span>
      </div>
    </div>
  );
}
