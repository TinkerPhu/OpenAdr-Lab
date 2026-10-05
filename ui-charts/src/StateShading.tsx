import { ReferenceArea } from "recharts";
import type { TimestampedRow } from "./mergeSeries";

/**
 * Point-classified shading for a time-axis chart: each row is asked what state it
 * is in, and every contiguous run of the same state becomes one shaded area.
 *
 * The third shading primitive next to `ZoneShading.tsx` (fixed plan zones) and
 * `DayNightShading.tsx` (a function of time alone). This one reads the data: a
 * chart declares *how to classify a point* and *how each state looks*, and the run
 * building, the gap handling and the opacity arithmetic happen here once. PV
 * curtailment and the EV's unplugged periods are its first two declarations.
 *
 * Not to be confused with `TimeSeriesChart`'s `bands` (`TimeSeriesBandSpec`), which
 * shades between a lower and an upper *value*; this shades a *time range*.
 *
 * Returns elements directly rather than a wrapping component, for the same reason
 * `ZoneShading.tsx` and `NowLine.tsx` do: recharts inspects its direct children's
 * types to decide how to render and position them.
 */

export interface StateShadingStyle {
  /** Comma-separated channel values, e.g. `"33,150,243"`. */
  rgb: string;
  /** Opacity of a full-weight run (0..1). A run's own weight scales it down. */
  alpha: number;
}

/** What one row says about itself: which state, and how much of it (0..1). */
export interface StateShadingClass<K extends string = string> {
  kind: K;
  weight: number;
}

export interface StateShadingSpec<K extends string = string> {
  /** Names this shading in element keys and in the `state-shading-<key>-<kind>` class. */
  key: string;
  /** `"background"` paints under the grid and lines, `"overlay"` paints over them. */
  layer: "background" | "overlay";
  /** `null` = this row is in no shaded state. `isFuture` is true for rows after `nowMs`. */
  classify: (
    values: TimestampedRow["values"],
    isFuture: boolean,
  ) => StateShadingClass<K> | null;
  styles: Record<K, StateShadingStyle>;
}

export interface StateShadingRun<K extends string = string> {
  x1: number;
  x2: number;
  kind: K;
  weight: number;
}

export interface StateShadingRunOptions {
  /** Rows after this instant are classified as future. Omit for a chart with no "now". */
  nowMs?: number;
  /** Two consecutive rows further apart than this are not joined: the run ends one
   *  ordinary step after the earlier row instead of spanning time nobody sampled. Omit
   *  when irregular spacing is expected (e.g. plan slots that widen with the horizon). */
  maxGapMs?: number;
}

/** Weights closer than this are one run — a float mean must not split a band. */
const WEIGHT_EPS = 0.005;

/**
 * The shading as plain numbers — the whole calculation, with no React in it, so it
 * can be tested by reading values rather than by rendering a chart.
 *
 * A run starts at its first row's `ts` and ends at the `ts` of the first row that is
 * in a different state, so each row shades the interval it stands for. The final run
 * has no next row to end at and extends by one more step.
 */
export function stateShadingRuns<K extends string>(
  rows: TimestampedRow[],
  classify: StateShadingSpec<K>["classify"],
  { nowMs, maxGapMs }: StateShadingRunOptions = {},
): StateShadingRun<K>[] {
  const runs: StateShadingRun<K>[] = [];
  let open: { x1: number; kind: K; weight: number } | null = null;

  const close = (x2: number) => {
    if (open && x2 > open.x1) runs.push({ ...open, x2 });
    open = null;
  };
  /** Width of the step that ends at row `i` — what "one more step" means there. */
  const stepBefore = (i: number) => (i > 0 ? Math.max(rows[i].ts - rows[i - 1].ts, 1) : 1);

  for (let i = 0; i < rows.length; i++) {
    const row = rows[i];
    if (open && i > 0 && maxGapMs !== undefined && row.ts - rows[i - 1].ts > maxGapMs) {
      close(rows[i - 1].ts + Math.min(stepBefore(i - 1), maxGapMs));
    }
    const cls = classify(row.values, nowMs !== undefined && row.ts > nowMs);
    const current = cls && cls.weight > WEIGHT_EPS ? cls : null;
    const same =
      open !== null &&
      current !== null &&
      open.kind === current.kind &&
      Math.abs(open.weight - current.weight) < WEIGHT_EPS;
    if (same) continue;
    close(row.ts);
    if (current) open = { x1: row.ts, kind: current.kind, weight: current.weight };
  }
  if (open && rows.length > 0) {
    const last = rows.length - 1;
    close(rows[last].ts + stepBefore(last));
  }
  return runs;
}

export function renderStateShading<K extends string>(
  yAxisId: string,
  rows: TimestampedRow[],
  spec: StateShadingSpec<K>,
  options?: StateShadingRunOptions,
) {
  return stateShadingRuns(rows, spec.classify, options).map((run) => {
    const style = spec.styles[run.kind];
    return (
      <ReferenceArea
        key={`${spec.key}-${run.x1}`}
        className={`state-shading-${spec.key}-${run.kind}`}
        yAxisId={yAxisId}
        x1={run.x1}
        x2={run.x2}
        fill={`rgba(${style.rgb},${(style.alpha * run.weight).toFixed(4)})`}
        // Explicit: recharts defaults ReferenceArea's fillOpacity to 0.5, which
        // would silently halve every alpha computed above.
        fillOpacity={1}
        ifOverflow="hidden"
      />
    );
  });
}
