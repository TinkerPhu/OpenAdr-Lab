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
  if (span >= minSpan) return [dataMin, dataMax];

  const center = (dataMin + dataMax) / 2;
  return [center - minSpan / 2, center + minSpan / 2];
}

/** Cost-rate axis floor [€/h] — keeps sub-cent residual noise from filling the chart. */
export const MIN_COST_RATE_SPAN_EUR_H = 0.05;

/** CO2-rate axis floor [g/h] — same rationale, sized for typical asset CO2 rates. */
export const MIN_CO2_RATE_SPAN_G_H = 50;
