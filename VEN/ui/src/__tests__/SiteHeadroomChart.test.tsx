/**
 * SiteHeadroomChart — past-band LOCF continuity
 *
 * The band's lower/upper accessors (`up_kw`, `down_kw` — SIGNED,
 * `site-capacity-seam-unification`, no negation) depend only on `upKw`/
 * `downKw` being present on a row, not on `gridPowerKw` (unlike before Spec
 * E, when the band was a delta relative to the grid-power line and needed
 * both present on the same row). This test still confirms `upKw`/`downKw`
 * are themselves forward-filled continuously across the dense flexibility
 * history ring's timestamps, which almost never land exactly on a
 * resampled grid-timeline point.
 */
import { render } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { ReactNode } from "react";
import type { TimestampedRow } from "../components/charts/mergeSeries";

const { propsCalls } = vi.hoisted(() => ({
  propsCalls: [] as Array<Record<string, unknown>>,
}));

vi.mock("../components/charts/TimeSeriesChart", () => ({
  TimeSeriesChart: (props: Record<string, unknown>) => {
    propsCalls.push(props);
    return null as ReactNode;
  },
}));

import { SiteHeadroomChart } from "../components/controller/charts/SiteHeadroomChart";
import type { AssetTimelinePoint } from "../components/controller/types";
import type {
  CapacityCurvesResponse,
  SiteFlexibilitySample,
  SiteFlexibilityForecastSlot,
} from "../api/types";

describe("SiteHeadroomChart — past band renders continuously", () => {
  beforeEach(() => {
    propsCalls.length = 0;
  });

  it("gridPowerKw is forward-filled so the band has a value at every history row", () => {
    const nowMs = 1_000_000_000;
    // Sparse grid timeline: one real point every 10 minutes.
    const gridTimeline: AssetTimelinePoint[] = [
      { ts: nowMs - 600_000, values: { power_kw: 2.0 } },
      { ts: nowMs - 300_000, values: { power_kw: 3.0 } },
      { ts: nowMs, values: { power_kw: 4.0 } },
    ];
    // Dense flexibility history: one sample every ~1 minute, at timestamps
    // that never exactly coincide with a grid-timeline point. Starts before
    // the earliest grid-timeline point, matching the real ring (1h of
    // history) vs. a much shorter chart window.
    const history: SiteFlexibilitySample[] = Array.from({ length: 11 }, (_, i) => ({
      ts: new Date(nowMs - 660_000 + i * 60_000).toISOString(),
      up_kw: 1.0 + i * 0.1,
      down_kw: 2.0 + i * 0.1,
    }));

    render(
      <SiteHeadroomChart
        gridTimeline={gridTimeline}
        history={history}
        nowMs={nowMs}
        hoursBack={0.25}
        hoursForward={0}
      />
    );

    expect(propsCalls).toHaveLength(1);
    const { data, bands } = propsCalls[0] as {
      data: TimestampedRow[];
      bands: Array<{
        lower: (row: TimestampedRow) => number | null;
        upper: (row: TimestampedRow) => number | null;
      }>;
    };
    const band = bands[0];

    // At least two CONSECUTIVE rows (required for recharts to draw any
    // visible Area segment with connectNulls={false}) must both have a
    // non-null band value — the bug produced isolated/no non-null pairs.
    let consecutiveNonNullPairs = 0;
    for (let i = 1; i < data.length; i++) {
      if (band.lower(data[i - 1]) != null && band.lower(data[i]) != null) {
        consecutiveNonNullPairs++;
      }
    }
    expect(consecutiveNonNullPairs).toBeGreaterThan(0);

    // Every row except the unavoidable leading edge (LOCF can't retroactively
    // fill a key before its very first appearance in the array) now has a
    // real band value — before the fix, almost none did.
    const rowsWithoutBandValue = data.filter((row) => band.lower(row) == null);
    expect(rowsWithoutBandValue.length).toBeLessThanOrEqual(1);
  });
});

