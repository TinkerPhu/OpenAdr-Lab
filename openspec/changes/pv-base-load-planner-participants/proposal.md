# Proposal

## Why

W1 and W2 made the planner plan, and the plan report, whatever each schedulable asset declares.
PV and base load are still outside that: the planner receives them as two site series
(`MilpInputs.p_pv_kw`, `p_base_kw`), resolved by fixed id from the simulator
(`simulator/plan_context.rs::resolve_pv_forecast_kw`, `resolve_base_load_forecast_kw`, both
`find_asset(ids::ASSET_*)`), behind a four-step precedence chain whose last two steps (the weather
series and the profile's sin model) only run when no live PV asset exists, which production never
has. PV curtailment is a grid variable (`p_pv_used`), not the PV's own decision. And one concept,
"forecast PV surplus over the base load", is computed in four places.

## What Changes

- PV and base load take part in the planner as assets: each builds an `AssetMilpContext` from its
  own forecast (`PvInverter::uncurtailed_power_kw_at`, `BaseLoad::forecast_kw_at`) and declares its
  own variables and net power. PV declares its curtailment variable (today `p_pv_used`) and its
  use tie-break; base load declares a fixed net power. The planner core names neither.
- A new context role says whether an asset is **dispatched** by the plan or only **forecast** by
  it. Forecast participants take part in the balance and the surplus, but get no allocation, no
  `planned_kw_by_asset` entry and no place in the controllable-import malus, so dispatch and the
  simulator are unchanged.
- One site series, `MilpInputs.forecast_surplus_kw`, computed once from the forecast participants,
  replaces the four `pv − base` computations (coexistence, import malus, EV free-PV cap,
  `surplus_available_kw`).
- The baseline override stays a site-level statement (`MilpInputs.p_base_adjust_kw`) applied in
  the balance and the surplus; SIMPLE level 2 caps import at the forecast participants' load
  before it, as today.
- Deleted: `resolve_pv_forecast_kw`, `resolve_base_load_forecast_kw`, `SiteInputs.pv_cfg`,
  `base_load`, `pv_live_forecast_kw`, `base_load_live_forecast_kw`, `weather_pv_kw`,
  `SolveRequest`'s matching fields, `forecast_series`, `GridMilpVars.p_pv_used`,
  `GlobalMilpInputs.p_pv_kw`/`p_base_kw`, `SolveOutput.p_pv_used_kw`.
- The plan's `pv_forecast_kw`, `pv_used_kw`, `baseline_kw` and `surplus_available_kw` keep their
  names and meaning as **site totals** summed from the forecast participants (decision D1), so
  dispatch (`resolve_pv_generation_limit_kw`), the arbiter's PV lever, the timeline, the UI and the
  BDD steps are untouched.

## Capabilities

### New Capabilities
- `planner-forecast-participants`: PV and base load as planner participants, the dispatched and
  forecast roles, and the one forecast-surplus series.

### Modified Capabilities
<!-- none: no spec under openspec/specs/ covers the planner's PV and base-load inputs -->

## Impact

- **VEN Rust:** `assets/pv*.rs`, `assets/base_load.rs` (new `*_declared.rs` modules),
  `controller/asset_milp_port.rs`, `controller/milp_planner/{inputs,input_stages,model_skeleton,
  results,types,mod}.rs`, `controller/milp_interactions.rs`, `controller/solver_port.rs`,
  `simulator/plan_context.rs`, `tasks/planning/assemble.rs`, `services/planning/mod.rs`, planner
  test fixtures.
- **Goldens:** `plans.txt` must stay byte-identical; `model_fingerprints.txt` may change only in
  as-sent digests (variables now declared per asset); `milp_inputs.txt` loses the PV and base
  series and gains the surplus series.
- **Not affected:** the plan's wire format, dispatch, the simulator, the UI, the BFF.
- **Out of scope (decision D4):** forecasts per PV array from its own geometry, per-array
  curtailment on the wire, and one measurement MQTT topic per asset id. They only matter with
  several PV or base-load assets, which profiles do not allow.
