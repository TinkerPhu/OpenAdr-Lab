**Naming note (found during implementation):** the new `impl Asset` physics
type is called `ShiftableLoadAsset` (`VEN/src/assets/shiftable_load.rs`), not
`ShiftableLoad` — that name is already taken by the HEMS request struct in
`entities/device_session.rs`, which this document also refers to throughout.
Everywhere below, "`ShiftableLoad`" (the request/config) and
"`ShiftableLoadAsset`" (the simulator physics type) are two different structs.

## Context

Today a shiftable load has **three independent implementations** of the same
concept, none of which is `SimState.asset_configs`:

1. **HEMS request/runtime** (`VEN/src/entities/device_session.rs`,
   `VEN/src/state/mod.rs`): `HemsState.shiftable_loads: Vec<ShiftableLoad>` (the
   accepted user request — power, duration, window) and
   `HemsState.shiftable_runtimes: Vec<ShiftableLoadRuntime>` (a hand-rolled
   started_at/ends_at countdown). `ShiftableLoadRuntime`'s own doc comment: "NOT a
   physics sim asset." `publish.rs::publish_sim_tick_result` polls the current
   plan slot's `allocations` every tick, detects a load that should be running but
   has no runtime yet, and manually constructs a `ShiftableLoadRuntime` — a
   bespoke, sim-external "start detector."
2. **MILP** (`milp_planner/types.rs::ShiftableLoadMilp`,
   `milp_interactions.rs::ShiftableLoadMilpVars`): a bespoke struct pair wired
   directly into `solver_phase1.rs`/`solver_phase2.rs`, bypassing
   `AssetMilpContext` — the exact debt R-23 already fixed for
   Battery/EV/Heater.
3. **Forecasting** (`capacity_forecast.rs`, `envelope_forecast.rs`): both take
   `&[ShiftableLoad]`/`&[ShiftableLoadRuntime]` as bolt-on parameters, with
   window-logic helpers (`already_run`, `valid_start_exists_at`) duplicated/
   cross-imported between the two files.

Crucially, `HemsState` is **not** persisted across restarts (no `Serialize`
derive, no save path — unlike `SimState`, which round-trips through
`simulator::persist::save`/`load_with_params` every tick). A VEN restart today
already silently drops all pending/running shiftable-load requests. This is a
constraint this design can rely on, not one it needs to fix.

Spec A (`asset-dispatch-trait-objects`, merged) established the target pattern
every other asset already follows: `SimState.asset_configs: Vec<Box<dyn Asset>>`,
capability traits (`MilpParticipant`, `RequestResolvable`, `Thermostat`,
`TickOverridable`) reached via `as_*()` accessors, `Any`-downcast as the escape
hatch, mutable runtime state persisted while config is rebuilt from
`AssetParams` on every restart (`SimState::from_params` /
`persist::load_with_params`).

## Goals / Non-Goals

**Goals:**
- Shiftable load becomes a real `Box<dyn Asset>` entry in
  `SimState.asset_configs`, stepped every tick, visible via `iter_assets()`,
  persisted the same way as every other asset's mutable state.
- `ShiftableLoadMilpContext` replaces the bespoke `ShiftableLoadMilp`/
  `ShiftableLoadMilpVars` path via the existing `AssetMilpContext` trait.
- `capacity_forecast.rs`/`envelope_forecast.rs` read shiftable loads from
  `SimSnapshot.assets` like every other asset; the bolt-on parameters and
  duplicated window helpers are deleted.
- `publish.rs`'s manual "detect allocation → construct runtime" polling loop is
  deleted; starting is driven by the same per-tick setpoint-application path
  every other asset already uses.

**Non-Goals:**
- Spec C's `max_effort_setpoint`/`limitTier` primitive — out of scope here;
  Spec B only needs to leave shiftable load in a state Spec C can build on.
- Interrupting/cancelling a load that has already started — physically
  non-interruptible per the confirmed model; unsupported both before and after
  this change.
- Persisting shiftable-load *requests* across a VEN restart — out of scope; see
  Decision D4 for why this is not a regression.
