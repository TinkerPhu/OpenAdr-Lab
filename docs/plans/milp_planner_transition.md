# MILP Planner Transition Plan

## Context

The HEMS planner has gone through two iterations, both failing in different ways:

1. **Greedy scheduler** (`planner.rs`, ~1700 lines) — myopic per-step rules engine. Could not
   plan optimally across the full horizon because each step was decided independently without
   knowledge of future prices or asset states.

2. **LP pre-planner** (`lp_planner.rs`, ~280 lines) — replaced the greedy battery charge logic
   with a linear program. Improved battery scheduling but the underlying model has no binary
   variables, so it cannot model discrete power setpoints (EV minimum charge current, heater
   mid/full power levels). EV and heater continued to use the greedy rules engine.

A MILP (Mixed-Integer Linear Program) prototype has been validated separately
(`d:/Tinker/milp_demo/`) and solves both problems. The design document is
`d:/Tinker/milp_demo/ems_optimization_model_design.md`; the reference Rust implementation
is `d:/Tinker/milp_demo/src/main.rs` (uses the same `good_lp` + HiGHS stack already present
in the project).

---

## Assessment Summary

The MILP design is a strong fit. Key findings:

- Solver infrastructure (`good_lp` + HiGHS) is already a project dependency — no new crates needed.
- Time discretization (N steps × Δt) is identical to the existing `total_steps` / `slot_h` variables.
- All MILP inputs are already assembled by `build_grid()` in `planner.rs` — tariffs, PV forecast,
  baseline load, import/export caps per slot. The data pipeline does not change.
- Battery parameters in `BatteryConfig` fully cover the MILP battery model.
- The MILP directly fixes both failure modes: full-horizon joint optimization (vs. greedy myopia)
  and binary variables for discrete power setpoints (EV semi-continuous, heater mid/full tiers).
- The `run_planner()` function signature and caller interfaces (dispatcher, monitor) do not change.

---

## Target Architecture

```
run_planner()
  ├── build_milp_inputs()       # replaces build_grid() + pre-planner preprocessing
  │     ├── tariff/capacity pipeline  (unchanged)
  │     ├── PV forecast per slot      (unchanged)
  │     └── EnergyPacket → LoadMode translation  (new)
  ├── solve_milp()              # good_lp / HiGHS MILP model
  └── translate_output_to_plan() # SolveOutput → Plan + Vec<PlanStep>
```

The public function signature stays the same:

```rust
pub fn run_planner(
    assets: &SimState,
    tariffs: &TariffTimeSeries,
    packets: &[EnergyPacket],
    capacity: &OadrCapacityState,
    reservations: &ReservationLayer,  // → collapses to capacity limit inputs
    profile: &Profile,
    now: DateTime<Utc>,
    trigger: PlanTrigger,
) -> (Plan, Vec<PlanStep>)
```

---

## Input Mapping

| MILP Parameter | Current Source | Notes |
|---|---|---|
| `c_imp[t]`, `c_exp[t]` | `PlanTimeSlot.import_tariff_eur_kwh / export_tariff_eur_kwh` | Direct |
| `g_imp[t]` | `PlanTimeSlot.co2_g_kwh` | Direct |
| `p_imp_max_cont[t]` | `OadrCapacityState.import_limit_kw` via `slot.import_cap_kw` | Direct |
| `p_exp_max_cont[t]` | `OadrCapacityState.export_limit_kw` via `slot.export_cap_kw` | Direct |
| `p_pv[t]` | `pv_kw_map` (per-slot PV forecast) | Direct |
| `p_base[t]` | `baseline_kw` from `BaseLoadConfig` or profile | Direct |
| Battery all params | `BatteryConfig` (capacity_kwh, max_charge_kw, max_discharge_kw, min_soc, round_trip_efficiency, initial_soc) | Direct |
| `a_ev[t]` (plugged mask) | `AssetState::Ev.plugged` per tick | Need to build mask across horizon |
| `p_ev_max_kw` | `EvConfig.max_charge_kw` | Direct |
| `p_ev_min_kw` | **MISSING** — must add to `EvConfig` | ~1.4–2.3 kW typical; add with default |
| `e_ev_core_kwh`, `t_deadline` | `EnergyPacket` with EV asset_id | Via LoadMode translation |
| `p_heat_mid_kw`, `p_heat_full_kw` | `HeaterConfig.max_kw` (only one level today) | Heater needs a `mid_kw` field added |
| `e_heat_req_kwh`, `t_deadline` | `EnergyPacket` with heater asset_id | Via LoadMode translation |
| `w_*` objective weights | Hardcoded constants (`CO2_WEIGHT`, etc.) | Promote to `PlannerConfig` |
| `c_bat_wear_eur_kwh` | Absent | Add to `PlannerConfig` |

---

## LoadMode Translation

The MILP requires per-asset `LoadMode ∈ {must_run, may_run, must_not_run}` and an optional
`t_deadline` step index. These are derived from the active `EnergyPacket` list:

