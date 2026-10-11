import {
  TEXT_PX,
  clockHands,
  clockText,
  countdownText,
  faceFor,
  timerLeft,
  isLive,
  placeFace,
  placeStyle,
  placeBox,
  placeZones,
  type Face,
  type FaceElement,
  type Rect,
} from "@flexdeck/face";
import { useEffect, useRef, useState, type CSSProperties } from "react";

import { cn } from "@flexdeck/ui";

import { Icon } from "../../../../common/components/Icon/Icon.component.tsx";
import { usePictureUrl } from "../../../../hooks/usePictureUrl/usePictureUrl.hook.ts";
import type { WidgetPreview, WireWidget } from "../../../../services/api/api.ts";
import { drawableColor } from "../../lib/face.lib.ts";

const SIGNAL = "#ff5a1f";
const TRACK = "#2a2a2f";

interface Props {
  widget: WireWidget;
  /** Which look to draw; absent: the one the agent says shows, else its own. */
  lookId?: string;
  w: number;
  h: number;
  /** The tablet's unit at this deck size: long edge / 1280, held to 0.7..1.4. */
  unit: number;
  radius: number;
  /** What the agent says the widget shows now. Absent: drawn from the config alone. */
  preview?: WidgetPreview;
  /** "Light up while" is on: the deck's glow, as the tablet draws it. */
  lit?: boolean;
}

/** A second's tick, for a clock that is showing. */
function useNow(active: boolean): Date {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    if (!active) return;
    const timer = setInterval(() => setNow(new Date()), 1000);
    return () => clearInterval(timer);
  }, [active]);
  return now;
}

/**
 * A key's face as the tablet draws it: its places from `placeFace`, its
 * values from the agent's preview. Everything is in the tablet's own pixels;
 * the grid preview scales the whole deck, never these numbers.
 *
 * The editor has no history for a graph and no live number for a gauge
 * until the agent says, so those show a sample — enough to see the shape
 * and the colour.
 */
