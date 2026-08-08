/**
 * The structural fix for the cursor/tooltip index-mismatch bug class (`117b44f`,
 * `f7b911e`): recharts resolves a hovered tooltip's value *by array index* into
 * whatever `data` array each `<Line>`/`<Area>` was given. If two series come from two
 * separately-indexed arrays (e.g. a 1-minute actual series and a 5-minute forecast
 * series), hovering point `i` can show the actual line's real value next to a forecast
 * value pulled from an unrelated timestamp at `otherArray[i]`.
 *
 * The fix is structural, not conventional: every series a chart renders must be folded
 * into ONE timestamp-keyed row array before rendering, and every `<Line>`/`<Area>`
 * reads its value via a `dataKey` accessor into that single array — never its own
 * independent `data` prop. This module provides that merge, plus the LOCF (last-observed-
 * carried-forward) fill sparse series need so a step-function line has a value at every
 * slot between two real samples, not just at the samples themselves.
 */

export interface TimestampedRow {
  ts: number;
  values: Record<string, number | null> | null;
}

/** One named point to fold into the merged row array — e.g. a forecast sample keyed by
 * its own target timestamp and the series key it should appear under in the merged row. */
export interface NamedSample {
  ts: number;
  key: string;
  value: number;
}

/**
 * Merge a base array of sparse-values rows with any number of extra named sample lists
 * into one timestamp-keyed, timestamp-sorted array. A row that already exists at a given
 * `ts` gets the extra sample folded into its `values` map; a `ts` with no base row gets a
 * new row created for it. This is the single array every series in a chart must read from.
 */
export function mergeTimestampedSeries(
  base: TimestampedRow[],
  extraSamples: NamedSample[] = []
): TimestampedRow[] {
  const byTs = new Map<number, TimestampedRow>();
  for (const row of base) {
    byTs.set(row.ts, { ts: row.ts, values: { ...(row.values ?? {}) } });
  }
  for (const sample of extraSamples) {
    const existing = byTs.get(sample.ts);
    if (existing) {
      existing.values = { ...(existing.values ?? {}), [sample.key]: sample.value };
    } else {
      byTs.set(sample.ts, { ts: sample.ts, values: { [sample.key]: sample.value } });
    }
  }
  return [...byTs.values()].sort((a, b) => a.ts - b.ts);
}

/**
 * Forward-fill the given keys across a timestamp-sorted merged array: wherever a key is
 * absent (null/undefined) at a row, carry forward the last value seen for that key at an
 * earlier row. Required for step-function/state series sampled sparser than the array's
 * own timestamp grid — without this, `connectNulls` would draw the line but hovering the
 * plateau between two real samples would show no value, disagreeing with what's drawn.
 */
export function locfFillKeys(rows: TimestampedRow[], keys: string[]): TimestampedRow[] {
  const last: Record<string, number | null> = {};
  for (const k of keys) last[k] = null;
  return rows.map((row) => {
    const values = { ...(row.values ?? {}) };
    let changed = false;
    for (const k of keys) {
      const v = values[k] ?? null;
      if (v !== null) {
        last[k] = v;
      } else if (last[k] !== null) {
        values[k] = last[k];
        changed = true;
      }
    }
    return changed ? { ...row, values } : row;
  });
}
