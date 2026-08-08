import { describe, it, expect } from "vitest";
import { mergeTimestampedSeries, locfFillKeys, type TimestampedRow } from "../components/charts/mergeSeries";
import { assertTooltipMatchesData } from "../components/charts/testUtils/assertTooltipMatchesData";

describe("mergeTimestampedSeries", () => {
  it("merges a base series with extra named samples into one timestamp-keyed array", () => {
    const base: TimestampedRow[] = [
      { ts: 1000, values: { power_kw: 2.0 } },
      { ts: 2000, values: { power_kw: 2.5 } },
    ];
    const merged = mergeTimestampedSeries(base, [{ ts: 1000, key: "forecast_kw", value: 1.8 }]);
    expect(merged).toHaveLength(2);
    expect(merged.find((r) => r.ts === 1000)?.values).toEqual({ power_kw: 2.0, forecast_kw: 1.8 });
  });

  it("creates a new row for a sample timestamp absent from the base series", () => {
    const base: TimestampedRow[] = [{ ts: 1000, values: { power_kw: 2.0 } }];
    const merged = mergeTimestampedSeries(base, [{ ts: 5000, key: "forecast_kw", value: 3.1 }]);
    expect(merged).toHaveLength(2);
    expect(merged.find((r) => r.ts === 5000)?.values).toEqual({ forecast_kw: 3.1 });
  });

  it("returns rows sorted by timestamp regardless of input order", () => {
    const base: TimestampedRow[] = [
      { ts: 3000, values: {} },
      { ts: 1000, values: {} },
    ];
    const merged = mergeTimestampedSeries(base, [{ ts: 2000, key: "x", value: 1 }]);
    expect(merged.map((r) => r.ts)).toEqual([1000, 2000, 3000]);
  });

  describe("the exact bug class this exists to prevent (117b44f / f7b911e)", () => {
    it("a series sampled at a different rate stays aligned by timestamp, not by array index", () => {
      // 1-minute actual samples.
      const actual: TimestampedRow[] = [
        { ts: 0, values: { power_kw: 1.0 } },
        { ts: 60_000, values: { power_kw: 1.1 } },
        { ts: 120_000, values: { power_kw: 1.2 } },
        { ts: 180_000, values: { power_kw: 1.3 } },
        { ts: 240_000, values: { power_kw: 1.4 } },
      ];
      // 5-minute-apart forecast samples — a coarser, differently-indexed source.
      const forecastSamples = [
        { ts: 0, key: "forecast_kw", value: 0.9 },
        { ts: 240_000, key: "forecast_kw", value: 1.35 },
      ];

      const merged = mergeTimestampedSeries(actual, forecastSamples);

      // Before the 117b44f fix, a chart rendering `actual` from one array and
      // `forecastSamples` from its OWN separate array would resolve recharts' hover at
      // index 1 (ts=60_000) to forecastSamples[1] = { ts: 240_000, value: 1.35 } — the
      // wrong timestamp entirely. With the merge, index 1's row simply has no
      // forecast_kw value (it's null until LOCF-filled), never a value borrowed from a
      // different index.
      const rowAt60s = merged.find((r) => r.ts === 60_000)!;
      expect(rowAt60s.values?.power_kw).toBe(1.1);
      expect(rowAt60s.values?.forecast_kw ?? null).toBeNull();

      const rowAt0 = merged.find((r) => r.ts === 0)!;
      expect(rowAt0.values?.power_kw).toBe(1.0);
      expect(rowAt0.values?.forecast_kw).toBe(0.9);
    });

    it("assertTooltipMatchesData catches an accessor that reads an independent, misaligned array", () => {
      const merged = mergeTimestampedSeries(
        [
          { ts: 0, values: { power_kw: 1.0 } },
          { ts: 60_000, values: { power_kw: 1.1 } },
        ],
        [{ ts: 0, key: "forecast_kw", value: 0.9 }]
      );

      // Correct accessor: reads the merged row itself — passes.
      expect(() =>
        assertTooltipMatchesData(merged, 60_000, {
          power_kw: (row) => row.values?.power_kw ?? null,
        })
      ).not.toThrow();

      // Deliberately reproduce the old bug: an accessor that ignores the merged row and
      // reads a second, independently-indexed array by position instead of by timestamp.
      const misalignedForecastArray = [{ value: 0.9 }, { value: 1.35 }];
      const indexOfRow = merged.findIndex((r) => r.ts === 60_000);
      expect(() =>
        assertTooltipMatchesData(merged, 60_000, {
          forecast_kw: () => misalignedForecastArray[indexOfRow]?.value ?? null,
        })
      ).toThrow();
    });
  });
});

describe("locfFillKeys", () => {
  it("carries the last known value forward into null slots", () => {
    const rows: TimestampedRow[] = [
      { ts: 0, values: { state: 50 } },
      { ts: 1000, values: {} },
      { ts: 2000, values: {} },
      { ts: 3000, values: { state: 55 } },
    ];
    const filled = locfFillKeys(rows, ["state"]);
    expect(filled.map((r) => r.values?.state)).toEqual([50, 50, 50, 55]);
  });

  it("leaves a key entirely absent (never sampled) as null rather than fabricating a value", () => {
    const rows: TimestampedRow[] = [{ ts: 0, values: {} }, { ts: 1000, values: {} }];
    const filled = locfFillKeys(rows, ["state"]);
    expect(filled.every((r) => (r.values?.state ?? null) === null)).toBe(true);
  });

  it("only fills the requested keys, leaving other sparse keys untouched", () => {
    const rows: TimestampedRow[] = [
      { ts: 0, values: { a: 1, b: 10 } },
      { ts: 1000, values: {} },
    ];
    const filled = locfFillKeys(rows, ["a"]);
    expect(filled[1].values?.a).toBe(1);
    expect(filled[1].values?.b ?? null).toBeNull();
  });
});