export function FaceView({ widget, lookId, w, h, unit, radius, preview, lit }: Props) {
  const state = preview?.state;
  const look = lookId ?? state?.stateId ?? "";
  const face: Face = faceFor(widget, look || undefined);
  // A strip of zones is the widget's own, whatever look shows; the face sits above it.
  const zones = widget.face?.zones ?? [];
  const strip = placeZones(zones.length, w, h, unit);
  const places = placeFace(face.layout, w, strip.face.h, unit);
  const elements = face.elements ?? [];
  const hasClock = elements.some((e) => e.kind === "ELEMENT_KIND_CLOCK" || e.kind === "ELEMENT_KIND_TIMER");
  const now = useNow(hasClock);

  const background = face.background ?? {};
  const imageValue = state?.backgroundImage || (isLive(background.image) ? "" : (background.image ?? ""));
  const image = usePictureUrl(imageValue || null);
  const ruleColor = drawableColor(state?.background);
  const color = ruleColor ?? drawableColor(background.color);
  const colorTo = drawableColor(background.colorTo);

  const fill: CSSProperties = lit
    ? { background: "linear-gradient(180deg, var(--fd-deck-lit), #1c1310)" }
    : background.transparent && !ruleColor
      ? { background: "transparent", boxShadow: "none" }
      : color && colorTo
        ? { background: `linear-gradient(180deg, ${color}, ${colorTo})` }
        : color
          ? { background: color }
          : {};

  return (
    <div className="absolute inset-0 overflow-hidden" style={{ ...fill, borderRadius: radius }}>
      {image ? (
        <img
          src={image}
          alt=""
          className="absolute inset-0 size-full"
          style={{ objectFit: background.fit === "IMAGE_FIT_CONTAIN" ? "contain" : "cover" }}
        />
      ) : null}
      {image && (background.dim ?? 0) > 0 ? (
        <div className="absolute inset-0" style={{ background: `rgb(0 0 0 / ${Math.min(90, background.dim ?? 0) / 100})` }} />
      ) : null}
      {image && face.layout === "FACE_LAYOUT_ART" ? (
        <div className="absolute inset-0" style={{ background: "linear-gradient(180deg, transparent 40%, rgb(0 0 0 / 0.78))" }} />
      ) : null}
      {background.graph ? (
        <Sparkline rect={{ x: 0, y: h * 0.35, w, h: h * 0.65 }} color={drawableColor(background.graphColor) ?? SIGNAL} faint />
      ) : null}
      {state?.pulse ? (
        <div className="pointer-events-none absolute inset-0 animate-pulse border-[3px]" style={{ borderColor: SIGNAL, borderRadius: radius }} />
      ) : null}
      {elements.map((element, index) => {
        const value = state?.elements?.find((v) => (v.index ?? 0) === index);
        return (
          <Element
            key={index}
            element={element}
            rect={element.box ? placeBox(element.box, w, strip.face.h) : places[element.slot ?? "SLOT_TITLE"]}
            layout={face.layout}
            unit={unit}
            text={textOf(element, value?.text, state, widget)}
            number={value?.number}
            color={drawableColor(value?.color) ?? drawableColor(element.color) ?? (lit ? "#ffb089" : undefined)}
            pulse={value?.pulse ?? false}
            now={now}
          />
        );
      })}
      {zones.length > 0 ? (
        <div
          className="absolute flex border-t border-white/25"
          style={{ left: 0, top: strip.zones[0].y, width: w, height: strip.zones[0].h, background: "rgb(0 0 0 / 0.43)" }}
        >
          {zones.map((zone, index) => {
            const on = state?.zonesOn?.includes(zone.name) && zone.iconOn;
            const icon = on ? zone.iconOn : zone.icon;
            const side = Math.min(strip.zones[index].w, strip.zones[index].h) * 0.5;
            return (
              <div
                key={zone.name || index}
                title={zone.name}
                className={cn("flex flex-1 items-center justify-center", index > 0 && "border-l border-white/20")}
                style={{ color: lit ? "#ffb089" : "var(--fd-deck-ink)" }}
              >
                {icon ? <Icon name={icon} size={side} decorative /> : null}
              </div>
            );
          })}
        </div>
      ) : null}
    </div>
  );
}

/** What an element says: its template as written, or the agent's filling of it. */
function textOf(element: FaceElement, live: string | undefined, state: WidgetPreview["state"], widget: WireWidget): string {
  const kind = element.kind ?? "ELEMENT_KIND_TEXT";
  if (!isLive(element.template)) {
    const literal = element.template ?? "";
    // An unnamed key shows its id, as on the tablet.
    return literal || ((element.slot ?? "SLOT_TITLE") === "SLOT_TITLE" && kind === "ELEMENT_KIND_TEXT" ? widget.props?.label || widget.id : "");
  }
  if (live !== undefined) return live;
  if (kind === "ELEMENT_KIND_ICON") return state?.icon || widget.props?.icon || "";
  // Not asked yet: the template itself, so the author sees what is bound.
  return (element.template ?? "").replace(/\{\{\s*([^}]*?)\s*\}\}/g, "‹$1›");
}