| Packet state | `ValueCurve` / deadline tier | → MILP `LoadMode` |
|---|---|---|
| No packet for this asset | — | `must_not_run` |
| ABANDONED / CANCELLED packet | — | `must_not_run` |
| ACTIVE/PENDING, firm deadline (`DeadlineTier::Hard`) | — | `must_run` + `t_deadline` |
| ACTIVE/PENDING, soft deadline or budget ceiling | `ValueCurve::Budget` | `may_run` + `t_deadline` |
| Asset present, no deadline | — | `may_run` (no deadline) |

---

## Output Translation

The MILP produces `SolveOutput` with per-step arrays. These must be mapped back to
`Plan` / `PlanTimeSlot` for downstream consumers (dispatcher, ledger, timeline API):

| `SolveOutput` field | → `PlanTimeSlot` / `Plan` field |
|---|---|
| `p_bat_ch[t]`, `p_bat_dis[t]` | `allocations` entry for `battery` asset |
| `p_ev[t]` | `allocations` entry for `ev` asset |
| `z_heat_mid[t]`, `z_heat_full[t]` → `p_heat[t]` | `allocations` entry for `heater` asset |
| `p_imp[t]`, `p_exp[t]` | `net_import_kw`, `net_export_kw` |
| `e_bat[t]` (SoC trajectory) | New field: `plan.soc_trajectory_kwh: Vec<f64>` |
| `objective_eur` | New field: `plan.objective_eur: f64` |
| Cost decomposition | New field: `plan.cost_breakdown: CostBreakdown` |

**Surplus allocation:** Given the power balance, the surplus fraction for each asset allocation
is: `surplus_power_kw = min(pv_forecast_kw - baseline_kw, 0.0).abs().min(asset_power_kw)`.
The grid fraction is `grid_power_kw = asset_power_kw - surplus_power_kw`.

**`marginal_value`:** The greedy planner computed this from `CalcCache`. For the MILP, this
field can be set to the dual variable of the energy constraint (if extractable from HiGHS) or
to the import tariff as a reasonable proxy. Mark as approximate in docs.

---

## Flexibility Envelope

The current `envelope.rs` derives `FlexibilityEnvelope` from FLEXIBLE slot residuals using
`flexibility_policy.rs`. With the MILP:

- FLEXIBLE slots still exist in the output plan (the MILP covers the full horizon; the
  firm/flexible tag is applied to output slots based on the existing `near_horizon` boundary).
- Flexibility is naturally available from the solution: assets not fully utilized by their
  deadline have remaining capacity in FLEXIBLE slots.
- `envelope.rs` can be simplified to a post-solve pass that reads the solved asset power
  arrays instead of running the policy simulations.
- `flexibility_policy.rs` becomes unnecessary if its only consumer is `envelope.rs`.

---

## Files to Remove

| File | Lines | Reason |
|---|---|---|
| `controller/planner.rs` | ~1700 | Entire greedy rules engine replaced |
| `controller/lp_planner.rs` | ~280 | Battery-only LP superseded by MILP |
| `controller/thresholds.rs` | ~35 | Greedy algorithm constants only |
| `controller/flexibility_policy.rs` | ~429 | Policy simulations replaced by post-solve pass |
| `controller/reservation.rs` | ~139 | Capacity reservation collapses into `p_imp_max_cont[t]` inputs |

Total: ~2600 lines removed.

## Files to Simplify

| File | Current Lines | Expected After | Reason |
|---|---|---|---|
| `controller/envelope.rs` | ~350 | ~80 | Rewritten as post-solve pass |

## Files to Add

| File | Purpose |
|---|---|
| `controller/milp_planner.rs` | MILP model, input builder, output translator |

## Files Kept Unchanged

`controller/dispatcher.rs`, `openadr_interface.rs`, `monitor.rs`, `reporter.rs`,
`user_request.rs`, `trace.rs`, `timeline.rs` — all untouched.

All of `entities/` — `Plan`, `PlanTimeSlot`, `EnergyPacket`, `TariffTimeSeries` — kept,
extended with the new output fields noted above.

---

## Profile Changes Required

### `EvConfig` — add `p_ev_min_kw`

```rust
#[serde(default = "default_ev_min_charge")]
pub min_charge_kw: f64,   // minimum charge power when plugged (semi-continuous lower bound)

fn default_ev_min_charge() -> f64 { 1.4 }  // typical EVSE minimum
```

### `HeaterConfig` — add `mid_kw`

```rust
#[serde(default = "default_heater_mid")]
pub mid_kw: f64,  // mid-power level; defaults to max_kw / 2.0
```

Or derive it from `max_kw` if only a single-level heater is present (set `mid_kw = max_kw`,
`full_kw = max_kw`; MILP then treats it as an on/off device).

### `PlannerConfig` — add objective weights

