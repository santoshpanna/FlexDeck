import { AnimatePresence, Reorder, motion, useDragControls } from "motion/react";
import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";

import { Glyph, cn, spring } from "@flexdeck/ui";

import type { Block } from "../../../../services/api/api.ts";
import { pluginLabel } from "../../lib/actionPicker.lib.ts";
import { argsText, duplicateBlock, insertBlock, literalValue, removeBlock, textArgs, valueText, type Branch } from "../../lib/flow.lib.ts";
import { ArgsFields } from "./ArgsFields.component.tsx";
import { ConditionEditor } from "./ConditionEditor.component.tsx";
import { DraftInput, ValueInput, VariableInput } from "./fields.tsx";
import { blockKey, inherit, useFlow } from "./flow.context.ts";
import { StepTag } from "./StepTag.component.tsx";

/**
 * A list of steps on a rail: numbered nodes down a dotted line, a card per
 * step, and a "+" between any two to insert there. Nested lists — an if's
 * branches, a repeat's body — are lanes of their own inside their card.
 *
 * Edits flow UP: a card hands back its new block, the lane hands back its new
 * list. Each level passes the edited block's identity on (`inherit`), so a card
 * keeps its place, its open state and its focus across the edit.
 */
export function FlowLane({
  blocks,
  onChange,
  empty = "Nothing yet.",
  depth = 0,
  traced,
  single = false,
}: {
  blocks: Block[];
  onChange: (blocks: Block[]) => void;
  empty?: string;
  depth?: number;
  /** Top-level steps a dry-run reached, by index, with what it saw. */
  traced?: readonly (string | null)[];
  /**
   * A key's gesture, where one action is the norm: while there is at most one
   * step, no numbered rail — just the action, and a quiet way to add another.
   */
  single?: boolean;
}) {
  const { pick } = useFlow();
  // The order while a step is being dragged: shown live, written once on drop,
  // so a drag spends one revision rather than one per step it passes.
  const [order, setOrder] = useState(blocks);
  const dragging = useRef(false);
  const plain = single && order.length <= 1;
  useEffect(() => {
    if (!dragging.current) setOrder(blocks);
  }, [blocks]);

  const insertAt = async (at: number) => {
    const block = await pick();
    if (block) onChange(insertBlock(blocks, [], at, block));
  };

  const dropped = () => {
    dragging.current = false;
    if (order.some((block, index) => block !== blocks[index])) onChange(order);
  };

  return (
    <div className="relative flex flex-col">
      {order.length > 0 && !plain ? (
        <div aria-hidden="true" className="absolute top-5 bottom-5 left-[15px] w-px bg-[repeating-linear-gradient(180deg,var(--color-line-strong)_0_4px,transparent_4px_8px)]" />
      ) : null}
      <Reorder.Group as="div" axis="y" values={order} onReorder={setOrder} className="flex flex-col">
        <AnimatePresence initial={false}>
          {order.map((block, index) => (
            <LaneItem
              key={blockKey(block)}
              block={block}
              index={index}
              depth={depth}
              trace={traced?.[index] ?? null}
              plain={plain}
              onInsert={() => void insertAt(index)}
              onDragStart={() => {
                dragging.current = true;
              }}
              onDrop={dropped}
              onChange={(next) => onChange(blocks.map((current) => (current === block ? inherit(block, next) : current)))}
              onRemove={() => onChange(removeBlock(blocks, [], blocks.indexOf(block)))}
              onDuplicate={() => onChange(duplicateBlock(blocks, [], blocks.indexOf(block)))}
            />
          ))}
        </AnimatePresence>
      </Reorder.Group>
      {plain && blocks.length > 0 ? (
        <button
          type="button"
          onClick={() => void insertAt(blocks.length)}
          className="mt-2 flex cursor-pointer items-center gap-1.5 self-start text-[12px] text-muted transition-colors hover:text-signal"
        >
          <Glyph name="plus" size={11} /> Also do something else
        </button>
      ) : (
      <button
        type="button"
        onClick={() => void insertAt(blocks.length)}
        className={cn(
          "group mt-2 flex h-10 cursor-pointer items-center gap-3 rounded-xl border border-dashed border-line-strong px-1.5 text-left transition-colors hover:border-signal hover:bg-signal-soft/40",
          blocks.length === 0 && "mt-0",
        )}
      >
        <span className="flex size-6 items-center justify-center rounded-full border border-dashed border-line-strong text-muted group-hover:border-signal group-hover:text-signal">
          <Glyph name="plus" size={12} />
        </span>
        <span className="font-mono text-[10px] tracking-[0.18em] text-muted uppercase group-hover:text-signal">
          {blocks.length === 0 ? empty : "Add step"} · ⌘K
        </span>
      </button>
      )}
    </div>
  );
}