function Element({
  element,
  rect,
  layout,
  unit,
  text,
  number,
  color,
  pulse,
  now,
}: {
  element: FaceElement;
  rect: Rect;
  layout: Face["layout"];
  unit: number;
  text: string;
  number: number | undefined;
  color: string | undefined;
  pulse: boolean;
  now: Date;
}) {
  const box: CSSProperties = { position: "absolute", left: rect.x, top: rect.y, width: rect.w, height: rect.h };
  const kind = element.kind ?? "ELEMENT_KIND_TEXT";
  const ink = color ?? "var(--fd-deck-ink)";
  const className = pulse ? "animate-pulse" : undefined;
  const range = (value: number | undefined) => {
    const [low, high] = (element.min ?? 0) === 0 && (element.max ?? 0) === 0 ? [0, 100] : [element.min ?? 0, element.max ?? 0];
    // The editor shows a sample until the agent says.
    const v = value ?? low + (high - low) * 0.62;
    return high > low ? Math.min(1, Math.max(0, (v - low) / (high - low))) : 0;
  };

  switch (kind) {
    case "ELEMENT_KIND_ICON": {
      const side = Math.min(rect.w, rect.h);
      return (
        <div className={className} style={{ ...box, display: "flex", alignItems: "center", justifyContent: "center", color: ink }}>
          {text ? <Icon name={text} size={side} strokeWidth={1.6} decorative /> : null}
        </div>
      );
    }
    case "ELEMENT_KIND_BAR": {
      const thickness = Math.min(rect.h, Math.max(4 * unit, Math.min(rect.h, rect.w) * 0.08));
      return (
        <div className={className} style={{ ...box }}>
          <div style={{ position: "absolute", left: 0, right: 0, bottom: 0, height: thickness, borderRadius: thickness / 2, background: TRACK }} />
          <div style={{ position: "absolute", left: 0, bottom: 0, height: thickness, width: `${range(number) * 100}%`, borderRadius: thickness / 2, background: color ?? SIGNAL }} />
        </div>
      );
    }
    case "ELEMENT_KIND_GAUGE": {
      const side = Math.min(rect.w, rect.h);
      const stroke = side * 0.1;
      const r = (side - stroke) / 2;
      const length = 2 * Math.PI * r * 0.75;
      return (
        <svg className={className} style={box} viewBox={`0 0 ${rect.w} ${rect.h}`} aria-hidden="true">
          <g transform={`translate(${rect.w / 2} ${rect.h / 2}) rotate(135)`}>
            <circle r={r} fill="none" stroke={TRACK} strokeWidth={stroke} strokeLinecap="round" strokeDasharray={`${length} ${2 * Math.PI * r}`} />
            <circle r={r} fill="none" stroke={color ?? SIGNAL} strokeWidth={stroke} strokeLinecap="round" strokeDasharray={`${length * range(number)} ${2 * Math.PI * r}`} />
          </g>
        </svg>
      );
    }
    case "ELEMENT_KIND_GRAPH":
      return <Sparkline rect={rect} color={color ?? SIGNAL} style={element.graph?.style} />;
    case "ELEMENT_KIND_TIMER":
      return <TimerWords rect={rect} element={element} layout={layout} unit={unit} ink={ink} left={number} running={text === "run"} now={now} />;
    case "ELEMENT_KIND_CLOCK": {
      const zone = element.clock?.zone ?? "";
      if (element.clock?.analog) {
        const side = Math.min(rect.w, rect.h);
        const r = side / 2;
        const hands = clockHands(now, zone);
        const hand = (degrees: number, length: number, width: number, stroke: string) => (
          <line x1={0} y1={0} x2={0} y2={-length} stroke={stroke} strokeWidth={width} strokeLinecap="round" transform={`rotate(${degrees})`} />
        );
        return (
          <svg style={box} viewBox={`0 0 ${rect.w} ${rect.h}`} aria-hidden="true">
            <g transform={`translate(${rect.w / 2} ${rect.h / 2})`} style={{ color: ink }}>
              <circle r={r - side * 0.025} fill="none" stroke="currentColor" strokeOpacity={0.35} strokeWidth={side * 0.025} />
              {Array.from({ length: 12 }, (_, tick) => (
                <line key={tick} x1={0} y1={-r * (tick % 3 === 0 ? 0.7 : 0.78)} x2={0} y2={-r * 0.86} stroke="currentColor" strokeOpacity={0.7} strokeWidth={side * 0.025} transform={`rotate(${tick * 30})`} />
              ))}
              {hand(hands.hour, r * 0.5, side * 0.06, "currentColor")}
              {hand(hands.minute, r * 0.74, side * 0.04, "currentColor")}
              {element.clock?.seconds ? hand(hands.second, r * 0.8, side * 0.015, SIGNAL) : null}
              <circle r={side * 0.04} fill="currentColor" />
            </g>
          </svg>
        );
      }
      const shown = clockText(now, element.clock?.format ?? "", zone, element.clock?.seconds ?? false);
      return (
        <Words
          rect={rect}
          text={shown}
          element={{ ...element, text: { font: "FONT_MONO", size: "TEXT_SIZE_FIT", ...element.text } }}
          layout={layout}
          unit={unit}
          ink={ink}
        />
      );
    }
    default:
      if ((element.slot ?? "SLOT_TITLE") === "SLOT_BADGE") {
        if (!text) return null;
        return (
          <div style={{ ...box, borderRadius: rect.h / 2, background: SIGNAL, display: "flex", alignItems: "center", justifyContent: "center" }}>
            <span style={{ fontSize: TEXT_PX.TEXT_SIZE_XS * unit, fontWeight: 700, color: color ?? "#fff", lineHeight: 1 }}>{text}</span>
          </div>
        );
      }
      return <Words rect={rect} text={text} element={element} layout={layout} unit={unit} ink={ink} className={className} />;
  }
}

