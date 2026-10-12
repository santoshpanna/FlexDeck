/** One page of `items`, and where it sits in the whole. */
export interface Paged<T> {
  items: T[];
  /** The page actually shown — clamped, so removing the last row of the last page steps back. */
  page: number;
  pages: number;
  /** 1-based index of the first and last row shown; both 0 when there are none. */
  from: number;
  to: number;
}

export function paginate<T>(items: readonly T[], page: number, size: number): Paged<T> {
  const pages = Math.max(1, Math.ceil(items.length / size));
  const shown = Math.min(Math.max(0, page), pages - 1);
  const start = shown * size;
  const slice = items.slice(start, start + size);
  return {
    items: slice,
    page: shown,
    pages,
    from: slice.length ? start + 1 : 0,
    to: start + slice.length,
  };
}
