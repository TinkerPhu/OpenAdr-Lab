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
    profile: &Profile,
    now: DateTime<Utc>,
    trigger: PlanTrigger,
) -> (Plan, Vec<PlanStep>)
```

> Note: `reservations: &ReservationLayer` has been removed. `reservation.rs` was deleted in
> the branch cleanup prior to this transition. Capacity limits flow from `OadrCapacityState` directly.

---

## Input Mapping

| MILP Parameter | Current Source | Notes |
|---|---|---|
| `c_imp[t]`, `c_exp[t]` | `TariffTimeSeries.import_eur_kwh / export_eur_kwh` sampled per slot | Direct |
| `g_imp_kgco2_kwh[t]` | `TariffTimeSeries.co2_g_kwh` sampled per slot | **Divide by 1000** — codebase stores gCO₂/kWh; MILP uses kgCO₂/kWh |
| `p_imp_max_cont_kw[t]` | `OadrCapacityState.import_limit_kw` (constant across horizon) | `None` → fall back to `GridConfig.max_import_kw` |
| `p_exp_max_cont_kw[t]` | `OadrCapacityState.export_limit_kw` (constant across horizon) | `None` → fall back to `GridConfig.max_export_kw` |
| `p_imp_max_phys_kw[t]` | `GridConfig.max_import_kw` (new profile field) | Physical meter/breaker hard limit; see Profile Changes |
| `p_exp_max_phys_kw[t]` | `GridConfig.max_export_kw` (new profile field) | Physical meter/breaker hard limit |
| `pen_imp_eur_kwh`, `pen_exp_eur_kwh` | `PlannerConfig` (new fields, default 0.0) | See Penalty Modeling section |
| `p_pv[t]` | PV asset's `forecast_kw(slot_start)` method | PV asset owns the forecast model; see PV Forecast Source section |
| `p_base[t]` | `BaseLoadConfig.baseline_kw` | Direct |
| `e_bat_nom_kwh`, `e_bat_min_kwh` | `BatteryConfig.capacity_kwh`, `min_soc × capacity_kwh` | Direct |
| `e_bat_max_kwh` | `BatteryConfig.capacity_kwh` | No max-SoC cap today; equals `e_bat_nom_kwh` |
| `e_bat_init_kwh` | **`SimState` live `BatteryState.soc × capacity_kwh`** | **Not** profile `initial_soc`; read current asset state |
| `p_bat_ch_max_kw`, `p_bat_dis_max_kw` | `BatteryConfig.max_charge_kw / max_discharge_kw` | Direct |
| `eff_bat_ch`, `eff_bat_dis` | `BatteryConfig.round_trip_efficiency` | Split: `eff = sqrt(round_trip_efficiency)` for each direction |
| `a_ev[t]` (plugged mask) | `EvState.plugged` + `t_ev_dead_step` | See EV Horizon Mask section |
| `p_ev_max_kw` | `EvConfig.max_charge_kw` | Direct |
| `p_ev_min_kw` | **MISSING** — must add to `EvConfig` | ~1.4–2.3 kW typical; add with default 1.4 |
| `e_ev_core_kwh`, `t_ev_dead_step` | `EnergyPacket` with EV asset_id | Via LoadMode translation |
| `e_ev_extra_max_kwh` | `EvConfig.battery_kwh × (1 − soc_target)` | Headroom above core; TODO: refine via user request |
| `v_ev_core_eur` | 0.0 (hardcoded) | Only used for `MayRun`; TODO: expose in user requests |
| `v_ev_extra_eur_kwh` | `PlannerConfig.v_ev_extra_eur_kwh` (new, default 0.10) | Reward per kWh of extra EV charge above core |
| `p_heat_mid_kw`, `p_heat_full_kw` | `HeaterConfig.mid_kw`, `HeaterConfig.max_kw` | `mid_kw` must be added; see Profile Changes |
| `e_heat_req_kwh`, `t_heat_dead_step` | `EnergyPacket` with heater asset_id | Via LoadMode translation |
| `v_heat_eur` | `PlannerConfig.v_heat_eur` (new, default 1.50) | See Heater Reward section |
| `w_energy` | `PlannerConfig.w_energy` (new, default 1.0) | Scales the energy cost term (import − export revenue) |
| `w_ghg` | `PlannerConfig.w_ghg` (new, default 0.0001) | €/kgCO₂ emissions weight |
| `w_grid` | `PlannerConfig.w_grid` (new, default 0.0) | Weight on total grid exchange volume |
| `c_bat_wear_eur_kwh` | `PlannerConfig.c_bat_wear_eur_kwh` (new, default 0.03) | Battery cycling wear cost |
| `w_viol` | `PlannerConfig.w_viol` (new, default 1.0) | Scales violation penalties |
| `w_services` | 1.0 (hardcoded) | Service reward multiplier; always 1.0 for now |
| `wm_mode` | `MustNotRun` (hardcoded) | Washing machine deferred; binary WM vars disabled |

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

## EV Horizon Mask

The MILP `a_ev[t]` (plugged/available flag per slot) is built as follows:

1. If `ev.plugged == false` in the current `SimState` → `a_ev = [false; N]` (EV absent, `MustNotRun`)
2. If `ev.plugged == true`:
   - Set `a_ev[t] = true` for all slots `t` where `slot_start ≤ t_ev_dead_step`
   - Set `a_ev[t] = false` for slots after the deadline (EV assumed to leave at departure)
   - If no EV packet exists (no deadline), set `a_ev[t] = true` for the full horizon

The deadline timestamp from `EnergyPacket` drives the departure assumption. A future improvement could use a learned departure schedule, but the packet deadline is the only available signal today.

---

## PV Forecast Source

The PV forecast per slot (`p_pv[t]`) is provided by the PV asset's own model, not implemented inside the planner. The PV simulator already implements the physics model (`sin(π·(hour−6)/12)` for 6am–6pm). The input builder should call a `forecast_kw(slot_start: DateTime<Utc>) -> f64` method on `PvConfig`, iterating over each planning slot.

This keeps the forecast model co-located with the PV asset. When a real PV forecast API is integrated in the future, only `PvConfig` changes — the planner input builder stays the same.

---

## Battery Allocation in Plan Slots

Battery power (`p_bat_ch[t]` / `p_bat_dis[t]`) is **not** stored in `PlanTimeSlot.allocations`. The `PacketAllocation` struct requires a `packet_id: Uuid`, and battery cycling is managed autonomously without a user packet.

Instead, battery setpoints are stored in two new fields directly on `PlanTimeSlot`:

```rust
/// Planned battery charge power this slot (kW, ≥ 0)
pub bat_charge_kw: f64,
/// Planned battery discharge power this slot (kW, ≥ 0)
pub bat_discharge_kw: f64,
```

The dispatcher reads these directly alongside `allocations`. The full SoC trajectory is available on `Plan.soc_trajectory_kwh`.

---

## Heater Reward (`v_heat_eur`)

In the MILP, `v_heat_eur` is a **comfort preference weight** used only when `heater_mode = MayRun`. It is added to the objective as a negative term (reward) when `z_heat_ready = 1`, meaning the heater has satisfied its energy requirement by the deadline.

The optimizer balances this reward against the import cost of heating:
- If `v_heat_eur > expected heating cost` → optimizer will heat
- If `v_heat_eur < expected heating cost` → optimizer may skip heating

This makes `v_heat_eur` a tuneable comfort knob: set it high to prioritize warmth regardless of tariff; set it low to allow heating to be deferred or skipped.

Default: `PlannerConfig.v_heat_eur = 1.50 €`, matching the MILP demo value. At typical heating power (3 kW) and a 300-slot 5-min grid, this threshold is exceeded only when the entire required heating energy costs more than €1.50 in import tariffs — a reasonable default that ensures heating in most normal-price periods.

---

## Penalty Modeling (Open Question)

The MILP demo uses `pen_imp_eur_kwh` / `pen_exp_eur_kwh` as a per-kWh violation cost when the contractual capacity limit is breached (via slack variables). This gives the optimizer a soft incentive to stay within the OpenADR limit.

Real-world OpenADR capacity limit violations are typically penalized differently: a **monthly peak demand charge** applies once the limit is exceeded (a step function, not per-kWh). Modelling this correctly would require tracking cumulative violations within the billing period and a binary "penalty incurred" variable with a large fixed cost — architecturally more complex.

**Decision for this implementation:** Set `pen_imp_eur_kwh = pen_exp_eur_kwh = 0.0` and treat the contractual limit as effectively hard by setting `p_imp_max_phys_kw = p_imp_max_cont_kw` (contractual = physical when no separate physical limit is active). The slack variable infrastructure remains in the MILP model for future activation.

The fields exist in `PlannerConfig` with `#[serde(default)]` so the penalty model can be enabled without code changes once designed.

