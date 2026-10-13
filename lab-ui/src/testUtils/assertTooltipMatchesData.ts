import { expect } from "vitest";
import type { TimestampedRow } from "../mergeSeries";

/**
 * Regression guard for the cursor/tooltip index-mismatch bug class (`117b44f`,
 * `f7b911e`): asserts that for a given hovered timestamp, every series' tooltip value
 * (produced by its `dataKey` accessor) equals the value actually stored in the merged
 * row at that timestamp — never a value read from some other, independently-indexed
 * array. Reusable by any composition's test suite that renders more than one series.
 *
 * `seriesAccessors` mirrors the `dataKey` accessor functions the chart passes to each
 * `<Line>`/`<Area>` — passing the SAME accessor functions the component actually uses is
 * what makes this a meaningful regression test rather than a tautology: if a future
 * change reintroduces a per-series `data` array independent of `mergedRows`, the
 * accessor (now reading the wrong array) will disagree with `mergedRows` at that
 * timestamp and this assertion fails.
 */
export function assertTooltipMatchesData(
  mergedRows: TimestampedRow[],
  hoveredTs: number,
  seriesAccessors: Record<string, (row: TimestampedRow) => number | null | undefined>
): void {
  const row = mergedRows.find((r) => r.ts === hoveredTs);
  expect(row, `no merged row exists at hovered ts=${hoveredTs}`).toBeDefined();

  for (const [seriesName, accessor] of Object.entries(seriesAccessors)) {
    const tooltipValue = accessor(row!);
    const directValue = row!.values?.[seriesName] ?? null;
    expect(
      tooltipValue ?? null,
      `series "${seriesName}" at ts=${hoveredTs}: tooltip accessor returned ${tooltipValue}, ` +
        `but the merged row's own value is ${directValue} — the accessor must read the SAME ` +
        `row every other series reads, never an independently-indexed array`
    ).toBe(directValue);
  }
}
