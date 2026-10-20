import { describe, it, expect } from "vitest";
import {
  cumulativeEnergyKwh,
  overlapWindow,
  planDelta,
  replanProgress,
  seriesStats,
} from "../utils/planDelta";
import type { FleetPlan, FleetPlanPoint } from "../api/types";

const T0 = Date.parse("2026-10-18T10:00:00Z");
const STEP = 3600; // one-hour grid keeps the arithmetic readable: 1 kW for a step is 1 kWh

const point = (i: number, importKw: number, exportKw = 0, vens = 2): FleetPlanPoint => ({
  ts: new Date(T0 + i * STEP * 1000).toISOString(),
  netImportKw: importKw,
  netExportKw: exportKw,
  contributingVens: vens,
});

function plan(points: FleetPlanPoint[], over: Partial<FleetPlan> = {}): FleetPlan {
  return {
    at: new Date(T0).toISOString(),
    stepSeconds: STEP,
    points,
    plans: [],
    vensWithoutPlan: [],
    ...over,
  };
}

describe("seriesStats", () => {
  it("is the energy in kWh (kW held for a step) and the peak in kW", () => {
    const s = seriesStats([point(0, 2), point(1, 4), point(2, 1)], STEP, "netImportKw");
    expect(s.energyKwh).toBe(7);
    expect(s.peakKw).toBe(4);
  });

  it("is zero for no points rather than undefined", () => {
    expect(seriesStats([], STEP, "netImportKw")).toEqual({ energyKwh: 0, peakKw: 0 });
  });

  it("uses the step it is given: 15-minute slots hold a quarter of a kWh per kW", () => {
    expect(seriesStats([point(0, 4), point(1, 4)], 900, "netImportKw").energyKwh).toBe(2);
  });
});

describe("cumulativeEnergyKwh", () => {
  it("is the running energy in kWh at the end of each slot", () => {
    expect(cumulativeEnergyKwh([point(0, 2), point(1, 4), point(2, 1)], STEP, "netImportKw")).toEqual([2, 6, 7]);
  });

  it("ends where seriesStats ends: one conversion from kW and a step to kWh", () => {
    const points = [point(0, 4), point(1, 4), point(2, 3)];
    const running = cumulativeEnergyKwh(points, 900, "netImportKw");
    expect(running[running.length - 1]).toBe(
      seriesStats(points, 900, "netImportKw").energyKwh,
    );
  });

  it("is empty for no points", () => {
    expect(cumulativeEnergyKwh([], STEP, "netImportKw")).toEqual([]);
  });
});

describe("overlapWindow", () => {
  it("is the time both plans cover, from the later start to the earlier end", () => {
    const before = plan([point(0, 4), point(1, 4), point(2, 4)]);
    const after = plan([point(1, 4), point(2, 4), point(3, 4)]);
    expect(overlapWindow(before, after)).toEqual({ fromMs: T0 + STEP * 1000, toMs: T0 + 3 * STEP * 1000 });
  });

  it("is null when the plans do not overlap or one has no plan", () => {
    expect(overlapWindow(plan([point(0, 4)]), plan([point(5, 4)]))).toBeNull();
    expect(overlapWindow(plan([]), plan([point(0, 4)]))).toBeNull();
  });
});

describe("planDelta", () => {
  it("states the change per direction, after minus before", () => {
    const before = plan([point(0, 4, 0), point(1, 8, 0), point(2, 4, 0)]);
    const after = plan([point(0, 4, 0), point(1, 4, 2), point(2, 4, 0)]);
    const d = planDelta(before, after);
    expect(d.import.before).toEqual({ energyKwh: 16, peakKw: 8 });
    expect(d.import.after).toEqual({ energyKwh: 12, peakKw: 4 });
    expect(d.import.deltaEnergyKwh).toBe(-4);
    expect(d.import.deltaPeakKw).toBe(-4);
    expect(d.export.deltaEnergyKwh).toBe(2);
    expect(d.export.deltaPeakKw).toBe(2);
  });

  it("compares only the window both plans cover, so a longer horizon is not a change", () => {
    // The newer plan starts an hour later and runs an hour further.
    const before = plan([point(0, 4), point(1, 4), point(2, 4)]);
    const after = plan([point(1, 4), point(2, 4), point(3, 4)]);
    const d = planDelta(before, after);
    expect(d.import.before.energyKwh).toBe(8);
    expect(d.import.after.energyKwh).toBe(8);
    expect(d.import.deltaEnergyKwh).toBe(0);
    expect(d.window).toEqual({
      from: new Date(T0 + STEP * 1000).toISOString(),
      to: new Date(T0 + 3 * STEP * 1000).toISOString(),
    });
  });

  it("has no window, and no pretend delta, when the plans do not overlap", () => {
    const d = planDelta(plan([point(0, 4)]), plan([point(5, 4)]));
    expect(d.window).toBeNull();
    expect(d.import.deltaEnergyKwh).toBeNull();
    expect(d.export.deltaPeakKw).toBeNull();
  });

  it("has no delta when either side has no plan", () => {
    expect(planDelta(plan([]), plan([point(0, 4)])).window).toBeNull();
  });
});

describe("replanProgress", () => {
  const published = T0;
  const plans = (created: string[]): FleetPlan["plans"] =>
    created.map((c, i) => ({ venName: `ven-${i + 1}`, planId: `p${i}`, createdAt: c }));
  const before = new Date(T0 - 60_000).toISOString();
  const after = new Date(T0 + 60_000).toISOString();

  it("counts the targeted VENs whose plan was adopted after the publish", () => {
    const result = plan([point(0, 1)], { plans: plans([after, after, before]), vensWithoutPlan: [] });
    const p = replanProgress(result, published, []);
    expect(p).toMatchObject({ replanned: 2, total: 3, settled: false });
    expect(p.label).toBe("2 of 3 VENs replanned");
  });

  it("is settled when every targeted VEN has", () => {
    const result = plan([point(0, 1)], { plans: plans([after, after]) });
    const p = replanProgress(result, published, []);
    expect(p).toMatchObject({ replanned: 2, total: 2, settled: true });
  });

  it("counts a VEN with no plan at all among the targeted, as not yet replanned", () => {
    const result = plan([point(0, 1)], { plans: plans([after]), vensWithoutPlan: ["ven-9"] });
    expect(replanProgress(result, published, [])).toMatchObject({ replanned: 1, total: 2 });
  });

  it("counts only the VENs the event targets when it names some", () => {
    const result = plan([point(0, 1)], {
      plans: plans([after, before, after]),
      vensWithoutPlan: ["ven-9"],
    });
    const p = replanProgress(result, published, ["ven-1", "ven-2"]);
    expect(p).toMatchObject({ replanned: 1, total: 2 });
  });
});