- Any VTN-facing or OpenADR-spec-facing change, and no UI contract change to
  the existing shiftable-load request/status shape.

## Decisions

### D1 — Asset enters `SimState.asset_configs` at request-acceptance time, not at MILP-chosen start time

A `ShiftableLoad` asset instance (state `started: false`) is created and pushed
into `SimState.asset_configs`/`assets` as soon as the HEMS request is accepted
(`routes/hems`), not deferred until the MILP has picked a start slot. This makes
it visible to `iter_assets()`, `capacity_forecast.rs`, `envelope_forecast.rs`, and
`AssetMilpContext` for its *entire* life — pending, running, and (briefly, until
removed — D3) completed — exactly like Battery/EV/Heater are visible whether
idle or active. This is what actually eliminates the bolt-on parameters: if the
asset only existed in `SimState` once running, the forecast modules would still
need a side-channel for "loads that might start soon."

**Alternative considered:** keep constructing the asset only once the MILP
allocates a start slot (closer to today's `ShiftableLoadRuntime` timing).
Rejected — it would leave forecasting needing to see not-yet-started loads via
some other channel, reintroducing exactly the bolt-on parameter this change
exists to remove.

### D2 — Starting is driven by the ordinary per-tick setpoint path, not a bespoke detector

Every simulated asset already receives its plan-chosen power setpoint each tick
and calls `step(setpoint)`. Shiftable load uses the same path: when the plan's
current-slot allocation for this `asset_id` carries a nonzero setpoint
(`power_kw`) for the first time, `step()` sets `started = true` and begins
tracking `elapsed`/`remaining` against `duration_min`. Per the confirmed
physical model, once `started` is true `step()` **ignores** whatever setpoint it
is subsequently called with and forces rated `power_kw` until the duration
elapses, then reports itself finished (see D3).

This deletes `publish.rs`'s manual polling block (`state.shiftable_runtimes()`,
`loads.iter().find(...)`, constructing a `ShiftableLoadRuntime` by hand) — the
simulator's existing tick loop already does this detection generically for
every other asset.

**Alternative considered:** keep a dedicated start-detection step outside the
asset's own `step()`, just retargeted to construct an `AssetState::ShiftableLoad`
instead of a `ShiftableLoadRuntime`. Rejected — this is the same bespoke
mechanism the change is meant to retire, only renamed.

### D3 — Dynamic asset roster: `SimState` gains `add_asset`/`remove_asset`

Battery/EvCharger/Heater/PvInverter/BaseLoad have a fixed-at-boot roster —
`SimState::from_params` builds `asset_configs` once from the static profile and
it never grows or shrinks. Shiftable loads are fundamentally dynamic: a user can
accept a new request, or cancel a pending one, at any time while the sim is
running, and the existing product behavior (arbitrary number of concurrent
shiftable-load requests, `remove_shiftable_load` cancel API) must not become
capped or lose the ability to remove a not-yet-started request.

This requires two new `SimState` methods with no precedent in the current asset
model:
- `add_asset(entry: AssetEntry, config: Box<dyn Asset>)` — pushes onto both
  parallel `assets`/`asset_configs` vectors, rejecting a duplicate `asset_id`
  (mirrors the existing `duplicate asset_id` check in
  `AppState::add_shiftable_load`).
- `remove_asset(id: &str)` — removes from both vectors by id. Called (a) when a
  user cancels a still-`started: false` request (mirrors today's
  `remove_shiftable_load`), and (b) once a `started: true` load's `step()`
  reports it finished, replacing today's `complete_shiftable`.

**Risk:** `persist::load_with_params`'s current "asset id lists must match
exactly, else discard the persisted state entirely" check
(`current_ids != loaded_ids` → fall back to a fresh state) would now trip
spuriously on ordinary shiftable-load churn (a load started/finished between
saves) and blow away *every* asset's mutable state, not just the shiftable
load's. → **Mitigation:** partition the id comparison so it only applies to the
fixed-roster asset kinds (Battery/EV/Heater/PV/BaseLoad, known statically from
`asset_params`); shiftable-load entries are reconciled separately — any
persisted shiftable-load id no longer present in the current live request list
is dropped, and any live entry not yet in the persisted snapshot is added fresh,
without discarding the rest of the restored state.

