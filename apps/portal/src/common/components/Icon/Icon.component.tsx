import { useContext, useEffect, useState } from "react";

import { usePictureUrl } from "../../../hooks/usePictureUrl/usePictureUrl.hook.ts";
import { ApiContext } from "../../../providers/ApiProvider/ApiProvider.provider.tsx";
import { DesktopContext } from "../../../providers/DesktopProvider/DesktopProvider.provider.tsx";
import type { IconPackIcon } from "../../../services/api/api.ts";
import { iconByName } from "../../icons/icons.lib.ts";

/** The prefix of a picture a plugin handed the desktop: `img:<sha256>`. */
const PICTURE = "img:";

/** The prefix of an installed icon pack's icon: `pack:<pack>/<name>`. */
const PACK = "pack:";

export interface IconProps {
  name: string;
  size?: number;
  /**
   * Stroke width on the 24px grid. Match it to the adjacent text weight — 1.5
   * beside regular text, 2 beside semibold. Defaults to the regular-text value.
   */
  strokeWidth?: number;
  /**
   * A decorative icon is hidden from assistive technology. Use it where the
   * surrounding control or text already names the icon, so the name is not
   * announced twice.
   */
  decorative?: boolean;
}

/**
 * A line icon from the bundled set.
 *
 * An unknown name renders a dashed placeholder box rather than nothing: a widget
 * whose icon silently disappears is indistinguishable from a rendering bug, and
 * the placeholder says "this name is not in the set" instead.
 */
export function Icon({
  name,
  size = 20,
  strokeWidth = 1.5,
  decorative = false,
}: IconProps) {
  if (name.startsWith(PICTURE)) {
    return <Picture sha={name.slice(PICTURE.length)} size={size} decorative={decorative} />;
  }
  if (name.startsWith(PACK)) {
    return <PackIcon reference={name.slice(PACK.length)} size={size} strokeWidth={strokeWidth} decorative={decorative} />;
  }
  const definition = iconByName(name);

  if (!definition) {
    return (
      <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true">
        <rect
          x="4"
          y="4"
          width="16"
          height="16"
          fill="none"
          stroke="currentColor"
          strokeWidth={1.4}
          strokeDasharray="3 3"
        />
      </svg>
    );
  }

  const labelled = decorative
    ? { "aria-hidden": true as const }
    : { role: "img" as const, "aria-label": definition.title };

  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
      {...labelled}
    >
      {definition.paths.map((d) => (
        <path key={d} d={d} />
      ))}
    </svg>
  );
}

/**
 * A picture from the selected desktop, such as album art.
 *
 * Until it arrives — and wherever it cannot, such as through the cloud relay —
 * a framed-landscape stand-in shows, so the slot reads as "a picture goes
 * here" rather than as the unknown-name placeholder.
 */
function Picture({ sha, size, decorative }: { sha: string; size: number; decorative: boolean }) {
  const url = usePictureUrl(sha);

  const labelled = decorative
    ? { "aria-hidden": true as const }
    : { role: "img" as const, "aria-label": "Picture" };

  if (url) {
    return (
      <img
        src={url}
        width={size}
        height={size}
        alt={decorative ? "" : "Picture"}
        style={{ objectFit: "cover", borderRadius: 2 }}
      />
    );
  }
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="round"
      strokeLinejoin="round"
      {...labelled}
    >
      <rect x="3" y="4" width="18" height="16" rx="1" />
      <circle cx="9" cy="9.5" r="1.5" />
      <path d="M21 16l-5-5-9 9" />
    </svg>
  );
}

/** One icon of an installed pack, once its pack has been fetched. */
function usePackIcon(reference: string): IconPackIcon | null {
  const api = useContext(ApiContext);
  const desktopId = useContext(DesktopContext)?.selected?.id ?? null;
  const [pack, name] = [reference.split("/")[0] ?? "", reference.split("/").slice(1).join("/")];
  const [icon, setIcon] = useState<IconPackIcon | null>(null);
  useEffect(() => {
    setIcon(null);
    if (!api || !desktopId || !pack) return;
    let live = true;
    void api.iconPack(desktopId, pack).then((found) => {
      if (live) setIcon(found?.icons.find((candidate) => candidate.name === name) ?? null);
    });
    return () => {
      live = false;
    };
  }, [api, desktopId, pack, name]);
  return icon;
}

function PackIcon({ reference, size, strokeWidth, decorative }: { reference: string; size: number; strokeWidth: number; decorative: boolean }) {
  const icon = usePackIcon(reference);
  if (icon?.image) {
    return <Picture sha={icon.image.slice(PICTURE.length)} size={size} decorative={decorative} />;
  }
  const labelled = decorative ? { "aria-hidden": true as const } : { role: "img" as const, "aria-label": icon?.name ?? reference };
  if (!icon?.paths?.length) {
    return (
      <svg width={size} height={size} viewBox="0 0 24 24" {...labelled}>
        <rect x="4" y="4" width="16" height="16" fill="none" stroke="currentColor" strokeWidth={1.4} strokeDasharray="3 3" />
      </svg>
    );
  }
  const [x, y, w, h] = icon.view ?? [0, 0, 24, 24];
  return (
    <svg
      width={size}
      height={size}
      viewBox={`${x} ${y} ${w} ${h}`}
      fill={icon.fill ? "currentColor" : "none"}
      stroke={icon.fill ? "none" : "currentColor"}
      strokeWidth={icon.fill ? undefined : (icon.stroke ?? (strokeWidth * Math.max(w, h)) / 24)}
      strokeLinecap="round"
      strokeLinejoin="round"
      {...labelled}
    >
      {icon.paths.map((d, index) => (
        <path key={index} d={d} />
      ))}
    </svg>
  );
}
