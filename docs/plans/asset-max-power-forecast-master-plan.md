# Master Plan: Asset Max-Power Forecast & Unified Capacity/Envelope Engine

> **Status:** planning — no implementation started. This is the sequencing document
> for turning `docs/plans/asset-max-power-forecast-spec.md` (the requirements — now
> the leading source of truth for this area) into working code, across five
> dependent specs. It supersedes `openspec/changes/capacity-envelope-unification/
> design.md`, which was built on a premise (relative-curve site headroom, shiftable
> loads excluded from the unified engine) the spec has since overturned.

## Why five specs instead of one change

The spec (`asset-max-power-forecast-spec.md`) defines a function —
`assetMaxPower(planState, t1, t2, direction, limitTier)` plus the triangle-builder
that calls it — as if the supporting machinery already existed. It doesn't. Getting
from here to there touches three separate architectural layers (asset physics
dispatch, MILP scheduling dispatch, and the forecast modules that consume both), and
each layer has its own closed enum or bespoke parameter list standing in the way.
Bundling all of it into one change would mean touching the asset trait hierarchy,
the MILP port, both forecast modules, and adding new domain concepts (`limitTier`,
a general state-at-future-time resolver) in one undifferentiated diff — hard to
review, hard to test incrementally, and impossible to stop halfway without leaving
the tree in a worse state than either endpoint.

Splitting along the dependency boundaries instead means each spec:
- has its own test-first verification and can ship independently to `main`,
- leaves the tree fully working (existing behavior unchanged) until the final spec
  switches consumers over,
- can be picked up in a separate session/worktree without re-deriving the whole
  design each time (this document is what a fresh session reads first).

## Dependency graph

```
Prep — tick-physics-deduplication (R-70)
   |
   v
Spec A (Asset trait: closed enum → trait object)
   |
   +--> Spec B (Shiftable load as a first-class Asset)
   |        |
   |        +--> Spec C (assetMaxPower + limitTier primitive)
   |        |        |
   |        |        +--> Spec E (Unified capacity/envelope engine)
   |        |                  ^
   +-----------------> Spec D (planState(t1) resolver) --------+
```

