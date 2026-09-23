/**
 * Types the charts own, rather than borrow from whichever app is drawing.
 *
 * These lived in `VEN/ui/src/api/types.ts` while the VEN UI was the only
 * consumer. They are not VEN concepts — a shaded time band and the colour of
 * "now" mean the same thing on a fleet chart — so they moved here with the
 * components, and the VEN UI re-exports `ZoneDef` so its own callers are
 * unchanged.
 */

import { formatTariffEurKwh } from "./unitFormat";

/** A shaded band on a time axis: a window, and the resolution it applies at. */
export type ZoneDef = { from: string; to: string; step_s: number };

/** The present instant, wherever it is drawn. One colour, so two charts in the
 *  same view cannot disagree about which line means "now". */
export const COLOR_NOW = "#f44336";

/**
 * The two tariff lines, wherever they are drawn.
 *
 * A tariff is an *announcement*, not a measurement — somebody said this is what
 * the next hour costs — and the Controller tab has always drawn that
 * distinction with a dash. These constants exist so the fleet view, the
 * Controller tab and Raw Diagnostics cannot drift apart about what an import
 * price looks like, which they had already started to do.
 *
 * `COLOR_IMPORT_TARIFF` shares its hex with `COLOR_NOW` by coincidence of
 * palette, not by concept. Keep them as separate constants and never define one
 * in terms of the other: "now" moving to a different red must not repaint every
 * import price in the lab.
 */
export const COLOR_IMPORT_TARIFF = "#f44336";
export const COLOR_EXPORT_TARIFF = "#4caf50";

/**
 * Everything about a tariff line except its colour, its axis and what it is
 * called — spread into a series spec.
 *
 * Typed structurally rather than as `Pick<TimeSeriesSeriesSpec, …>` on purpose:
 * `TimeSeriesChart.tsx` imports this module, so importing its types back would
 * close a cycle.
 */
export const TARIFF_LINE_STYLE: {
  strokeDasharray: string;
  connectNulls: boolean;
  formatter: (value: number) => string;
} = {
  strokeDasharray: "5 5",
  // A price holds until the next announcement, so a missing sample is a gap in
  // reporting, not a gap in the tariff.
  connectNulls: true,
  formatter: formatTariffEurKwh,
};
