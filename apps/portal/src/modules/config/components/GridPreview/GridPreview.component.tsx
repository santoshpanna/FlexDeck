import { computeGrid, layoutPage, rectOf, type Orientation } from "@flexdeck/grid";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";

import { CropMarks, Segmented, cn, spring } from "@flexdeck/ui";

import { Icon } from "../../../../common/components/Icon/Icon.component.tsx";
import { useElementWidth } from "../../../../hooks/useElementWidth/useElementWidth.hook.ts";
import type { WidgetPreview, WirePage, WireProfile, WireWidget } from "../../../../services/api/api.ts";
import { FaceView } from "../FaceView/FaceView.component.tsx";
import { useGridDrag } from "../../hooks/useGridDrag/useGridDrag.hook.ts";
import { labelOf } from "../../lib/config.lib.ts";
import { backgroundCss, faceColors } from "../../lib/face.lib.ts";
import { usePictureUrl } from "../../../../hooks/usePictureUrl/usePictureUrl.hook.ts";
import { emptyCells } from "../../lib/interaction.lib.ts";
import { layoutFor } from "../../lib/orientation.lib.ts";
import { sliderReading } from "../../lib/slider.lib.ts";
import type { WidgetState } from "../../lib/state.lib.ts";

/**
 * A slider as the tablet draws it: name on top, a recessed track filled to the
 * value in signal orange, a machined thumb. The value is the agent's live one —
 * the same the tablet shows, pushed as it changes.
 */
function SliderFace({
  widget,
  name,
  vertical,
  fontSize,
  live,
}: {
  widget: WireWidget;
  name: string;
  vertical: boolean;
  fontSize: number;
  live: number | undefined;
}) {
  const reading = sliderReading(widget.slider, live);
  const percent = `${reading.fraction * 100}%`;
  return (
    <div className="absolute inset-0 flex flex-col gap-[0.5em] p-[0.6em] text-[var(--fd-deck-ink)]" style={{ fontSize }}>
      <span className="flex items-center justify-between gap-[0.4em]">
        <span className="flex min-w-0 items-center gap-[0.4em]">
          {widget.props?.icon ? <Icon name={widget.props.icon} size={fontSize * 1.1} decorative /> : null}
          <span className="truncate">{name}</span>
        </span>
        <span className="font-mono text-[0.85em] text-[#ff8a5c]">{reading.text}</span>
      </span>
      <span
        aria-hidden="true"
        className="relative flex-1 overflow-hidden rounded-[0.9em] border border-[var(--fd-deck-key-line)] bg-[var(--fd-deck)] shadow-[inset_0_0.2em_0.8em_rgb(0_0_0/0.85)]"
      >
        <span
          className="absolute bottom-0 left-0 bg-linear-to-t from-[rgb(255_90_31/0.35)] to-[rgb(255_90_31/0.85)]"
          style={vertical ? { height: percent, width: "100%" } : { width: percent, height: "100%" }}
        />
        <span
          className="absolute rounded-[0.35em] bg-linear-to-b from-[#f4f3ee] to-[#b9b8b2] shadow-[0_0.15em_0.4em_rgb(0_0_0/0.6)]"
          style={
            vertical
              ? { left: "8%", right: "8%", height: "1.1em", bottom: percent, transform: "translateY(50%)" }
              : { top: "8%", bottom: "8%", width: "1.1em", left: percent, transform: "translateX(-50%)" }
          }
        />
      </span>
    </div>
  );
}

/** The page's own background, behind every key, as the tablet draws it. */
function PageBackground({ page }: { page: WirePage | null }) {
  const background = page?.background;
  const url = usePictureUrl(background?.image && !background.image.includes("{{") ? background.image : null);
  const paint = backgroundCss(background);
  if (!background || (!paint && !url)) return null;
  return (
    <div
      aria-hidden="true"
      className="absolute inset-0"
      style={{ background: paint ?? undefined }}
    >
      {url ? (
        <img src={url} alt="" className="absolute inset-0 size-full" style={{ objectFit: background.fit === "IMAGE_FIT_CONTAIN" ? "contain" : "cover" }} />
      ) : null}
      {url && (background.dim ?? 0) > 0 ? <div className="absolute inset-0" style={{ background: `rgb(0 0 0 / ${Math.min(90, background.dim ?? 0) / 100})` }} /> : null}
    </div>
  );
}

