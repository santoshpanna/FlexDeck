import { Children, cloneElement, type ReactElement } from "react";

import { changelog } from "../../../common/release/release.ts";
import type { ChangelogViewProps } from "../view/changelog.view.tsx";

export interface ChangelogControllerProps {
  children: ReactElement<Partial<ChangelogViewProps>>;
}

/** The changelog page's controller: hands the generated changelog to the view. */
export function ChangelogController({ children }: ChangelogControllerProps): ReactElement {
  const props: ChangelogViewProps = { next: changelog.next, releases: changelog.releases };
  return cloneElement(Children.only(children), props);
}
