## Context

`Asset` (Spec A) already gives every asset kind `capability(state) ->
AssetCapability` (a point-in-time `max_import_kw`/`max_export_kw` ceiling) and
`simulate_forward(initial, setpoints) -> Trajectory` (project a setpoint
schedule forward, correctly modeling exhaustion/clamping via repeated
`step()` calls). `capacity_forecast.rs` needs "if the site committed now to a
sustained extreme in `direction`, how does achievable power decay over
`t2`" — which is exactly `capability()` (the extreme) composed with
`simulate_forward()` (the decay) — but today reimplements this by hand, once
per asset kind (`battery_events`, `ev_events`, `heater_events`, `pv_events`,
`base_load_events`), and two of the five reimplementations are confirmed
wrong (proposal.md's Why section).

**Confirmed today, read directly from the current code:**
- `PvInverter::capability_inner` always reports `max_import_kw: 0.0` (PV
  never imports) — but `capacity_forecast.rs::pv_events`' Import branch
  doesn't ask `capability()` at all; it independently computes
  `(-planned_kw).max(0.0)` from forecast frames and adds it as a *positive*
  contribution. Bug confirmed.
- `Heater::capability_inner` always reports `max_export_kw: 0.0` (a heater
  never exports) — but `heater_events`' Export branch doesn't ask
  `capability()` either; it adds the heater's current draw
  (`asset.power_kw`) as a positive contribution. Bug confirmed, still
  present as of this writing.
- Both bugs have the identical shape: the bespoke function invents its own
  answer to "what's this asset's extreme for this direction" instead of
  asking the asset itself, and gets it wrong in the same way twice.

## Goals / Non-Goals

**Goals:**
- A generic `max_effort_setpoint` primitive that every asset kind answers
  correctly for its own physics, verified per kind with a worked numeric
  example (mirroring `capacity_forecast.rs`'s own existing
  `pv_export_uses_ceiling_not_ceiling_minus_current` test style).
- `LimitTier` as a real, named concept, honestly scoped to what the codebase
  actually supports today (see Decision D2) rather than pretending three
  fully-independent per-asset ceilings already exist.
- `assetMaxPower` as a thin, generically-composed function with no new
  simulation logic, callable in isolation and unit-tested against
  `simulate_forward`'s existing, already-tested decay behavior.

**Non-Goals:**
- Cutting `capacity_forecast.rs`'s five bespoke functions over to call
  `assetMaxPower` — that is Spec E's job (proposal.md's Impact section).
  This change fixes nothing observable by itself; it builds the tool Spec E
  will use to fix the two confirmed bugs for real.
- `envelope_forecast.rs`'s equivalent per-asset logic — same reasoning,
  same deferral to Spec E.
- Building out full Contractual/UserSet ceiling plumbing for asset kinds
  that have none today (Battery/EV/Heater/BaseLoad/ShiftableLoad) — Decision
  D2 documents the honest current state and picks a safe default, not a new
  feature to expose those ceilings end-to-end (no UI, no VTN wiring for
  per-asset contractual limits beyond what already exists).

## Decisions

### D1 — `max_effort_setpoint` defaults to `capability()`, only `ShiftableLoadAsset` needs a real override

For every continuous/reservoir asset (Battery, EvCharger, Heater, PvInverter,
BaseLoad), "go all-in on `direction`" *is* the `Physical`-tier ceiling
`capability()` already reports — confirmed above for PV/Heater specifically,
and true by the same reasoning for Battery/EV (charge/discharge at max rate)
and BaseLoad (no controllable extreme — floor equals ceiling already).

So `Asset::max_effort_setpoint` gets a **default body**:
```rust
fn max_effort_setpoint(&self, state: &AssetState, direction: CommitmentDirection, tier: LimitTier) -> f64 {
    let cap = self.capability(state); // tier handling: see D2
    match direction {
        CommitmentDirection::Import => cap.max_import_kw,
        CommitmentDirection::Export => cap.max_export_kw,
    }
}
```
No per-type override needed for Battery/EV/Heater/PV/BaseLoad — this default
body, applied uniformly, *is* the fix for both confirmed bugs, with zero
PV-specific or heater-specific code anywhere. This is the same shape of win
Spec A's capability-trait defaults already proved out.

**`ShiftableLoadAsset` cannot use this default** — see D3.

