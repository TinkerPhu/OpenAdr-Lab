# Proposal

## Why

The arbiter, the VTN dispatch override and the comms-loss fail-safe each steer one asset per kind,
found by its fixed id: `sim.assets.get(ids::ASSET_BATTERY)`, `ASSET_EV`, `ASSET_HEATER`
(26 uses in `controller/arbiter*`, 6 in `tasks/sim_tick/dispatch_override.rs`, 4 more in the tick
glue). A second battery, EV or heater exists for the planner (W1–W4) but not for real-time
control: it is never used as a lever, never clamped on comms loss, and never gets its own
opportunistic charging or emergency mode. This is W5a, the behavioural half of W5 ("remove the
fixed-id lookups").

## What Changes

- Each asset declares the arbiter lever it offers (`Asset::arbiter_lever`, carried on
  `AssetSnapshot`): storage (battery), surplus charging (EV), pausable load (heater). The heater's
  emergency lever stays declared by `emergency_what_ifs`; PV curtailment stays one site lever.
- The arbiter builds one lever per declaring asset, ranks all of them by marginal cost and breaks
  ties by roster order (profile order). Lever identity is the lever kind plus its asset id.
- **Changed diagnostics wording:** the active lever is reported as `<kind>:<asset_id>`
  (`battery:battery`, `heater_pause:heater`); site levers keep their kind (`pv_curtail`).
- The carried levers, the release rule and the lag bridge apply to every storage and surplus-
  charging asset, not to `battery`/`ev` by id.
- Opportunistic surplus charging serves EVs in roster order, each with its own settings and its
  own plan allocation.
- The heater emergency mode and the EV departure time reach the simulator per asset.
- The VTN dispatch override is a target the ranked storage levers fill (one allocation rule).
- The comms-loss fail-safe clamps every asset that declares a lever, to the configured percentage
  of its own rated range.

## Capabilities

### New Capabilities
- `arbiter-levers-per-asset`: real-time control acts on every asset that declares a lever.

## Impact

- VEN: `controller/arbiter*`, `controller/dispatcher.rs`, `controller/sim_snapshot.rs`,
  `simulator/snapshot.rs`, `simulator/mod.rs`, `simulator/tick_inputs.rs`, `assets/*` (lever and
  rated range declarations, per-asset tick overrides), `tasks/sim_tick/*`, `state/arbiter.rs`.
- UI: shows the new lever wording (no code change expected beyond fixtures).
- Single-asset profiles: identical setpoints; only the reported lever names change.
- **Out of scope (W5b):** PV and base-load plumbing (forecast publishing, timeline, recorder,
  learning window, comms-loss PV snapshot, sim inject, `/sensors` temperature).
