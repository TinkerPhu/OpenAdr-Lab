## ADDED Requirements

### Requirement: Planner has a PV-export decision variable
The MILP planner SHALL represent PV export as a bounded decision variable `p_pv_used[t]` for each
planning slot `t`, satisfying `0 ≤ p_pv_used[t] ≤ p_pv_kw[t]` where `p_pv_kw[t]` is the existing
forecast input. The power-balance constraint in both solver phases SHALL use `p_pv_used[t]`
instead of the raw forecast constant. The objective SHALL NOT contain any direct cost term for
`p_pv_used[t]` — curtailment below the forecast SHALL only ever be chosen because it relieves an
existing constraint (export capacity limit or its soft-violation penalty), never as a cost-neutral
free choice.

#### Scenario: Uncurtailed slot keeps full PV forecast
- **WHEN** a planning slot has no active export capacity constraint that PV generation would
  violate
- **THEN** the solved `p_pv_used[t]` equals `p_pv_kw[t]` for that slot

#### Scenario: Export cap forces curtailment
- **WHEN** a planning slot's PV forecast exceeds the contractual or physical export limit for that
  slot
- **THEN** the solver reduces `p_pv_used[t]` below `p_pv_kw[t]` by exactly the amount needed to
  satisfy the export limit, rather than incurring the soft-violation penalty when a zero-cost
  curtailed solution exists

#### Scenario: No controllable assets present
- **WHEN** a VEN profile has PV and base load only (no battery, EV, or heater)
- **THEN** the planner still declares `p_pv_used[t]` and solves without error

### Requirement: Plan reports the PV curtailment decision
Each `PlanSlot` SHALL expose `pv_used_kw`, the solved value of `p_pv_used[t]` for that slot,
alongside the existing `pv_forecast_kw`. When the planner falls back to the infeasibility path
(no successful solve), `pv_used_kw` SHALL equal `pv_forecast_kw` (no curtailment assumed).

#### Scenario: Plan slot exposes both forecast and used PV values
- **WHEN** a `Plan` is produced by a successful solve
- **THEN** each `PlanSlot` has both `pv_forecast_kw` and `pv_used_kw`, with
  `pv_used_kw <= pv_forecast_kw`

#### Scenario: Fallback plan assumes no curtailment
- **WHEN** the planner falls back to `fallback_plan` after an infeasible solve
- **THEN** every `PlanSlot.pv_used_kw` equals that slot's `pv_forecast_kw`

### Requirement: Resolved export limit is applied to the simulated PV asset every tick
The tick pipeline SHALL compute a single effective PV export limit each tick as the more
restrictive (smaller magnitude) of (a) the live capacity state's `export_limit_kw`
(`OadrCapacityState`, driven by VTN `EXPORT_CAPACITY_LIMIT` events or sim-injection) and (b) the
current plan slot's curtailment target derived from `pv_used_kw`, and SHALL write that value to
`PvInverter.export_limit_kw` before each simulator physics step. This applies whether the limit
originates from a VTN event or from the planner's own decision.

#### Scenario: VTN export limit actually curtails simulated PV output
- **WHEN** an active VTN `EXPORT_CAPACITY_LIMIT` sets a capacity export limit below the current PV
  forecast
- **THEN** the simulated PV asset's actual export power is clamped to that limit on the next tick

#### Scenario: Plan-driven curtailment actually curtails simulated PV output
- **WHEN** the current plan slot's `pv_used_kw` is below `pv_forecast_kw` for that slot and no VTN
  capacity limit is active
- **THEN** the simulated PV asset's actual export power is clamped to `pv_used_kw` on the next tick

#### Scenario: Tighter of two active limits wins
- **WHEN** both a VTN capacity export limit and a plan-driven curtailment target are active for
  the same tick, with different magnitudes
- **THEN** the applied `PvInverter.export_limit_kw` reflects whichever value permits less export

#### Scenario: No active limit leaves PV uncurtailed
- **WHEN** neither a VTN capacity export limit nor a plan-driven curtailment target is active
- **THEN** `PvInverter.export_limit_kw` is `None` and PV output is unclamped

### Requirement: PV curtailment decision is visible in the VEN UI
The VEN UI SHALL display, for the current and upcoming plan, whether and by how much PV export is
being curtailed relative to the forecast, using the `pv_used_kw` field.

#### Scenario: UI shows curtailment when present
- **WHEN** a plan slot has `pv_used_kw < pv_forecast_kw`
- **THEN** the VEN UI's plan-facing PV view displays the curtailed amount for that slot

#### Scenario: UI shows no curtailment indicator when PV is fully used
- **WHEN** a plan slot has `pv_used_kw == pv_forecast_kw`
- **THEN** the VEN UI's plan-facing PV view shows no curtailment indicator for that slot
