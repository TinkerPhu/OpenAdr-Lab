/**
 * Recharts YAxis domain enforcing a minimum span so a series that stays near
 * zero (e.g. control-loop residual noise) does not get auto-scaled to fill
 * the full chart height and read as a toggling square wave. 0 is always
 * within the returned domain — these are cost/CO2 rate axes where "no cost"
 * is the meaningful baseline. A real signal whose actual range already
 * exceeds `minSpan` is returned unchanged (never compressed).
 *
 * Found via the phase 3/4 implementation review: the EV cost-rate line
 * flickered between 0 and 0.00034 €/h — a ~1.5 W grid-residual artifact —
 * and with no domain floor recharts stretched that micro-range across the
 * chart's full height, making negligible noise look like a real signal.
 */
/** Snap-to-milli precision: strips float noise (e.g. a ~1e-16 grid-residual sum that should
 * be exactly 0) so it never reaches recharts' tick generator, which otherwise renders it
 * verbatim (e.g. "18e-17") whenever a computed tick happens to land on it. */
const DOMAIN_ROUNDING_DECIMALS = 6;

function roundToStep(v: number): number {
  // toFixed (not division by a step) avoids reintroducing float error of its own —
  // e.g. Math.round(4.4 / 1e-6) * 1e-6 yields 4.3999999999999995, not 4.4.
  return Number(v.toFixed(DOMAIN_ROUNDING_DECIMALS));
}

export function minSpanDomain(
  values: Array<number | null | undefined>,
  minSpan: number
): [number, number] {
  let dataMin = 0;
  let dataMax = 0;
  for (const v of values) {
    if (v === null || v === undefined || !Number.isFinite(v)) continue;
    if (v < dataMin) dataMin = v;
    if (v > dataMax) dataMax = v;
  }

  const span = dataMax - dataMin;
  if (span >= minSpan) return [roundToStep(dataMin), roundToStep(dataMax)];

  const center = roundToStep((dataMin + dataMax) / 2);
  return [roundToStep(center - minSpan / 2), roundToStep(center + minSpan / 2)];
}

/** Cost-rate axis floor [€/h] — keeps sub-cent residual noise from filling the chart. */
export const MIN_COST_RATE_SPAN_EUR_H = 0.05;

/** CO2-rate axis floor [g/h] — same rationale, sized for typical asset CO2 rates. */
export const MIN_CO2_RATE_SPAN_G_H = 50;

/** Power axis floor [kW] = 5 W — the residual/computed series (e.g. site residual) are
 * unmeasured leftovers, so they hover near zero far more than any physically metered asset;
 * without a floor, sub-watt arithmetic noise gets auto-scaled to fill the chart height the
 * same way the cost-rate axis did (see this file's `minSpanDomain` doc comment). 5 W (rather
 * than 1 W) was chosen because 1 W ticks still rendered as multi-decimal kW values
 * (e.g. "0.00025 kW") that were hard to read even once formatted — see `formatPowerTick`. */
export const MIN_POWER_SPAN_KW = 0.005;

/** Format a power-axis tick/tooltip value [kW] with a fixed, readable rule instead of raw
 * float stringification: Watts (no decimals) below 1 kW, kW (≤2 decimals) at or above —
 * avoids both scientific notation (e.g. "18e-17") and long decimal strings (e.g. ".00025")
 * that plain `${v} kW` labels produced. */
export function formatPowerTick(valueKw: number): string {
  if (Math.abs(valueKw) < 1) return `${Math.round(valueKw * 1000)} W`;
  return `${valueKw.toFixed(2)} kW`;
}
