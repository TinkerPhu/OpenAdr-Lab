import type { TariffSnapshot as ApiTariffSnapshot } from "../../api/types";
import type { AssetTimelinePoint, TariffTimePoint } from "./types";

const ASSET_IDS = ["ev", "heater", "pv", "battery", "base_load"] as const;

// ─── Internal LOCF tariff lookup ─────────────────────────────────────────────

type TariffLookupEntry = {
  ts: number;
  importEurKwh: number | null;
  exportEurKwh: number | null;
  co2GKwh: number | null;
};

/** Convert ApiTariffSnapshot[] to a sorted lookup table (ascending ts). */
function buildTariffLookup(tariffs: ApiTariffSnapshot[]): TariffLookupEntry[] {
  return tariffs
    .map((t) => ({
      ts: new Date(t.interval_start).getTime(),
      importEurKwh: t.import_tariff_eur_kwh ?? null,
      exportEurKwh: t.export_tariff_eur_kwh ?? null,
      co2GKwh: t.co2_g_kwh ?? null,
    }))
    .sort((a, b) => a.ts - b.ts);
}

/** LOCF: return the last entry with ts ≤ pointTs, or null if none. */
function locfTariffAt(pointTs: number, lookup: TariffLookupEntry[]): TariffLookupEntry | null {
  for (let i = lookup.length - 1; i >= 0; i--) {
    if (lookup[i].ts <= pointTs) return lookup[i];
  }
  return null;
}

// ─── Public builders ──────────────────────────────────────────────────────────

// 1. Tariff price steps from /tariffs (one point per interval_start)
export function buildTariffPricePoints(tariffs: ApiTariffSnapshot[]): TariffTimePoint[] {
  return tariffs.map((t) => ({
    ts: new Date(t.interval_start).getTime(),
    importPriceEurKwh: t.import_tariff_eur_kwh ?? null,
    exportPriceEurKwh: t.export_tariff_eur_kwh ?? null,
    co2GKwh: t.co2_g_kwh ?? null,
    totalCostRateEurH: null,
    totalCo2RateGH: null,
    gridPowerKw: null,
  }));
}

// 2. Power + cost points from timeline (grid history has power_kw; cost_rate_eur_h and
//    co2_rate_g_h are only present for future plan slots — history and now-point lack them)
export function buildPowerPoints(points: AssetTimelinePoint[]): TariffTimePoint[] {
  return points.map((p) => ({
    ts: p.ts,
    importPriceEurKwh: null,
    exportPriceEurKwh: null,
    co2GKwh: null,
    totalCostRateEurH: p.values?.["cost_rate_eur_h"] ?? null,
    totalCo2RateGH: p.values?.["co2_rate_g_h"] ?? null,
    gridPowerKw: p.values?.["power_kw"] ?? null,
  }));
}

/**
 * Fill null totalCostRateEurH and totalCo2RateGH on merged (sorted by ts)
 * TariffTimePoints that have gridPowerKw, using LOCF over tariffs from /tariffs.
 *
 * Grid history and the now-point carry only power_kw (backend stores no tariff in
 * the history ring buffer). /tariffs includes historical intervals, so LOCF always
 * finds an applicable rate within the 1-hour history window.
 *
 * Cost rate sign convention:
 *   import (power ≥ 0): positive  — cost to the user
 *   export (power < 0): negative  — revenue for the user
 *
 * CO₂ rate sign convention:
 *   import (power ≥ 0): positive  — grid emissions attributed to the site
 *   export (power < 0): negative  — displaced grid emissions (clean export)
 */
export function fillCostRateFromTariffs(
  points: TariffTimePoint[],
  tariffs: ApiTariffSnapshot[]
): TariffTimePoint[] {
  const lookup = buildTariffLookup(tariffs);
  return points.map((p) => {
    if (p.gridPowerKw === null) return p;
    if (p.totalCostRateEurH !== null && p.totalCo2RateGH !== null) return p;
    const entry = locfTariffAt(p.ts, lookup);
    if (!entry) return p;

    const powerKw = p.gridPowerKw;
    const costRate: number | null =
      p.totalCostRateEurH !== null
        ? p.totalCostRateEurH
        : powerKw >= 0
          ? entry.importEurKwh !== null ? powerKw * entry.importEurKwh : null
          : entry.exportEurKwh !== null ? powerKw * entry.exportEurKwh : null; // negative
    const co2Rate: number | null =
      p.totalCo2RateGH !== null
        ? p.totalCo2RateGH
        : entry.co2GKwh !== null ? powerKw * entry.co2GKwh : null; // negative when exporting

    return { ...p, totalCostRateEurH: costRate, totalCo2RateGH: co2Rate };
  });
}

