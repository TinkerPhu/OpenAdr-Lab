// One complete `Plan` / `PlanTimeSlot` for every UI test (R-133): the types are generated from
// Rust, so a fixture that builds them by hand has to state every field the wire carries. Tests
// override only what they are about.
import type { AssetAllocation, Plan, PlanTimeSlot } from "../../api/types";

export function makeSlot(overrides: Partial<PlanTimeSlot> = {}): PlanTimeSlot {
  return {
    slot_index: 0,
    start: "2026-04-04T10:00:00Z",
    end: "2026-04-04T10:05:00Z",
    import_tariff_eur_kwh: 0.12,
    export_tariff_eur_kwh: 0.05,
    co2_g_kwh: 200,
    grid_effective_cost: 0.12,
    marginal_cost_import_eur_per_kwh: 0,
    marginal_cost_export_eur_per_kwh: 0,
    rate_estimated: false,
    import_cap_kw: 10,
    export_cap_kw: 10,
    baseline_kw: 1.0,
    pv_forecast_kw: 0,
    pv_used_kw: 0,
    surplus_available_kw: 0,
    allocations: [],
    net_import_kw: 0,
    net_export_kw: 0,
    import_flexibility_kw: 0,
    export_flexibility_kw: 0,
    planned_kw_by_asset: {},
    planned_state_by_asset: {},
    forecast_by_asset: {},
    ...overrides,
  };
}

export function makeAlloc(overrides: Partial<AssetAllocation> = {}): AssetAllocation {
  return {
    asset_id: "battery",
    power_kw: 3.5,
    surplus_power_kw: 0,
    grid_power_kw: 3.5,
    marginal_value: 0.12,
    cost_eur: 0.029,
    co2_g: 58.3,
    ...overrides,
  };
}

export function makePlan(overrides: Partial<Plan> = {}): Plan {
  return {
    id: "plan-001",
    created_at: "2026-04-04T10:00:00Z",
    trigger: "PERIODIC",
    horizon: {
      start_time: "2026-04-04T10:00:00Z",
      end_time: "2026-04-05T10:00:00Z",
      step_size_s: 300,
      num_steps: 288,
      far_horizon: "2026-04-05T10:00:00Z",
      zones: [{ step_s: 300, slots: 288 }],
    },
    planned_asset_ids: [],
    slots: [makeSlot()],
    summary: { total_cost_eur: 1.0, total_co2_g: 500, total_import_kwh: 3.0, total_export_kwh: 0 },
    session_flexibility: [],
    warnings: [],
    objective: "min_cost",
    objective_eur: 0,
    friction_eur: 0,
    cost_breakdown: {
      c_energy_eur: 0,
      c_ghg_eur: 0,
      c_grid_eur: 0,
      c_wear_eur: 0,
      c_violations_eur: 0,
      v_services_eur: 0,
      c_peak_penalty_eur: 0,
    },
    solve_status: "OPTIMAL",
    phase_report: null,
    penalty_rules_active: [],
    solver_ms: null,
    mip_gap_target: null,
    ...overrides,
  };
}
