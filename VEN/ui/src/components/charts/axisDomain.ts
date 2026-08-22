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

/**
 * Like `minSpanDomain`, but does NOT anchor the domain at 0 — `minSpanDomain` seeds
 * `dataMin`/`dataMax` at 0 and only widens from there, which is correct for rate axes
 * where "no cost"/"no CO2" is itself a meaningful baseline worth always showing, but wrong
 * for a strictly-positive price series like tariff: an always-positive series (e.g.
 * 0.28–0.32 €/kWh) would still get a domain of `[0, 0.32]` from `minSpanDomain`, visually
 * compressing the real variation into the top slice of the axis — the exact "squeezed"
 * defect a floor is supposed to fix, just reintroduced by the 0-anchor. This function fits
 * tightly to the real data (like recharts' own `["auto","auto"]`) and only widens
 * symmetrically around the data's own center when the real span is narrower than `minSpan`.
 */
export function tightSpanDomain(
  values: Array<number | null | undefined>,
  minSpan: number
): [number, number] {
  let dataMin: number | null = null;
  let dataMax: number | null = null;
  for (const v of values) {
    if (v === null || v === undefined || !Number.isFinite(v)) continue;
    if (dataMin === null || v < dataMin) dataMin = v;
    if (dataMax === null || v > dataMax) dataMax = v;
  }
  if (dataMin === null || dataMax === null) return [-minSpan / 2, minSpan / 2];

  const span = dataMax - dataMin;
  if (span >= minSpan) return [roundToStep(dataMin), roundToStep(dataMax)];

  const center = roundToStep((dataMin + dataMax) / 2);
  return [roundToStep(center - minSpan / 2), roundToStep(center + minSpan / 2)];
}

/** Cost-rate axis floor [€/h] — keeps sub-cent residual noise from filling the chart. */
export const MIN_COST_RATE_SPAN_EUR_H = 0.05;

/** CO2-rate axis floor [g/h] — same rationale, sized for typical asset CO2 rates. */
export const MIN_CO2_RATE_SPAN_G_H = 50;

/** CO2-intensity axis floor [g/kWh], used with `tightSpanDomain` — like tariff, intensity
 * is a strictly-positive per-energy quantity, not a rate with a meaningful 0 baseline. */
export const MIN_CO2_INTENSITY_SPAN_G_KWH = 20;

/** Tariff axis floor [€/kWh], used with `tightSpanDomain` (not `minSpanDomain` — tariff is
 * a strictly-positive price, not a rate that meaningfully swings through a 0 baseline; see
 * `tightSpanDomain`'s doc comment). The tariff axis previously had no floor at all, letting
 * near-flat tariff periods auto-scale to fill the chart. */
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
 * internally for their own auto ticks), so ticks read like round numbers (0.5, 1, 2, 5, ...)
 * instead of an arbitrary fraction of the domain span. `niceAxis` normally searches the whole
 * candidate ladder itself; this one-shot form is its fallback for degenerate domains. */
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

/** Fewest/most Y-axis gridlines a chart cell should carry. The lower bound is 3 on purpose:
 * a narrow band far from zero (e.g. 1.32–1.48) is better served by three round labels
 * (1.3 / 1.4 / 1.5) than by five with an extra digit each. */
const MIN_Y_TICKS = 3;
const MAX_Y_TICKS = 7;

/** How far the niced domain may exceed the real data span. Rounding the endpoints outward is
 * what makes the extreme labels round, but an unbounded coarsening would squeeze the actual
 * signal into a sliver of the axis — the same defect `tightSpanDomain` exists to avoid. */
const MAX_SPAN_GROWTH = 1.5;

export interface NiceAxis {
  /** Domain snapped outward to whole steps — the first and last tick. */
  domain: [number, number];
  ticks: number[];
  /** The chosen 1/2/5×10^n step; feed to `tickFormatterForStep` for matching label decimals. */
  step: number;
}

function ticksForStep(min: number, max: number, step: number): number[] {
  const first = Math.floor(min / step + 1e-9);
  const last = Math.ceil(max / step - 1e-9);
  const ticks: number[] = [];
  for (let k = first; k <= last; k++) ticks.push(roundToStep(k * step));
  return ticks;
}

