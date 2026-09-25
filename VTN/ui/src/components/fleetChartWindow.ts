import { roundedTimeTicks } from "@lab/charts/axisDomain";

/**
 * What two stacked Fleet charts must agree on to actually line up.
 *
 * The tariff chart sits directly above the power chart so a price step can be
 * read against the power change it caused. That only works if both charts share
 * three things, and all three are easy to get subtly wrong in two places:
 *
 *   1. the time domain — `tMin`/`tMax`, not recharts' auto-fit, which sizes the
 *      x-axis to whatever data each chart happens to have;
 *   2. the y-axis width — two axes differing by 2px give plot areas differing by
 *      2px, and the step no longer sits above the change;
 *   3. the margin — both must use `TimeSeriesChart`'s default. In particular the
 *      tariff chart must NOT copy the VEN Controller tab's `right: 40`, which
 *      exists there only to make room for a second, right-hand axis;
 *   4. the ticks — identical `xAxisTicks` AND `xAxisInterval={0}`. Supplying the
 *      same array is not enough on its own: recharts' default `preserveEnd`
 *      thinning runs over whatever it is given and can keep different members
 *      on two plots whose widths differ by a pixel.
 *
 * Keeping the first two here means the page computes them once and hands the
 * same values to both charts, rather than each deriving its own.
 */

/** The window both Fleet charts draw, from one clock reading. */
export function fleetChartWindow(
  nowMs: number,
  windowMinutes: number,
): { tMin: number; tMax: number } {
  return { tMin: nowMs - windowMinutes * 60_000, tMax: nowMs };
}

/**
 * Y-axis width [px] for every chart on the Fleet page. 48 is what the VEN
 * Controller tab's tariff axis uses, and it fits the kW labels too.
 */
export const FLEET_AXIS_WIDTH = 48;

/**
 * The tick instants both Fleet charts label.
 *
 * `roundedTimeTicks` snaps to round wall-clock times rather than to the
 * window's own ragged edges, and falls back to hourly when the asked-for
 * spacing would be too dense — it already reasons about recharts otherwise
 * keeping whichever offset a dense array happens to start on.
 */
export function fleetChartTicks(
  tMin: number,
  tMax: number,
  tickMinutes: number,
): number[] {
  return roundedTimeTicks(tMin, tMax, tickMinutes);
}

/** With `xAxisTicks`, this is what makes two charts label the *same* instants. */
export const FLEET_TICK_INTERVAL = 0;
