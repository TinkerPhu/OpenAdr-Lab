/**
 * Controller — shared display types.
 *
 * Nomenclature: tariff = X/kWh (unit price); rate = X/h (instantaneous flow).
 * The VEN API endpoint GET /rates and type RateSnapshot return tariff data despite the name.
 */

import { COLOR_IMPORT_TARIFF, COLOR_EXPORT_TARIFF } from "@lab/ui/types";
import type { AssetKind } from "../../api/generated/AssetKind";

// ─── Asset identifiers ────────────────────────────────────────────────────────

/** An asset's id: free, set by the profile (or by the request that created a shiftable load).
 * Its kind is `AssetSnapshot.asset_type`, never this string; display goes through
 * `assetDisplay.ts`. */
export type AssetId = string;

// ─── Shared UI colors ─────────────────────────────────────────────────────────

/** NOW reference line / label across all timeline charts. */
// Moved to the shared UI source (`@lab/ui`) with NowLine; re-exported so the
// controller components that already import it here keep working.
export { COLOR_NOW } from "@lab/ui/types";

/**
 * Non-asset series colors — tariff/cost/CO2/grid semantics that appear in more than one
 * chart. Single source of truth: any chart plotting one of these series looks up its
 * color here instead of defining its own constant, so the same concept never renders in
 * different colors depending on which chart draws it.
 */
export const SERIES_COLORS: Record<string, string> = {
  // Defined in the shared chart kit: the VTN's fleet view draws the same two
  // concepts, and a tariff colour that lives in the VEN app cannot be the
  // single source of truth for a chart in another app.
  import_tariff: COLOR_IMPORT_TARIFF,
  export_tariff: COLOR_EXPORT_TARIFF,
  cost_rate: "#212121",
  co2_rate: "#ff9800",
  grid_line: "#212121",
  /** Generic single-series power line (raw-diagnostics charts with no per-asset breakdown). */
  power: "#1976d2",
  /** Dynamic Operating Envelope (IMPORT/EXPORT_CAPACITY_LIMIT) — distinct from tariff
   * colors since it's a different physical quantity (kW, not €/kWh) sharing the diagram. */
  import_capacity_limit: "#5d4037",
  export_capacity_limit: "#00838f",
  /** Contracted allowance (subscription + reservation) — the capacity-limit hues, lighter. */
  import_capacity_allowance: "#a1887f",
  export_capacity_allowance: "#4dd0e1",
};

// ─── Summary (left section of each asset cell) ───────────────────────────────

export type UserRequestSummary = {
  requestedEnergyKwh: number;
  dueTime: Date;
};

export type AssetSummary = {
  assetId: AssetId;
  label: string;
  /** The asset's declared kind (`asset_type`); undefined for an asset the roster does not hold. */
  assetType: AssetKind | undefined;
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
  /** Power the asset draws regardless of its setpoint (`/sim` forced_power_kw, e.g. a
   *  heater thermostat emergency) [kW]; null while it follows its setpoint */
  forcedPowerKw: number | null;
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

/** A `/timeline/*` point with `ts` in epoch ms; stated over the generated wire type in
 *  `api/types.ts`, re-exported here for the controller components that import it. */
export type { AssetTimelinePoint } from "../../api/types";

// ─── Tariff (grid tariff cell) ───────────────────────────────────────────────

/** Current tariff conditions snapshot for the Tariff Cell left section. */
export type TariffConditions = {
  importPriceEurKwh: number | null;
  exportPriceEurKwh: number | null;
  /** CO₂eq tariff [g CO₂eq/kWh] — NOT a rate */
  co2GKwh: number | null;
  /** Derived: net_power_kw × applicable tariff → cost rate [€/h] */
  totalCostRateEurH: number;
  /** Derived: net_power_kw × co2GKwh → CO₂ rate [g/h] */
  totalCo2RateGH: number;
  /** Grid power [kW], the site meter reading (`grid.net_power_kw`) */
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
  /** Contracted import allowance [kW] — subscription + reservation (IMPORT_CAPACITY_SUBSCRIPTION/RESERVATION) */
  importAllowanceKw: number | null;
  /** Contracted export allowance [kW] — subscription + reservation (EXPORT_CAPACITY_SUBSCRIPTION/RESERVATION) */
  exportAllowanceKw: number | null;
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
