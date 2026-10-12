import { useCallback, useRef, useState, type CSSProperties } from "react";

/**
 * Hold a paged list at the tallest any of its pages has been, so a shorter
 * last page does not pull everything below it up — and push it back down on
 * the next page.
 *
 * Only while there are several pages: a single page is just a list, and
 * holding its height would leave a gap where a removed row was. The held
 * height is forgotten whenever the number of pages changes.
 */
export function useSteadyHeight<T extends HTMLElement>(pages: number): {
  ref: (element: T | null) => void;
  style: CSSProperties;
} {
  const [height, setHeight] = useState(0);
  const tallest = useRef(0);
  const lastPages = useRef(pages);
  const active = pages > 1;

  if (lastPages.current !== pages) {
    lastPages.current = pages;
    tallest.current = 0;
    if (height !== 0) setHeight(0);
  }

  const ref = useCallback(
    (element: T | null) => {
      if (!element || !active) return;
      const measured = element.offsetHeight;
      if (measured > tallest.current) {
        tallest.current = measured;
        setHeight(measured);
      }
    },
    [active],
  );

  return { ref, style: active && height ? { minHeight: height } : {} };
}
