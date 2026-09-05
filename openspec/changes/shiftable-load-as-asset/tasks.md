# Tasks: Shiftable Load as a First-Class Asset

## 1. Survey

- [x] 1.1 Re-run the grep for `ShiftableLoad|ShiftableLoadRuntime|ShiftableLoadMilp`
      across `VEN/src` and confirm the file list still matches proposal.md's
      Impact section (files may have moved since drafting). Confirmed: same
      surface, plus `AssetState`/`AssetKind` themselves are closed enums that
      also need a new arm (design.md D5, `assets/mod.rs::actual_power_kw()`).
- [x] 1.2 Confirm `HemsState` has no `Serialize`/persistence path (design.md
      D4's load-bearing assumption) — re-check `VEN/src/state/mod.rs` and
      `simulator::persist` haven't changed since design.md was written.
      Confirmed unchanged.
- [x] 1.3 Read `persist::load_with_params`'s current id-equality check
      (`VEN/src/simulator/persist.rs`) and confirm the exact fixed-roster vs.
      dynamic-roster split needed for D3's mitigation.

## 2. `AssetState` for shiftable load

**Naming correction found during implementation:** the new physics type is
`ShiftableLoadAsset`, not `ShiftableLoad` — that name is taken by the existing
HEMS request struct (`entities/device_session.rs`). See design.md's naming
note. Also: no `AssetParams::ShiftableLoad`/`ShiftableLoadParams` variant was
added — per D4, config is dynamic (request-sourced), so instances are built
directly (`ShiftableLoadAsset { .. }`) and passed to `SimState::add_asset`
(section 3), bypassing the closed `AssetParams` enum and `to_boxed_asset`
entirely. This also means `simulator/mod.rs`'s `AssetParams`-matching
constructor needed no changes.

- [x] 2.1 Test-first: write unit tests for `ShiftableLoadState` (`started`,
      `elapsed_min`, `actual_power_kw`) and `ShiftableLoadAsset` config
      (`power_kw`, `duration_min`, `earliest_start`, `latest_end`).
- [x] 2.2 Add `AssetState::ShiftableLoad(ShiftableLoadState)`
      (`VEN/src/assets/mod.rs`) and its one required match arm in
      `AssetState::actual_power_kw()` — confirmed via `cargo check` this is
      the only exhaustive `AssetState` match outside each asset's own file.
- [x] 2.3 Implement `ShiftableLoadAsset: impl Asset` (`VEN/src/assets/shiftable_load.rs`,
      new file) — `step()` per design.md D2: first nonzero commanded setpoint
      sets `started = true` and locks to rated `power_kw` for the remaining
      duration regardless of later setpoints; reports finished once the
      duration elapses.
- [x] 2.4 Unit tests: fixed power (no modulation), non-interruptible-once-started
      (a later zero setpoint does not stop it), finishes exactly at
      `started_at + duration_min`. 9 tests added, all passing
      (`cargo test --bin ven-app shiftable_load`); full `cargo check` clean,
      confirming no other exhaustive match on `AssetState` broke.

## 3. Dynamic asset roster (design.md D3)

- [x] 3.1 Test-first: `SimState::add_asset` — pushes onto `assets` +
      `asset_configs`, rejects a duplicate `asset_id` (mirrors today's
      `AppState::add_shiftable_load` duplicate check).
- [x] 3.2 Test-first: `SimState::remove_asset(id)` — removes from both parallel
      vectors by id; no-op-safe if the id isn't present.
- [x] 3.3 Implement `add_asset`/`remove_asset` on `SimState`
      (`VEN/src/simulator/mod.rs`). 5 tests in `simulator/tests.rs`'s new
      `add_remove_asset_tests` module, all passing.
- [x] 3.4 Test-first: `persist::load_with_params` reconciliation — partition
      the id-equality check into fixed-roster (exact match, existing behavior
      unchanged) vs. dynamic-roster (reconcile per-id, drop a stale dynamic
      entry without discarding the rest of the state). Regression test added:
      an unrelated Battery's persisted SoC survives a restart where a
      shiftable-load entry was dropped; the two pre-existing persist tests
      (round-trip, fixed-roster-renamed-id fallback) still pass unchanged,
      confirming the fixed-roster path's behavior is preserved exactly.
- [x] 3.5 Implement the reconciliation split in `persist.rs`. Full suite:
      1251 passed, 0 failed (was 1236 before this change; 15 new tests across
      sections 2-3). `cargo fmt`/`clippy -D warnings` clean (added
      `#[allow(dead_code)]` on `ShiftableLoadAsset`'s struct/impl — not yet
      constructed in production code until section 4 wires the HEMS lifecycle
      in, matching the same staged-rollout pattern Spec A used).

## 4. HEMS request lifecycle wiring (design.md D6)

- [x] 4.1 Test-first: accepting a shiftable-load request
      (`routes/hems/sessions.rs::post_requests`'s shiftable-load fast-path)
      also calls `SimState::add_asset` (via `ctx.sim.lock().await`) with
      `started = false` (`ShiftableLoadAsset::initial_state()`).
- [x] 4.2 Resolved design.md's open question: cancelling an already-`started`
      load is now rejected (`DomainError::SessionConflict`) — implemented in
      `UserRequestService::cancel`, extended to take `sim: &mut SimState`
      (its one call site, `routes/hems/sessions.rs::delete_request`, and 3
      existing unit tests updated). A not-yet-started load's pending asset is
      removed via `sim.remove_asset` in the same call. 2 new tests:
      `test_cancel_rejects_a_started_shiftable_load`,
      `test_cancel_removes_a_pending_shiftable_loads_asset`.
- [x] 4.3 Test-first: window validation at acceptance time — implemented as a
      guard clause in `post_requests` (422 response), matching the existing
      local convention for this fast-path block (ad hoc HTTP error responses,
      not `DomainError` — that convention already governs the "latest_end
      required" check immediately above it in the same function).
- [x] 4.4 Implement 4.1-4.3. (done inline with the test-first tasks above)
- [x] 4.5 Deleted `HemsState.shiftable_runtimes` and its accessor methods
      (`shiftable_runtimes`, `start_shiftable`, `complete_shiftable`'s
      runtime-retain line). `complete_shiftable` itself is kept (closes out
      the request + marks the linked `UserRequest` Completed) but is now
      called from a new, much smaller `publish.rs` block that detects a
      shiftable load's asset_id disappearing from the live `SimSnapshot`
      (see 4.6) rather than an expired-runtime scan.
- [x] 4.5a Added `Asset::is_removable(&self, state: &AssetState) -> bool`
      with a default `false` (design.md D3a); `ShiftableLoadAsset` overrides
      it via its own `is_finished()`. `SimState::tick()` gained a generic
      post-step pass (after the per-asset loop, not interleaved with it —
      removal shifts indices) that removes every asset reporting
      `is_removable`. 3 new tests in `simulator::tests::shiftable_load_removal_tests`.
- [x] 4.6 Deleted `publish.rs`'s manual start/complete polling block and the
      "augment SimSnapshot with running shiftable runtimes" block (both now
      redundant — `simulator::snapshot::to_sim_snapshot` already builds a
      correct, generically-typed `AssetSnapshot` for every asset in
      `iter_assets()`, `ShiftableLoadAsset` included). Replaced with a single
      small block: a still-open request whose `asset_id` has disappeared from
      `sim_snap.assets` has finished; close it out via `complete_shiftable`.
      `publish_sim_tick_result`'s now-unused `plan_snap` parameter removed
      (single call site, `tick.rs`).
- [x] 4.7 Deleted `ShiftableLoadRuntime` from `entities/device_session.rs`;
      confirmed via `grep -rn "ShiftableLoadRuntime\|shiftable_runtimes"
      VEN/src` — empty.

## 5. MILP: `ShiftableLoadMilpContext` (design.md D5)

**Tie-break question resolved:** re-reading `shiftable_tiebreak_expr` closely
showed it was already per-instance — it sums over `pool.shiftable`'s own
`y_shift` vars per load, with no real cross-instance coupling. So it needed
no change at all: it stays a separate call in each solver phase (unchanged),
reading `pool.shiftable` regardless of how that `Vec` gets populated.
`ShiftableLoadMilpContext::objective()` itself returns `0.0` — no per-instance
economic term of its own.

**Real findings requiring more than the planned wiring (design.md D5's two
flagged integration points, plus two more found while implementing):**
1. `AssetKind` needed a new variant + **7** exhaustive-match arms, not 3 as
   design.md estimated: `solver_phase1.rs` (1 objective loop),
   `solver_phase2.rs` (3: declare, phase1-cap objective, friction objective),
   `solver_duals.rs` (2: declare — no-op arm since shiftable's fixed-value
   vars are declared unconditionally outside that match, and objective).
2. `MilpParticipant::build_milp_context` *did* need a signature change after
   all, contrary to the earlier "no changes needed" correction: a shiftable
   load's `asset_id` is per-instance and dynamic (unlike Battery/EV/Heater's
   compile-time-fixed ids), and nothing in the existing signature carries it.
   Added a new `asset_id: &str` parameter (second position); Battery/EV/Heater
   impls ignore it, `plan_context.rs`'s one call site passes `&entry.id`.
3. `run_planner`'s `debug_assert!` enforced "at most one context per
   `AssetKind`" — true for Battery/EV/Heater (one per site) but false for
   shiftable loads (a site can have several). Changed to a per-kind count
   check that exempts `ShiftableLoad`.
4. `solver_duals.rs`'s shadow-price re-solve never went through
   `declare_vars_into_pool`/a bolt-on `&[ShiftableLoad]` param for *any* kind
   (see its own module doc) — it re-declares fixed-value vars straight from
   `MilpInputs.shiftable_loads`, which still needs to exist and get
   populated; see 5.4.
- [x] 5.1 Ported the existing `ShiftableLoadMilp`-based tests. 3 solver-parity
      failures surfaced (`cost_sign.rs`'s opportunity-cost test,
      `planner.rs`'s tie-break and defer-for-savings tests) — root cause: the
      *tests'* `build_asset_contexts` helper builds contexts from
      `profile.assets` (static config), which shiftable loads were never part
      of, so nothing added a `ShiftableLoadMilpContext` to `asset_contexts`
      once the bespoke `&[ShiftableLoad]`-driven path was removed. Fixed by
      adding a `push_shiftable_load_contexts` test helper (mirrors
      `plan_context.rs::build_asset_contexts`'s real generic pass, for
      shiftable loads only) and calling it in the 3 affected tests. All 3 now
      pass; schedules match the pre-migration behavior.
- [x] 5.2 N/A — see the tie-break finding above; nothing to decide.
- [x] 5.3 Added `AssetKind::ShiftableLoad`; fixed all 7 exhaustive-match sites
      (see finding 1 above) plus `run_planner`'s `debug_assert!` (finding 3).
- [x] 5.4 `MilpInputs.shiftable_loads: Vec<ShiftableLoadMilpContext>` (renamed
      from `ShiftableLoadMilp`, kept — needed by `solver_duals.rs`, finding 4)
      is now populated inside `inputs.rs`'s existing generic
      `ctx.milp_params()` match (a new `AssetMilpParams::ShiftableLoad(...)`
      arm), not from a bolt-on `&[ShiftableLoad]` parameter. `build_milp_inputs`
      dropped that parameter entirely (only its production call site and the
      innermost test wrapper needed updating — outer test wrappers still
      accept-and-no-longer-forward it, avoiding a 40+-call-site cascade through
      `run_planner`'s own still-`shiftable_loads`-taking signature, which
      *is* still genuinely used by `translate_to_plan`/`fallback_plan`).
- [x] 5.5 Added `ShiftableLoadScalars` (domain ring, `asset_milp_port.rs`) and
      `AssetMilpParams::ShiftableLoad`. Reused the existing
      `types::ShiftableLoadMilp` struct (renamed `ShiftableLoadMilpContext`,
      left in place in `types.rs` rather than moved to `asset_port.rs` —
      trait impls don't need to live in the same file as the struct, and
      moving it would have been pure churn) instead of introducing a second,
      near-identical struct.
- [x] 5.6 Implemented `AssetMilpContext for ShiftableLoadMilpContext` —
      `asset_id`, `asset_kind`, `milp_params`, `declare_vars_into_pool`
      (pushes into `pool.shiftable`, kept as a `Vec` — see 5.9), `constraints`
      (the hard "exactly one start slot" requirement, moved here from a
      bespoke loop), `objective` (returns `0.0`, see tie-break finding). There
      is no `read_solution` method on the real trait (design.md/tasks.md's
      original wording was wrong) — every kind's results are read directly
      from `pool.{bat,ev,heater,shiftable}` in each solver phase's own
      `read_solve_output`, unchanged for shiftable.
      `impl MilpParticipant for ShiftableLoadAsset::build_milp_context`
      replicates `inputs.rs`'s exact pre-migration `valid_start_slots`/
      `duration_slots` math (including the `time_to_slot` helper) for solver
      parity, plus the new already-`started` case: a single fixed slot 0 for
      whatever duration remains — the start decision is already made, not a
      MILP choice.
- [x] 5.7 Wired in: `solver_phase1.rs`/`solver_phase2.rs` now populate
      `pool.shiftable` via the generic `for ctx in asset_contexts { ctx.declare_vars_into_pool(...) }`
      loop (already existed, unchanged); deleted the bespoke pre-loop
      `shift_vars` construction and the bespoke `sum_y == 1` constraint loop
      (now `ShiftableLoadMilpContext::constraints()`, reached through the
      already-generic `for ctx in asset_contexts { ctx.constraints(...) }`
      loop). `shiftable_tiebreak_expr(&pool.shiftable)` calls: unchanged.
- [x] 5.8 N/A as a distinct task — see finding 4: no kind's results ever went
      through a trait readback path; shiftable's stayed exactly where it was
      (`pool.shiftable`), just now populated generically.
- [x] 5.9 Kept `MilpVarPool.shiftable: Vec<ShiftableLoadMilpVars>` as-is
      (contrary to the original task text) — it's the natural, already-`Vec`-
      shaped pool slot for a multi-instance kind, populated by the trait call
      instead of a bespoke pre-loop. No pool restructuring needed.
- [x] 5.10 Renamed `ShiftableLoadMilp` → `ShiftableLoadMilpContext` in place
      (types.rs) rather than deleting it — see 5.5. `ShiftableLoadMilpVars`
      (`milp_interactions.rs`) is unchanged and still in active use.

**Verification:** full suite 1254 passed, 0 failed (unchanged from before
section 5 — a pure internal refactor). `cargo fmt`/`clippy -D warnings`,
file-size audit, and architecture invariants all clean.

## 6. Forecasting cutover (proposal.md scope)

**Scope split found during implementation, confirmed with the user:**
`capacity_forecast.rs` reads the live `SimSnapshot` (rich `AssetSnapshot.values`
map) and could fully drop both bolt-on parameters. `envelope_forecast.rs`
reads per-slot `AssetForecastFrame`/`AssetForecastPoint`, which only carries
`planned_kw`/`cap_max_import_kw`/`cap_max_export_kw` — no type tag, no values
map — so it cannot derive a shiftable load's *window* (`earliest_start`/
`latest_end`/`duration_min`) from frames without extending `AssetForecastPoint`
itself, which is out of this change's scope (belongs with Spec D's
`planState(t1)` resolver, per the master plan). Resolved as **minimal**:
`envelope_forecast.rs` keeps `shiftable_loads: &[ShiftableLoad]` (static
request data, not the duplicated-*state* problem this change targets) and
only drops `shiftable_runtimes: &[ShiftableLoadRuntime]`, replacing
`already_run()`'s runtime lookup with a `started`-flag read off the live
`SimSnapshot` (already available at the call site). This still eliminates the
real duplication (`ShiftableLoadRuntime` itself, fully deleted in section 4).

- [x] 6.1 Test-first: ported `capacity_forecast.rs`'s existing shiftable-load
      tests to construct their fixtures via a `SimSnapshot.assets` entry
      (`asset_type: "shiftable_load"`, window encoded as
      `earliest_start_unix`/`latest_end_unix` — `AssetSnapshot.values` is a
      flat `HashMap<String, f64>`) instead of `&[ShiftableLoad]`/
      `&[ShiftableLoadRuntime]`.
- [x] 6.2 Removed both bolt-on parameters from `compute_capacity_curve`'s
      public signature entirely; `shiftable_events` now reads shiftable-load
      asset entries from the snapshot.
- [x] 6.3 `envelope_forecast.rs`, per the minimal-scope decision above: kept
      `shiftable_loads: &[ShiftableLoad]`, replaced `shiftable_runtimes:
      &[ShiftableLoadRuntime]` with `snapshot: &SimSnapshot` throughout
      (`compute_headroom_forecast`, `already_run`, `shiftable_down_kw`,
      `shiftable_up_kw`). `valid_start_exists_at` is no longer cross-imported
      by `capacity_forecast.rs` (which now has its own snapshot-based
      equivalent inline in `shiftable_events`) — the two modules each own
      their own small window-check now, a residual, accepted duplication
      given the minimal-scope decision, not the `ShiftableLoadRuntime`
      double-bookkeeping this change exists to remove.
- [x] 6.4 Updated `tasks/sim_tick/{context.rs,forecast_wiring.rs,finalize.rs}`:
      `TickContext.shiftable_runtimes` field deleted; `compute_tick_forecasts`
      builds its `SimSnapshot` once, ahead of both `compute_headroom_forecast`
      and `compute_capacity_curve`, instead of threading a separate runtimes
      parameter through either.

## 7. Cross-cutting cleanup and verification

- [ ] 7.1 Run `grep -rn "ShiftableLoadRuntime\|ShiftableLoadMilp\b\|ShiftableLoadMilpVars"
      VEN/src` and confirm it's empty (the request-facing `ShiftableLoad`
      struct in `device_session.rs` is expected to remain — it's the user
      request, not the deleted runtime/MILP types).