---

## Output Translation

The MILP produces `SolveOutput` with per-step arrays. These must be mapped back to
`Plan` / `PlanTimeSlot` for downstream consumers (dispatcher, ledger, timeline API):

| `SolveOutput` field | → `PlanTimeSlot` / `Plan` field |
|---|---|
| `p_bat_ch[t]`, `p_bat_dis[t]` | New fields `bat_charge_kw`, `bat_discharge_kw` on `PlanTimeSlot` (see Battery Allocation section) |
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

> **Already done.** All five files below were deleted in branch cleanup commits prior to this
> transition (`remove remainders of old planners`, `rename controller_v2 to controller`).

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

| File | Purpose | Status |
|---|---|---|
| `controller/milp_planner.rs` | MILP model, input builder, output translator | Stub exists; body to be implemented |

## Files Kept Unchanged

`controller/dispatcher.rs`, `openadr_interface.rs`, `monitor.rs`, `reporter.rs`,
`user_request.rs`, `trace.rs`, `timeline.rs` — all untouched.

All of `entities/` — `Plan`, `PlanTimeSlot`, `EnergyPacket`, `TariffTimeSeries` — kept,
extended with the new output fields noted above.

---

## Profile Changes Required

### `EvConfig` — add `min_charge_kw`

```rust
/// Minimum charge power when plugged in (kW). EVSE semi-continuous lower bound:
/// if charging at all, power must be at least this value (no trickle).
#[serde(default = "default_ev_min_charge")]
pub min_charge_kw: f64,

fn default_ev_min_charge() -> f64 { 1.4 }  // typical EVSE minimum (6A × 230V)
```

