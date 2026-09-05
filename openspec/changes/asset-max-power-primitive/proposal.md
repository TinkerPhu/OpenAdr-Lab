# Proposal: `assetMaxPower` Primitive + `limitTier`

## Why

`capacity_forecast.rs` computes "if the site committed now to a sustained
extreme (max import or max export), how does achievable power decay over
time" by hand-writing one bespoke function per asset kind
(`battery_events`, `ev_events`, `heater_events`, `pv_events`,
`base_load_events`). Two of these are confirmed wrong, and the bug is the
*same* bug in both places (root-caused during the since-deleted
`capacity-envelope-unification` investigation):

- **`pv_events`' Import branch** adds PV's currently-planned output as a
  *positive* contribution to achievable import ("curtailment headroom").
  Every other asset commits to its own physical extreme for the requested
  direction; PV's extreme for "maximize import" is curtailed to zero. Once
  curtailed, PV contributes **nothing** — not a signed term of any kind. Fix
  agreed at the time but never implemented as a general rule (only as a
  proposed direct fix to this one function).
- **`heater_events`' Export branch** — confirmed still present today — adds
  the heater's *current draw* as a positive contribution to achievable
  export, on the identical "current consumption is curtailable, credit it"
  reasoning already rejected for PV. The heater's extreme for "maximize
  export" is turned off; turning off a consumer doesn't manufacture new
  exportable generation. This has never been fixed.

Both bugs exist because there is no general concept of "this asset's own
physical extreme for a direction" — each function reimplements it, and two
of the five reimplementations get it wrong in the same way. `Asset` already
has `capability()` (a point-in-time ceiling) and `simulate_forward()` (project
a setpoint schedule forward, correctly modeling exhaustion/clamping) from
Spec A/B. What's missing is the one connecting piece: which setpoint
schedule represents "this asset's own extreme, held for a duration."

## What Changes

- A new trait primitive, `max_effort_setpoint(&self, state: &AssetState,
  direction: CommitmentDirection, tier: LimitTier) -> f64`, on `Asset` (with a
  default body for continuous/reservoir assets — see design.md) returning the
  constant setpoint representing "go all-in on `direction`" under `tier`.
- `LimitTier` (`Physical | Contractual | UserSet`) as a real enum. `Physical`
  is free — it's exactly what `capability()` already reports. `Contractual`
  and `UserSet` are new: no clean per-asset concept for either exists in the
  codebase today (see design.md's audit of what currently stands in for
  them, asset by asset).
- `assetMaxPower(plan_state, t1, t2, direction, tier) -> (power_kw, energy_kwh)`:
  resolves the extreme setpoint(s), builds a schedule, calls the existing
  `Asset::simulate_forward`, and integrates power over the returned
  trajectory. No new simulation logic — this composes what Spec A/B already
  built.
- Fixes both confirmed bugs **by construction**: PV's Import extreme and
  Heater's Export extreme both resolve to `0.0` because `capability()`
  already reports `max_import_kw: 0.0` / `max_export_kw: 0.0` respectively
  for those asset/direction pairs — no PV-specific or heater-specific
  carve-out needed once the general primitive exists.

## Capabilities

### New Capabilities
- `asset-max-power-primitive`: a generic, per-asset "maximum achievable
  power and energy under a sustained commitment" computation
  (`max_effort_setpoint` + `assetMaxPower`), verified per asset kind with
  worked numeric examples, replacing the *concept* that
  `capacity_forecast.rs`'s five bespoke functions each reimplement today.

### Modified Capabilities
(none — no existing `openspec/specs/` capability currently documents the
sustained-commitment capacity curve; `capacity_forecast.rs`'s own behavior is
governed by its own code/tests, not a prior openspec capability)

## Impact

- `VEN/src/assets/asset_trait.rs` — new `max_effort_setpoint` method on
  `Asset`, default-implemented for the common case.
- `VEN/src/entities/capacity_curve.rs` (or wherever `LimitTier` most
  naturally lives, domain ring) — new `LimitTier` enum.
- `VEN/src/assets/{battery,ev,heater,pv,base_load,shiftable_load}.rs` — each
  asset either inherits the default or overrides it (`ShiftableLoadAsset`
  certainly needs an override — see design.md for why its "extreme" isn't a
  constant setpoint).
- A new `assetMaxPower` function (exact module TBD in design.md — a
  candidate home is alongside `Asset::simulate_forward` itself, or a new
  small module in `controller/`).
- **Explicitly out of scope**: cutting `capacity_forecast.rs`'s five bespoke
  functions over to call `assetMaxPower` instead of their own logic. Per the
  master plan's dependency graph, that cutover belongs to Spec E (the
  unified capacity/envelope engine), which needs Spec C's primitive *and*
  Spec D's `planState(t1)` resolver first. This change only builds and
  verifies the primitive in isolation (per-asset unit tests + worked
  examples), matching the master plan's own stated verification scope for
  Spec C.
- No VTN-facing, OpenADR-spec-facing, or UI-facing change — this is an
  internal primitive with no new observable behavior until Spec E wires it
  into a forecast module.
