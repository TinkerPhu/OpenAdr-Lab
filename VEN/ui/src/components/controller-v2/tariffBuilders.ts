import type { TariffSnapshot as ApiTariffSnapshot } from "../../api/types";
import type { AssetTimelinePoint, TariffTimePoint } from "./types";

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
 * Fill null totalCostRateEurH on merged (sorted by ts) points that have gridPowerKw,
 * by LOCF-looking up the applicable import tariff from the same array.
 *
 * Grid history and the now-point carry only power_kw (backend stores no tariff in the
 * history ring buffer). /tariffs provides intervals going back in time, so scanning
 * backward finds the active tariff for any timestamp in the history window.
 *
 * Formula matches the backend plan-slot computation: max(0, power_kw) * import_tariff.
 */
export function fillCostRateFromTariffs(points: TariffTimePoint[]): TariffTimePoint[] {
  return points.map((p, i) => {
    if (p.totalCostRateEurH !== null || p.gridPowerKw === null) return p;
    // Scan backward for the nearest preceding point with a known import tariff.
    let applicable: number | null = null;
    for (let j = i; j >= 0; j--) {
      if (points[j].importPriceEurKwh !== null) {
        applicable = points[j].importPriceEurKwh;
        break;
      }
    }
    if (applicable === null) return p;
    return { ...p, totalCostRateEurH: Math.max(0, p.gridPowerKw) * applicable };
  });
}
