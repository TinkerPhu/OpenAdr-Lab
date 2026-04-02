import type { TariffSnapshot as ApiTariffSnapshot } from "../../api/types";
import type { AssetTimelinePoint, TariffTimePoint } from "./types";

// ─── Internal LOCF tariff lookup ─────────────────────────────────────────────

type TariffLookupEntry = {
  ts: number;
  importEurKwh: number | null;
  co2GKwh: number | null;
};

/** Convert ApiTariffSnapshot[] to a sorted lookup table (ascending ts). */
function buildTariffLookup(tariffs: ApiTariffSnapshot[]): TariffLookupEntry[] {
  return tariffs
    .map((t) => ({
      ts: new Date(t.interval_start).getTime(),
      importEurKwh: t.import_tariff_eur_kwh ?? null,
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
    gridPowerKw: null,
  }));
}

// 2. Power + cost points from timeline (grid history has power_kw; cost_rate_eur_h is
//    only present for future plan slots — history and now-point lack it)
export function buildPowerPoints(points: AssetTimelinePoint[]): TariffTimePoint[] {
  return points.map((p) => ({
    ts: p.ts,
    importPriceEurKwh: null,
    exportPriceEurKwh: null,
    co2GKwh: null,
    totalCostRateEurH: p.values?.["cost_rate_eur_h"] ?? null,
    gridPowerKw: p.values?.["power_kw"] ?? null,
  }));
}

/**
 * Fill null totalCostRateEurH on merged (sorted by ts) TariffTimePoints that have
 * gridPowerKw, using LOCF over tariffs from /tariffs.
 *
 * Grid history and the now-point carry only power_kw (backend stores no tariff in
 * the history ring buffer). /tariffs includes historical intervals, so LOCF always
 * finds an applicable rate within the 1-hour history window.
 *
 * Formula matches the backend plan-slot computation: max(0, power_kw) * import_tariff.
 */
export function fillCostRateFromTariffs(
  points: TariffTimePoint[],
  tariffs: ApiTariffSnapshot[]
): TariffTimePoint[] {
  const lookup = buildTariffLookup(tariffs);
  return points.map((p) => {
    if (p.totalCostRateEurH !== null || p.gridPowerKw === null) return p;
    const entry = locfTariffAt(p.ts, lookup);
    if (!entry?.importEurKwh) return p;
    return { ...p, totalCostRateEurH: Math.max(0, p.gridPowerKw) * entry.importEurKwh };
  });
}

/**
 * Fill null cost_rate_eur_h and co2_rate_g_h on AssetTimelinePoints that have
 * power_kw, using LOCF over tariffs from /tariffs.
 *
 * Per-asset history points carry only power_kw (and asset-specific state fields).
 * The backend only emits cost/CO₂ rates for future plan-slot allocations.
 *
 * Formulas: max(0, power_kw) * import_tariff  and  max(0, power_kw) * co2_g_kwh.
 * Note: this is an approximation for history — future plan rates account for PV
 * surplus credits, but for past data we use the spot import tariff directly.
 */
export function fillAssetRatesFromTariffs(
  points: AssetTimelinePoint[],
  tariffs: ApiTariffSnapshot[]
): AssetTimelinePoint[] {
  const lookup = buildTariffLookup(tariffs);
  return points.map((p) => {
    const vals = p.values;
    if (!vals || vals["power_kw"] == null) return p;
    if (vals["cost_rate_eur_h"] != null) return p; // already set by backend (plan slot)
    const entry = locfTariffAt(p.ts, lookup);
    if (!entry) return p;
    const powerKw = vals["power_kw"];
    const newValues: Record<string, number> = { ...vals };
    if (entry.importEurKwh !== null) {
      newValues["cost_rate_eur_h"] = Math.max(0, powerKw) * entry.importEurKwh;
    }
    if (entry.co2GKwh !== null) {
      newValues["co2_rate_g_h"] = Math.max(0, powerKw) * entry.co2GKwh;
    }
    return { ...p, values: newValues };
  });
}