### D4 — Shiftable-load asset config is *not* rebuilt from a static profile file, and that's fine

For Battery/EV/Heater, `AssetParams` come from the profile file and
`load_with_params` always rebuilds `asset_configs` from *current* params so a
profile edit takes effect on restart (Spec A's persist.rs contract, unchanged).
A shiftable load's config (power_kw, duration, window) is per-request data with
no profile-file equivalent — its source of truth is the live
`HemsState.shiftable_loads` list.

Because `HemsState` is not persisted today (confirmed in Context — no
`Serialize` derive, no save path), a VEN restart already drops all pending/
running shiftable-load requests before this change. After this change, the same
restart produces zero shiftable-load entries in the rebuilt `asset_configs`
(there is nothing in the in-memory `HemsState` to source them from) — identical
observable behavior to today, just arrived at because the asset roster is
rebuilt from an already-empty list rather than because `ShiftableLoadRuntime`
was never persisted in the first place. No new persistence work is needed, and
none is added.

**Alternative considered:** persist `HemsState.shiftable_loads` so requests
survive a restart. Rejected as out of scope — it's an unrelated, pre-existing
gap (also true of `ev_session`, `heater_target`, and every other `HemsState`
field), not something this change should fix incidentally.

### D5 — `ShiftableLoadMilpContext` implementing `AssetMilpContext`

Following the Battery/EV/Heater precedent (`asset_port.rs` for struct
definitions, `assets/shiftable_load.rs` for the cross-file inherent impl
blocks implementing the trait's real methods — `asset_id`, `asset_kind`,
`milp_params`, `declare_vars_into_pool`, `constraints`, `objective`, and
`read_solution`, per `controller/asset_milp_port.rs`'s actual `AssetMilpContext`
definition), a new `ShiftableLoadScalars` context struct captures what
`ShiftableLoadMilp` already computes (`power_kw`, `duration_slots`,
`valid_start_slots`) plus a `MilpLoadMode` analogous to `EvMilpMode`/
`HeaterMilpMode` (`MustRun` if not yet started and still schedulable,
`MustNotRun` once `started` — the decision is already made, encode it as a
fixed schedule, not a re-optimized one).

**This is not a drop-in replacement — two integration points need real design
work, not just wiring (found by reading the actual solver code, not assumed):**

1. **`MilpParticipant::build_milp_context` is one shared trait method**
   (`assets/asset_trait.rs:292`), not a per-type-free signature — it already
   carries Battery/EV/Heater-specific parameters (`ev_session`,
   `heater_target`, `ev_min_charge_kw`, `heater_anchor`, ...). **Checked and
   ruled out as a concern:** `ShiftableLoad`'s own config (`power_kw`,
   `duration_min`, `earliest_start`, `latest_end`, all on `self`) plus the
   already-present `state`/`n`/`cum_s`/`now` parameters are sufficient to
   compute its valid-start-slot window — `now + Duration::seconds(cum_s[t])`
   gives each slot's absolute timestamp. No new parameter is needed;
   `ShiftableLoad`'s impl underscores-out every EV/heater-specific parameter
   exactly like `Battery`'s impl already does. Battery/EV/Heater's impls and
   the one call site (`simulator/plan_context.rs:95`) are untouched.
