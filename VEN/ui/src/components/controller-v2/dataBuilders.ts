/**
 * Controller V2 — pure data builder functions.
 *
 * All functions are side-effect-free and unit-testable.
 *
 * Nomenclature: tariff = X/kWh (unit price); rate = X/h (instantaneous flow).
 * The API type `RateSnapshot` holds per-kWh tariff data despite the name.
 */

import type {
  AssetId,
  AssetSummary,
  AssetTimePoint,
  StackedAreaPoint,
  TariffSnapshot,
  TariffTimePoint,
} from "./types";
import { ASSET_COLORS } from "./types";
import type {
  Plan,
  RateSnapshot,
  SimSnapshot,
  TraceEntry,
  UserRequest,
} from "../../api/types";

// ─── findCurrentTariff ────────────────────────────────────────────────────────

/**
 * Return the RateSnapshot whose interval covers tsMs.
 * If no exact match, returns the most recent past interval.
 * Returns null if tariffs is empty.
 *
 * Note: RateSnapshot is the API type name; the data it holds is tariff (per-kWh).
 */
export function findCurrentTariff(
  tariffs: RateSnapshot[],
  tsMs: number
): RateSnapshot | null {
  if (tariffs.length === 0) return null;

  const sorted = [...tariffs].sort(
    (a, b) => new Date(a.interval_start).getTime() - new Date(b.interval_start).getTime()
  );

  let best: RateSnapshot | null = null;
  for (const t of sorted) {
    const start = new Date(t.interval_start).getTime();
    const end = new Date(t.interval_end).getTime();
    if (tsMs >= start && tsMs < end) return t;
    if (start <= tsMs) best = t;
  }
  return best;
}

// ─── deriveAssetSummaries ─────────────────────────────────────────────────────

/**
 * Build AssetSummary[] from live sim data, tariffs, user requests, and plan.
 * One entry per asset present in SimSnapshot.
 */
export function deriveAssetSummaries(
  sim: SimSnapshot,
  tariffs: RateSnapshot[],
  userRequests: UserRequest[],
  plan: Plan | null,
  nowMs: number
): AssetSummary[] {
  const currentTariff = findCurrentTariff(tariffs, nowMs);
  const summaries: AssetSummary[] = [];

  function makeSummary(
    assetId: AssetId,
    label: string,
    powerKw: number,
    socPct: number | null
  ): AssetSummary {
    const costRateEurH =
      powerKw >= 0
        ? powerKw * (currentTariff?.import_price_eur_kwh ?? 0)
        : Math.abs(powerKw) * (currentTariff?.export_price_eur_kwh ?? 0);
    const co2RateGH = powerKw * (currentTariff?.co2_g_kwh ?? 0);

    const forecastEnergyKwh = computeForecastEnergy(plan, assetId, nowMs);

    const activeRequest = findActiveRequest(userRequests, assetId, nowMs);

    return {
      assetId,
      label,
      color: ASSET_COLORS[assetId] ?? "#888",
      powerKw,
      costRateEurH,
      co2RateGH,
      socPct,
      forecastEnergyKwh,
      activeRequest,
    };
  }

  if (sim.ev) {
    summaries.push(
      makeSummary("ev", "EV", sim.ev.current_kw, sim.ev.soc * 100)
    );
  }
  if (sim.heater) {
    summaries.push(
      makeSummary("heater", "Heater", sim.heater.current_kw, null)
    );
  }
  if (sim.pv) {
    summaries.push(
      makeSummary("pv", "PV", -Math.abs(sim.pv.current_kw), null)
    );
  }
  if (sim.battery) {
    summaries.push(
      makeSummary(
        "battery",
        "Battery",
        sim.battery.current_kw,
        sim.battery.soc * 100
      )
    );
  }
  // BaseLoad is always present
  summaries.push(
    makeSummary("base_load", "Base Load", sim.base_load_w / 1000, null)
  );

  return summaries;
}

function computeForecastEnergy(
  plan: Plan | null,
  assetId: AssetId,
  nowMs: number
): number | null {
  if (!plan) return null;
  const slots = [...plan.firm_slots, ...plan.flexible_slots];
  let totalKwh = 0;
  let found = false;
  for (const slot of slots) {
    const slotEnd = new Date(slot.end).getTime();
    if (slotEnd <= nowMs) continue;
    for (const alloc of slot.allocations) {
      if (alloc.asset_id === assetId) {
        const slotStart = Math.max(new Date(slot.start).getTime(), nowMs);
        const durationH = (slotEnd - slotStart) / 3_600_000;
        totalKwh += Math.abs(alloc.power_kw) * durationH;
        found = true;
      }
    }
  }
  return found ? totalKwh : null;
}