/**
 * The one Y-axis tick rule for every chart in VEN/ui: ticks land on exact multiples of a step
 * whose own mantissa is 1, 2 or 5, so labels read as round numbers (0.2, 500, 1.4) instead of
 * an arbitrary fraction of the data span. This is the Y-axis counterpart of `roundedTimeTicks`
 * on the X axis — snapped to an absolute grid, not to wherever the domain happens to start.
 *
 * It replaces `zeroAnchoredTicks`, which only ever fired for domains straddling zero and
 * returned `undefined` otherwise — so every single-sign axis (PV export revenue in €/h, CO2 in
 * g/h, a strictly-positive tariff in €/kWh) silently fell back to recharts' own tick
 * generation over an unrounded domain, producing labels like `-0.31275 €/h`. Because this
 * function is total, the compositions apply it to every axis with no caller opt-in — see
 * `TimeSeriesChart`/`StackedTimeSeriesChart`, which is where the rule is actually enforced.
 *
 * Step choice: the *coarsest* candidate step that still yields `MIN_Y_TICKS`..`MAX_Y_TICKS`
 * ticks without inflating the domain past `MAX_SPAN_GROWTH`. Coarsest-wins keeps labels at one
 * or two significant digits in the common case, while a narrow band far from zero still gets
 * the finer step it needs (1.3 / 1.4 / 1.5) rather than being flattened.
 */
export function niceAxis(domain: [number, number], targetTickCount = 5): NiceAxis {
  const [rawMin, rawMax] = domain;
  const min = Number.isFinite(rawMin) ? rawMin : 0;
  const max = Number.isFinite(rawMax) ? rawMax : 0;
  // A zero-span domain has no scale of its own to derive a step from; fall back to the
  // magnitude of the value itself (or 1 when that is 0 too) and widen around it, so the axis
  // still renders a real tick ladder instead of a single label. A degenerate domain sitting
  // exactly at 0 (e.g. CurveChart's `[0, max(...)]` or PlanHistory's solve-time axis when every
  // sample is 0 ms) widens upward only, never symmetrically — every real caller that can reach
  // this branch with a zero value is a non-negative quantity (price, ms, kW), so a symmetric
  // widen would draw a negative tick the data can never actually take (e.g. "-0.5 ms"). A
  // degenerate domain away from 0 (e.g. [2, 2]) has no such sign constraint and keeps widening
  // symmetrically.
  const dataSpan = max - min;
  const span = dataSpan > 0 ? dataSpan : Math.abs(max) || 1;
  const zeroPoint = dataSpan <= 0 && min === 0 && max === 0;
  const lo = dataSpan > 0 ? min : zeroPoint ? 0 : min - span / 2;
  const hi = dataSpan > 0 ? max : zeroPoint ? span : max + span / 2;

  const baseExponent = Math.floor(Math.log10(span));
  let best: NiceAxis | null = null;
  for (let exponent = baseExponent - 2; exponent <= baseExponent + 1; exponent++) {
    for (const mantissa of [1, 2, 5]) {
      const step = mantissa * Math.pow(10, exponent);
      const ticks = ticksForStep(lo, hi, step);
      if (ticks.length < MIN_Y_TICKS || ticks.length > MAX_Y_TICKS) continue;
      const nicedSpan = ticks[ticks.length - 1] - ticks[0];
      if (nicedSpan > span * MAX_SPAN_GROWTH) continue;
      // Candidates are generated fine → coarse, so the last accepted one is the coarsest.
      best = { domain: [ticks[0], ticks[ticks.length - 1]], ticks, step };
    }
  }
  if (best) return best;

  // No candidate satisfied both bounds (degenerate/zero-span domains): keep the original
  // target density and accept whatever growth that implies — a rendered axis with round
  // ticks still beats recharts' raw-domain fallback.
  const step = niceStep(span / Math.max(targetTickCount - 1, 1));
  const ticks = ticksForStep(lo, hi, step);
  return { domain: [ticks[0], ticks[ticks.length - 1]], ticks, step };
}

/** Label formatter matching a `niceAxis` step: exactly as many decimals as the step needs, so
 * a 0.05 step prints "-0.35" and a 200 step prints "600" — and float noise (0.30000000000004)
 * never reaches the label. Used as the default axis `tickFormatter` when an axis declares no
 * unit-specific one of its own (e.g. power axes keep `formatPowerTick`). */
export function tickFormatterForStep(step: number): (value: number) => string {
  const decimals = Math.max(0, -Math.floor(Math.log10(Math.abs(step)) + 1e-9));
  return (value: number) => value.toFixed(decimals);
}
