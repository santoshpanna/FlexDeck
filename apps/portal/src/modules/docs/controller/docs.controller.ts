import { Children, cloneElement, createElement, type ReactElement } from "react";
import { Navigate, useParams } from "react-router-dom";

import { DOCS } from "../lib/docs.content.ts";
import { NAV, nextDoc } from "../lib/docs.lib.ts";
import type { DocsViewProps } from "../view/docs.view.tsx";

export interface DocsControllerProps {
  children: ReactElement<Partial<DocsViewProps>>;
}

/**
 * The docs controller: which page the URL names. `/docs` and an unknown slug
 * both land on the first page — a stale link is likelier than a wish to see an
 * error, the same call the route table makes for `*`.
 */
export function DocsController({ children }: DocsControllerProps): ReactElement {
  const { slug } = useParams();
  const doc = DOCS.find((candidate) => candidate.slug === slug);
  if (!doc) return createElement(Navigate, { to: `/docs/${DOCS[0]?.slug ?? ""}`, replace: true });
  const props: DocsViewProps = { doc, docs: DOCS, nav: NAV, next: nextDoc(DOCS, doc.slug) };
  return cloneElement(Children.only(children), props);
}