function findActiveRequest(
  userRequests: UserRequest[],
  assetId: AssetId,
  nowMs: number
) {
  const active = userRequests.filter(
    (r) => r.asset_id === assetId && r.status === "ACTIVE"
  );
  if (active.length === 0) return null;

  // Pick the one with the earliest due time that hasn't passed, or the largest energy if all passed
  const withDue = active.map((r) => {
    const tier = r.deadlines[0];
    const dueTime = tier ? new Date(tier.latest_end) : new Date(0);
    return { r, dueTime };
  });

  const notPassed = withDue.filter((x) => x.dueTime.getTime() > nowMs);
  const chosen = notPassed.length > 0
    ? notPassed.sort((a, b) => a.dueTime.getTime() - b.dueTime.getTime())[0]
    : withDue.sort((a, b) => b.r.target_energy_kwh - a.r.target_energy_kwh)[0];

  if (!chosen) return null;
  return {
    requestedEnergyKwh: chosen.r.target_energy_kwh,
    dueTime: chosen.dueTime,
  };
}

// ─── buildAssetTimeline ───────────────────────────────────────────────────────

/**
 * Build AssetTimePoint[] for one asset by merging:
 * - Past: TraceEntry setpoints (left of nowMs, solid line)
 * - Future: Plan firm/flexible slot allocations (right of nowMs, dashed)
 */
export function buildAssetTimeline(
  assetId: AssetId,
  traceEntries: TraceEntry[],
  plan: Plan | null,
  tariffs: RateSnapshot[],
  nowMs: number
): AssetTimePoint[] {
  const points: AssetTimePoint[] = [];

  // Past: from trace
  for (const entry of traceEntries) {
    const ts = new Date(entry.ts).getTime();
    const powerKw = getTraceAssetPower(assetId, entry);
    if (powerKw === null) continue;

    const tariff = findCurrentTariff(tariffs, ts);
    const costRateEurH = tariff
      ? powerKw >= 0
        ? powerKw * (tariff.import_price_eur_kwh ?? 0)
        : Math.abs(powerKw) * (tariff.export_price_eur_kwh ?? 0)
      : null;
    const co2RateGH = tariff
      ? powerKw * (tariff.co2_g_kwh ?? 0)
      : null;

    points.push({ ts, powerKw, costRateEurH, co2RateGH, isPast: ts < nowMs });
  }

  // Future: from plan allocations
  if (plan) {
    const slots = [...plan.firm_slots, ...plan.flexible_slots].sort(
      (a, b) => new Date(a.start).getTime() - new Date(b.start).getTime()
    );
    for (const slot of slots) {
      const ts = new Date(slot.start).getTime();
      if (ts < nowMs) continue;
      for (const alloc of slot.allocations) {
        if (alloc.asset_id === assetId) {
          const powerKw = alloc.power_kw;
          const tariff = findCurrentTariff(tariffs, ts);
          const costRateEurH = tariff
            ? powerKw >= 0
              ? powerKw * (tariff.import_price_eur_kwh ?? 0)
              : Math.abs(powerKw) * (tariff.export_price_eur_kwh ?? 0)
            : null;
          const co2RateGH = tariff
            ? powerKw * (tariff.co2_g_kwh ?? 0)
            : null;
          points.push({ ts, powerKw, costRateEurH, co2RateGH, isPast: false });
        }
      }
    }
  }

  return points.sort((a, b) => a.ts - b.ts);
}

function getTraceAssetPower(
  assetId: AssetId,
  entry: TraceEntry
): number | null {
  switch (assetId) {
    case "ev":
      return entry.setpoints.ev_charge_kw;
    case "heater":
      return entry.setpoints.heater_kw;
    case "pv":
      // PV is export (negative), limit is not direct power — skip for now
      return entry.setpoints.pv_export_limit_kw !== null
        ? -Math.abs(entry.setpoints.pv_export_limit_kw)
        : null;
    default:
      return null;
  }
}

// ─── buildStackedAreaData ─────────────────────────────────────────────────────

/**
 * Build StackedAreaPoint[] from trace (past) and plan (future).
 * Each asset is split into _pos and _neg series.
 */