- **Prep — `openspec/changes/tick-physics-deduplication/`** (found during a
  2026-09-03 architectural audit, tracked as debt R-70) must land before Spec
  A's Phase 2a PV/BaseLoad tasks. Spec A's own Decision D5 adds a new
  `TickOverridable` capability trait directly on top of `SimState::tick()`'s
  hand-written per-type override match — the exact code that currently
  contains three independently-maintained copies of PV irradiance/base-load
  physics (`entities::solar::natural_irradiance_at`, the preview functions'
  hand-copies, and `tick()`'s own third copy). Doing the dedup after Spec A
  would bake the triplication into the new trait methods permanently instead
  of fixing it. This is not one of the five specs — it's a small, independent
  prerequisite fix that happens to sit in the same function Spec A is about to
  restructure.
- **A** is the foundation: nothing else should be built on top of the closed-enum
  dispatch pattern if it's about to be replaced.
- **B** depends on A because it's the first asset added *as* a trait object rather
  than as a 6th enum variant — it's simultaneously "add shiftable load" and "prove A
  works for a real new asset type," which is the right order to catch A's gaps early
  and cheaply (one asset, not five, if the trait shape is wrong).
- **C** depends on B because `limitTier` and the max-effort setpoint primitive must
  be defined for *all* assets uniformly, including the newly-added shiftable load —
  defining it against 4 assets and bolting shiftable load on afterward risks a
  primitive that doesn't actually fit the discrete/non-interruptible case.
- **D** depends only on A (it resolves `AssetState` at a future `t1`, which needs the
  trait-object `Asset` interface to be settled, but doesn't need shiftable load or
  `limitTier` to exist). It can be built in parallel with B/C by a second
  session/worktree once A has merged.
- **E** depends on both C and D — it is the spec's actual `assetMaxPower`/
  triangle-builder, and needs both the per-asset primitive (C) and the starting-state
  resolver (D) to exist first. E is also where `capacity_forecast.rs` and
  `envelope_forecast.rs` are finally retired in favor of one engine.

Nothing downstream of A should start until A has merged and all four existing test
suites are green against it — A changes how *every* asset is dispatched, so a bug
introduced there surfaces everywhere, not just in the new code paths.

---

## Spec A — Asset dispatch: closed enum → trait object

**Problem it solves:** `AssetConfig`/`AssetState` (`VEN/src/assets/mod.rs`) are
closed 5-variant enums (`Battery, Ev, Heater, Pv, BaseLoad`) dispatched through the
`delegate_asset!`/`delegate_asset_state!` macros. Adding a 6th physics type means
editing the enum and every macro call site — mechanical, but a many-file diff on
every future addition, and inconsistent with the MILP layer, which R-23 already
moved to trait-object dispatch (`Vec<Box<dyn AssetMilpContext>>`) specifically to
stop `milp_planner` importing concrete asset types.

**What changes:** `AssetConfig`/`AssetState` become (or are wrapped by) a
`Box<dyn Asset>`-style registry. The existing `Asset` trait (`step`, `capability`,
`flexibility_floor`, `simulate_forward`) already has the right shape — this is a
dispatch-mechanism change, not a physics or interface change. `AssetHandle` already
exists and shows the pattern works; the work is removing the closed-enum layer that
currently sits *underneath* it.

**Why now, why first:** every other spec adds a new asset-shaped thing (shiftable
load) or a new cross-asset primitive (`limitTier`, max-effort setpoint). Doing that
work against a dispatch mechanism about to be replaced means paying the migration
cost twice — once to add the thing to the closed enum, again to move it to trait
objects. Converting first means every subsequent spec is written against the target
architecture from the start.

**Explicit non-goal:** this is not "make assets pluggable by third parties at
runtime." It matches the `AssetMilpContext` precedent — new asset kind = new file +
trait impl, no central enum to hunt down — for a known, compile-time-fixed catalog
of asset types. A runtime plugin ABI is a materially larger investment with no
current forcing requirement; do not scope-creep into it.

**Verification:** pure refactor — all four test suites (UI unit, Rust unit/
integration, E2E BDD, resilience) must pass unchanged before and after, per this
repo's `refactoring` convention. No new behavior, so no new BDD scenario is required
for this spec specifically.

---

## Spec B — Shiftable load as a first-class Asset

**Problem it solves:** shiftable loads are not `AssetConfig`/`AssetState` variants
at all today. They're threaded as separate `&[ShiftableLoad]`/
`&[ShiftableLoadRuntime]` parameters into both `capacity_forecast.rs` and
`envelope_forecast.rs`, with window-logic helpers (`already_run`,
`valid_start_exists_at`) duplicated/cross-imported between the two modules. In the
MILP planner they get a third, separate treatment: a bespoke `ShiftableLoadMilp`
struct wired directly into `solver_phase1`/`solver_phase2`, bypassing
`AssetMilpContext` entirely — the same architectural debt R-23 already fixed for
Battery/EV/Heater, just not yet applied here.

**Physical model (confirmed this session):** a shiftable load is an EV-shaped asset
with three simplifications:
- fixed power (no modulation: `p_min_kw == p_max_kw`, unlike EV's continuous/stepped
  range),
- non-interruptible once started (unlike EV/heater, which can be paused/resumed
  mid-session — `step()` must force rated power for the remainder of its duration
  once `started`, ignoring whatever setpoint the caller requests),
- a hard `[earliest_start, latest_end]` window rather than EV's soft-preference
  departure time — missing the window is not "suboptimal," it's infeasible.

**What changes:**
1. New `AssetState::ShiftableLoad(ShiftableLoadState)` /
   `AssetConfig::ShiftableLoad(ShiftableLoadConfig)` (as a trait-object asset, per
   Spec A's pattern — not bolted onto the old closed enum). State tracks
   `started: bool` and either `remaining_energy_kwh` or elapsed run time, plus a
   reference to its window.
2. `step()` enforces the non-interruptible/fixed-power physics described above.
3. `AssetMilpContext` gets a `ShiftableLoad` implementation (new `ShiftableLoadScalars`
   alongside `BatteryScalars`/`EvScalars`/`HeaterScalars`, reusing the
   `MilpLoadMode`/`t_dead_step` shape already used for EV/heater deadlines),
   replacing the bespoke `ShiftableLoadMilp` path in `solver_phase1`/`solver_phase2`.
4. `SimSnapshot.assets` gains real shiftable-load entries; both forecast modules
   drop their bespoke `&[ShiftableLoad]`/`&[ShiftableLoadRuntime]` parameters and
   read them from the map like every other asset.

**Why before C:** `limitTier` and the max-effort-setpoint primitive (Spec C) must be
defined against the *hardest* case (a discrete, non-interruptible asset) as well as
the continuous ones, or the primitive will need retrofitting the moment shiftable
load is added under it. Doing B first means C is designed against the real final
asset roster.

**Verification:** test-first per repo convention. Regression coverage: existing
`capacity_forecast.rs`/`envelope_forecast.rs` shiftable-load tests must be ported to
exercise the new asset entry instead of the old bespoke parameters, and the real
planner's `ShiftableLoadMilp` → `AssetMilpContext` migration needs its own MILP
solver test parity check (same schedules produced before/after, on at least the
existing `tests/planner.rs`/`tests/solver.rs` shiftable-load cases).

---

## Spec C — `assetMaxPower` primitive + `limitTier`

**Problem it solves:** this is the one genuinely new trait method the spec
introduces. Everything else it needs (per-asset trajectory simulation) already
exists as `Asset::simulate_forward(initial, setpoints) -> Trajectory` — a schedule
of `(ts, setpoint_kw)` pairs projected forward with clamping already built in. What's
missing is *which* setpoint schedule represents "this asset's own physical extreme
for `direction`, under `limitTier`, held for duration t2."

**What changes:**
1. A new small trait primitive — e.g.
   `fn max_effort_setpoint(&self, state: &AssetState, direction: CommitmentDirection, tier: LimitTier) -> f64`
   — returning the constant (or, for shiftable load, the correctly-placed) setpoint
   that represents going all-in on `direction`. For continuous reservoir assets
   (battery/EV/heater) this is just their rated import/export power under the given
   tier. For shiftable load it encodes the placement rule confirmed this session:
   **import → earliest allowed start** (maximize remaining budget), **export →
   latest allowed start** (minimum forced import for as long as possible — this
   session's correction to the spec's own draft wording, which said "as long as
   possible" but the actual mechanism is "start as late as the window allows").
2. `limitTier` (`Physical | Contractual | UserSet`) becomes a real config concept:
   each `AssetConfig` needs to expose its ceiling under each tier (today there is
   only one, implicit, "physical" ceiling per asset). This likely means a small
   per-asset config addition, not a new asset type.
3. `assetMaxPower(planState, t1, t2, direction, limitTier) → (power, energy)` is then
   a thin composition: resolve `max_effort_setpoint`, build a
   `[(t1, setpoint), (t1+t2, setpoint)]` schedule (or a richer one for
   direction-dependent placement like shiftable load's start time), call
   `simulate_forward`, read `power` off the trajectory's endpoint and `energy` by
   integrating `power_kw × dt` over the returned points. No new simulation logic —
   this composes primitives that already exist post-A/B.

**Why this resolves `design.md` Findings 1 & 2 by construction:** PV's Import
extreme is "curtail to zero" — `max_effort_setpoint` returns 0, contributing no
event, matching the agreed PV fix without a PV-specific carve-out. Heater's Export
extreme is "turn off" — same mechanism, same result, finally fixing the
never-applied heater bug from a general rule instead of a second special case.

**Verification:** unit tests per asset type (battery/EV/heater/PV/base-load/
shiftable-load), each confirming `max_effort_setpoint` picks the physically correct
extreme, plus a worked numeric example per asset mirroring
`pv_export_uses_ceiling_not_ceiling_minus_current`'s style (already in
`capacity_forecast.rs`'s test module) so the reasoning is auditable, not just
asserted.

---

## Spec D — `planState(t1)` resolver

**Problem it solves:** `assetMaxPower` needs a *starting* `AssetState` at an
arbitrary future `t1`, forecasted along the plan's own schedule — "if the plan runs
as intended until `t1`, what state is each asset in." Partial infrastructure exists:
`simulator::forecast::build_forecast_frames` already re-simulates every asset
forward along the plan's schedule for `envelope_forecast.rs`, but it emits a scalar
`AssetForecastPoint` (`planned_kw`, `cap_max_import_kw`, `cap_max_export_kw`) per
slot, not a full `AssetState` — not enough to seed a further `simulate_forward` call
from that point.

**What changes:** generalize the forecast-frame builder to optionally return (or add
a sibling function returning) the full `AssetState` at a requested `t1`, reusing the
same per-asset `step()` calls it already makes internally rather than re-deriving
state from scratch. `t1 = 0` (`now`) must return the live snapshot state exactly —
no forecasting error at the one point where ground truth is available.

**Why independent of B/C:** this resolver only needs the trait-object `Asset`
interface (Spec A) to exist; it doesn't need shiftable load modeled or
`limitTier`/`assetMaxPower` defined to be built and tested on its own (against
battery/EV/heater/PV/base-load first, shiftable load once B lands). This is the one
spec in this plan that can run in parallel with B/C in a separate worktree, since
its only hard dependency is A.

**Verification:** for each existing asset type, confirm the resolver's forecasted
state at a chosen future `t1` matches a direct `simulate_forward` computation over
the same plan schedule (they should be the same computation reused, not two
implementations of the same idea — a divergence here would silently reintroduce the
"two independently-implemented curves" problem this whole effort exists to remove).

**Related but not gating — R-69 (`openspec/changes/battery-efficiency-model-reconciliation/`):**
this verification only checks the resolver against `simulate_forward`, both of
which use `battery.rs`'s (asymmetric) efficiency model — it does not check
whether that forecasted state agrees with what the MILP planner itself believed
when it produced the plan (`battery_milp.rs`'s symmetric model). That
pre-existing mismatch (found during the 2026-09-03 audit) is not created or
worsened by Spec D — it already exists in today's `build_forecast_frames` — so
it isn't a blocking prerequisite the way R-70 is for Spec A. But it's the kind
of thing Spec D's own stated goal (an accurate "state if the plan runs as
intended") would ideally not carry forward silently. Worth resolving R-69
before or alongside Spec D if convenient, and worth adding one extra
verification case to Spec D that compares resolved battery state against
`plan.soc_trajectory_kwh`/`planned_state_by_asset` specifically — if R-69 is
still unresolved when Spec D lands, that comparison will fail and make the
pre-existing gap visible rather than silently inherited.

---

## Spec E — Unified capacity/envelope engine

**Problem it solves:** replaces `capacity_forecast.rs` and `envelope_forecast.rs`
with one engine built on C (per-asset extreme-commitment primitive) and D
(starting-state resolver), per the spec's `maxPower(t1, t2, limitTier)`/
`maxEnergyForecast(t1, t2, limitTier)` triangle-builder definition.

**Confirmed this session — does not require the full triangle:** the spec describes
a continuous `(t1, t2)` domain; it does not require materializing the whole
triangular grid. Both of today's existing consumers are 1-D slices of that domain
and should stay that way:
- **Capacity Forecast** (Diagnostics page) = fix `t1 = 0` (now), sweep `t2` from 0 to
  48h — same shape as today's `compute_capacity_curve`, just computed through the
  unified primitive.
- **Site Headroom** (Controller/History) = sweep `t1` across plan slot timestamps,
  fix `t2 = 0` — same shape as today's `compute_headroom_forecast`.
- **New capability** (not built until asked for): a future-anchored `t1` with a
  `t2` sweep from there — "if the plan holds until 3pm, then we committed to an
  extreme, how does capability decay from there." This is now possible because D
  can resolve state at any `t1`, but it needs its own UI anchor-time control
  (design.md's old open point #7) before it's a finished feature, not a backend
  capability with no consumer (`no-half-built-features`). Scope it as a clearly
  separate follow-on within E, not a blocking requirement for E's initial landing.

**What changes:**
1. `capacity_forecast.rs` and `envelope_forecast.rs` are deleted; their call sites
   (Diagnostics `CapacityForecastChart`, Controller/History `SiteHeadroomChart`) call
   the new unified engine with the appropriate fixed axis.
2. Both curves become **absolute** achievable-power quantities (this session's and
   the spec's shared direction — `t2=0` returns the plan's own unmodified capability
   at `t1`, not a delta from planned dispatch). This finally resolves `design.md`'s
   open point #5, which had flagged relative-vs-absolute as the single gating
   product decision for the whole unification — it's resolved by the spec itself
   now, not left open.
3. `SiteHeadroomChart`'s rendering (today: a band around the live grid-power line,
   which only makes sense for a relative delta) needs rework to display an absolute
   quantity — carried over from `design.md`'s open point #6, still valid, now
   actually actionable since the underlying number is settling on absolute.
4. Shiftable load's contribution to both curves is no longer a plan-relative
   "is the plan's currently-chosen slot deferrable" computation
   (`envelope_forecast.rs`'s `is_planned_running_at`/`planned_start`) — it's the
   same `assetMaxPower` call every other asset gets, using its own window via the
   earliest/latest-start placement from Spec C. This is the resolution to
   `design.md`'s open point #2, which is now overturned as **incorrect**, not just
   superseded: shiftable load's absolute capability *is* expressible without
   reference to the plan's chosen slot, contrary to what that document concluded.

**Verification:** per `workflow` rule 4, since this changes user-observable
Diagnostics/Controller behavior, add or extend a BDD scenario in `tests/features/`
exercising the new absolute-quantity headroom/capacity display end-to-end, not just
unit tests on the engine. Existing per-asset unit tests from `capacity_forecast.rs`
should be ported and re-verified against the unified engine's output rather than
deleted outright, since they encode the worked numeric examples this whole design
has leaned on for auditability.

---

## Disposition of existing documents

- **`openspec/changes/capacity-envelope-unification/design.md`**: delete. Its two
  concrete fixes (PV Finding 1, agreed; heater Finding 2, never applied) are
  subsumed by Spec C's general "commit to your own extreme" rule rather than
  needing separate patches. Its open points #2 and #5 are actively wrong per this
  session's decisions, not just stale. Point #3 (shiftable-load placement being
  "greedy, not MILP-optimal") is factually incorrect for the real planner — checked
  this session; `ShiftableLoadMilp` already gets genuine MILP variables in
  `solver_phase1`/`solver_phase2`, it's only `capacity_forecast.rs`'s own diagnostic
  placement that's greedy, and that whole function is being deleted in Spec E
  anyway. Nothing in it needs to survive into `KEY_LEARNINGS.md` beyond what's
  already captured in this document's reasoning above.
- **`openspec/changes/flexibility-capacity-forecast/`**: already an empty stub
  (`.openspec.yaml` only, no design/spec/tasks) — delete rather than reuse; Spec E
  below is its replacement.
- **`docs/plans/asset-max-power-forecast-spec.md`**: stays as the requirements
  document / source of truth this plan implements. Update its shiftable-load
  Export-direction wording once Spec C lands, since this session corrected the
  mechanism ("start as late as possible," not the draft's "for as long as
  possible") — small wording fix, not a re-open of the open item.
- **This document**: once all five specs are implemented and tested, fold anything
  with durable lessons into `KEY_LEARNINGS.md` (the "physics belongs in the asset,"
  "closed-enum vs. trait-object dispatch" tension is a good candidate) and delete
  this plan file, per the `workflow` no-lingering-plans rule.

## Open items carried forward (not blocking, but not forgotten)

- Heater's reservoir math still ignores ongoing thermal loss and the forced-on
  floor re-engaging at `temp_min_c` (`design.md`'s old point #4) — still a
  documented, accepted simplification; Spec C's per-asset `max_effort_setpoint`
  implementation for heater should carry this caveat forward in its doc comment,
  not silently drop it.
- The pre-existing `soc_trajectory_kwh`/`planned_state_by_asset` UI gap
  (`design.md`'s old point #9) is unrelated to this effort and should be tracked
  separately as its own `ui-transparency` item, not folded in here.
- Export semantics for consumption-only assets in general (the spec's own open
  item — "does export mean literal negative power, or flexibility toward reduced
  import") is answered concretely for shiftable load this session (latest-start
  placement = minimum forced import). Whether the same convention should generalize
  to base load or other future consumption-only assets is still open; revisit if/
  when a second consumption-only asset type is added.

## Suggested execution order

0. `tick-physics-deduplication` (R-70) — before Spec A's PV/BaseLoad tasks
   specifically; can land any time before that point, doesn't need to be the
   very first commit in the branch, just before Spec A's Phase 2a reaches
   PV/BaseLoad.
1. Spec A (foundation, blocks everything).
2. Spec B and Spec D can run in parallel (separate worktrees) once A merges — B
   needs A only; D needs A only. `battery-efficiency-model-reconciliation`
   (R-69) can also land any time before or during this window if convenient —
   not blocking, but see Spec D's "Related but not gating" note.
3. Spec C after B (needs the full asset roster including shiftable load to define
   `limitTier`/`max_effort_setpoint` against).
4. Spec E after both C and D.
5. Delete `design.md` and the empty `flexibility-capacity-forecast/` stub as part of
   Spec A's PR (no reason to wait — they're already known-superseded, and per the
   `workflow` rule, stale plans shouldn't linger even one extra spec-cycle).