2. **`AssetKind` is a closed 3-variant enum matched exhaustively, with no
   wildcard arm, at three sites**: `solver_phase1.rs`, `solver_phase2.rs`, and
   `solver_duals.rs` (e.g. `solver_phase1.rs:127`,
   `match ctx.asset_kind() { Battery => .., Ev => .., Heater => .. }`).
   Today's shiftable-load objective contribution is *not* per-instance through
   this loop at all — it's a separate cross-instance tie-break term,
   `shiftable_tiebreak_expr(&pool.shiftable)` (`solver_phase1.rs:146`), which
   biases toward earliest start **across all shiftable loads together**, not
   something the `for ctx in asset_contexts { ctx.objective(...) }` per-instance
   shape naturally expresses. Adding `AssetKind::ShiftableLoad` requires
   updating all three exhaustive matches, **and** deciding how the aggregate
   tie-break is expressed once shiftable loads are just N more
   `Box<dyn AssetMilpContext>` entries in a flat list instead of their own
   `pool.shiftable: Vec<ShiftableLoadMilpVars>` field — either (a) each
   `ShiftableLoadMilpContext::objective()` bakes in its own per-instance bias
   term (e.g. proportional to its own chosen start slot), or (b) a filter step
   (`asset_contexts.iter().filter(|c| c.asset_kind() == ShiftableLoad)`)
   recreates the cross-instance aggregate outside the per-kind match, mirroring
   today's `pool.shiftable`-based call just sourced from the generic list. This
   is an open design choice (see Open Questions), not settled by this document.

This replaces `ShiftableLoadMilp`/`ShiftableLoadMilpVars` and their bespoke
wiring in `solver_phase1.rs`/`solver_phase2.rs`/`solver_duals.rs`/`results.rs`
once both integration points above are resolved.

### D3a — Generic removal trigger: a new `Asset::is_removable` default method

**Gap found in review, after D3/D5 were written:** `ShiftableLoadAsset::step()`
correctly stops drawing power once finished, but nothing calls
`SimState::remove_asset` for that case — `is_finished()` is a
`ShiftableLoadAsset`-only inherent method, unreachable from generic tick-loop
code without downcasting every asset via `Any`, which is exactly the
per-kind-branching this codebase's `declare-dont-branch` convention forbids.

Fix: add a tenth universal `Asset` trait method with a safe default —
`fn is_removable(&self, _state: &AssetState) -> bool { false }` — every
existing asset kind inherits the default unchanged (they're never dynamically
removed). `ShiftableLoadAsset` overrides it to call its own `is_finished()`.
A generic post-tick pass (`SimState::tick()` or the `sim_tick` task, run after
the per-asset step loop so it isn't mutating `assets`/`asset_configs` while
also iterating them) collects ids where `is_removable(&entry.state)` is true
and calls `remove_asset` for each — no per-kind branching, matching how
`as_milp_participant`/`as_request_resolvable`/etc. already default to an inert
value for kinds that don't have the capability.

### D6 — HEMS request surface unchanged; it now drives asset lifecycle instead of `HemsState.shiftable_runtimes`

`routes/hems`'s accept API keeps its current request shape (`ShiftableLoad` in
`entities/device_session.rs` stays as-is — it's the user's "do this sometime in
this window" record, not the physics config). Accepting a request additionally
calls `SimState::add_asset` (D3).

Cancellation goes through `AppState::cancel_request` (`state/mod.rs:411-434`),
the generic request-cancellation path shared with EV/heater and used by the
unified `/user-requests` API. **Correction from an earlier draft of this
document:** that path does **not** currently guard against cancelling an
already-started shiftable load — it unconditionally does
`shiftable_loads.retain(...)` / `shiftable_runtimes.retain(...)` regardless of
run state. There is no existing "can't cancel while running" behavior to
preserve; this change must make an explicit choice instead of assuming one:
- **Recommended:** once the asset's `started` flag is true, `cancel_request`'s
  `ShiftableLoad` arm calls into the asset for a cancel attempt that the asset
  itself rejects (the physics is genuinely non-interruptible per the confirmed
  model — a cancel that silently removed a running load's `Box<dyn Asset>`
  entry would desync `SimSnapshot`'s power accounting from what the simulator
  is still actually drawing). Not-yet-started requests still cancel via
  `SimState::remove_asset` as before.
- **Alternative:** preserve today's permissive behavior (cancel always
  succeeds, even mid-run) and let the physics continue running with no linked
  request — rejected as it leaves an asset in `SimState` with no corresponding
  HEMS request, an orphaned-state shape no other asset kind has.

