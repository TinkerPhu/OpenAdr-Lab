# Design

## Context

After W1/W2 the planner core plans every schedulable asset from what it declares, but PV and base
load still enter as site series. This change makes them declared participants without changing a
single plan, and without touching dispatch, the simulator or the plan's wire format.

## Inventory: where PV and base load live today

**Forecast resolution (the PV precedence chain):**

| Where | What it does | Becomes |
|---|---|---|
| `tasks/planning/assemble.rs` → `simulator/plan_context.rs::resolve_pv_forecast_kw` | `find_asset(ASSET_PV)`, downcast to `PvInverter`, `-uncurtailed_power_kw_at` per slot | `PvInverter::build_milp_context` (the asset's own answer); function deleted |
| `simulator/plan_context.rs::resolve_base_load_forecast_kw` | `find_asset(ASSET_BASE_LOAD)`, `forecast_kw_at` per slot | `BaseLoad::build_milp_context`; function deleted |
| `services/planning/mod.rs::resolve_weather_pv_kw_for_cycle` → `SolveRequest.weather_pv_kw` | Weather PV for the planner when no live PV asset exists | Deleted from the cycle. The live `PvInverter` already reads the weather (`weather_forecast`); `services/forecast.rs` keeps its own weather PV forecast for the UI |
| `input_stages.rs::forecast_series` | Chain: test pin → live → weather → `PvParams::forecast_kw` sin model; base: live → flat `baseline_kw` | Deleted. Production always has a live PV asset when the profile has PV (the simulator roster is the profile's), so the last two steps only ran in planner tests |
| `cycle_state.rs` `pv_forecast_override` (sim inject `pv_plan_kw`) | Test pin, wins over everything | Passed to the PV's context build (`MilpBuildInputs.pv_forecast_override`); the PV applies it |

**The model:**

| Where | Today | Becomes |
|---|---|---|
| `model_skeleton.rs` balance | `p_imp + p_pv_used − Σ net − p_exp == p_base` | `p_imp − Σ net − p_exp == p_base_adjust` (PV's net is `−p_pv_used`, base load's is its forecast) |
| `model_skeleton.rs` bound | `p_pv_used ≤ p_pv_kw` | PV's own variable bound |
| `GridMilpVars.p_pv_used`, `SolveOutput.p_pv_used_kw`, warm start | Grid-level | PV's own variable, solution and warm start |
| `milp_interactions.rs::pv_use_tiebreak_expr` (cost and friction) | Over the grid's `p_pv_used` | PV's own `ObjectivePart::Cost` and `::Friction` |

**"Forecast PV surplus over the base load" (four copies):**

| Where | Rule |
|---|---|
| `milp_interactions.rs` coexistence (`declare_vars`) | `p_pv − p_base >= p_min` |
| `milp_interactions.rs` controllable-import malus | `max(0, p_pv − p_base)` |
| `assets/ev_declared.rs::inject_grid_slots` (free-PV cap) | `max(0, pv − base)` |
| `results.rs` `surplus_available_kw` (twice: solved and fallback) | `max(0, p_pv − p_base)` |

All become calls of `MilpInputs::forecast_surplus_kw()`: PV minus the base load after the
baseline override, signed, each reader flooring it where it did. Signed because the coexistence
test compares the unfloored value with the EV's minimum power, which some fixtures set to 0.

**Plan output:** `results.rs` writes `baseline_kw`, `pv_forecast_kw`, `pv_used_kw` from the site
series. Readers: `dispatcher::resolve_pv_generation_limit_kw`, `arbiter_levers::pv_curtailment_lever`,
`controller/timeline.rs` (by `ASSET_PV`/`ASSET_BASE_LOAD`), `services/forecast.rs` (forecast
accuracy), `PlanDecisionMatrix.tsx`, `PlanPowerStack.tsx`, `dispatcher_steps.py`,
`weather_forecast_steps.py`. Kept as site totals (D1), so none of them changes.

## Decisions

**D1. The plan keeps `pv_forecast_kw`, `pv_used_kw`, `baseline_kw` as site totals; no per-asset PV
or base-load entries.** Summed from the forecast participants' readback.
- *Alternative:* per-asset entries (PV in `planned_kw_by_asset`). Rejected for now: the simulator
  treats a `planned_kw_by_asset` value as a setpoint and the forward simulation as a PV generation
  *limit*, so a planned PV of −3 kW would cap a sunnier-than-forecast PV at 3 kW. Every reader
  above would need a per-asset rewrite, for a one-PV site that gains nothing. That is W6 work, and
  only worth it with several arrays.

**D2. A context declares its role: dispatched or forecast.** `AssetMilpContext::dispatched()`
(default `true`). Forecast participants are excluded from allocations, `planned_kw_by_asset`,
`planned_asset_ids`, session flexibility and the controllable-import malus's sum
(`site_net_power_expr` gains a dispatched-only sibling). They report their site-series share
through a new `AssetSolution.forecast: Option<ForecastLegs { forecast_kw, used_kw }>` (signed:
generation negative), which `results.rs` sums like W2's storage throughput.
- *Alternative:* keep PV and base load out of the pool and only move their forecasts onto
  contexts. Rejected: the curtailment variable would stay a grid variable, which is the part that
  is not the PV's own decision.

**D3. The baseline override stays a site statement.** `MilpInputs.p_base_adjust_kw` (zero unless
a `BaselineOverride` is active) sits on the balance's right-hand side and lowers the forecast
surplus; the plan's `baseline_kw` is base load plus it, as today. SIMPLE level 2 caps import at the
forecast participants' load before the adjustment, as today. The base-load asset stays the sole
authority on its own forecast; the user's "I will add 2 kW" is a statement about the site, not a
competing forecast of that asset.
- *Alternative:* pass the override into the base-load context. Rejected: SIMPLE level 2 would then
  need the asset to expose a second, pre-override series.

**D4. Out of scope: per-array forecasts from `PvArrayGeometry`, per-array curtailment on the wire,
per-asset measurement MQTT topics.** The planner side is generic (two PV contexts plan correctly,
pinned by a test), but profiles allow one PV and one base load, so per-array forecasting and
per-asset topics would add code that does nothing. The measurement topics are also a wire change
for the publishers. Recorded in the journal as the remaining W3 part, not as debt.

**D5. Tests build PV and base load from a series.** The planner fixtures today get PV from
`PvParams::forecast_kw` (sin model) through `SiteInputs.pv_cfg`. They will build the PV context
directly from that same series (`PvMilpContext::from_forecast`), so every recorded plan keeps its
numbers. Production builds from the live `PvInverter` as `resolve_pv_forecast_kw` did.

**D6. A profile with a `weather_pv` section must have a PV asset** (`profile::validate`). That
makes the dropped "weather PV without a PV asset" planner path unreachable by construction; all 14
profiles with the section already have one.
The rule ties only the panel geometry to a PV asset, not the weather feed: the feed (and its
`temperature_c`) keeps working without `weather_pv`, which the heater's heat-loss forecast (GB-57)
needs. The site's coordinates live inside `weather_pv` today; when an asset other than PV needs
them, they move to a site-level section rather than making `weather_pv` a site setting.

**D7. Model order.** PV and base-load contexts come after the schedulable ones in
`build_asset_contexts`, which keeps every existing variable's position; the PV variable moves from
the grid block to the PV's own declaration, so the as-sent fingerprint digests change and the
term-sorted ones must not.

## Verification

- `plans.txt` byte-identical; fingerprint changes classified by script (as-sent only);
  `milp_inputs.txt` changes only by the moved series.
- New tests: forecast participants get no allocation; two PV contexts plan independently; the
  surplus is one series every reader shares; `weather_pv` without PV is refused; the PV pin wins.
- Architecture audit rule 16 already covers the planner core; the fixed-id lookups in
  `plan_context.rs` disappear.
- All four suites on Node2.
