import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";

import type { ActionDefinition, Block, VariableChoice } from "../../../../services/api/api.ts";
import type { LoadChoices } from "../../hooks/useArgumentChoices/useArgumentChoices.hook.ts";
import { FlowContext, type FlowEnvironment } from "./flow.context.ts";
import { StepPalette } from "./StepPalette.component.tsx";

/**
 * Hosts one step palette for every lane inside it. A lane asks for a step with
 * `pick()` and gets the chosen block back (or `null`), so the palette is one
 * modal, not one per "+". ⌘K / Ctrl+K opens it to append to the top level.
 */
export function FlowProvider({
  catalogue,
  variables,
  loadChoices,
  onAppend,
  children,
}: {
  catalogue: readonly ActionDefinition[];
  variables: readonly VariableChoice[];
  loadChoices?: LoadChoices;
  /** What ⌘K adds a step to. Without it the shortcut does nothing. */
  onAppend?: (block: Block) => void;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const resolver = useRef<((block: Block | null) => void) | null>(null);

  const pick = useCallback(
    () =>
      new Promise<Block | null>((resolve) => {
        resolver.current?.(null);
        resolver.current = resolve;
        setOpen(true);
      }),
    [],
  );

  const settle = (block: Block | null) => {
    setOpen(false);
    resolver.current?.(block);
    resolver.current = null;
  };

  const append = useRef(onAppend);
  append.current = onAppend;
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k" && append.current) {
        event.preventDefault();
        void pick().then((block) => block && append.current?.(block));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [pick]);

  const value = useMemo<FlowEnvironment>(() => ({ catalogue, variables, loadChoices, pick }), [catalogue, variables, loadChoices, pick]);

  return (
    <FlowContext.Provider value={value}>
      {children}
      <StepPalette open={open} catalogue={catalogue} onPick={(block) => settle(block)} onClose={() => settle(null)} />
    </FlowContext.Provider>
  );
}
