/** How many characters a browser code has, without its dash. */
export const CODE_LENGTH = 8;

/**
 * A browser code as it is being typed, shown the way the desktop shows it:
 * `554G-Z4P5`.
 *
 * The person may type the dash or not — theirs is dropped and one is put back
 * after the fourth character. A dash typed right there is kept, so "554G-"
 * reads as typed rather than snapping back to "554G". Letters people mistype
 * for digits read as those digits, as the desktop reads them.
 */
export function formatCode(typed: string): string {
  const characters = typed
    .toUpperCase()
    .replace(/[^0-9A-Z]/g, "")
    .replace(/O/g, "0")
    .replace(/[IL]/g, "1")
    .slice(0, CODE_LENGTH);

  if (characters.length > 4) return `${characters.slice(0, 4)}-${characters.slice(4)}`;
  if (characters.length === 4 && /-\s*$/.test(typed)) return `${characters}-`;
  return characters;
}
