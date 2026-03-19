/**
 * Controller V2 — shared display types.
 *
 * Nomenclature: tariff = X/kWh (unit price); rate = X/h (instantaneous flow).
 * The VEN API endpoint GET /rates and type RateSnapshot return tariff data despite the name.
 */

// ─── Asset identifiers ────────────────────────────────────────────────────────

export type AssetId = "ev" | "heater" | "pv" | "battery" | "base_load";

// ─── Asset color palette (fixed per asset type) ───────────────────────────────

export const ASSET_COLORS: Record<string, string> = {
  ev: "#2196F3",
  heater: "#FF5722",
  pv: "#FFC107",
  battery: "#9C27B0",
  base_load: "#607D8B",
};

// ─── Summary (left section of each asset cell) ───────────────────────────────

export type UserRequestSummary = {
  requestedEnergyKwh: number;
  dueTime: Date;
};

export type AssetSummary = {
  assetId: AssetId;
  label: string;
  color: string;
  /** Signed kW — positive = import from grid, negative = export */
  powerKw: number;
  /** Derived: |powerKw| × current tariff [€/kWh] → rate [€/h] */
  costRateEurH: number;
  /** Derived: powerKw × co2_g_kwh → CO₂eq rate [g CO₂eq/h] */
  co2RateGH: number;
  /** State of charge [0–100], null for non-SoC assets */
  socPct: number | null;
  /** Sum of plan allocations for visible window [kWh], null if no plan */
  forecastEnergyKwh: number | null;
  /** Closest active user request, null if none */
  activeRequest: UserRequestSummary | null;
};

// ─── Timeline (mid section of each asset cell) ───────────────────────────────

/**
 * Backend-sourced timeline point from GET /timeline/{asset_id}.
 * ts is epoch ms (parsed from the ISO string the API returns).
 * values is a sparse map: keys present depend on the asset and data availability.
 * Common keys: "power_kw", "cost_rate_eur_h", "co2_rate_g_h"
 * Grid keys also include: "import_price_eur_kwh", "export_price_eur_kwh", etc.
 */
export type AssetTimelinePoint = {
  /** Epoch ms — X-axis value */
  ts: number;
  /** Sparse values map — NaN values filtered out by the backend */
  values: Record<string, number>;
};

/** @deprecated Use AssetTimelinePoint instead. Kept until dataBuilders cleanup (T031). */
export type AssetTimePoint = {
  /** Epoch ms — X-axis value */
  ts: number;
  /** Signed kW — null if no data at this point */
  powerKw: number | null;
  /** Derived cost rate [€/h] — null if no tariff data */
  costRateEurH: number | null;
  /** Derived CO₂eq rate [g CO₂eq/h] — null if no tariff data */
  co2RateGH: number | null;
  /** true if ts < nowMs (solid line); false = dashed (future plan) */
  isPast: boolean;
};

// ─── Tariff (grid tariff cell) ───────────────────────────────────────────────

/** Current tariff conditions snapshot for the Tariff Cell left section. */
export type TariffSnapshot = {
  importPriceEurKwh: number | null;
  exportPriceEurKwh: number | null;
  /** CO₂eq tariff [g CO₂eq/kWh] — NOT a rate */
  co2GKwh: number | null;
  /** Derived: net_power_kw × applicable tariff → cost rate [€/h] */
  totalCostRateEurH: number;
  /** Grid power [kW] = net_power_w / 1000 */
  gridPowerKw: number;
};

/** One entry per tariff interval for the Tariff Cell right-section graph. */
export type TariffTimePoint = {
  ts: number;
  importPriceEurKwh: number | null;
  exportPriceEurKwh: number | null;
  /** CO₂eq tariff [g CO₂eq/kWh] */
  co2GKwh: number | null;
  /** Derived total cost rate [€/h] at this interval */
  totalCostRateEurH: number | null;
  /** Grid power [kW] from trace (past) or plan net_import_kw (future) */
  gridPowerKw: number | null;
};

// ─── Stacked area (accumulated asset power cell) ──────────────────────────────

/**
 * One entry per time step for the stacked area chart.
 * `_pos` = Math.max(0, kw)  → stacks above x-axis (stackId="positive")
 * `_neg` = Math.min(0, kw)  → stacks below x-axis (stackId="negative")
 */
export type StackedAreaPoint = {
  ts: number;
  ev_pos: number;
  ev_neg: number;
  heater_pos: number;
  heater_neg: number;
  pv_pos: number;
  pv_neg: number;
  battery_pos: number;
  battery_neg: number;
  base_load_pos: number;
  base_load_neg: number;
  gridPowerKw: number | null;
};

// ─── UI state ────────────────────────────────────────────────────────────────

/** Cell ID format: "asset:{assetId}" | "grid:tariff" | "grid:accumulated" */
export type CellId = string;

export type PinnedState = {
  pinnedCellIds: CellId[];
};

export type CollapseState = Record<
  CellId,
  { leftCollapsed: boolean; rightCollapsed: boolean }
>;
