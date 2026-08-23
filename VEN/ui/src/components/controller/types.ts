/**
 * Controller — shared display types.
 *
 * Nomenclature: tariff = X/kWh (unit price); rate = X/h (instantaneous flow).
 * The VEN API endpoint GET /rates and type RateSnapshot return tariff data despite the name.
 */

// ─── Asset identifiers ────────────────────────────────────────────────────────

/** Known physics assets. Dynamic shiftable loads (e.g. "wm") are also valid. */
export type AssetId = "ev" | "heater" | "pv" | "battery" | "base_load" | (string & {});

// ─── Shared UI colors ─────────────────────────────────────────────────────────

/** NOW reference line / label across all timeline charts. */
export const COLOR_NOW = "#f44336";

/** Fallback color for unknown/unregistered asset IDs. */
export const COLOR_ASSET_FALLBACK = "#888";

// ─── Asset color palette (fixed per asset type) ───────────────────────────────

export const ASSET_COLORS: Record<string, string> = {
  ev: "#2196F3",
  heater: "#FF5722",
  pv: "#FFC107",
  battery: "#9C27B0",
  base_load: "#607D8B",
  wm: "#FF9800",
  dw: "#795548",
};

/**
 * Non-asset series colors — tariff/cost/CO2/grid semantics that appear in more than one
 * chart. Single source of truth: any chart plotting one of these series looks up its
 * color here instead of defining its own constant, so the same concept never renders in
 * different colors depending on which chart draws it.
 */
export const SERIES_COLORS: Record<string, string> = {
  import_tariff: "#f44336",
  export_tariff: "#4caf50",
  cost_rate: "#212121",
  co2_rate: "#ff9800",
  grid_line: "#212121",
  /** Generic single-series power line (raw-diagnostics charts with no per-asset breakdown). */
  power: "#1976d2",
  /** Dynamic Operating Envelope (IMPORT/EXPORT_CAPACITY_LIMIT) — distinct from tariff
   * colors since it's a different physical quantity (kW, not €/kWh) sharing the diagram. */
  import_capacity_limit: "#5d4037",
  export_capacity_limit: "#00838f",
};

/** Human-readable labels for known asset IDs. */
export const ASSET_LABELS: Record<string, string> = {
  ev: "EV",
  heater: "Heater",
  pv: "PV",
  battery: "Battery",
  base_load: "Base Load",
  wm: "Washing Machine",
  dw: "Dishwasher",
};

/**
 * Planning role per asset type.
 * "forecast" = physics-predicted, non-controllable (pv, base_load).
 * "planned"  = MILP-assigned, controllable (ev, battery, heater, shiftable loads).
 * Unknown assets default to "planned" at call site.
 */
export const ASSET_PLANNING_ROLE: Record<string, "forecast" | "planned"> = {
  pv: "forecast",
  base_load: "forecast",
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
  /** Tank temperature [°C], null for non-thermal assets */
  tempC: number | null;
  /** Sum of plan allocations for visible window [kWh], null if no plan */
  forecastEnergyKwh: number | null;
  /** Closest active user request, null if none */
  activeRequest: UserRequestSummary | null;
  /** Nameplate max import (charge/consumption) power [kW], null if not applicable */
  maxImportKw: number | null;
  /** Nameplate max export (discharge/generation) power [kW], null if not applicable */
  maxExportKw: number | null;
  /** Nameplate energy capacity [kWh], null if not applicable */
  capacityKwh: number | null;
};

// ─── Timeline (mid section of each asset cell) ───────────────────────────────

/**
 * Backend-sourced timeline point from GET /timeline/{asset_id}.
 * ts is epoch ms (parsed from the ISO string the API returns).
 * values is a sparse map: keys present depend on the asset and data availability.
 * Common keys: "power_kw", "cost_rate_eur_h", "co2_rate_g_h"
 * Grid keys also include: "import_tariff_eur_kwh", "export_tariff_eur_kwh", etc.
 */
export type AssetTimelinePoint = {
  /** Epoch ms — X-axis value */
  ts: number;
  /** Sparse values map — NaN values filtered out by the backend. null for empty grid buckets. */
  values: Record<string, number> | null;
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
  /** Derived: net_power_kw × co2GKwh → CO₂ rate [g/h] */
  totalCo2RateGH: number;
  /** Grid power [kW] = net_power_w / 1000 */
  gridPowerKw: number;
};

/** One entry per tariff interval for the Tariff Cell right-section graph. */
export type TariffTimePoint = {
  ts: number;
  importPriceEurKwh: number | null;
  exportPriceEurKwh: number | null;
  /** CO₂eq tariff [g CO₂eq/kWh] — static intensity, kept for LOCF lookup */
  co2GKwh: number | null;
  /** Derived total cost rate [€/h] at this interval — negative when exporting (revenue) */
  totalCostRateEurH: number | null;
  /** Derived total CO₂ rate [g/h] at this interval — negative when exporting (displaced emissions) */
  totalCo2RateGH: number | null;
  /** Grid power [kW] from trace (past) or plan net_import_kw (future) */
  gridPowerKw: number | null;
  /** Dynamic Operating Envelope import limit [kW] — direct VTN signal (IMPORT_CAPACITY_LIMIT) */
  importLimitKw: number | null;
  /** Dynamic Operating Envelope export limit [kW] — direct VTN signal (EXPORT_CAPACITY_LIMIT) */
  exportLimitKw: number | null;
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
  /** Dynamic shiftable load pos/neg keys (e.g. wm_pos, wm_neg). */
  [key: string]: number | null;
};

// ─── UI state ────────────────────────────────────────────────────────────────

/** Cell ID format: "asset:{assetId}" | "grid:tariff" | "grid:rates" | "grid:accumulated" */
export type CellId = string;

export type CollapseState = Record<
  CellId,
  { rightCollapsed: boolean }
>;