/** One step on the rail: dragged by its grip only, so the fields inside it stay usable. */
function LaneItem({
  block,
  index,
  depth,
  trace,
  plain,
  onInsert,
  onDragStart,
  onDrop,
  onChange,
  onRemove,
  onDuplicate,
}: {
  block: Block;
  index: number;
  depth: number;
  trace: string | null;
  plain: boolean;
  onInsert: () => void;
  onDragStart: () => void;
  onDrop: () => void;
  onChange: (next: Block) => void;
  onRemove: () => void;
  onDuplicate: () => void;
}) {
  const controls = useDragControls();
  return (
    <Reorder.Item
      as="div"
      value={block}
      dragListener={false}
      dragControls={controls}
      onDragEnd={onDrop}
      initial={{ opacity: 0, y: -8, scale: 0.98 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, scale: 0.97, transition: { duration: 0.15 } }}
      whileDrag={{ scale: 1.02, zIndex: 20, boxShadow: "0 18px 40px var(--fd-key-cast)" }}
      transition={spring.panel}
      className="relative rounded-2xl"
    >
      {plain ? null : <Gap onInsert={onInsert} />}
      <div className="flex items-start gap-3">
        {plain ? null : (
        <span
          className={cn(
            "relative z-[1] mt-2.5 flex size-8 shrink-0 items-center justify-center rounded-[10px] border bg-body font-mono text-[10px]",
            trace ? "border-ok/50 text-ok" : "border-line text-muted",
          )}
        >
          {String(index + 1).padStart(2, "0")}
        </span>
        )}
        <div className="min-w-0 flex-1">
          <StepCard
            block={block}
            depth={depth}
            onChange={onChange}
            onRemove={onRemove}
            onDuplicate={onDuplicate}
            onGrab={(event) => {
              onDragStart();
              controls.start(event);
            }}
            trace={trace}
          />
        </div>
      </div>
    </Reorder.Item>
  );
}

/** The thin strip above each card: hover it for a "+" that inserts right there. */
function Gap({ onInsert }: { onInsert: () => void }) {
  return (
    <div className="group relative z-[2] flex h-3 items-center">
      <button
        type="button"
        aria-label="Insert a step here"
        onClick={onInsert}
        className="ml-[7px] flex size-[18px] cursor-pointer items-center justify-center rounded-full border border-dashed border-line-strong bg-body text-muted opacity-0 transition-all group-hover:opacity-100 hover:scale-110 hover:border-signal hover:text-signal focus-visible:opacity-100"
      >
        <Glyph name="plus" size={10} />
      </button>
    </div>
  );
}