`HemsState.shiftable_runtimes` and its accessor methods (`shiftable_runtimes`,
`start_shiftable`, `complete_shiftable`) are deleted — `SimSnapshot.assets` is
the single source of truth for "is this load running," matching every other
asset kind.

## Risks / Trade-offs

- **[Risk]** Dynamic asset add/remove is new simulator capability with no
  existing test coverage pattern (every current test constructs a fixed asset
  list once). → **Mitigation:** add dedicated `SimState::add_asset`/
  `remove_asset` unit tests before touching any call site (test-first per repo
  convention), including a duplicate-id rejection test and a
  remove-then-persist-then-reload round trip.
- **[Risk]** `persist::load_with_params`'s id-list equality check must be
  restructured (D3) without weakening its purpose (guarding against silently
  serving stale mutable state for a *renamed* fixed-roster asset). →
  **Mitigation:** keep the existing exact-match check for the fixed-roster
  subset; add narrow, separately-tested reconciliation logic only for the
  dynamic subset.
- **[Risk]** MILP solver behavior change: `ShiftableLoadMilpContext` must
  produce identical schedules to today's `ShiftableLoadMilp` on existing
  fixtures, or a shiftable-load regression ships silently. →
  **Mitigation:** a solver-parity test comparing schedules before/after on the
  existing `tests/planner.rs`/`tests/solver.rs` shiftable-load cases, per the
  master plan's own verification note for this spec.
- **[Trade-off]** `AssetState::ShiftableLoad`/`ShiftableLoadMilpContext` add a
  sixth asset kind's worth of surface to every place that pattern-matches or
  enumerates asset kinds (UI panels, diagnostics, `AssetKind` enum) — same
  linear cost every prior asset addition has paid, not a new problem class.

## Migration Plan

No data migration — `HemsState` is not persisted (D4), so there is no on-disk
shiftable-load state to migrate. Roll out as a single atomic merge (matching
Spec A): implement the new asset type and `AssetMilpContext` impl behind
existing tests, cut over `capacity_forecast.rs`/`envelope_forecast.rs` and the
MILP solver phases, delete `ShiftableLoadRuntime`/`ShiftableLoadMilp`/
`ShiftableLoadMilpVars` and the `publish.rs` polling block in the same change
once the replacement path is verified. Rollback is a plain `git revert` — no
persisted-state compatibility concern either direction.

## Open Questions — all resolved during implementation

- ~~Which aggregate-tie-break option~~ — moot: `shiftable_tiebreak_expr` was
  already per-instance (sums each load's own `y_shift` vars independently, no
  real cross-instance coupling despite the name). Left completely unchanged,
  called separately in each solver phase exactly as before.
- ~~Cancel semantics~~ — resolved per D6's recommendation: cancelling an
  already-`started` load is rejected (`DomainError::SessionConflict`),
  implemented and tested in `UserRequestService::cancel`.
- ~~Does a pending load contribute to `capacity_forecast.rs`'s current-instant
  `cap_kw`~~ — confirmed via test: a pending load's `started` flag is false,
  so `shiftable_events` includes it in the forecast curve at its earliest
  valid start, contributing nothing to the *current instant* — matches every
  other asset kind's "current real power" convention.
- ~~Window-already-expired validation~~ — resolved: validated at acceptance
  time in `post_requests` (422 response), per the boundary-translation
  convention.

One genuinely new question surfaced only once MILP wiring was underway (not
foreseeable from the request/physics/forecast layers alone): `AssetKind`
turned out to be matched exhaustively at **7** sites across
`solver_phase1.rs`/`solver_phase2.rs`/`solver_duals.rs`, not the 3 originally
found, and `MilpParticipant::build_milp_context`'s shared signature *did* need
a new `asset_id: &str` parameter after all (a shiftable load's id is dynamic,
unlike Battery/EV/Heater's fixed ones) — seed D5's correction note was wrong.
Both are now fixed; see tasks.md section 5's findings for the full list.
