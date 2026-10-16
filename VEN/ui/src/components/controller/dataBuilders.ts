/**
 * Controller — pure data builder functions.
 *
 * All functions are side-effect-free and unit-testable.
 *
 * Nomenclature: tariff = X/kWh (unit price); rate = X/h (instantaneous flow).
 * The API type `TariffSnapshot` (aliased ApiTariffSnapshot here) holds per-kWh
 * tariff interval data. The component-level `TariffConditions` (from ./types)
 * is the derived current-tariff snapshot for GridTariffCell.
 */

import type {
  AssetId,
  AssetSummary,
  TariffConditions,
} from "./types";
import { assetColor, assetLabel } from "./assetDisplay";
import type { AssetKind } from "../../api/generated/AssetKind";
import type {
  AssetSnapshot,
  TariffSnapshot as ApiTariffSnapshot,
  SimSnapshot,
  UserRequest,
} from "../../api/types";
import type { AssetTimelinePoint } from "./types";
import { assetValue } from "../../api/assetValue";
import { assetsInRosterOrder } from "../../api/assets";

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

/** Same shape as computeCostRateEurH, but for CO₂ intensity — a single g/kWh figure
 * applies regardless of import/export direction (unlike tariff's asymmetric import/export
 * price), so this is just powerKw × intensity with no directional branch. */