function Words({
  rect,
  text,
  element,
  layout,
  unit,
  ink,
  className,
}: {
  rect: Rect;
  text: string;
  element: FaceElement;
  layout: Face["layout"];
  unit: number;
  ink: string;
  className?: string;
}) {
  const place = placeStyle(layout, element.slot ?? "SLOT_TITLE");
  const size = !element.text?.size || element.text.size === "TEXT_SIZE_AUTO" ? place.size : element.text.size;
  const maxLines = element.text?.maxLines || place.maxLines;
  const align = !element.text?.align || element.text.align === "TEXT_ALIGN_AUTO" ? place.align : element.text.align;
  const bold = element.text?.bold ?? place.bold;
  const fontSize =
    size === "TEXT_SIZE_FIT"
      ? Math.max(8 * unit, Math.min(rect.h * 0.8, rect.w / Math.max(1, text.length * 0.58)))
      : TEXT_PX[size] * unit;
  const family =
    element.text?.font === "FONT_MONO" ? "var(--font-mono, ui-monospace, monospace)" : element.text?.font === "FONT_SERIF" ? "Georgia, serif" : "inherit";
  // Too long, and asked to scroll: one line going round, as on the tablet.
  const wide = text.length * fontSize * 0.55;
  if (element.text?.marquee && size !== "TEXT_SIZE_FIT" && wide > rect.w) {
    const gap = rect.w * 0.35;
    const seconds = (wide + gap) / (36 * unit);
    return (
      <div className={className} style={{ position: "absolute", left: rect.x, top: rect.y, width: rect.w, height: rect.h, overflow: "hidden", display: "flex", alignItems: "center", color: ink, opacity: place.quiet && !element.color ? 0.66 : 1 }}>
        <span style={{ display: "inline-flex", whiteSpace: "nowrap", fontSize, fontWeight: bold ? 700 : 400, fontFamily: family, lineHeight: 1.15, animation: `fd-marquee ${seconds}s linear infinite` }}>
          <span style={{ paddingRight: gap }}>{text}</span>
          <span style={{ paddingRight: gap }} aria-hidden="true">
            {text}
          </span>
        </span>
      </div>
    );
  }
  return (
    <div
      className={className}
      style={{
        position: "absolute",
        left: rect.x,
        top: rect.y,
        width: rect.w,
        height: rect.h,
        display: "flex",
        alignItems: "center",
        justifyContent: align === "TEXT_ALIGN_START" ? "flex-start" : align === "TEXT_ALIGN_END" ? "flex-end" : "center",
        textAlign: align === "TEXT_ALIGN_START" ? "left" : align === "TEXT_ALIGN_END" ? "right" : "center",
        color: ink,
        opacity: place.quiet && !element.color ? 0.66 : 1,
      }}
    >
      <span
        style={{
          fontSize,
          fontWeight: bold ? 700 : 400,
          fontFamily: family,
          lineHeight: 1.15,
          overflow: "hidden",
          display: "-webkit-box",
          WebkitLineClamp: maxLines,
          WebkitBoxOrient: "vertical",
          wordBreak: "break-word",
          whiteSpace: maxLines === 1 ? "nowrap" : undefined,
          textOverflow: "ellipsis",
          maxWidth: "100%",
        }}
      >
        {text}
      </span>
    </div>
  );
}