**Alternative considered:** give every asset kind its own
`max_effort_setpoint` override, mirroring `capacity_forecast.rs`'s existing
per-kind-function structure. Rejected — it would just relocate the same
"reimplement it and risk getting it wrong" pattern this change exists to
eliminate; the entire value of routing through `capability()` is that
`capability()` is already correct and already tested per asset kind.

### D2 — `LimitTier`: `Physical` is real today; `Contractual`/`UserSet` fall back to `Physical` for now, honestly

Auditing what each asset kind actually has below its physical ceiling today:

| Asset | Physical (today) | Contractual (today) | UserSet (today) |
|---|---|---|---|
| Battery/EvCharger/Heater/BaseLoad | `capability()` | **none** | **none** |
| PvInverter | `capability()` | `generation_limit_kw` when `curtailment_source` is `Plan`/`Capacity`/`Arbiter`/`CommsLoss` | `generation_limit_kw` when `curtailment_source` is `Manual` |
| ShiftableLoadAsset | window/duration (D3) | **none** | **none** |

Only PV has anything today that cleanly maps to a sub-Physical ceiling, and
even there it's one collapsed `Option<f64>` field distinguished by a
`PvCurtailmentSource` tag, not two independent tiers. Every other kind has
*no* per-asset contractual or user-set ceiling concept at all — the closest
analogues (site-level VTN `import_limit_kw`/`export_limit_kw`, profile
static params) aren't per-asset and aren't what `limitTier` as specified
means.

**Decision:** `max_effort_setpoint`'s default body (D1) takes `tier` but,
for `Contractual`/`UserSet`, currently just returns the same `Physical`
answer for every kind except PV — accurate to what the codebase actually
has, not a design fiction. `PvInverter` overrides the default to fold in
`generation_limit_kw` (clamping the Physical ceiling to it) for whichever
tier its live `curtailment_source` corresponds to. This means `LimitTier`
has real, observable effect for PV today and is a documented no-op
elsewhere — future work that adds a real contractual/user-set ceiling to
another asset kind only needs to override `max_effort_setpoint` for that one
kind, not touch this primitive's shape.

**Alternative considered:** invent placeholder per-asset contractual/
user-set fields now so all three tiers are "real" everywhere. Rejected —
speculative plumbing with no current consumer (violates this repo's
`generic-over-bespoke`/no-speculative-design conventions); building it only
when a real Contractual/UserSet ceiling exists for a given kind keeps the
primitive honest about what it currently does.

### D3 — `ShiftableLoadAsset`'s "extreme" is a placement decision, not a constant — needs its own primitive shape

