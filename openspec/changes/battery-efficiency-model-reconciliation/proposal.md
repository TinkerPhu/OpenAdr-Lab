# Proposal: Reconcile Battery Round-Trip-Efficiency Models

## Why

Found during an architectural audit of the assets/simulator area (2026-09-03),
independent of the `asset-dispatch-trait-objects` change in flight.

The real battery physics (`VEN/src/assets/battery.rs:66-72`, `step_inner`) puts
100% of round-trip loss on the **charge** leg only:

```rust
let energy_kwh = actual * dt_h * if actual > 0.0 { self.round_trip_efficiency } else { 1.0 };
```

Discharge is lossless (multiplier `1.0`). `forecast()` in the same file
(lines 178-182) uses the identical convention.

The MILP planner's battery model (`VEN/src/assets/battery_milp.rs:227,235-236`)
instead splits the loss symmetrically across both legs via `sqrt`:

```rust
let eff = self.round_trip_efficiency.sqrt();   // eff_ch = eff_dis = sqrt(rte)
```

with the SoC-evolution constraint `e[t+1] = e[t] + dt*eff_ch*p_ch[t] -
dt*(1/eff_dis)*p_dis[t]`.

Both models agree on total round-trip efficiency for one full charge/discharge
cycle (`sqrt(rte)*sqrt(rte) = rte = rte*1.0`), so a test comparing only
endpoint energy over a full cycle won't catch the divergence. But the
**intermediate SoC trajectory differs** for any partial cycle — the normal
case under this project's 5-minute rolling replan. The planner's belief about
SoC-at-time-t can silently diverge from the simulator's real SoC-at-time-t
whenever charge and discharge aren't perfectly paired, biasing SoC-gated
decisions (`min_soc` floors, capability ceilings, terminal-SoC rewards)
without ever surfacing as a "wrong total energy" bug.

EV's asset code has no efficiency modeling at all (checked — no `efficiency`
field in `ev.rs`/`ev_milp.rs`), so this is battery-specific, not a systemic
pattern across storage assets.

## What Changes

One of the two models is corrected to match the other — which one is the
open decision this change exists to make (see `design.md`). No new capability;
this is a physics-correctness fix to two existing, already-shipped models.

## Non-Goals

- Not a change to EV's (lack of) efficiency modeling — out of scope, no
  evidence of the same bug there.
- Not bundled with `openspec/changes/asset-dispatch-trait-objects/` — that
  change's explicit goal is zero behavior change; it will move `battery.rs`'s
  current logic verbatim regardless of this fix's outcome. This change can
  land before or after that one; see "Sequencing" below.

## Capabilities

No capability added/modified. Physics correctness fix only — no `specs/`
delta.

## Impact

- `VEN/src/assets/battery.rs` (`step_inner`, `forecast`) and/or
  `VEN/src/assets/battery_milp.rs` (`declare_vars_into_pool`/constraint
  building around lines 227, 235-236), depending on which direction
  `design.md` resolves.
- Existing battery unit tests in both files will need new/adjusted assertions
  covering a **partial** charge/discharge cycle specifically — the current
  test suite's blind spot (full-cycle-only assertions) is exactly what let
  this drift ship unnoticed.

## Sequencing

Independent of the master plan (`docs/plans/asset-max-power-forecast-master-plan.md`)
and the in-flight `asset-dispatch-trait-objects` change. Both touch
`battery.rs`, so landing this first avoids editing the same physics twice, but
it is not a hard prerequisite — `asset-dispatch-trait-objects` explicitly
preserves current behavior verbatim regardless of which model wins here.