/** The look the "on"/"off" preview switch means for this widget, if it has it. */
function lookForProbe(widget: WireWidget, probe: "on" | "off"): string | undefined {
  return widget.states?.looks?.some((look) => look.id === probe) ? probe : undefined;
}

/** Whether a slider in this rectangle runs up rather than across. */
function sliderIsVertical(widget: WireWidget, rect: { w: number; h: number }): boolean {
  switch (widget.slider?.orientation) {
    case "SLIDER_ORIENTATION_VERTICAL":
      return true;
    case "SLIDER_ORIENTATION_HORIZONTAL":
      return false;
    default:
      return rect.h > rect.w;
  }
}

interface Props {
  profile: WireProfile | null;
  page: WirePage | null;
  /** Which layout is shown — and edited. Moves, resizes and adds are in its cells. */
  orientation: Orientation;
  onOrientation: (orientation: Orientation) => void;
  /** Names the figure for assistive tech. The page is named in the pager strip. */
  caption: string;
  selectedId?: string | null;
  onSelect?: (id: string) => void;
  /** Open a widget's settings — a click on it, or its hover "edit" action. */
  onEdit?: (widgetId: string) => void;
  /** Delete one widget — its hover action. */
  onDelete?: (widgetId: string) => void;
  /** How the selected widget should be drawn while authoring. */
  previewState?: WidgetState;
  /** Drawn in the bezel under the screen: the tablet's own status strip. */
  footer?: ReactNode;
  /** What each widget shows now, by id, as the agent works it out. */
  previews?: Record<string, WidgetPreview>;
  /** The state the selected widget is drawn in while editing; empty for whichever applies. */
  previewLook?: string;
  /** In the SHOWN orientation's cells. */
  onMove?: (widgetId: string, col: number, row: number) => void;
  onResize?: (widgetId: string, w: number, h: number) => void;
  onAdd?: (col: number, row: number) => void;
}

/// The largest the device-accurate screen is drawn while authoring.
const MAX_SCALE = 0.75;
/// The bezel's padding, in screen pixels, on each side of the deck.
const BEZEL = 18;

/**
 * Render a page with the *shared* layout algorithm, as the tablet will.
 *
 * The grid package is the reason this preview can be trusted: the rectangles
 * here and the tablet's come from the same rules and fixtures. The scale is a
 * CSS transform on the rendered grid, never on the numbers the layout works
 * from. Keys are painted in the DEVICE's colours (`--fd-deck-*`), not the
 * editor's, so the preview cannot flatter the screen.
 */