describe("SiteHeadroomChart — xAxisTickIntervalMinutes wiring (shared roundedTimeTicks pattern)", () => {
  beforeEach(() => {
    propsCalls.length = 0;
  });

  it("forwards rounded, wall-clock-snapped ticks to TimeSeriesChart when the prop is set", () => {
    const nowMs = 1_000_000_000;
    render(
      <SiteHeadroomChart
        gridTimeline={[]}
        history={[]}
        nowMs={nowMs}
        hoursBack={1}
        hoursForward={0}
        xAxisTickIntervalMinutes={10}
      />
    );

    const { xAxisTicks } = propsCalls[0] as { xAxisTicks?: number[] };
    expect(xAxisTicks).toBeDefined();
    expect(xAxisTicks!.length).toBeGreaterThan(0);
    const intervalMs = 10 * 60_000;
    for (const tick of xAxisTicks!) {
      expect(tick % intervalMs).toBe(0);
    }
  });

  it("leaves ticks undefined (recharts default) when the prop is omitted", () => {
    const nowMs = 1_000_000_000;
    render(
      <SiteHeadroomChart gridTimeline={[]} history={[]} nowMs={nowMs} hoursBack={1} hoursForward={0} />
    );

    const { xAxisTicks } = propsCalls[0] as { xAxisTicks?: number[] };
    expect(xAxisTicks).toBeUndefined();
  });
});

describe("SiteHeadroomChart — forecast prop feeds the future band with real per-slot values", () => {
  beforeEach(() => {
    propsCalls.length = 0;
  });

  it("future rows carry the forecast's own varying values, not a flat LOCF copy of the last history sample", () => {
    const nowMs = 1_000_000_000;
    const gridTimeline: AssetTimelinePoint[] = [
      { ts: nowMs - 300_000, values: { power_kw: 2.0 } },
      { ts: nowMs, values: { power_kw: 2.0 } },
      { ts: nowMs + 300_000, values: { power_kw: 2.0 } },
      { ts: nowMs + 600_000, values: { power_kw: 2.0 } },
    ];
    const history: SiteFlexibilitySample[] = [
      { ts: new Date(nowMs - 300_000).toISOString(), up_kw: 1.0, down_kw: 1.0 },
      { ts: new Date(nowMs).toISOString(), up_kw: 1.0, down_kw: 1.0 },
    ];
    // Forecast values genuinely differ per slot — if the fix regressed to flat LOCF-extending
    // the last history sample (up_kw/down_kw both 1.0), these distinct values would never appear.
    const forecast: SiteFlexibilityForecastSlot[] = [
      { ts: new Date(nowMs + 300_000).toISOString(), up_kw: 3.0, down_kw: 4.0 },
      { ts: new Date(nowMs + 600_000).toISOString(), up_kw: 5.0, down_kw: 6.0 },
    ];

    render(
      <SiteHeadroomChart
        gridTimeline={gridTimeline}
        history={history}
        forecast={forecast}
        nowMs={nowMs}
        hoursBack={0.1}
        hoursForward={0.2}
      />
    );

    const { data, bands } = propsCalls[0] as {
      data: TimestampedRow[];
      bands: Array<{
        lower: (row: TimestampedRow) => number | null;
        upper: (row: TimestampedRow) => number | null;
      }>;
    };
    const band = bands[0];

    const futureRow1 = data.find((row) => row.ts === nowMs + 300_000);
    const futureRow2 = data.find((row) => row.ts === nowMs + 600_000);
    expect(futureRow1).toBeDefined();
    expect(futureRow2).toBeDefined();
    // lower = up_kw directly, no negation (site-capacity-seam-unification:
    // up_kw is signed now, matching CapacityCurveStep.power_kw) --
    // independent of gridPowerKw.
    expect(band.lower(futureRow1!)).toBeCloseTo(3.0);
    expect(band.lower(futureRow2!)).toBeCloseTo(5.0);
    expect(band.lower(futureRow1!)).not.toBeCloseTo(band.lower(futureRow2!)!);
  });
});