Every other asset's `max_effort_setpoint` is a single number held constant
over `[t1, t1+t2]`. A shiftable load's extreme is fundamentally different:
its power is always `0` or `power_kw` (never anything else — this is
Spec B's whole physical model), so "the extreme setpoint" isn't a
meaningful question; the actual lever is **when within its window it
starts**:
- **Import** direction: start as early as the window allows (maximizes
  remaining budget for later flexibility) — `earliest_start`.
- **Export** direction: start as late as the window allows (minimum forced
  import for as long as possible before the window forces it) — this
  session's correction to the master plan's own draft wording, which said
  "as long as possible" but the actual mechanism is "start as late as
  possible."

This can't be expressed by `max_effort_setpoint(...)  -> f64` composed with a
flat two-point schedule the way every other kind works. Two ways to resolve
this, matching the master plan's own acknowledgment ("or a richer [schedule]
for direction-dependent placement"):

1. **Add a second, optional method** — e.g.
   `fn max_effort_schedule(&self, state, direction, tier, t1, t2) ->
   Vec<(DateTime<Utc>, f64)>` — with a default body that builds the flat
   two-point `[(t1, max_effort_setpoint(...)), (t1+t2, same)]` schedule every
   continuous asset uses, and only `ShiftableLoadAsset` overrides it to
   return the real placed schedule (0 until the chosen start, `power_kw`
   thereafter, 0 again once `duration_min` elapses if that's before `t1+t2`).
   `assetMaxPower` calls this method, not `max_effort_setpoint` directly.
2. Keep `max_effort_setpoint` as the only primitive and have
   `ShiftableLoadAsset` return `power_kw` from it (a real, correct magnitude
   — it just needs no help modulating), with `assetMaxPower` itself
   special-casing shiftable-load placement via a downcast or a second small
   port method.

**Chosen: option 1.** It matches `Asset`'s existing pattern (a method with a
sane default body that only the structurally-different kind needs to
override — the same shape D1 itself uses, and the same shape Spec A's
capability traits use throughout) and keeps `assetMaxPower` itself
completely generic, with zero per-kind branching. Option 2 would reintroduce
exactly the "generic code with a special case for one kind" pattern this
whole master plan exists to move away from.

### D4 — `assetMaxPower`'s home

`assetMaxPower(plan_state: &AssetState, t1: DateTime<Utc>, t2: Duration,
direction: CommitmentDirection, tier: LimitTier) -> (f64, f64)` (power at
`t1+t2`, energy integrated over `[t1, t1+t2]`) is a free function, not a
trait method — it needs no per-kind override (that's what D3's
`max_effort_schedule` is for), just an `&dyn Asset` to call into. Proposed
home: `VEN/src/assets/asset_trait.rs`, alongside `Trajectory`/
`TrajectoryPoint`/`simulate_forward`'s default body, since it's a pure
composition over that same trait's methods and has no `controller`-layer
dependencies of its own.

```rust
pub fn asset_max_power(
    asset: &dyn Asset,
    state: &AssetState,
    t1: DateTime<Utc>,
    t2: Duration,
    direction: CommitmentDirection,
    tier: LimitTier,
) -> (f64, f64) {
    let schedule = asset.max_effort_schedule(state, direction, tier, t1, t1 + t2);
    let trajectory = asset.simulate_forward(state, &schedule);
    // power = trajectory's endpoint; energy = integrate power_kw × dt over the trajectory
}
```

## Risks / Trade-offs

- **[Risk]** `max_effort_schedule`'s default body must produce *exactly* the
  two-point schedule `simulate_forward` already expects (confirmed shape
  from Spec A: `&[(DateTime<Utc>, f64)]`, at least 2 points for a
  well-formed window) — an off-by-one here would silently miscompute every
  continuous asset's energy integral. → **Mitigation:** test `assetMaxPower`
  against `simulate_forward`'s own existing test fixtures for at least one
  reservoir asset (Battery), asserting the same numeric result a manual
  `simulate_forward` call over the same window produces.
- **[Risk]** `LimitTier::Contractual`/`UserSet` falling back to `Physical`
  for 5 of 6 asset kinds could be mistaken later for "already fully
  implemented" rather than "honestly incomplete, PV is the only real
  instance." → **Mitigation:** doc comment on the enum itself states this
  plainly; `docs/architecture/VEN_ARCHITECTURE.md`'s writeup (task list)
  states it too, not just a design.md note that gets deleted with this
  change directory.
- **[Trade-off]** Introducing `max_effort_schedule` as a second trait method
  alongside `max_effort_setpoint` is one more thing implementors could in
  principle diverge on (a kind could override `max_effort_setpoint` without
  updating `max_effort_schedule`'s default reliance on it, if that default
  isn't written to call through it). Mitigated structurally: the default
  body for `max_effort_schedule` must call `self.max_effort_setpoint(...)`
  internally, not reimplement it — see tasks.md's explicit requirement.

## Migration Plan

No data migration — this is a pure additive primitive (new trait methods
with default bodies, one new enum, one new free function), consumed by
nothing yet (Spec E does the wiring). Rollout is a single atomic merge, same
as Spec A/B. Rollback is a plain `git revert` — nothing else in the tree
calls any of this change's new surface, so there's no compatibility concern
either direction.

## Open Questions

- Exact placement of `LimitTier` (this doc proposes `entities/capacity_curve.rs`,
  alongside `CommitmentDirection`, since the two are always used together —
  confirm no better home exists once implementation starts).
- Whether `max_effort_schedule`'s default-body signature should take
  `t1`/`t2` (duration) or `t1`/`t_end` (absolute) — `simulate_forward`'s own
  existing signature takes absolute timestamps per point, so leaning toward
  `t1`/`t_end` for the trait method to avoid a duration-to-timestamp
  conversion in every default-body call site, with `asset_max_power` (the
  public free function) taking `t2: Duration` as proposal.md's stated
  signature promises, converting once at the boundary.
