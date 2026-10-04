/**
 * The shading is checked by reading values, not by rendering a chart — the same
 * approach dayNightShading.test.ts takes for the other render-function primitives.
 */
import { describe, it, expect } from "vitest";
import {
  stateShadingRuns,
  renderStateShading,
  type StateShadingSpec,
} from "@lab/charts/StateShading";
import type { TimestampedRow } from "@lab/charts/mergeSeries";

const minute = 60_000;
const row = (min: number, values: Record<string, number> | null): TimestampedRow => ({
  ts: min * minute,
  values,
});

/** "on" rows are shaded with weight `w` (default 1); everything else is not. */
const classify: StateShadingSpec<"on" | "later">["classify"] = (values, isFuture) => {
  const w = values?.["w"];
  if (w == null) return null;
  return { kind: isFuture ? "later" : "on", weight: w };
};

const spec: StateShadingSpec<"on" | "later"> = {
  key: "demo",
  layer: "background",
  classify,
  styles: {
    on: { rgb: "1,2,3", alpha: 0.4 },
    later: { rgb: "1,2,3", alpha: 0.2, dashedOutline: true },
  },
};

describe("stateShadingRuns", () => {
  it("joins consecutive rows in the same state into one run ending where the state ends", () => {
    const runs = stateShadingRuns(
      [row(0, { w: 1 }), row(1, { w: 1 }), row(2, {}), row(3, {})],
      classify,
    );
    expect(runs).toEqual([{ x1: 0, x2: 2 * minute, kind: "on", weight: 1 }]);
  });

  it("yields no run for rows that classify to null or to weight 0", () => {
    expect(stateShadingRuns([row(0, {}), row(1, null), row(2, { w: 0 })], classify)).toEqual([]);
  });

  it("splits a run where the weight changes", () => {
    const runs = stateShadingRuns([row(0, { w: 1 }), row(1, { w: 0.5 }), row(2, {})], classify);
    expect(runs).toEqual([
      { x1: 0, x2: minute, kind: "on", weight: 1 },
      { x1: minute, x2: 2 * minute, kind: "on", weight: 0.5 },
    ]);
  });

  it("splits a run where the kind changes at now", () => {
    const runs = stateShadingRuns(
      [row(0, { w: 1 }), row(1, { w: 1 }), row(2, { w: 1 }), row(3, {})],
      classify,
      { nowMs: 1 * minute },
    );
    expect(runs).toEqual([
      { x1: 0, x2: 2 * minute, kind: "on", weight: 1 },
      { x1: 2 * minute, x2: 3 * minute, kind: "later", weight: 1 },
    ]);
  });

  it("extends the final run by one step, since no later row ends it", () => {
    const runs = stateShadingRuns([row(0, {}), row(5, { w: 1 }), row(10, { w: 1 })], classify);
    expect(runs).toEqual([{ x1: 5 * minute, x2: 15 * minute, kind: "on", weight: 1 }]);
  });

  it("does not span a data gap longer than maxGapMs", () => {
    // Shaded at 0 and 1, then nothing sampled until minute 240, shaded again.
    const runs = stateShadingRuns(
      [row(0, { w: 1 }), row(1, { w: 1 }), row(240, { w: 1 }), row(241, {})],
      classify,
      { maxGapMs: 5 * minute },
    );
    expect(runs).toEqual([
      { x1: 0, x2: 2 * minute, kind: "on", weight: 1 },
      { x1: 240 * minute, x2: 241 * minute, kind: "on", weight: 1 },
    ]);
  });

  it("joins across irregular spacing when no maxGapMs is given", () => {
    const runs = stateShadingRuns(
      [row(0, { w: 1 }), row(5, { w: 1 }), row(35, { w: 1 }), row(65, {})],
      classify,
    );
    expect(runs).toEqual([{ x1: 0, x2: 65 * minute, kind: "on", weight: 1 }]);
  });
});

describe("renderStateShading", () => {
  const props = (rows: TimestampedRow[], nowMs?: number) =>
    renderStateShading("power", rows, spec, { nowMs }).map(
      (el) => el.props as Record<string, unknown>,
    );

  it("folds the weight into the fill alpha and pins fillOpacity to 1", () => {
    const [full, half] = props([row(0, { w: 1 }), row(1, { w: 0.5 }), row(2, {})]);
    expect(full.fill).toBe("rgba(1,2,3,0.4000)");
    expect(half.fill).toBe("rgba(1,2,3,0.2000)");
    // recharts' default of 0.5 would silently halve both.
    expect(full.fillOpacity).toBe(1);
    expect(half.fillOpacity).toBe(1);
  });

  it("names each area by spec key and kind, and outlines only the kinds that ask for it", () => {
    const [past, future] = props([row(0, { w: 1 }), row(2, { w: 1 }), row(3, {})], 1 * minute);
    expect(past.className).toBe("state-shading-demo-on");
    expect(past.strokeDasharray).toBeUndefined();
    expect(future.className).toBe("state-shading-demo-later");
    expect(future.strokeDasharray).toBeDefined();
  });
});