### `HeaterConfig` — add `mid_kw`

```rust
/// Mid-power level (kW). Used by the MILP to model a two-level heater (mid / full).
/// If the heater only has one level, set mid_kw = max_kw; the MILP treats it as on/off.
/// Default: max_kw / 2.0
#[serde(default)]  // derived at runtime if absent: mid_kw = max_kw / 2.0
pub mid_kw: Option<f64>,
```

### `GridConfig` — new profile section (physical grid limits)

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct GridConfig {
    /// Physical import limit at the meter or main breaker (kW).
    /// Hard ceiling — the MILP uses this as `p_imp_max_phys_kw`.
    /// When no OpenADR capacity event is active, also used as the contractual limit.
    /// Default: 25.0 kW (typical residential 3-phase 32A supply)
    #[serde(default = "default_max_import_kw")]
    pub max_import_kw: f64,

    /// Physical export limit (inverter / grid-tie maximum) (kW).
    /// Default: 10.0 kW
    #[serde(default = "default_max_export_kw")]
    pub max_export_kw: f64,
}
```

### `PlannerConfig` — add objective weights and `PlannerObjective`

```rust
/// Scales the energy cost term (import tariff cost − export revenue).
/// 1.0 = full economic optimization. 0.0 = ignore energy cost (e.g. pure GHG mode).
/// Default: 1.0
#[serde(default = "default_w_energy")]
pub w_energy: f64,

/// Weight on GHG emissions: equivalent €/kgCO₂ added to objective.
/// Default: 0.0001 ≈ €100/tonne CO₂ — a light carbon price signal.
#[serde(default = "default_w_ghg")]
pub w_ghg: f64,

/// Penalty per kWh of total grid exchange (import + export), in €/kWh.
/// Drives the optimizer to self-consume and minimize grid dependency.
/// Default: 0.0 — disabled (pure cost optimization).
#[serde(default = "default_w_grid")]
pub w_grid: f64,

/// Battery cycling wear cost in €/kWh charged or discharged.
/// Prevents excessive cycling when price arbitrage margin is thin.
/// Default: 0.03 €/kWh — typical lithium battery degradation proxy.
#[serde(default = "default_bat_wear")]
pub c_bat_wear_eur_kwh: f64,

