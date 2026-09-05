# Proposal: Shiftable Load as a First-Class Asset

## Why

Shiftable loads (washing machine, heat-pump cycle) are the one HEMS-controllable
load type that never became a simulator asset. `ShiftableLoad` (config: fixed
`power_kw`, `duration_min`, `[earliest_start, latest_end]`) and
`ShiftableLoadRuntime` (`VEN/src/entities/device_session.rs`) live entirely in
`AppState.hems` alongside user-request bookkeeping — `ShiftableLoadRuntime`'s own
doc comment says it plainly: "Tracks countdown until the load finishes; **NOT a
physics sim asset**." It never enters `SimState.asset_configs`, is never `step()`d
by the simulator, and is invisible to `iter_assets()`/persistence/UI diagnostics
the way Battery, EvCharger, Heater, PvInverter, and BaseLoad all are.

Consequently it gets a third, independent implementation of the same idea the
other four assets already solved once via `AssetMilpContext` (R-23) and again via
`Box<dyn Asset>` (Spec A, `asset-dispatch-trait-objects`): a bespoke
`ShiftableLoadMilp`/`ShiftableLoadMilpVars` pair wired directly into
`solver_phase1.rs`/`solver_phase2.rs`, and hand-duplicated window-logic helpers
(`already_run`, `valid_start_exists_at`) cross-imported between
`capacity_forecast.rs` and `envelope_forecast.rs`. Both forecast modules take
`&[ShiftableLoad]`/`&[ShiftableLoadRuntime]` as bolt-on parameters instead of
reading assets from `SimSnapshot` like every other asset kind.

This matters now because `docs/plans/asset-max-power-forecast-master-plan.md`'s
Spec C (`assetMaxPower` + `limitTier`) needs to define its `max_effort_setpoint`
primitive against the hardest case — a discrete, non-interruptible asset — from
the start, or the primitive will need retrofitting the moment shiftable load is
added under it. Spec B must land first so Spec C is designed against the real
final asset roster, and both need shiftable load actually simulated (not just
scheduled) for `Asset::simulate_forward` to work on it at all.

## What Changes

- **BREAKING** (internal): shiftable load becomes a real simulator asset. A
  scheduled/started shiftable load is represented as a `Box<dyn Asset>` entry in
  `SimState.asset_configs`/`SimSnapshot.assets`, stepped every simulator tick like
  Battery/EvCharger/Heater/PvInverter/BaseLoad, persisted the same way, and
  visible via `iter_assets()`/diagnostics. `ShiftableLoadRuntime`'s countdown
  bookkeeping in `AppState.hems` is replaced by this real asset state — it is not
  kept as a second, separate tracking mechanism.
- New `ShiftableLoad` (`impl Asset`) physics type (`VEN/src/assets/`, following
  Spec A's pattern) with:
  - fixed power while running (`p_min_kw == p_max_kw`, no modulation),
  - non-interruptible-once-started `step()` (once triggered, forces rated power
    for the remainder of the run regardless of the setpoint the caller requests),
  - a hard `[earliest_start, latest_end]` window — missing it is infeasible, not
    suboptimal (unlike EV's soft-preference departure time).
- New `ShiftableLoadMilpContext` implementing the existing
  `AssetMilpContext`/`MilpParticipant` trait (`VEN/src/controller/milp_planner/asset_port.rs`),
  replacing the bespoke `ShiftableLoadMilp` (`types.rs`) /
  `ShiftableLoadMilpVars` (`milp_interactions.rs`) wiring in `solver_phase1.rs`
  and `solver_phase2.rs`.
- `capacity_forecast.rs` and `envelope_forecast.rs` drop their bolt-on
  `&[ShiftableLoad]`/`&[ShiftableLoadRuntime]` parameters and duplicated
  window-logic helpers, reading shiftable-load entries from `SimSnapshot.assets`
  instead, matching every other asset kind.
- The HEMS-facing request lifecycle (accepting a `ShiftableLoad` user request via
  `routes/hems/`, the dispatcher deciding when to start it per the MILP-chosen
  slot) is preserved; only the *runtime tracking* mechanism changes from a
  bespoke `Vec<ShiftableLoadRuntime>` counter to a real simulated asset instance.

## Capabilities

### New Capabilities
- `shiftable-load-simulation`: shiftable loads (washing machine / heat-pump
  cycle style tasks) are modeled as first-class simulated assets — fixed power,
  non-interruptible once started, hard start/end window — participating in the
  simulator tick loop, MILP planning via `AssetMilpContext`, and capacity/envelope
  forecasting the same way every other asset type does.

### Modified Capabilities
(none — no existing `openspec/specs/` capability currently documents shiftable-load
scheduling; this is a new capability, not a change to a previously-specified one)

## Impact

- `VEN/src/assets/` — new `ShiftableLoad` asset type + tests.
- `VEN/src/entities/device_session.rs` — `ShiftableLoad` (request) config stays;
  `ShiftableLoadRuntime` is removed once its role is fully absorbed by the new
  asset's state.
- `VEN/src/state/mod.rs` — `AppState.hems.shiftable_runtimes` and its
  start/complete/list methods are replaced by simulator asset lifecycle calls.
- `VEN/src/controller/milp_planner/{types.rs,milp_interactions.rs,solver_phase1.rs,solver_phase2.rs,solver_duals.rs,results.rs,asset_port.rs}` —
  bespoke `ShiftableLoadMilp`/`ShiftableLoadMilpVars` replaced by
  `ShiftableLoadMilpContext: AssetMilpContext`.
  `VEN/src/controller/{capacity_forecast.rs,envelope_forecast.rs}` — bolt-on
  parameters and duplicated window-logic helpers removed.
- `VEN/src/tasks/sim_tick/{context.rs,publish.rs,forecast_wiring.rs}` — wiring
  updated to source shiftable loads from `SimSnapshot.assets`.
- `VEN/src/routes/hems/{mod.rs,sessions.rs}`, `VEN/src/services/user_request.rs`,
  `VEN/src/entities/user_request.rs` — request-acceptance surface unchanged in
  behavior; internal plumbing to the dispatcher updated to create/start the new
  asset instead of a `ShiftableLoadRuntime`.
- No VTN-facing or OpenADR-spec-facing change; no UI contract change (existing
  shiftable-load UI panels keep the same request/status shape — `docs/reference/DOCUMENTATION_STYLE.md`
  and `ui-transparency` apply if any new derived state needs a surface).