function StepCard({
  block,
  depth,
  onChange,
  onRemove,
  onDuplicate,
  onGrab,
  trace,
}: {
  block: Block;
  depth: number;
  onChange: (next: Block) => void;
  onRemove: () => void;
  onDuplicate: () => void;
  /** A press on the grip: starts dragging this step. */
  onGrab: (event: ReactPointerEvent) => void;
  trace: string | null;
}) {
  const { catalogue, variables, pick } = useFlow();
  const writable = variables.filter((variable) => !variable.readOnly);

  /** Swap this step's action for another, keeping the arguments the two share. */
  const change = async () => {
    if (block.block !== "doAction") return;
    const next = await pick();
    if (!next) return;
    if (next.block !== "doAction") {
      onChange(next);
      return;
    }
    const keep = Object.fromEntries(
      Object.entries(block.args ?? {}).filter(([name]) =>
        catalogue.find((entry) => entry.plugin === next.plugin && entry.action === next.action)?.arguments.some((argument) => argument.name === name),
      ),
    );
    onChange({ block: "doAction", plugin: next.plugin, action: next.action, args: keep });
  };
  const definition =
    block.block === "doAction" ? (catalogue.find((entry) => entry.plugin === block.plugin && entry.action === block.action) ?? null) : null;

  let title: ReactNode;
  let body: ReactNode = null;
  switch (block.block) {
    case "doAction":
      title = (
        <button
          type="button"
          onClick={() => void change()}
          title="Choose a different action"
          className="group/change -mx-1.5 -my-1 flex w-full min-w-0 cursor-pointer items-center gap-2 rounded-lg px-1.5 py-1 text-left transition-colors hover:bg-plate-2"
        >
          <span className="min-w-0 flex-1">
            {definition ? (
              <>
                <span className="block font-mono text-[10px] tracking-[0.12em] text-muted uppercase">{pluginLabel(definition)}</span>
                <span className="block text-sm font-medium leading-snug">{definition.description || definition.action}</span>
              </>
            ) : (
              <span className="block text-sm text-warn">
                {block.plugin || "?"}:{block.action || "?"} — not on this desktop
              </span>
            )}
          </span>
          <span className="flex shrink-0 items-center gap-1 rounded-md border border-line px-1.5 py-0.5 font-mono text-[9px] tracking-[0.14em] text-muted uppercase opacity-0 transition-opacity group-hover/change:opacity-100 group-hover/card:opacity-70">
            Change
          </span>
        </button>
      );
      body = definition ? (
        <ArgsFields
          definition={definition}
          args={argsText(block.args ?? {})}
          onChange={(args) => onChange({ ...block, args: textArgs(args) })}
        />
      ) : null;
      break;
    case "wait": {
      const ms = "number" in block.ms ? block.ms.number : 0;
      title = "Wait";
      body = (
        <div className="flex items-center gap-3">
          <input
            type="range"
            min={0}
            max={10000}
            step={100}
            value={Math.min(ms, 10000)}
            aria-label="How long to wait"
            onChange={(event) => onChange({ ...block, ms: { number: Number(event.target.value) } })}
            className="h-1.5 flex-1 cursor-pointer accent-[var(--color-signal)]"
          />
          <DraftInput
            aria-label="Milliseconds"
            value={String(ms)}
            onCommit={(text) => onChange({ ...block, ms: { number: Math.max(0, Number(text) || 0) } })}
            className="w-20 text-right font-mono"
          />
          <span className="font-mono text-[11px] text-muted">ms</span>
        </div>
      );
      break;
    }
    case "setVariable":
      title = "Remember a value";
      body = (
        <div className="flex flex-wrap items-center gap-2">
          {/* Only what a step may change: the agent refuses read-only and unknown names. */}
          <VariableInput label="Variable" value={block.name} onCommit={(name) => onChange({ ...block, name })} variables={writable} className="w-auto min-w-[140px] flex-1" />
          <span className="text-[13px] text-muted">=</span>
          <ValueInput
            label="Value"
            value={valueText(block.value)}
            onCommit={(text) => onChange({ ...block, value: literalValue(text) })}
            variables={variables}
            placeholder="value or {{another}}"
            className="w-auto min-w-[140px] flex-1"
          />
        </div>
      );
      break;
    case "repeat": {
      const times = "number" in block.times ? block.times.number : 1;
      title = (
        <span className="flex items-center gap-2">
          Repeat
          <DraftInput
            aria-label="Times"
            value={String(times)}
            onCommit={(text) => onChange({ ...block, times: { number: Math.max(1, Math.round(Number(text) || 1)) } })}
            className="h-7 w-14 text-center font-mono"
          />
          times
        </span>
      );
      body = <Branchy block={block} branch="body" label="Each time" depth={depth} onChange={onChange} />;
      break;
    }
    case "if":
      title = "If";
      body = (
        <div className="flex flex-col gap-3">
          <ConditionEditor condition={block.condition} onChange={(condition) => onChange({ ...block, condition })} />
          <div className="grid gap-3 xl:grid-cols-2">
            <Branchy block={block} branch="then" label="Then" tone="ok" depth={depth} onChange={onChange} />
            <Branchy block={block} branch="otherwise" label="Otherwise" depth={depth} onChange={onChange} />
          </div>
        </div>
      );
      break;
  }

  return (
    <div
      className={cn(
        "group/card flex flex-col gap-3 rounded-2xl border bg-plate p-3.5 transition-colors",
        trace ? "border-ok/40" : "border-line",
        depth > 0 && "bg-plate-2",
      )}
    >
      <div className="flex items-center gap-3">
        <button
          type="button"
          aria-label="Drag to reorder"
          title="Drag to reorder"
          onPointerDown={onGrab}
          className="-ml-1 flex h-7 w-4 shrink-0 cursor-grab touch-none items-center justify-center text-faint hover:text-ink active:cursor-grabbing"
        >
          <svg width="8" height="14" viewBox="0 0 8 14" fill="currentColor" aria-hidden="true">
            <circle cx="2" cy="2" r="1.2" /><circle cx="6" cy="2" r="1.2" /><circle cx="2" cy="7" r="1.2" /><circle cx="6" cy="7" r="1.2" /><circle cx="2" cy="12" r="1.2" /><circle cx="6" cy="12" r="1.2" />
          </svg>
        </button>
        <StepTag kind={block.block} plugin={definition ? pluginLabel(definition) : undefined} />
        <div className="min-w-0 flex-1 text-sm leading-snug">{title}</div>
        <div className="flex items-center gap-0.5 opacity-40 transition-opacity group-hover/card:opacity-100 focus-within:opacity-100">
          <IconButton label="Duplicate step" onClick={onDuplicate}>
            <Glyph name="copy" size={13} />
          </IconButton>
          <IconButton label="Remove step" onClick={onRemove} danger>
            <Glyph name="close" size={13} />
          </IconButton>
        </div>
      </div>
      {body ? <div>{body}</div> : null}
      {trace ? <div className="border-t border-dashed border-ok/30 pt-2 font-mono text-[11px] text-ok">{trace}</div> : null}
    </div>
  );
}