/// Scales contractual limit violation penalties. 1.0 = normal; 0.0 = disabled.
/// See Penalty Modeling section.
/// Default: 1.0
#[serde(default = "default_w_viol")]
pub w_viol: f64,

/// Per-kWh penalty for exceeding the contractual import limit (€/kWh slack).
/// Default: 0.0 (disabled — see Penalty Modeling section).
#[serde(default)]
pub pen_imp_eur_kwh: f64,

/// Per-kWh penalty for exceeding the contractual export limit (€/kWh slack).
/// Default: 0.0 (disabled).
#[serde(default)]
pub pen_exp_eur_kwh: f64,

/// Reward per kWh of EV charging above the core energy requirement.
/// Incentivises opportunistic top-up charging when cost is low.
/// Default: 0.10 €/kWh
#[serde(default = "default_v_ev_extra")]
pub v_ev_extra_eur_kwh: f64,

/// Reward for meeting the heater energy deadline (€, used in MayRun mode only).
/// See Heater Reward section. Acts as a comfort preference knob.
/// Default: 1.50 €
#[serde(default = "default_v_heat")]
pub v_heat_eur: f64,

/// Optimization objective preset. Selects a named weight configuration.
/// Individual weight fields above can still be used with `Custom` to fine-tune.
#[serde(default)]
pub objective: PlannerObjective,
```

### `PlannerObjective` — optimization preset enum

Replaces the concept of `PlannerMode` (which was about algorithm selection). All presets use
the MILP solver; they differ only in what the solver is asked to minimize/maximize.

```rust
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlannerObjective {
    /// Minimize energy bill. Balanced weights: energy cost + light GHG + light grid penalty + wear.
    /// (w_energy=1, w_ghg=0.20, w_grid=0.02, c_bat_wear=0.03)
    #[default]
    MinCost,

    /// Minimize carbon emissions above all else.
    /// (w_energy=0, w_ghg=10, w_grid=0, c_bat_wear=0)
    MinGhg,

    /// Minimize grid exchange volume (maximize self-consumption).
    /// (w_energy=0, w_ghg=0, w_grid=1, c_bat_wear=0)
    MinGrid,

    /// Maximize revenue from export and grid services.
    /// (w_energy=1, w_ghg=0, w_grid=0, c_bat_wear=0.03)
    MaxRevenue,

    /// Use the individual weight fields directly without any preset override.
    Custom,
}
```

When `objective != Custom`, the preset's weight ratios are applied at solve time, overriding
the individual fields. This lets operators switch objectives in YAML without tuning weights.

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
   `GridConfig` (physical limits), and all new `PlannerConfig` fields (weights, `PlannerObjective`).
   Update profile YAML files with sensible defaults.

2. **Plan entity extensions** — add `soc_trajectory_kwh`, `objective_eur`, `CostBreakdown`
   to `entities/plan.rs`; add `bat_charge_kw`, `bat_discharge_kw` to `PlanTimeSlot`.
   Keep all existing fields.

3. **Write `milp_planner.rs`** — input builder (mapping functions) + MILP model (adapted from
   demo, using dynamic `Vec<f64>` instead of `[f64; N]`) + output translator.
   Entry point: `pub fn run_planner(...)` (same signature as current stub).

4. **No mode branching needed** — `planner.rs` and `lp_planner.rs` are already removed.
   `run_planner()` in `milp_planner.rs` is the only planner. The dispatcher calls it unchanged.
   `PlannerObjective` selects the weight preset at solve time.

5. **Add `forecast_kw` to PV asset** — expose a `forecast_kw(ts: DateTime<Utc>) -> f64` method
   on `PvConfig` (or a free function in `assets/pv.rs`) so the MILP input builder can query the
   PV forecast per slot without embedding the model in the planner.

6. **Simplify `envelope.rs`** — rewrite as a post-solve pass over MILP output. Delete
   `flexibility_policy.rs` if its only consumer was `envelope.rs`.

7. **Dead code already removed** — `planner.rs`, `lp_planner.rs`, `thresholds.rs`,
   `reservation.rs`, and `flexibility_policy.rs` were deleted in the branch cleanup commits
   prior to this transition. No action needed.

8. **BDD test update** — existing BDD scenarios that assert on plan output will need updates
   for the new response shape (new fields, `bat_charge_kw` / `bat_discharge_kw` replacing
   battery allocations, `soc_trajectory_kwh`).

9. **UI extensions** — add SoC trajectory chart, per-asset power chart, objective decomposition
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
- **Monthly peak demand penalty** — the per-kWh `pen_imp_eur_kwh` model does not correctly
  represent monthly demand charges. Requires a step-cost binary variable approach. Deferred;
  `pen_*` fields default to 0.0 and the infrastructure is in place for future activation.
- **PV forecast improvements** — replace the `sin(π·(hour−6)/12)` physics model with a
  real forecast API (e.g. weather service or NWP integration). The interface is already
  decoupled: only `PvConfig.forecast_kw()` needs updating.
- **EV departure schedule** — replace the "plugged until deadline" horizon mask with a
  learned or user-provided departure time profile.
- **Preset fine-tuning via user requests** — `v_ev_core_eur` and per-request comfort values
  should eventually flow from user request settings, not hardcoded defaults.

---

## Implementation Plan (Phased)

The transition is too large for a single commit. The phases below are ordered so that BDD
stays green through Phase 3 — the planner stub is untouched until Phase 4.

### Phase 1 — Foundations (no behavior change)

All struct/field additions. No logic changes.

- `EvConfig.min_charge_kw`, `HeaterConfig.mid_kw`, `GridConfig` (new profile section)
- `PlannerConfig` new weight fields, `PlannerObjective` enum
- `Plan.soc_trajectory_kwh`, `Plan.objective_eur`, `Plan.cost_breakdown: CostBreakdown`
- `PlanTimeSlot.bat_charge_kw`, `PlanTimeSlot.bat_discharge_kw`
- `PvConfig.forecast_kw(ts: DateTime<Utc>) -> f64` method

**Gate:** `cargo check` clean. All new fields carry `#[serde(default)]` so API responses
don't change shape. BDD fully green.

