import { useEffect, useRef, useState, type RefObject } from "react";

/**
 * The current width of an element, kept up to date as it resizes.
 *
 * Zero until the first measurement, so a caller must read zero as "not known
 * yet" rather than as a real width — treating it as a width would collapse the
 * thing it is sizing on the very first render.
 */
export function useElementWidth<T extends HTMLElement>(): {
  ref: RefObject<T | null>;
  width: number;
} {
  const ref = useRef<T | null>(null);
  const [width, setWidth] = useState(0);

  useEffect(() => {
    const element = ref.current;
    if (!element) {
      return;
    }

    setWidth(element.getBoundingClientRect().width);

    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        setWidth(entry.contentRect.width);
      }
    });
    observer.observe(element);

    return () => observer.disconnect();
  }, []);

  return { ref, width };
}