- [ ] 7.2 `scripts/audit_file_sizes.py` — confirm no `VEN/src/` file exceeds the
      500-production-line budget after the new `shiftable_load.rs` and the
      edits to `solver_phase1.rs`/`solver_phase2.rs`/`capacity_forecast.rs`/
      `envelope_forecast.rs`.
- [ ] 7.3 Verify the architecture invariants (`ven-architecture` in
      `.claude/CLAUDE.md`): no `use crate::assets::` in
      `controller/milp_planner` production code; no `use crate::profile` in
      `entities/`/`controller/`/`routes/`.
- [ ] 7.4 `cargo fmt --check` and `cargo clippy --all-targets --all-features -- -D warnings`
      (via `wsl_lock.sh`, local WSL).
- [ ] 7.5 Full Rust unit+integration suite green (`wsl cargo test -p ven-app`).
- [ ] 7.6 UI unit suites green (`VEN/ui`, `VTN/ui` — confirm no UI change is
      actually needed; if any shiftable-load status panel assumes the old
      runtime shape, update it).
- [ ] 7.7 E2E BDD suite on Node2 (`DOCKER_HOST=Node2 bash run_all_tests.sh --e2e`),
      including a BDD scenario exercising the shiftable-load use case end to
      end (accept a request → observe it run → observe it finish), per
      `workflow` item 4 in `.claude/CLAUDE.md`.