function Branchy({
  block,
  branch,
  label,
  tone,
  depth,
  onChange,
}: {
  block: Extract<Block, { block: "if" | "repeat" }>;
  branch: Branch;
  label: string;
  tone?: "ok";
  depth: number;
  onChange: (next: Block) => void;
}) {
  const [open, setOpen] = useState(true);
  const list = block.block === "repeat" ? block.body : branch === "then" ? block.then : (block.otherwise ?? []);
  const set = (next: Block[]) => {
    if (block.block === "repeat") onChange({ ...block, body: next });
    else if (branch === "then") onChange({ ...block, then: next });
    else {
      const copy = { ...block };
      if (next.length > 0) copy.otherwise = next;
      else delete copy.otherwise;
      onChange(copy);
    }
  };
  return (
    <div className="flex min-w-0 flex-col gap-2 rounded-xl border border-dashed border-line-strong p-2.5">
      <button type="button" onClick={() => setOpen(!open)} className="flex cursor-pointer items-center justify-between">
        <span className={cn("lbl", tone === "ok" && "text-ok")}>
          {label} · {list.length}
        </span>
        <motion.span animate={{ rotate: open ? 0 : -90 }} transition={spring.panel} className="text-muted">
          <Glyph name="chevronDown" size={12} />
        </motion.span>
      </button>
      <AnimatePresence initial={false}>
        {open ? (
          <motion.div initial={{ height: 0, opacity: 0 }} animate={{ height: "auto", opacity: 1 }} exit={{ height: 0, opacity: 0 }} transition={spring.panel}>
            <FlowLane blocks={list} onChange={set} depth={depth + 1} empty={`Add a step to ${label.toLowerCase()}`} />
          </motion.div>
        ) : null}
      </AnimatePresence>
    </div>
  );
}

function IconButton({
  label,
  onClick,
  disabled,
  danger,
  children,
}: {
  label: string;
  onClick: () => void;
  disabled?: boolean;
  danger?: boolean;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      disabled={disabled}
      onClick={onClick}
      className={cn(
        "flex size-7 cursor-pointer items-center justify-center rounded-lg font-mono text-[12px] text-muted transition-colors hover:bg-plate-2 disabled:cursor-default disabled:opacity-30",
        danger ? "hover:text-bad" : "hover:text-ink",
      )}
    >
      {children}
    </button>
  );
}
