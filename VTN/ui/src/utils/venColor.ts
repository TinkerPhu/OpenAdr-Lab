/**
 * A stable colour per VEN.
 *
 * Derived from the name, never from position in a list: a VEN that drops out
 * of the window would otherwise shift every colour after it, and an operator
 * watching a fleet react would be re-learning the legend on every refetch.
 * The same name gets the same colour across refreshes, across pages, and on
 * two people's screens.
 */

/** FNV-1a, for no reason beyond being short, stable and well spread. The value
 *  is only ever used modulo the hue circle. */
function hashName(name: string): number {
  let hash = 0x811c9dc5;
  for (let i = 0; i < name.length; i++) {
    hash ^= name.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash;
}

/**
 * Saturation and lightness are fixed so every line reads at the same weight on
 * a white chart; only the hue varies. The golden-angle step spreads adjacent
 * names (ven-1, ven-2, ven-3) far apart on the circle instead of into
 * neighbouring shades of the same colour.
 */
export function venColor(venName: string): string {
  const hue = Math.round((hashName(venName) * 137.508) % 360);
  return `hsl(${hue}, 62%, 45%)`;
}

/** The fleet total. Deliberately not a hue from the same wheel: it is a
 *  different kind of line, and it should not look like a twenty-first site. */
export const FLEET_SUM_COLOR = "#212121";
