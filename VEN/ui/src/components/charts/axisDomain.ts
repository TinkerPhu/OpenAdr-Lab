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

/** Tariff axis floor [€/kWh] — same rationale as the cost-rate/CO2 floors above; the
 * tariff axis previously had no floor at all, letting near-flat tariff periods get
 * auto-scaled to fill the chart the same way the unfloored cost-rate axis used to. */
export const MIN_TARIFF_SPAN_EUR_KWH = 0.02;

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

/** Ticks beyond this count are considered too dense to label individually (e.g. a 24h
 * window at 30-min spacing = 48 ticks) — `roundedTimeTicks` falls back to hourly spacing
 * rather than let the chart's own overlap-avoidance thin an arbitrary subset. */
const MAX_DENSE_TICKS = 16;

const HOUR_MS = 3_600_000;

/** Evenly-spaced X-axis tick timestamps within `[fromMs, toMs]`, snapped to the wall-clock
 * (e.g. 10:00, 10:30, 11:00) — recharts' default "nice" tick generation instead lands on
 * whatever offset the axis domain itself happens to start at (e.g. 10:09, 10:39), since the
 * domain here is `now - hoursBack*3600000`, not a round time.
 *
 * When `intervalMinutes` would produce more than `MAX_DENSE_TICKS` labels, falls back to
 * hourly spacing instead. This isn't just a denser array for the chart to thin itself —
 * recharts' own overlap-avoidance would arbitrarily keep whichever offset the dense array
 * happens to start on (e.g. all `:30` marks, none `:00`). Hourly ticks are always exactly on
 * the hour regardless of `fromMs`'s offset (`Math.ceil` to a 60-minute step always lands on
 * `:00`), so falling back here guarantees full-hour labels whenever the finer spacing
 * wouldn't fit. */
export function roundedTimeTicks(fromMs: number, toMs: number, intervalMinutes = 30): number[] {
  const intervalMs = intervalMinutes * 60_000;
  const ticks: number[] = [];
  for (let t = Math.ceil(fromMs / intervalMs) * intervalMs; t <= toMs; t += intervalMs) {
    ticks.push(t);
  }
  if (intervalMs < HOUR_MS && ticks.length > MAX_DENSE_TICKS) {
    return roundedTimeTicks(fromMs, toMs, 60);
  }
  return ticks;
}

/** Rounds a raw tick step up to a "nice" 1/2/5×10^n value (the same family d3/recharts use
 * internally for their own auto ticks), so zero-anchored ticks read like round numbers
 * (0.5, 1, 2, 5, ...) instead of an arbitrary fraction of the domain span. */
function niceStep(rawStep: number): number {
  if (!Number.isFinite(rawStep) || rawStep <= 0) return 1;
  const exponent = Math.floor(Math.log10(rawStep));
  const fraction = rawStep / Math.pow(10, exponent);
  let niceFraction: number;
  if (fraction <= 1) niceFraction = 1;
  else if (fraction <= 2) niceFraction = 2;
  else if (fraction <= 5) niceFraction = 5;
  else niceFraction = 10;
  return niceFraction * Math.pow(10, exponent);
}

/**
 * Explicit Y-axis tick set for a domain that straddles zero, guaranteeing 0.0 is always one
 * of the rendered ticks and that every other tick is a whole step away from it in both
 * directions — rather than recharts' default "nice" tick generation, which computes ticks
 * independently of where zero falls in the domain and can skip 0 entirely on a mixed-sign
 * range (e.g. a domain of [-2, 5] with recharts' own step choice landing on -2, 0.75, 3.5,
 * 6.25 — no 0 tick at all).
 *
 * Returns `undefined` when the domain does not straddle zero (entirely non-negative or
 * entirely non-positive), so the caller falls back to recharts' default tick generation
 * unchanged — this only changes behavior for the mixed-sign case it exists to fix.
 */
export function zeroAnchoredTicks(
  domain: [number, number],
  targetTickCount = 5
): number[] | undefined {
  const [min, max] = domain;
  if (min >= 0 || max <= 0) return undefined;

  const span = max - min;
  const rawStep = span / Math.max(targetTickCount - 1, 1);
  const step = niceStep(rawStep);
  if (step <= 0) return undefined;

  const ticks: number[] = [0];
  const epsilon = step * 1e-6;
  for (let t = step; t <= max + epsilon; t += step) ticks.push(roundToStep(t));
  for (let t = -step; t >= min - epsilon; t -= step) ticks.push(roundToStep(t));
  return ticks.sort((a, b) => a - b);
}