/** A sample history: the editor has none, so this shows the shape and colour. */
/** A countdown: from what the agent last said, counted down here, as on the tablet. */
function TimerWords({
  rect,
  element,
  layout,
  unit,
  ink,
  left,
  running,
  now,
}: {
  rect: Rect;
  element: FaceElement;
  layout: Face["layout"];
  unit: number;
  ink: string;
  left: number | undefined;
  running: boolean;
  now: Date;
}) {
  // When this value arrived: the count runs from then.
  const sent = useRef({ left, running, at: Date.now() });
  if (sent.current.left !== left || sent.current.running !== running) sent.current = { left, running, at: Date.now() };
  // Not asked yet: a full pomodoro, so the author sees the shape.
  const shown = left === undefined ? "25:00" : countdownText(timerLeft(left, running, sent.current.at, now.getTime()));
  return <Words rect={rect} text={shown} element={{ ...element, text: { font: "FONT_MONO", size: "TEXT_SIZE_FIT", ...element.text } }} layout={layout} unit={unit} ink={ink} />;
}

function Sparkline({ rect, color, faint, style }: { rect: Rect; color: string; faint?: boolean; style?: string }) {
  const points = Array.from({ length: 24 }, (_, i) => 0.5 + 0.28 * Math.sin(i / 2.2) + 0.12 * Math.sin(i * 1.7));
  const x = (i: number) => (i / (points.length - 1)) * rect.w;
  const y = (v: number) => rect.h - v * rect.h;
  if (style === "GRAPH_STYLE_BARS") {
    const w = rect.w / points.length;
    return (
      <svg style={{ position: "absolute", left: rect.x, top: rect.y, width: rect.w, height: rect.h }} viewBox={`0 0 ${rect.w} ${rect.h}`} aria-hidden="true">
        {points.map((v, i) => (
          <rect key={i} x={i * w + w * 0.15} y={y(v)} width={w * 0.7} height={rect.h - y(v)} fill={color} opacity={faint ? 0.3 : 0.9} />
        ))}
      </svg>
    );
  }
  const line = points.map((v, i) => `${i === 0 ? "M" : "L"}${x(i).toFixed(1)},${y(v).toFixed(1)}`).join(" ");
  const id = `spark-${Math.round(rect.x)}-${Math.round(rect.y)}-${Math.round(rect.w)}`;
  return (
    <svg style={{ position: "absolute", left: rect.x, top: rect.y, width: rect.w, height: rect.h }} viewBox={`0 0 ${rect.w} ${rect.h}`} aria-hidden="true">
      <defs>
        <linearGradient id={id} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor={color} stopOpacity={faint ? 0.35 : 0.45} />
          <stop offset="1" stopColor={color} stopOpacity={0} />
        </linearGradient>
      </defs>
      {style !== "GRAPH_STYLE_LINE" ? <path d={`${line} L${rect.w},${rect.h} L0,${rect.h} Z`} fill={`url(#${id})`} /> : null}
      <path d={line} fill="none" stroke={color} strokeOpacity={faint ? 0.55 : 1} strokeWidth={2} />
    </svg>
  );
}
