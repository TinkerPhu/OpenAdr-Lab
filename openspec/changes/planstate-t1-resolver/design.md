## Context

`simulator::forecast::build_forecast_frames` (`VEN/src/simulator/forecast.rs`)
already does almost all of the needed work, just for a different output shape:

- For battery/EV/heater, `insert_simulated_points` builds one setpoint per
  remaining plan slot from `slot.planned_kw_by_asset`, calls
  `AssetHandle::simulate_forward` once, and gets back a `Trajectory` whose
  `points[i].state` is the *full* `AssetState` after slot `i`'s step (per
  `TrajectoryPoint`'s own doc comment in `assets/max_power.rs`). It then reads
  only `handle.capability(&point.state)` off each point and discards the state
  itself.
- `base_load` is explicitly skipped ("Uncontrollable, fixed point — never
  contributes flexibility") — no frame entry is built for it at all.
- PV (`insert_pv_points`) never calls `simulate_forward`. Its ceiling comes
  fresh from weather via `entities::solar::pv_ceiling_kw` every call; nothing
  evolves `PvState` across the horizon.

`assets/max_power.rs`'s `asset_max_power(asset, state, t1, t2, ...)` (Spec C)
needs exactly the state this trajectory already computes, just exposed rather
than discarded.

**Confirmed this session, not assumed:** `PvState { actual_power_kw,
generation_limit_kw, curtailment_source }` is a snapshot of what was *live*
during the tick that produced it — `curtailment_source` is driven by whatever
external decision (manual command, plan, capacity limiter, arbiter,
comms-loss) is currently in effect, not by any physics model that forecasts
how it will change. There is no existing mechanism, anywhere in this
codebase, that predicts a *future* curtailment source. PV's own `step()`
exists and is called on every live tick, but it evolves state from `self`'s
*current* config fields (`irradiance`, `weather_power_kw`, etc.) — feeding it
through `simulate_forward` with today's frozen config held constant into the
future would produce a specific, wrong-looking number (as if today's exact
sun angle/weather never changed), not a genuine forecast.

## Goals / Non-Goals

**Goals:**
- Expose "what is asset X's `AssetState` at future time `t1`, if the plan
  holds" for every asset kind `assetMaxPower` needs it for, reusing
  `build_forecast_frames`'s existing per-asset simulation rather than
  re-deriving it.
- `t1` at or before `now` returns the live snapshot state exactly (zero
  simulation, zero forecast error) — this is the one point where ground
  truth exists.

**Non-Goals:**
- Forecasting PV's `curtailment_source` — no model for this exists anywhere
  in the codebase today; inventing one is out of scope for a resolver whose
  job is to reuse existing machinery, not add new physics.
- Interpolating `AssetState` between plan slot boundaries. `t1` is expected
  to be one of the plan's own remaining slot start times (which is how Spec
  E's triangle-builder will call this, per the master plan's `t1`/`t2` sweep
  description) — a `t1` that doesn't land exactly on a slot boundary snaps
  down to the latest slot boundary at or before it (documented, not silently
  wrong).
- Wiring this into `capacity_forecast.rs`/`envelope_forecast.rs` or calling
  `asset_max_power` with it — Spec E's job.
- Resolving R-69 (battery efficiency model asymmetry) — see Risks below.

## Decisions

**D1 — Share the trajectory computation, don't re-derive it.** Extract
`insert_simulated_points`'s trajectory-building step (build the per-slot
setpoint schedule from `slot.planned_kw_by_asset`, call
`AssetHandle::simulate_forward`) into a small internal helper,
`simulated_trajectory(entry, cfg, future_slots) -> Trajectory`. Both
`insert_simulated_points` (existing, capability-per-slot) and the new
resolver (state-at-`t1`) call this same helper. This is the change's central
guarantee: there is exactly one place that runs `simulate_forward` for the
plan-driven forecast, matching this whole master plan's stated purpose of
never having two independent implementations of "the same forecast."

**D2 — New function signature:**

```rust
pub fn resolve_plan_state_at(
    sim: &SimState,
    plan: &Plan,
    t1: DateTime<Utc>,
    now: DateTime<Utc>,
) -> HashMap<String, AssetState>
```

Returns one entry per asset id. For `t1 <= now`, every asset's entry is its
live `SimState` value (`entry.state.clone()`), no simulation. For `t1 >
now`: battery/EV/heater/base_load go through D1's shared helper and report
the trajectory point at the latest remaining slot with `start <= t1` (falling
back to the last available point if `t1` is beyond the plan's horizon,
documented as a known edge case, not a panic). PV always returns the live
state unchanged, per the Context section's finding — this is an honest
scope limit, not an oversight, and is documented on the function itself so a
future reader doesn't assume more precision exists than the model supports.
`base_load` is included here even though `build_forecast_frames` skips it
for capability-forecast purposes — `assetMaxPower`'s own roster (per Spec C)
includes base_load, and `simulate_forward` already works for it via the same
generic path, so there is no reason to special-case it out of a "state at
t1" resolver whose only job is to answer that question for every asset kind.

**D3 — `HashMap<String, AssetState>`, not a bespoke per-kind return type.**
Mirrors `SimSnapshot`'s and `build_forecast_frames`'s own existing
id-keyed-map convention rather than inventing a new shape. `assetMaxPower`
callers already work with `(asset, state)` pairs per id elsewhere in this
codebase; this keeps the same access pattern.

## Risks / Trade-offs

- **[Risk] R-69 (battery efficiency asymmetry) is not resolved by this
  change** — `resolve_plan_state_at`'s battery entries reuse
  `battery.rs`'s (asymmetric) `step_inner`, the same model
  `build_forecast_frames` already uses, so this change doesn't create or
  worsen the mismatch against `battery_milp.rs`'s symmetric model. →
  **Mitigation:** per the master plan's explicit instruction, add one
  verification case (see tasks.md) that compares a resolved future battery
  state's SoC against `plan.soc_trajectory_kwh`/`planned_state_by_asset` at
  the same slot. If R-69 is still open when this change lands, that
  comparison fails loudly instead of silently inheriting the gap; if R-69
  has since landed, it passes and stays as a regression guard.
- **[Risk] PV's "state at t1" is always today's live state, which will read
  as suspiciously constant across a whole horizon sweep.** → **Mitigation:**
  documented explicitly on `resolve_plan_state_at` and in this design.md, not
  left to be discovered by a confused future reader; Spec E's own PV
  handling should keep using `insert_pv_points`'s live weather-driven
  ceiling for PV specifically rather than routing it through this resolver's
  state, exactly as `build_forecast_frames` already keeps the two paths
  separate today.
- **[Risk] `t1` beyond the plan's horizon has no real answer.** →
  **Mitigation:** returns the last available trajectory point rather than
  panicking or fabricating an extrapolation; documented as a known
  approximation, matching this codebase's general preference for an honest,
  labeled limitation over invented precision.