- [ ] 7.8 Resilience suite on Node2 (`DOCKER_HOST=Node2 bash run_all_tests.sh --resilience`).

## 8. Documentation

- [ ] 8.1 `docs/history/project_journal.md` — narrative entry covering the
      three-implementations-to-one consolidation, the dynamic-asset-roster
      decision (D3), and the HemsState-not-persisted finding (D4) that avoided
      new persistence work.
- [ ] 8.2 `docs/reference/KEY_LEARNINGS.md` — durable lesson: check whether a
      "config" struct is actually static (profile-file-sourced, rebuild-on-
      restart) or dynamic (request-sourced, no restart guarantee) before
      assuming Spec A's persist-config-skip pattern applies unchanged.
- [ ] 8.3 `docs/architecture/VEN_ARCHITECTURE.md` — add shiftable load to the
      asset roster table/diagram; document the dynamic add/remove capability
      as a documented exception to the otherwise-fixed roster.
- [ ] 8.4 `docs/use-cases/*.md` — document the shiftable-load user-facing
      scenario (accept a washing-machine-style request, it runs within its
      window, cannot be interrupted) if not already covered; link the BDD
      scenario added in 7.7.
- [ ] 8.5 Update `docs/plans/asset-max-power-forecast-master-plan.md` to mark
      Spec B complete, or delete the change dir per the `workflow` rule once
      all of the above is done and tests are green (do not archive).