describe("SiteHeadroomChart — capacity curve overlay starts exactly at now, no backward extension", () => {
  beforeEach(() => {
    propsCalls.length = 0;
  });

  it("import/export commitment series have no value before now and pick up the curve's own steps from now onward", () => {
    const nowMs = 1_000_000_000;
    const gridTimeline: AssetTimelinePoint[] = [
      { ts: nowMs - 300_000, values: { power_kw: 2.0 } },
      { ts: nowMs, values: { power_kw: 2.0 } },
      { ts: nowMs + 300_000, values: { power_kw: 2.0 } },
    ];
    const capacity: CapacityCurvesResponse = {
      import: {
        direction: "import",
        start: new Date(nowMs).toISOString(),
        steps: [
          { elapsed_s: 0, power_kw: 8.0 },
          { elapsed_s: 300, power_kw: 6.0 },
        ],
      },
      export: {
        direction: "export",
        start: new Date(nowMs).toISOString(),
        steps: [
          { elapsed_s: 0, power_kw: -3.0 },
          { elapsed_s: 300, power_kw: 1.0 },
        ],
      },
    };

    render(
      <SiteHeadroomChart
        gridTimeline={gridTimeline}
        history={[]}
        capacity={capacity}
        nowMs={nowMs}
        hoursBack={0.1}
        hoursForward={0.1}
      />
    );

    const { data, series } = propsCalls[0] as {
      data: TimestampedRow[];
      series: Array<{
        key: string;
        dataKey: (row: TimestampedRow) => number | null;
      }>;
    };
    const importSeries = series.find((s) => s.key === "Import commitment [kW]")!;
    const exportSeries = series.find((s) => s.key === "Export commitment [kW]")!;
    expect(importSeries).toBeDefined();
    expect(exportSeries).toBeDefined();

    const pastRow = data.find((row) => row.ts === nowMs - 300_000);
    const nowRow = data.find((row) => row.ts === nowMs);
    const futureRow = data.find((row) => row.ts === nowMs + 300_000);
    expect(pastRow).toBeDefined();
    expect(nowRow).toBeDefined();
    expect(futureRow).toBeDefined();

    // No value before `now` — the capacity curve's own t1 is always "now".
    expect(importSeries.dataKey(pastRow!)).toBeNull();
    expect(exportSeries.dataKey(pastRow!)).toBeNull();
    // From `now` onward, the curve's own step values appear -- including the
    // Export curve's positive excursion (base-load-exceeds-export-capacity,
    // normal per this component's own doc comment, not a sign bug).
    expect(importSeries.dataKey(nowRow!)).toBeCloseTo(8.0);
    expect(exportSeries.dataKey(nowRow!)).toBeCloseTo(-3.0);
    expect(importSeries.dataKey(futureRow!)).toBeCloseTo(6.0);
    expect(exportSeries.dataKey(futureRow!)).toBeCloseTo(1.0);
  });

  it("renders with no importCapKw/exportCapKw values when capacity is null", () => {
    const nowMs = 1_000_000_000;
    render(
      <SiteHeadroomChart
        gridTimeline={[]}
        history={[]}
        capacity={null}
        nowMs={nowMs}
        hoursBack={0.1}
        hoursForward={0.1}
      />
    );

    const { data, series } = propsCalls[0] as {
      data: TimestampedRow[];
      series: Array<{
        key: string;
        dataKey: (row: TimestampedRow) => number | null;
      }>;
    };
    const importSeries = series.find((s) => s.key === "Import commitment [kW]")!;
    const exportSeries = series.find((s) => s.key === "Export commitment [kW]")!;
    for (const row of data) {
      expect(importSeries.dataKey(row)).toBeNull();
      expect(exportSeries.dataKey(row)).toBeNull();
    }
  });
});
