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

export function seriesStats(points: FleetPlanPoint[], stepSeconds: number, flow: Flow): SeriesStats {
  const kw = points.map((p) => p[flow]);
  return {
    energyKwh: (kw.reduce((a, b) => a + b, 0) * stepSeconds) / SECONDS_PER_HOUR,
    peakKw: kw.length === 0 ? 0 : Math.max(...kw),
  };
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

export function planDelta(before: FleetPlan, after: FleetPlan): PlanDelta {
  const beforeEnd = endOf(before);
  const afterEnd = endOf(after);
  const from =
    before.points.length && after.points.length
      ? Math.max(Date.parse(before.points[0].ts), Date.parse(after.points[0].ts))
      : null;
  const to = beforeEnd !== null && afterEnd !== null ? Math.min(beforeEnd, afterEnd) : null;
  const overlaps = from !== null && to !== null && from < to;

  const within = (plan: FleetPlan) =>
    overlaps
      ? plan.points.filter((p) => Date.parse(p.ts) >= from && Date.parse(p.ts) < to)
      : [];

  const direction = (flow: Flow): DirectionDelta => {
    const b = seriesStats(within(before), before.stepSeconds, flow);
    const a = seriesStats(within(after), after.stepSeconds, flow);
    return {
      before: b,
      after: a,
      deltaEnergyKwh: overlaps ? a.energyKwh - b.energyKwh : null,
      deltaPeakKw: overlaps ? a.peakKw - b.peakKw : null,
    };
  };

  return {
    import: direction("netImportKw"),
    export: direction("netExportKw"),
    window: overlaps
      ? { from: new Date(from).toISOString(), to: new Date(to).toISOString() }
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