```rust
#[serde(default = "default_w_ghg")]
pub w_ghg: f64,                   // €/kgCO₂ (default 0.0001 ≈ €100/tonne)

#[serde(default = "default_bat_wear")]
pub c_bat_wear_eur_kwh: f64,      // €/kWh cycling cost (default 0.03)

#[serde(default = "default_w_grid")]
pub w_grid: f64,                  // grid exchange penalty (default 0.0)
```

### `PlannerMode` — add `Milp` variant

```rust
pub enum PlannerMode {
    Rules,   // legacy greedy (kept temporarily during transition)
    Lp,      // battery-only LP (kept temporarily)
    Milp,    // full MILP (new default after transition)
}
```

---

## Plan Output Extensions

New fields on `Plan` (serialized to `GET /plan` response):

```rust
/// Battery SoC trajectory [kWh] at the end of each plan step (length = num_steps + 1)
pub soc_trajectory_kwh: Vec<f64>,

/// Total MILP objective value (€, includes all cost and reward terms)
pub objective_eur: f64,

/// Decomposed cost components for diagnostics
pub cost_breakdown: CostBreakdown,
```

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostBreakdown {
    pub c_energy_eur: f64,
    pub c_ghg_eur: f64,
    pub c_grid_eur: f64,
    pub c_wear_eur: f64,
    pub c_violations_eur: f64,
    pub v_services_eur: f64,
}
```

---

## UI Visualization Extensions

The MILP output enables charts that were previously not possible:

### 1. SoC Trajectory Chart (new)
- X-axis: planning horizon timeline
- Y-axis: battery SoC in kWh (or %)
- Source: `plan.soc_trajectory_kwh`
- Overlaid with `e_bat_min_kwh` and `e_bat_max_kwh` bounds

### 2. Per-Asset Stacked Power Chart (new)
- X-axis: time slots
- Y-axis: kW
- Stacked areas: EV, battery charge, battery discharge, heater, base load, PV
- Grid import/export as a line overlay
- Source: `PlanTimeSlot.allocations` (same structure, now populated by MILP)

### 3. Objective Decomposition Panel (new)
- Summary card showing `CostBreakdown` fields
- Source: `plan.cost_breakdown`

### 4. Existing planner viz page (`014-planner-viz-page`)
- Extend with the SoC and power charts above
- Keep the existing tariff/timeline strips
- The firm/flexible slot boundary divider is preserved

---

## Implementation Steps

1. **Profile additions** — add `min_charge_kw` to `EvConfig`, `mid_kw` to `HeaterConfig`,
   objective weights to `PlannerConfig`, `Milp` variant to `PlannerMode`. Update profile
   YAML files with sensible defaults.

2. **Plan entity extensions** — add `soc_trajectory_kwh`, `objective_eur`, `CostBreakdown`
   to `entities/plan.rs`. Keep all existing fields.

3. **Write `milp_planner.rs`** — input builder + MILP model (adapted from demo) + output
   translator. Entry point: `pub fn run_planner(...)` (same signature as current planner).

4. **Wire into `PlannerMode::Milp`** — the dispatcher calls `run_planner()` unchanged;
   internally the function branches on `profile.planner.mode`. Set `Milp` as the new default
   in profiles.

5. **Simplify `envelope.rs`** — rewrite as a post-solve pass over MILP output. Delete
   `flexibility_policy.rs` if its only consumer was `envelope.rs`.

6. **Remove dead code** — delete `planner.rs`, `lp_planner.rs`, `thresholds.rs`,
   `reservation.rs` after verifying no remaining callers. Remove `flexibility_policy.rs`.
   Remove `PlannerMode::Rules` and `PlannerMode::Lp` variants once transition is confirmed.

7. **BDD test update** — existing BDD scenarios that assert on plan output will need updates
   for the new response shape (new `soc_trajectory_kwh` field, revised `allocations`
   population logic).

8. **UI extensions** — add SoC trajectory chart, per-asset power chart, objective decomposition
   panel to the planner viz page.

---

## Risks and Mitigations

| Risk | Mitigation |
|---|---|
| MILP solve time too slow for 20s replan cycle | HiGHS solves comparable problems in <100ms; add `WithTimeLimit` fallback that returns last valid plan if solver exceeds budget |
| Infeasible problem (e.g. EV deadline physically unreachable) | Use soft constraints (slack variables) for deadline energy — already in the design |
| `FlexibilityEnvelope` consumers break during simplification | Keep `FlexibilityEnvelope` struct unchanged; only rewrite how it's populated |
| BDD test breakage from plan shape changes | Run full BDD suite after each step; fix before proceeding |
| `PacketAllocation.marginal_value` semantics change | Document as approximate (tariff proxy) for now; no consumer currently makes hard decisions on this field |

---

## Deferred (out of scope for this transition)

- **Washing machine asset** — MILP supports it fully (`y_wm[s]` start-time selector). Defer
  until a WM asset is added to the simulator.
- **Stochastic / scenario-based planning** — not needed; deterministic MILP is sufficient.
- **V2G** — not in current scope.
- **LP relaxation duals for marginal value** — replace tariff proxy with actual dual variables
  once the basic transition is stable.
