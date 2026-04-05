/**
 * Controller V2 — pure data builder functions.
 *
 * All functions are side-effect-free and unit-testable.
 *
 * Nomenclature: tariff = X/kWh (unit price); rate = X/h (instantaneous flow).
 * The API type `TariffSnapshot` (aliased ApiTariffSnapshot here) holds per-kWh
 * tariff interval data. The component-level `TariffSnapshot` (from ./types)
 * is the derived current-tariff snapshot for GridTariffCell.
 */

import type {
  AssetId,
  AssetSummary,
  TariffSnapshot,
} from "./types";
import { ASSET_COLORS } from "./types";
import type {
  TariffSnapshot as ApiTariffSnapshot,
  SimSnapshot,
  UserRequest,
} from "../../api/types";
import type { AssetTimelinePoint } from "./types";

// ─── computeCostRateEurH ─────────────────────────────────────────────────────

/**
 * Cost rate in EUR/h for a given power flow and tariff interval.
 * Positive powerKw → import cost (positive). Negative → export revenue (negative).
 * Pass the grid-attributed power for consuming assets (powerKw × gridFraction)
 * so PV-covered consumption is correctly costed at zero.
 */
export function computeCostRateEurH(
  powerKw: number,
  tariff: ApiTariffSnapshot | null
): number {
  if (powerKw >= 0) return powerKw * (tariff?.import_tariff_eur_kwh ?? 0);
  const exportRate = tariff?.export_tariff_eur_kwh ?? 0;
  if (exportRate === 0) return 0; // no export tariff → no revenue signal; avoids -0
  return powerKw * exportRate; // negative = revenue
}

// ─── findCurrentTariff ────────────────────────────────────────────────────────

/**
 * Return the ApiTariffSnapshot whose interval covers tsMs.
 * If no exact match, returns the most recent past interval.
 * Returns null if tariffs is empty.
 */
export function findCurrentTariff(
  tariffs: ApiTariffSnapshot[],
  tsMs: number
): ApiTariffSnapshot | null {
  if (tariffs.length === 0) return null;

  const sorted = [...tariffs].sort(
    (a, b) => new Date(a.interval_start).getTime() - new Date(b.interval_start).getTime()
  );

  let best: ApiTariffSnapshot | null = null;
  for (const t of sorted) {
    if (new Date(t.interval_start).getTime() <= tsMs) best = t;
    else break;
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
  tariffs: ApiTariffSnapshot[],
  userRequests: UserRequest[],
  allTimelines: Record<string, AssetTimelinePoint[]>,
  nowMs: number
): AssetSummary[] {
  const currentTariff = findCurrentTariff(tariffs, nowMs);
  const summaries: AssetSummary[] = [];

  // Grid fraction: share of total consumption that is grid-sourced.
  // Consuming assets multiply their power by this before computing cost so that
  // PV-covered (or battery-discharge-covered) load is correctly costed at zero.
  const gridImportKw = Math.max(0, sim.grid.net_power_w / 1000);
  const totalConsumeKw = Object.values(sim.assets).reduce(
    (sum, a) => sum + Math.max(0, a.power_kw),
    0
  );
  const gridFraction = totalConsumeKw > 0 ? Math.min(1, gridImportKw / totalConsumeKw) : 0;

  function makeSummary(
    assetId: AssetId,
    label: string,
    powerKw: number,
    socPct: number | null
  ): AssetSummary {
    // For consuming assets (powerKw >= 0) scale by gridFraction so only the
    // grid-sourced portion incurs cost. Generating assets (powerKw < 0) are
    // passed through unchanged (export revenue is always grid-facing).
    const effectivePowerKw = powerKw >= 0 ? powerKw * gridFraction : powerKw;
    const costRateEurH = computeCostRateEurH(effectivePowerKw, currentTariff);
    const co2RateGH = effectivePowerKw * (currentTariff?.co2_g_kwh ?? 0);

    const forecastEnergyKwh = computeForecastEnergy(allTimelines[assetId] ?? [], nowMs);

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

  const evAsset = sim.assets["ev"];
  if (evAsset) {
    summaries.push(
      makeSummary("ev", "EV", evAsset.power_kw, (evAsset.soc ?? 0) * 100)
    );
  }
  const heaterAsset = sim.assets["heater"];
  if (heaterAsset) {
    summaries.push(
      makeSummary("heater", "Heater", heaterAsset.power_kw, null)
    );
  }
  const pvAsset = sim.assets["pv"];
  if (pvAsset) {
    summaries.push(
      makeSummary("pv", "PV", -Math.abs(pvAsset.power_kw), null)
    );
  }
  const batteryAsset = sim.assets["battery"];
  if (batteryAsset) {
    summaries.push(
      makeSummary("battery", "Battery", batteryAsset.power_kw, (batteryAsset.soc ?? 0) * 100)
    );
  }
  // BaseLoad is always present
  const baseLoadAsset = sim.assets["base_load"];
  summaries.push(
    makeSummary("base_load", "Base Load", (baseLoadAsset?.power_kw ?? 0), null)
  );

  return summaries;
}

function computeForecastEnergy(
  timelinePoints: AssetTimelinePoint[],
  nowMs: number
): number | null {
  const future = timelinePoints.filter((p) => p.ts > nowMs);
  if (future.length === 0) return null;
  let totalKwh = 0;
  let found = false;
  for (let i = 0; i < future.length; i++) {
    const power = future[i].values?.["power_kw"];
    if (power === undefined || power === null) continue;
    const nextTs = future[i + 1]?.ts ?? future[i].ts;
    const prevGap = i > 0 ? future[i].ts - future[i - 1].ts : 0;
    const durationMs = i < future.length - 1 ? nextTs - future[i].ts : prevGap;
    const durationH = durationMs / 3_600_000;
    totalKwh += Math.abs(power) * durationH;
    found = true;
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

// ─── deriveTariffSnapshot ─────────────────────────────────────────────────────

/**
 * Build the current TariffSnapshot for the GridTariffCell left section.
 */
export function deriveTariffSnapshot(
  sim: SimSnapshot,
  tariffs: ApiTariffSnapshot[],
  nowMs: number
): TariffSnapshot {
  const t = findCurrentTariff(tariffs, nowMs);
  const gridPowerKw = sim.grid.net_power_w / 1000;
  const importP = t?.import_tariff_eur_kwh ?? null;
  const exportP = t?.export_tariff_eur_kwh ?? null;
  const totalCostRateEurH = computeCostRateEurH(gridPowerKw, t);

  return {
    importPriceEurKwh: importP,
    exportPriceEurKwh: exportP,
    co2GKwh: t?.co2_g_kwh ?? null,
    totalCostRateEurH,
    gridPowerKw,
  };
}
