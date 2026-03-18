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
  Plan,
  TariffSnapshot as ApiTariffSnapshot,
  SimSnapshot,
  UserRequest,
} from "../../api/types";

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
  tariffs: ApiTariffSnapshot[],
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

  const evAsset = sim.assets["ev"];
  if (evAsset) {
    summaries.push(
      makeSummary("ev", "EV", evAsset.current_kw ?? 0, (evAsset.soc ?? 0) * 100)
    );
  }
  const heaterAsset = sim.assets["heater"];
  if (heaterAsset) {
    summaries.push(
      makeSummary("heater", "Heater", heaterAsset.current_kw ?? 0, null)
    );
  }
  const pvAsset = sim.assets["pv"];
  if (pvAsset) {
    summaries.push(
      makeSummary("pv", "PV", -Math.abs(pvAsset.current_kw ?? 0), null)
    );
  }
  const batteryAsset = sim.assets["battery"];
  if (batteryAsset) {
    summaries.push(
      makeSummary("battery", "Battery", batteryAsset.current_kw ?? 0, (batteryAsset.soc ?? 0) * 100)
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