/**
 * Enrich every asset timeline in allTimelines with correct per-timestamp
 * cost_rate_eur_h and co2_rate_g_h for history/now-point entries.
 *
 * History points from the backend carry only power_kw. The backend emits
 * cost/CO₂ rates only for future plan-slot allocations (already PV-corrected).
 *
 * Import (power_kw ≥ 0):
 *   gridFraction(t) = min(1, grid_import_kw(t) / sum_positive_asset_powers(t))
 *   cost_rate_eur_h = power_kw × gridFraction × import_tariff   (≥ 0, cost)
 *   co2_rate_g_h    = power_kw × gridFraction × co2_g_kwh       (≥ 0)
 *
 * Export (power_kw < 0):
 *   cost_rate_eur_h = power_kw × export_tariff                   (≤ 0, revenue)
 *   co2_rate_g_h    = power_kw × co2_g_kwh                       (≤ 0, displaced)
 *   gridFraction does not apply — exporters are always grid-facing.
 *
 * Sign convention matches backend plan-slot allocations (negative = credit/revenue).
 *
 * Returns a new allTimelines record with enriched per-asset arrays; the grid and
 * any other non-asset keys are passed through unchanged.
 */
export function enrichAllAssetTimelines(
  allTimelines: Record<string, AssetTimelinePoint[]>,
  tariffs: ApiTariffSnapshot[]
): Record<string, AssetTimelinePoint[]> {
  const lookup = buildTariffLookup(tariffs);

  // Build ts → {gridKw, totalConsumeKw} for gridFraction computation.
  // The uniform grid (RF-05c) guarantees all timelines share the same ts values.
  type PointInfo = { gridKw: number; totalConsumeKw: number };
  const infoByTs = new Map<number, PointInfo>();

  for (const gp of allTimelines["grid"] ?? []) {
    const gridKw = Math.max(0, gp.values?.["power_kw"] ?? 0);
    infoByTs.set(gp.ts, { gridKw, totalConsumeKw: 0 });
  }

  for (const assetId of ASSET_IDS) {
    for (const p of allTimelines[assetId] ?? []) {
      const info = infoByTs.get(p.ts);
      if (info) {
        info.totalConsumeKw += Math.max(0, p.values?.["power_kw"] ?? 0);
      }
    }
  }

  // Enrich each asset's timeline.
  const result: Record<string, AssetTimelinePoint[]> = {};

  for (const assetId of ASSET_IDS) {
    result[assetId] = (allTimelines[assetId] ?? []).map((p) => {
      const vals = p.values;
      if (!vals || vals["power_kw"] == null) return p;
      if (vals["cost_rate_eur_h"] != null) return p; // plan slot — already correct

      const entry = locfTariffAt(p.ts, lookup);
      if (!entry) return p;

      const powerKw = vals["power_kw"];
      const newValues: Record<string, number> = { ...vals };

      if (powerKw >= 0) {
        // Consuming: scale by gridFraction so PV-covered load costs zero.
        const info = infoByTs.get(p.ts);
        const gridFraction =
          info && info.totalConsumeKw > 0
            ? Math.min(1, info.gridKw / info.totalConsumeKw)
            : 0;
        const effectiveKw = powerKw * gridFraction;
        if (entry.importEurKwh !== null) {
          newValues["cost_rate_eur_h"] = effectiveKw * entry.importEurKwh;
        }
        if (entry.co2GKwh !== null) {
          newValues["co2_rate_g_h"] = effectiveKw * entry.co2GKwh;
        }
      } else {
        // Exporting: revenue is negative (credit), gridFraction does not apply.
        if (entry.exportEurKwh !== null) {
          newValues["cost_rate_eur_h"] = powerKw * entry.exportEurKwh; // ≤ 0
        }
        if (entry.co2GKwh !== null) {
          newValues["co2_rate_g_h"] = powerKw * entry.co2GKwh; // ≤ 0 (displaced)
        }
      }

      return { ...p, values: newValues };
    });
  }

  // Pass through grid and any other non-asset timelines unchanged.
  for (const key of Object.keys(allTimelines)) {
    if (!(ASSET_IDS as readonly string[]).includes(key)) {
      result[key] = allTimelines[key];
    }
  }

  return result;
}