export function computeCo2RateGH(powerKw: number, tariff: ApiTariffSnapshot | null): number {
  return powerKw * (tariff?.co2_g_kwh ?? 0);
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

const numeric = (v: unknown): number | null => (typeof v === "number" ? v : null);

/** What a Controller cell shows for an asset, read from its own snapshot. */
type SummaryFields = {
  powerKw: number;
  socPct?: number | null;
  tempC?: number | null;
  maxImportKw?: number | null;
  maxExportKw?: number | null;
  capacityKwh?: number | null;
};

/** Each asset kind's cell: what it reads from the asset's snapshot — declared per
 * type, not branched on per id (`declare-dont-branch`, W6). */
const SUMMARY_BY_TYPE: Partial<Record<AssetKind, { read: (a: AssetSnapshot) => SummaryFields }>> = {
  ev: {
    read: (a) => ({
      powerKw: a.power_kw,
      socPct: (assetValue(a, "soc") ?? 0) * 100,
      maxImportKw: numeric(assetValue(a, "max_charge_kw")),
      capacityKwh: numeric(assetValue(a, "battery_kwh")),
    }),
  },
  heater: {
    read: (a) => {
      const tempMinC = numeric(assetValue(a, "temp_min_c"));
      const tempMaxC = numeric(assetValue(a, "temp_max_c"));
      const thermalMassKwhPerC = numeric(assetValue(a, "thermal_mass_kwh_per_c"));
      return {
        powerKw: a.power_kw,
        tempC: numeric(assetValue(a, "temp_c")),
        maxImportKw: numeric(assetValue(a, "max_kw")),
        capacityKwh:
          tempMinC !== null && tempMaxC !== null && thermalMassKwhPerC !== null
            ? (tempMaxC - tempMinC) * thermalMassKwhPerC
            : null,
      };
    },
  },
  pv: {
    read: (a) => ({
      powerKw: -Math.abs(a.power_kw),
      maxExportKw: numeric(assetValue(a, "rated_kw")),
    }),
  },
  battery: {
    read: (a) => ({
      powerKw: a.power_kw,
      socPct: (assetValue(a, "soc") ?? 0) * 100,
      maxImportKw: numeric(assetValue(a, "max_charge_kw")),
      maxExportKw: numeric(assetValue(a, "max_discharge_kw")),
      capacityKwh: numeric(assetValue(a, "capacity_kwh")),
    }),
  },
  base_load: { read: (a) => ({ powerKw: a.power_kw }) },
};

/** The order the Controller lists cells in, by asset type; others follow. */
const SUMMARY_TYPE_ORDER: AssetKind[] = ["ev", "heater", "pv", "battery", "base_load"];

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
  const gridImportKw = Math.max(0, sim.grid.net_power_kw);
  const totalConsumeKw = Object.values(sim.assets).reduce(
    (sum, a) => sum + Math.max(0, a.power_kw),
    0
  );
  const gridFraction = totalConsumeKw > 0 ? Math.min(1, gridImportKw / totalConsumeKw) : 0;

  function makeSummary(
    assetId: AssetId,
    label: string,
    powerKw: number,
    socPct: number | null,
    tempC: number | null = null,
    maxImportKw: number | null = null,
    maxExportKw: number | null = null,
    capacityKwh: number | null = null,
    assetType: AssetKind | undefined = undefined
  ): AssetSummary {
    // For consuming assets (powerKw >= 0) scale by gridFraction so only the
    // grid-sourced portion incurs cost. Generating assets (powerKw < 0) are
    // passed through unchanged (export revenue is always grid-facing).
    const effectivePowerKw = powerKw >= 0 ? powerKw * gridFraction : powerKw;
    const costRateEurH = computeCostRateEurH(effectivePowerKw, currentTariff);
    const co2RateGH = effectivePowerKw * (currentTariff?.co2_g_kwh ?? 0);

    const forecastEnergyKwh = computeForecastEnergy(allTimelines[assetId] ?? [], nowMs);

    const activeRequest = findActiveRequest(userRequests, assetId, nowMs);

    const forced = sim.assets[assetId]?.forced_power_kw;
    const forcedPowerKw = typeof forced === "number" ? forced : null;

    return {
      assetId,
      assetType,
      label,
      color: assetColor(sim, assetId),
      powerKw,
      costRateEurH,
      co2RateGH,
      socPct,
      tempC,
      forcedPowerKw,
      forecastEnergyKwh,
      activeRequest,
      maxImportKw,
      maxExportKw,
      capacityKwh,
    };
  }

  // One summary per asset of the roster (W6), ordered by kind as the cells always were, then
  // by roster; each read from its own snapshot through its kind's declaration and labelled by
  // `assetLabel` ("EV · car-b" when the VEN holds two EVs).
  // Invariant this relies on: every key in `sim.assets` is a real simulator asset with a
  // `GET /capability/:asset_id` endpoint behind it. A backend-only pseudo-asset injected
  // into the same map would be summarized here as if it were a device, rendering a tile and
  // firing a capability request that 404s — which is exactly what the removed `site-residual`
  // virtual asset did (see docs/architecture/forecasting_model.md).
  const typeRank = (t: AssetKind) => {
    const i = SUMMARY_TYPE_ORDER.indexOf(t);
    return i < 0 ? SUMMARY_TYPE_ORDER.length : i;
  };
  const ordered = assetsInRosterOrder(sim).sort(
    ([, a], [, b]) => typeRank(a.asset_type) - typeRank(b.asset_type),
  );
  for (const [assetId, asset] of ordered) {
    const spec = SUMMARY_BY_TYPE[asset.asset_type];
    const label = assetLabel(sim, assetId);
    if (!spec) {
      summaries.push(
        makeSummary(assetId, label, asset.power_kw, null, null, null, null, null, asset.asset_type),
      );
      continue;
    }
    const f = spec.read(asset);
    summaries.push(
      makeSummary(
        assetId,
        label,
        f.powerKw,
        f.socPct ?? null,
        f.tempC ?? null,
        f.maxImportKw ?? null,
        f.maxExportKw ?? null,
        f.capacityKwh ?? null,
        asset.asset_type,
      ),
    );
  }

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

// ─── deriveTariffConditions ───────────────────────────────────────────────────

/**
 * Build the current TariffConditions for the GridTariffCell left section.
 */
export function deriveTariffConditions(
  sim: SimSnapshot,
  tariffs: ApiTariffSnapshot[],
  nowMs: number
): TariffConditions {
  const t = findCurrentTariff(tariffs, nowMs);
  const gridPowerKw = sim.grid.net_power_kw;
  const importP = t?.import_tariff_eur_kwh ?? null;
  const exportP = t?.export_tariff_eur_kwh ?? null;
  const totalCostRateEurH = computeCostRateEurH(gridPowerKw, t);
  const totalCo2RateGH = computeCo2RateGH(gridPowerKw, t);

  return {
    importPriceEurKwh: importP,
    exportPriceEurKwh: exportP,
    co2GKwh: t?.co2_g_kwh ?? null,
    totalCostRateEurH,
    totalCo2RateGH,
    gridPowerKw,
  };
}
