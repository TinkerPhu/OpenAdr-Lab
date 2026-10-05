/**
 * What each asset's timeline chart draws beyond its power line — declared once per
 * asset, looked up by every page that renders an asset chart (Controller cells and the
 * History page both go through `AssetTimelineChart`). Adding an asset-specific line or
 * shading means adding to its entry here, not a branch on `assetId` at a call site
 * (`declare-dont-branch`).
 */
import type { StateShadingSpec } from "@lab/charts/StateShading";
import type { TimestampedRow } from "@lab/charts/mergeSeries";
import { ASSET_COLORS } from "./types";

export interface AssetChartSpec {
  /** The asset's state line on the hidden state axis: SoC (0..1) or tank temperature. */
  stateKey?: "soc" | "temp_c";
  /** Time ranges to shade, classified from each point's `values`. */
  shadings?: StateShadingSpec[];
}

// ─── PV: curtailment ──────────────────────────────────────────────────────────

const CURTAILMENT_EPS_KW = 0.05;

type CurtailmentKind = "hardware" | "planned" | "unplanned";

/** Classify one point's curtailment state from its `values` map. `null` = no shading.
 *
 * Past points carry `generation_limit_kw` (the commanded limit, present only when a limit was
 * active), `curtailment_source` (`PvCurtailmentSource::as_f64()`: 1 plan, 2 capacity,
 * 3 arbiter, 4 manual, 5 comms-loss — only meaningful alongside generation_limit_kw; the
 * live timeline and the History rows carry the same code), and `inverter_max_kw` (the
 * static hardware ceiling, live points only). Every source but the plan is
 * externally-imposed or reactive, not the plan's own forecasted choice, so all of them
 * classify as "unplanned". Future (plan) points
 * instead carry `pv_forecast_kw` next to `power_kw` — the plan's forecast is already clamped
 * to `inverter_max_kw` at solve time, so any gap there is always a planned choice, never a
 * hardware ceiling to distinguish separately.
 */
function classifyPvPoint(values: TimestampedRow["values"]): CurtailmentKind | null {
  const powerKw = values?.["power_kw"];
  if (powerKw == null) return null;

  const pvForecastKw = values?.["pv_forecast_kw"];
  if (pvForecastKw != null) {
    // Future (plan) point.
    return pvForecastKw - -powerKw > CURTAILMENT_EPS_KW ? "planned" : null;
  }

  // Past (history) point.
  const generationLimitKw = values?.["generation_limit_kw"];
  const inverterMaxKw = values?.["inverter_max_kw"];
  if (
    generationLimitKw != null &&
    Math.abs(powerKw - generationLimitKw) < CURTAILMENT_EPS_KW &&
    (inverterMaxKw == null || Math.abs(generationLimitKw) < inverterMaxKw - CURTAILMENT_EPS_KW)
  ) {
    const source = values?.["curtailment_source"];
    return source != null && source >= 2 ? "unplanned" : "planned";
  }
  if (inverterMaxKw != null && Math.abs(-powerKw - inverterMaxKw) < CURTAILMENT_EPS_KW) {
    return "hardware";
  }
  return null;
}

/** Hardware-capped (neutral), planned (amber), unplanned (red). Painted over the lines. */
const PV_CURTAILMENT_SHADING: StateShadingSpec<CurtailmentKind> = {
  key: "pv-curtailment",
  layer: "overlay",
  classify: (values) => {
    const kind = classifyPvPoint(values);
    return kind ? { kind, weight: 1 } : null;
  },
  styles: {
    hardware: { rgb: "120,120,120", alpha: 0.075 },
    planned: { rgb: "230,160,20", alpha: 0.09 },
    unplanned: { rgb: "210,30,30", alpha: 0.11 },
  },
};

// ─── EV: unplugged ────────────────────────────────────────────────────────────

/** `"#2196F3"` → `"33,150,243"`, so the shading takes its hue from the asset's one colour. */
function hexToRgb(hex: string): string {
  const n = parseInt(hex.slice(1), 16);
  return `${(n >> 16) & 255},${(n >> 8) & 255},${n & 255}`;
}

type UnpluggedKind = "unplugged" | "predicted_away";

/**
 * The band marks *absence*: no band means the EV is there to charge, like every asset
 * that is always present. `plugged` is the EV's own value, 1 = plugged — measured on
 * past points (a 0..1 fraction once a bucket or a history minute spans an unplug) and the
 * plan's predicted presence on future ones. The weight is the unplugged share, so a
 * half-plugged minute is half as dark. A point with no `plugged` value is not shaded:
 * nothing was said, and inventing an absence would be worse than showing none.
 */
const EV_UNPLUGGED_SHADING: StateShadingSpec<UnpluggedKind> = {
  key: "ev",
  layer: "background",
  classify: (values, isFuture) => {
    const plugged = values?.["plugged"];
    if (plugged == null) return null;
    return {
      kind: isFuture ? "predicted_away" : "unplugged",
      weight: Math.min(1, Math.max(0, 1 - plugged)),
    };
  },
  styles: {
    unplugged: { rgb: hexToRgb(ASSET_COLORS.ev), alpha: 0.28 },
    predicted_away: { rgb: hexToRgb(ASSET_COLORS.ev), alpha: 0.14 },
  },
};

// ─── The table ────────────────────────────────────────────────────────────────

const ASSET_CHART_SPECS: Record<string, AssetChartSpec> = {
  ev: { stateKey: "soc", shadings: [EV_UNPLUGGED_SHADING] },
  battery: { stateKey: "soc" },
  heater: { stateKey: "temp_c" },
  pv: { shadings: [PV_CURTAILMENT_SHADING] },
};

const NO_SPEC: AssetChartSpec = {};

/** Unknown assets (shiftable loads, base load) draw the power line alone. */
export function assetChartSpec(assetId: string): AssetChartSpec {
  return ASSET_CHART_SPECS[assetId] ?? NO_SPEC;
}