export function buildStackedAreaData(
  traceEntries: TraceEntry[],
  plan: Plan | null,
  nowMs: number
): StackedAreaPoint[] {
  const points: StackedAreaPoint[] = [];

  const zero = () => ({
    ev_pos: 0, ev_neg: 0,
    heater_pos: 0, heater_neg: 0,
    pv_pos: 0, pv_neg: 0,
    battery_pos: 0, battery_neg: 0,
    base_load_pos: 0, base_load_neg: 0,
  });

  // Past: from trace
  for (const entry of traceEntries) {
    const ts = new Date(entry.ts).getTime();
    const ev = entry.setpoints.ev_charge_kw;
    const heater = entry.setpoints.heater_kw;
    const pvLimit = entry.setpoints.pv_export_limit_kw;
    const pv = pvLimit !== null ? -Math.abs(pvLimit) : 0;

    points.push({
      ts,
      ...zero(),
      ev_pos: Math.max(0, ev),
      ev_neg: Math.min(0, ev),
      heater_pos: Math.max(0, heater),
      heater_neg: Math.min(0, heater),
      pv_pos: Math.max(0, pv),
      pv_neg: Math.min(0, pv),
    });
  }

  // Future: from plan
  if (plan) {
    const slots = [...plan.firm_slots, ...plan.flexible_slots].sort(
      (a, b) => new Date(a.start).getTime() - new Date(b.start).getTime()
    );
    for (const slot of slots) {
      const ts = new Date(slot.start).getTime();
      if (ts < nowMs) continue;
      const pt: StackedAreaPoint = { ts, ...zero() };
      for (const alloc of slot.allocations) {
        const kw = alloc.power_kw;
        switch (alloc.asset_id as AssetId) {
          case "ev":
            pt.ev_pos = Math.max(0, kw);
            pt.ev_neg = Math.min(0, kw);
            break;
          case "heater":
            pt.heater_pos = Math.max(0, kw);
            pt.heater_neg = Math.min(0, kw);
            break;
          case "pv":
            pt.pv_pos = Math.max(0, kw);
            pt.pv_neg = Math.min(0, kw);
            break;
          case "battery":
            pt.battery_pos = Math.max(0, kw);
            pt.battery_neg = Math.min(0, kw);
            break;
        }
      }
      if (slot.allocations.some((a) => a.asset_id === "base_load")) {
        // base_load planned via baseline_kw if available
      }
      points.push(pt);
    }
  }

  return points.sort((a, b) => a.ts - b.ts);
}

// ─── buildTariffTimeline ──────────────────────────────────────────────────────

/**
 * Build TariffTimePoint[] for the Tariff Cell graph.
 * Merges RateSnapshot intervals with trace net power (past) and plan net_import_kw (future).
 */
export function buildTariffTimeline(
  tariffs: RateSnapshot[],
  traceEntries: TraceEntry[],
  plan: Plan | null,
  _nowMs: number
): TariffTimePoint[] {
  if (tariffs.length === 0) return [];

  // Build a map of ts → net grid power from trace
  const traceNetPower = new Map<number, number>();
  for (const entry of traceEntries) {
    // trace doesn't directly have net_power — we don't have a reliable per-tick net from trace alone
    // Use null for past grid power in this simplified implementation
    const ts = new Date(entry.ts).getTime();
    traceNetPower.set(ts, 0); // placeholder
  }

  // Build a map of ts → net_import_kw from plan
  const planNetPower = new Map<number, number>();
  if (plan) {
    for (const slot of [...plan.firm_slots, ...plan.flexible_slots]) {
      const ts = new Date(slot.start).getTime();
      planNetPower.set(ts, slot.net_import_kw - slot.net_export_kw);
    }
  }

  return tariffs.map((t) => {
    const ts = new Date(t.interval_start).getTime();
    const isForecast = t.is_forecast;
    const importP = t.import_price_eur_kwh;
    const exportP = t.export_price_eur_kwh;
    const co2 = t.co2_g_kwh;

    // Grid power: use plan data for future, null for past
    const gridPowerKw = isForecast
      ? (planNetPower.get(ts) ?? null)
      : null;

    const totalCostRateEurH =
      gridPowerKw !== null && importP !== null && exportP !== null
        ? gridPowerKw >= 0
          ? gridPowerKw * importP
          : Math.abs(gridPowerKw) * exportP
        : null;

    return {
      ts,
      importPriceEurKwh: importP,
      exportPriceEurKwh: exportP,
      co2GKwh: co2,
      totalCostRateEurH,
      gridPowerKw,
      isForecast,
    };
  }).sort((a, b) => a.ts - b.ts);
}

// ─── deriveTariffSnapshot ─────────────────────────────────────────────────────

/**
 * Build the current TariffSnapshot for the Tariff Cell left section.
 */
export function deriveTariffSnapshot(
  sim: SimSnapshot,
  tariffs: RateSnapshot[],
  nowMs: number
): TariffSnapshot {
  const t = findCurrentTariff(tariffs, nowMs);
  const gridPowerKw = sim.net_power_w / 1000;
  const importP = t?.import_price_eur_kwh ?? null;
  const exportP = t?.export_price_eur_kwh ?? null;
  const totalCostRateEurH =
    gridPowerKw >= 0
      ? gridPowerKw * (importP ?? 0)
      : Math.abs(gridPowerKw) * (exportP ?? 0);

  return {
    importPriceEurKwh: importP,
    exportPriceEurKwh: exportP,
    co2GKwh: t?.co2_g_kwh ?? null,
    totalCostRateEurH,
    gridPowerKw,
  };
}