export function GridPreview({ profile, page, orientation, onOrientation, caption, selectedId, onSelect, onEdit, onDelete, previewState, previews, previewLook, onMove, onResize, onAdd, footer }: Props) {
  // Each orientation is its own layout: its grid, and each widget's place in it
  // — chosen, or assumed until someone moves it (`placePage`). Everything below
  // works in the cells shown, and the edits are made in them too.
  const shown = layoutFor(profile, page, orientation);
  const viewport = useViewportHeight();
  const placedIds = new Set(shown.widgets.map((span) => span.id));
  const assumed = new Set(shown.widgets.filter((span) => span.assumed).map((span) => span.id));
  const widgets = (page?.widgets ?? []).filter((widget) => placedIds.has(widget.id));
  const rects = layoutPage(shown.spec, shown.widgets);
  const geometry = computeGrid(shown.spec);
  const orientedById = new Map(shown.widgets.map((span) => [span.id, span] as const));
  const { ref, width } = useElementWidth<HTMLElement>();
  const frameRef = useRef<HTMLDivElement>(null);

  const forDrag = (widget: WireWidget): WireWidget => {
    const oriented = orientedById.get(widget.id);
    return oriented ? { ...widget, ...oriented } : widget;
  };

  // Fit the frame's width AND the window's height: a portrait deck fitted to
  // width alone runs off the bottom of the screen.
  const room = Math.max(0, width - BEZEL * 2);
  const tall = Math.max(240, viewport - 250);
  const scale =
    room > 0 && shown.spec.usableW > 0 && shown.spec.usableH > 0
      ? Math.min(MAX_SCALE, room / shown.spec.usableW, tall / shown.spec.usableH)
      : MAX_SCALE;

  const drag = useGridDrag({
    spec: shown.spec,
    frameRef,
    scale,
    onSelect: (id) => onSelect?.(id),
    onTap: (id) => onEdit?.(id),
    onMove: (id, col, row) => onMove?.(id, col, row),
    onResize: (id, w, h) => onResize?.(id, w, h),
  });

  const screenW = shown.spec.usableW * scale;
  const screenH = shown.spec.usableH * scale;

  // The tablet's label size, in GRID pixels: 16px × long edge / 1280, held to
  // 0.7..1.4, exactly as `DeckView.show` computes it. Radius follows the same scale.
  const longEdge = Math.max(shown.spec.usableW, shown.spec.usableH);
  const unit = longEdge > 0 ? Math.min(1.4, Math.max(0.7, longEdge / 1280)) : 1;
  const labelSize = 16 * unit;
  const radius = 16 * unit;

  const gaps = drag.candidate ? [] : emptyCells(shown.spec, shown.widgets);
  const ghost = drag.candidate ? rectOf(geometry, drag.candidate) : null;

  return (
    <figure ref={ref} aria-label={caption} className="m-0 flex flex-col items-center gap-3">
      <figcaption className="flex w-full items-center justify-between gap-3" style={{ maxWidth: screenW + BEZEL * 2 }}>
        <span className="font-mono text-[10px] tracking-[0.16em] text-muted">
          {shown.spec.cols} × {shown.spec.rows} · {widgets.length} {widgets.length === 1 ? "KEY" : "KEYS"}
        </span>
        <Segmented
          size="sm"
          label="Panel orientation"
          value={orientation}
          onChange={onOrientation}
          options={[
            { value: "landscape", label: "Landscape" },
            { value: "portrait", label: "Portrait" },
          ]}
        />
      </figcaption>

      <motion.div
        layout
        transition={spring.soft}
        className="relative rounded-[28px] border border-line bg-linear-to-b from-[#202024] to-[var(--fd-deck-bezel)] shadow-[inset_0_1px_0_rgb(255_255_255/0.06),0_30px_60px_var(--fd-key-cast)]"
        style={{ padding: BEZEL }}
      >
        <CropMarks inset={-10} className="border-line-strong" />
        <div
          ref={frameRef}
          className="group/frame relative touch-none overflow-hidden rounded-[14px] select-none"
          style={{
            width: screenW,
            height: screenH,
            background: "var(--fd-deck)",
            backgroundImage: "radial-gradient(var(--fd-deck-dot) 1px, transparent 1px)",
            backgroundSize: `${Math.max(8, 22 * scale)}px ${Math.max(8, 22 * scale)}px`,
            boxShadow: "inset 0 2px 14px rgb(0 0 0 / 0.8)",
          }}
          onDragStart={(event) => event.preventDefault()}
          {...drag.frameHandlers}
        >
          <div
            className="absolute top-0 left-0 origin-top-left"
            style={{ width: shown.spec.usableW, height: shown.spec.usableH, transform: `scale(${scale})`, "--preview-scale": scale } as CSSProperties}
          >
            <PageBackground page={page} />
            {gaps.map((cell) => {
              const rect = rectOf(geometry, { id: "", col: cell.col, row: cell.row, w: 1, h: 1 });
              return (
                <button
                  key={`gap-${cell.col},${cell.row}`}
                  type="button"
                  className="absolute flex cursor-pointer items-center justify-center border border-dashed border-transparent text-transparent transition-colors group-hover/frame:border-[var(--fd-deck-key-line)] group-hover/frame:text-[var(--fd-deck-muted)] hover:!border-signal hover:!bg-[rgb(255_90_31/0.06)] hover:!text-signal"
                  style={{ left: rect.x, top: rect.y, width: rect.w, height: rect.h, borderRadius: radius }}
                  title="Add a key here"
                  aria-label={`add a key at column ${cell.col + 1}, row ${cell.row + 1}`}
                  onClick={() => onAdd?.(cell.col, cell.row)}
                >
                  <span className="flex" style={{ transform: `scale(${1 / scale})` }}>
                    <Icon name="plus" size={16} decorative />
                  </span>
                </button>
              );
            })}

            <AnimatePresence initial={false}>
              {widgets.map((widget) => {
                const rect = rects[widget.id];
                if (!rect) return null;
                const selected = widget.id === selectedId;
                const name = labelOf(widget);
                const probe = selected && previewState && previewState !== "idle" ? previewState : null;
                // Lit while its look is "on" now — the scene key whose scene is
                // live — unless a look colours it, which then speaks for itself.
                const active = previews?.[widget.id]?.state?.stateId === "on" && !previews?.[widget.id]?.state?.background;
                const lit = probe === "on" || (!probe && active);
                const face = faceColors(widget, lit);
                // A transparent face shows the page behind it: no key at all.
                const see = widget.face?.background?.transparent === true && !previews?.[widget.id]?.state?.background;
                const unavailable = previews?.[widget.id]?.state?.unavailable;
                return (
                  <motion.div
                    key={widget.id}
                    initial={{ opacity: 0, scale: 0.85 }}
                    animate={{ opacity: probe === "off" ? 0.45 : probe === "disabled" ? 0.35 : 1, scale: probe === "pressed" ? 0.96 : 1, left: rect.x, top: rect.y, width: rect.w, height: rect.h }}
                    exit={{ opacity: 0, scale: 0.85 }}
                    transition={spring.panel}
                    className="group/key absolute cursor-move touch-none"
                    style={{ left: rect.x, top: rect.y, width: rect.w, height: rect.h }}
                    onPointerDown={(event) => drag.startMove(forDrag(widget), event)}
                  >
                    <div
                      className={cn("absolute inset-0 overflow-hidden text-center text-[var(--fd-deck-ink)]", probe === "disabled" && "grayscale", unavailable && "opacity-40 grayscale")}
                      style={{
                        borderRadius: radius,
                        ...(face.ink ? { color: face.ink } : {}),
                        background: see
                          ? "transparent"
                          : lit
                            ? "linear-gradient(180deg, var(--fd-deck-lit), #1c1310)"
                            : (face.background ?? "linear-gradient(180deg, var(--fd-deck-key-top), var(--fd-deck-key-bottom))"),
                        boxShadow: see ? "none" : `inset 0 1px 0 rgb(255 255 255 / 0.07), 0 ${4 * unit}px 0 var(--fd-deck-key-base)`,
                      }}
                    >
                      {widget.type === "WIDGET_TYPE_SLIDER" ? (
                        <SliderFace
                          widget={widget}
                          name={name}
                          vertical={sliderIsVertical(widget, rect)}
                          fontSize={labelSize}
                          live={previews?.[widget.id]?.state?.value}
                        />
                      ) : (
                        <FaceView
                          widget={widget}
                          lookId={selected && previewLook ? previewLook : probe === "on" || probe === "off" ? lookForProbe(widget, probe) : undefined}
                          w={rect.w}
                          h={rect.h}
                          unit={unit}
                          radius={radius}
                          preview={previews?.[widget.id]}
                          lit={lit && !widget.face}
                        />
                      )}
                      {/* The top-left corner holds every marker, in one row so they never
                          sit on each other or on the label along the bottom. */}
                      {previews?.[widget.id]?.state?.problem || previews?.[widget.id]?.error || (orientation === "portrait" && assumed.has(widget.id)) ? (
                        <span className="absolute flex items-center" style={{ left: 8 * unit, top: 8 * unit, gap: 4 * unit }}>
                          {previews?.[widget.id]?.state?.problem ? (
                            <span
                              title={previews[widget.id]?.state?.problem}
                              aria-label={previews[widget.id]?.state?.problem}
                              className="rounded-full bg-[#fbbf24]"
                              style={{ width: 7 * unit, height: 7 * unit, margin: 2 * unit }}
                            />
                          ) : null}
                          {previews?.[widget.id]?.error ? (
                            <span
                              title={previews[widget.id]?.error}
                              className="rounded-[4px] bg-[#7f1d1d] font-mono text-white"
                              style={{ fontSize: 9 * unit, padding: `${1 * unit}px ${4 * unit}px`, letterSpacing: "0.14em" }}
                            >
                              FIX
                            </span>
                          ) : null}
                          {orientation === "portrait" && assumed.has(widget.id) ? (
                            <span
                              title="Placed for you from landscape. Drag it to choose its place."
                              className="rounded-[4px] border border-dashed border-[var(--fd-deck-muted)] font-mono text-[var(--fd-deck-muted)]"
                              style={{ fontSize: 9 * unit, padding: `${1 * unit}px ${4 * unit}px`, letterSpacing: "0.14em" }}
                            >
                              AUTO
                            </span>
                          ) : null}
                        </span>
                      ) : null}
                      {lit && !widget.props?.hideDot ? (
                        <span
                          aria-hidden="true"
                          className="absolute rounded-full"
                          style={{
                            top: 10 * unit,
                            right: 10 * unit,
                            width: 7 * unit,
                            height: 7 * unit,
                            background: "#4ade80",
                            boxShadow: `0 0 ${8 * unit}px rgb(74 222 128 / 0.7)`,
                          }}
                        />
                      ) : null}
                      {see ? null : (
                        <span
                          aria-hidden="true"
                          className="pointer-events-none absolute inset-0 border"
                          style={{ borderRadius: radius, borderColor: lit ? "var(--fd-deck-lit-line)" : "var(--fd-deck-key-line)" }}
                        />
                      )}
                    </div>
                      {unavailable ? (
                        <span title={unavailable} aria-label={unavailable} className="pointer-events-none absolute inset-0 flex items-center justify-center">
                          <svg viewBox="0 0 24 24" width={28 * unit} height={28 * unit} fill="none" stroke="#fbbf24" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                            <path d="M12 3 2.5 20h19L12 3Z" />
                            <path d="M12 10v4.5M12 17.5v.01" />
                          </svg>
                        </span>
                      ) : null}

                    {selected ? (
                      <motion.div
                        layoutId="key-selection"
                        transition={spring.panel}
                        className="pointer-events-none absolute -inset-[3px] border-2 border-signal shadow-[0_0_0_4px_rgb(255_90_31/0.18)]"
                        style={{ borderRadius: radius + 3 }}
                      />
                    ) : null}

                    <div
                      className="absolute top-0 right-0 flex origin-top-right gap-0.5 p-0.5 opacity-0 transition-opacity group-hover/key:opacity-100 focus-within:opacity-100"
                      style={{ transform: `scale(${1 / scale})` }}
                    >
                      <button
                        type="button"
                        className="flex size-6 cursor-pointer items-center justify-center rounded-md bg-[var(--fd-deck-ink)] text-[#111]"
                        title={`Edit ${name}`}
                        aria-label={`Edit ${name}`}
                        onPointerDown={(event) => event.stopPropagation()}
                        onClick={() => onEdit?.(widget.id)}
                      >
                        <Icon name="pencil" size={13} decorative />
                      </button>
                      <button
                        type="button"
                        className="flex size-6 cursor-pointer items-center justify-center rounded-md bg-[var(--fd-deck-ink)] text-[#111] hover:bg-bad hover:text-white"
                        title={`Delete ${name}`}
                        aria-label={`Delete ${name}`}
                        onPointerDown={(event) => event.stopPropagation()}
                        onClick={() => onDelete?.(widget.id)}
                      >
                        <Icon name="trash" size={13} decorative />
                      </button>
                    </div>

                    <span
                      role="presentation"
                      title="Drag to resize"
                      className={cn(
                        "absolute right-0 bottom-0 size-4 origin-bottom-right cursor-nwse-resize touch-none opacity-0 transition-opacity group-hover/key:opacity-100",
                        selected && "opacity-100",
                      )}
                      style={{
                        transform: `scale(${1 / scale})`,
                        background: "linear-gradient(135deg, transparent 50%, #ff5a1f 50%)",
                        borderBottomRightRadius: 6,
                      }}
                      onPointerDown={(event) => drag.startResize(forDrag(widget), event)}
                    />
                  </motion.div>
                );
              })}
            </AnimatePresence>

            {ghost ? (
              <motion.div
                className="pointer-events-none absolute border-2 border-dashed border-signal bg-[rgb(255_90_31/0.08)]"
                initial={false}
                animate={{ left: ghost.x, top: ghost.y, width: ghost.w, height: ghost.h }}
                transition={spring.key}
                style={{ borderRadius: radius }}
              />
            ) : null}
          </div>
        </div>
        {footer ? <div className="pt-3">{footer}</div> : null}
      </motion.div>
    </figure>
  );
}

/** The window's height, kept current, so the deck can fit it. */
function useViewportHeight(): number {
  const [height, setHeight] = useState(() => (typeof window === "undefined" ? 900 : window.innerHeight));
  useEffect(() => {
    const update = () => setHeight(window.innerHeight);
    window.addEventListener("resize", update);
    return () => window.removeEventListener("resize", update);
  }, []);
  return height;
}
