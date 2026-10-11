import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

/**
 * Join class names and let the LAST conflicting utility win.
 *
 * Every component takes a `className` and passes it through here after its own
 * variant classes, so `<Key className="h-12">` overrides the variant's height
 * instead of fighting it in source order.
 */
export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}
