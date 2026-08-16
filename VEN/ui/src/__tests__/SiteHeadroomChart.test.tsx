/**
 * SiteHeadroomChart — past-band LOCF fix
 *
 * The dense (~1s cadence) flexibility history ring and the much sparser
 * resampled grid-timeline points almost never land on the same timestamp.
 * The band's lower/upper accessors require gridPowerKw AND upKw/downKw
 * non-null on the SAME row — if gridPowerKw isn't forward-filled the same
 * way upKw/downKw are, the band has essentially no row where both are
 * non-null, so it never renders anything for the past.
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
import type { SiteFlexibilitySample, SiteFlexibilityForecastSlot } from "../api/types";

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
    // gridPowerKw is flat at 2.0 here, so lower = gridPowerKw - up_kw distinguishes the slots.
    expect(band.lower(futureRow1!)).toBeCloseTo(2.0 - 3.0);
    expect(band.lower(futureRow2!)).toBeCloseTo(2.0 - 5.0);
    expect(band.lower(futureRow1!)).not.toBeCloseTo(band.lower(futureRow2!)!);
  });
});
