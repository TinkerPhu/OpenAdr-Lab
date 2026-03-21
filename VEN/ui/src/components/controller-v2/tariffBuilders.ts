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

// 2. Power + cost points from timeline (future plan slots only — grid history is empty)
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
