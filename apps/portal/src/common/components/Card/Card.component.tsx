import type { ReactNode } from "react";

import { cn } from "@flexdeck/ui";

/**
 * A panel on the device body: a mono label for a title, actions opposite it,
 * and the content beneath.
 */
export function Card({
  title,
  actions,
  children,
  className,
}: {
  title?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section className={cn("flex flex-col gap-4 rounded-plate border border-line bg-plate p-5", className)}>
      {title || actions ? (
        <header className="flex flex-wrap items-center justify-between gap-3">
          {title ? <h2 className="lbl m-0">{title}</h2> : null}
          {actions ? <div className="flex items-center gap-2">{actions}</div> : null}
        </header>
      ) : null}
      {children}
    </section>
  );
}
