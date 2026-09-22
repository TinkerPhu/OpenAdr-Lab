/**
 * Types the charts own, rather than borrow from whichever app is drawing.
 *
 * These lived in `VEN/ui/src/api/types.ts` while the VEN UI was the only
 * consumer. They are not VEN concepts — a shaded time band and the colour of
 * "now" mean the same thing on a fleet chart — so they moved here with the
 * components, and the VEN UI re-exports `ZoneDef` so its own callers are
 * unchanged.
 */

/** A shaded band on a time axis: a window, and the resolution it applies at. */
export type ZoneDef = { from: string; to: string; step_s: number };

/** The present instant, wherever it is drawn. One colour, so two charts in the
 *  same view cannot disagree about which line means "now". */
export const COLOR_NOW = "#f44336";