### Phase 2 — Input Builder (testable in isolation)

Add `MilpInputs` struct (dynamic `Vec<f64>` version of the demo's `Inputs`) and
`build_milp_inputs(assets, tariffs, packets, capacity, profile, now) -> MilpInputs`.

Contains all the tricky mappings: CO₂ `/1000`, `sqrt(rte)` efficiency split, live SoC from
`SimState`, EV horizon mask, `LoadMode` translation per packet.

**Gate:** Unit tests cover each mapping rule individually. Planner stub untouched — BDD green.

### Phase 3 — MILP Solver (testable in isolation)

Port `solve_demo()` from `d:/Tinker/milp_demo/` to
`solve_milp(inputs: &MilpInputs, weights: &Weights) -> Result<SolveOutput>`.
Dynamic N (`Vec` not `[f64; N]`). WM vars disabled (`MustNotRun`).

**Gate:** Unit tests: synthetic inputs → feasible solution, battery SoC trajectory plausible,
terminal constraint satisfied. Planner stub untouched — BDD green.

### Phase 4 — Output Translator + Wire-up (flip the switch)

Add `translate_to_plan(sol, inputs, profile, now, trigger, packets) -> (Plan, Vec<PlanStep>)`.
Replace the stub body in `run_planner()` with the full pipeline:
`build_milp_inputs → solve_milp → translate_to_plan`.

**Gate:** `GET /plan` returns a real MILP solution. Smoke-test on Pi4.
**Expect BDD failures here** — fix them in Phase 5.

### Phase 5 — BDD Fixes

Run full BDD suite. Fix assertions that break on the new plan shape
(`soc_trajectory_kwh`, `bat_charge_kw`, `cost_breakdown`, revised allocations).

**Gate:** All BDD green.

### Phase 6 — Envelope Simplification

Rewrite `envelope.rs` as a post-solve pass reading `bat_charge_kw`/`bat_discharge_kw`
and `allocations` from the solved plan. Low-risk after the solver output is stable.

**Gate:** BDD still green. `envelope.rs` under 100 lines.

### Phase 7 — UI Extensions (can be deferred)

SoC trajectory chart, per-asset stacked power chart, objective decomposition panel
on the planner viz page (see UI Visualization Extensions section above).
