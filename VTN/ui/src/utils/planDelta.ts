/**
 * What changed between two fleet plans: the planned import and export before a tariff was
 * published, and now.
 *
 * Both plans come from the same `GET /api/fleet/plan` (the BFF's one summing rule), so nothing here
 * adds VENs up; it only reads totals off the two series. Energy is kW held for a grid step, and the
 * comparison covers only the window both plans reach: a plan adopted later runs further ahead, and
 * counting that extra time as "more energy" would report the horizon, not the tariff.
 */
import type { FleetPlan, FleetPlanPoint } from "../api/types";

export interface SeriesStats {
  energyKwh: number;
  peakKw: number;
}

type Flow = "netImportKw" | "netExportKw";

const SECONDS_PER_HOUR = 3600;

/** Energy in kWh of `kw` held for one grid step: the one place a plan's kW and step become kWh. */
const stepEnergyKwh = (kw: number, stepSeconds: number) => (kw * stepSeconds) / SECONDS_PER_HOUR;

export function seriesStats(points: FleetPlanPoint[], stepSeconds: number, flow: Flow): SeriesStats {
  const kw = points.map((p) => p[flow]);
  return {
    energyKwh: stepEnergyKwh(
      kw.reduce((a, b) => a + b, 0),
      stepSeconds,
    ),
    peakKw: kw.length === 0 ? 0 : Math.max(...kw),
  };
}

/** The running energy in kWh at the end of each slot, one value per point. */
export function cumulativeEnergyKwh(points: FleetPlanPoint[], stepSeconds: number, flow: Flow): number[] {
  let total = 0;
  return points.map((p) => (total += stepEnergyKwh(p[flow], stepSeconds)));
}

export interface DirectionDelta {
  before: SeriesStats;
  after: SeriesStats;
  /** after minus before; null when the plans share no window to compare over. */
  deltaEnergyKwh: number | null;
  deltaPeakKw: number | null;
}

export interface PlanDelta {
  import: DirectionDelta;
  export: DirectionDelta;
  /** The window both plans cover, or null when they do not overlap. */
  window: { from: string; to: string } | null;
}

const endOf = (plan: FleetPlan) =>
  plan.points.length === 0
    ? null
    : Date.parse(plan.points[plan.points.length - 1].ts) + plan.stepSeconds * 1000;

export interface PlanWindow {
  fromMs: number;
  toMs: number;
}

/** The time both plans cover, or null when either has no plan or they do not overlap. */
export function overlapWindow(before: FleetPlan, after: FleetPlan): PlanWindow | null {
  const beforeEnd = endOf(before);
  const afterEnd = endOf(after);
  if (beforeEnd === null || afterEnd === null) return null;
  const fromMs = Math.max(Date.parse(before.points[0].ts), Date.parse(after.points[0].ts));
  const toMs = Math.min(beforeEnd, afterEnd);
  return fromMs < toMs ? { fromMs, toMs } : null;
}

/** The plan's points that start inside `window`. */
export function pointsWithin(plan: FleetPlan, window: PlanWindow): FleetPlanPoint[] {
  return plan.points.filter((p) => Date.parse(p.ts) >= window.fromMs && Date.parse(p.ts) < window.toMs);
}

export function planDelta(before: FleetPlan, after: FleetPlan): PlanDelta {
  const window = overlapWindow(before, after);

  const direction = (flow: Flow): DirectionDelta => {
    const b = seriesStats(window ? pointsWithin(before, window) : [], before.stepSeconds, flow);
    const a = seriesStats(window ? pointsWithin(after, window) : [], after.stepSeconds, flow);
    return {
      before: b,
      after: a,
      deltaEnergyKwh: window ? a.energyKwh - b.energyKwh : null,
      deltaPeakKw: window ? a.peakKw - b.peakKw : null,
    };
  };

  return {
    import: direction("netImportKw"),
    export: direction("netExportKw"),
    window: window
      ? { from: new Date(window.fromMs).toISOString(), to: new Date(window.toMs).toISOString() }
      : null,
  };
}

export interface ReplanProgress {
  replanned: number;
  total: number;
  settled: boolean;
  label: string;
}

/**
 * How many of the targeted VENs now work to a plan adopted after the publish instant.
 *
 * `targets` empty means every VEN the fleet feed knows (the OpenADR reading of an empty target
 * list). A VEN with no plan at all counts as targeted and not yet replanned.
 */
export function replanProgress(
  result: FleetPlan,
  publishedAtMs: number,
  targets: string[],
): ReplanProgress {
  const known = [...result.plans.map((p) => p.venName), ...result.vensWithoutPlan];
  const targeted = new Set(targets.length === 0 ? known : targets);
  const replanned = result.plans.filter(
    (p) => targeted.has(p.venName) && Date.parse(p.createdAt) > publishedAtMs,
  ).length;
  const total = targeted.size;
  return {
    replanned,
    total,
    settled: total > 0 && replanned === total,
    label: `${replanned} of ${total} VENs replanned`,
  };
}
