import type { AnchorHTMLAttributes } from "react";
import { Link } from "react-router-dom";

/**
 * A link that may leave this site. A route (`/docs`) stays a router link with
 * no reload; an absolute URL — the editor, when the public pages and the editor
 * are deployed apart — is a plain anchor.
 */
export function AppLink({ to, ...rest }: AnchorHTMLAttributes<HTMLAnchorElement> & { to: string }) {
  if (/^https?:\/\//.test(to)) return <a href={to} {...rest} />;
  return <Link to={to} {...rest} />;
}
