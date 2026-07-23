## 1. Sim-inject field (data model)

- [x] 1.1 Add `pv_export_limit_kw: Option<f64>` to `SimInjectState`
      (`VEN/src/entities/sim_inject.rs`), defaulting to `None`, following
      the exact shape of `grid_export_limit_kw` in the same struct.
- [x] 1.2 Add `pv_export_limit_kw` to `PostSimInjectBody`
      (`VEN/src/routes/sim.rs`), wire it through the existing `merge_f64!`
      macro.
- [x] 1.3 Unit test: merging a `pv_export_limit_kw` value into
      `SimInjectState` sets/clears the field correctly (mirror the
      existing `grid_export_limit_kw` merge test if one exists, else add
      alongside it). (No prior merge tests existed in `routes/sim.rs`;
      added a `#[cfg(test)] mod tests` block covering set/clear/absent.)

## 2. Replan trigger

- [x] 2.1 Add `pv_export_limit_kw` to the sim-inject replan-trigger
      condition in `routes/sim.rs` (the check currently at `sim.rs:232`
      that includes `body.grid_export_limit_kw.is_some()`). Extracted the
      inline boolean into a pure `body_triggers_replan()` function for
      testability.
- [x] 2.2 Unit/integration test: POSTing a changed `pv_export_limit_kw`
      triggers an out-of-cycle replan, same assertion shape as the
      existing `grid_export_limit_kw` replan-trigger test. (Tested via the
      new pure `body_triggers_replan()` function — no prior integration
      test of this handler existed to mirror.)

## 3. PV physics wiring (the core fix)

- [x] 3.1 Thread `pv_export_limit_kw` from sim-inject through
      `tasks/sim_tick/tick.rs` into `SimState::tick(...)`'s argument list,
      alongside how `weather_power_kw`/`pv_irradiance_override` are
      already threaded.
- [x] 3.2 In `simulator/mod.rs`, assign the threaded value onto
      `AssetConfig::Pv(pv).export_limit_kw` each tick (same block that
      currently sets `pv.weather_power_kw`, `~simulator/mod.rs:252-262`).
      Apply the sign conversion (positive magnitude in sim-inject/API →
      `≤ 0` internally) at this boundary, matching the convention at
      `assets/grid.rs:47-49` / `tasks/sim_tick/helpers.rs:200-201`.
- [x] 3.3 Unit test: the existing `step_inner_clamps_weather_power_kw_to_export_limit`
      (`pv.rs:371-378`) already covers physics respecting `export_limit_kw`
      directly. Since the *new* code in this change is the sign-conversion
      boundary in `simulator/mod.rs` (not `pv.rs`), that coverage was added
      as an integration test in `simulator/tests.rs` instead (see 3.4) —
      no separate `assets/pv.rs` unit test added, to avoid duplicating
      `step_inner`'s existing clamp coverage.
- [x] 3.4 Integration tests in `simulator/tests.rs`
      (`mod pv_export_limit_tests`): ceiling below natural output clamps
      PV export with correct sign conversion; ceiling above natural output
      has no effect; clearing the ceiling restores natural output on the
      next tick.

## 4. Fold VTN capacity into the same ceiling mechanism; remove dead dispatcher clamp

**Scope correction during implementation:** the VTN's `EXPORT_CAPACITY_LIMIT`
signal (`capacity.export_limit_kw`) was originally meant to stay
out-of-scope (see design.md non-goals as first written), enforced only by
the operator sim-inject. Mid-implementation this was revisited: the VTN
signal is now folded into the *same* `PvInverter.export_limit_kw`
mechanism as the operator override — both are combined (more restrictive
wins) in `tasks/sim_tick/tick.rs::effective_pv_export_ceiling_kw` before
being passed into `SimState::tick`. This makes VTN-driven curtailment
actually work, which was clarified as the primary motivation. Also
corrected a misdiagnosis: `PvInverter.export_limit_kw` (the field) was
already respected by `step_inner` and `peek_pv_kw` — only the
dispatcher's separate `setpoint_kw`-based clamp (a different, redundant
mechanism) was truly dead. No change to `step_inner`'s handling of its
`setpoint_kw` parameter was needed or made.

- [x] 4.1 Remove the PV export-limit clamp block in
      `controller/dispatcher.rs::build_setpoints` (the `if let
      Some(export_cap) = capacity.export_limit_kw { ... }` block) since PV
      physics never consumed the setpoint it produced.
- [x] 4.2 Remove the now-unused `capacity: &OadrCapacityState` parameter
      from `build_setpoints` (nothing else in the function read it), and
      the now-fully-dead `effective_capacity` merge in
      `helpers.rs::build_tick_setpoints` (its only consumer was the
      removed block). Updated both `dispatcher.rs` test call sites and the
      `tick.rs` call site accordingly. Per explicit direction, the
      `grid_import_limit_kw`/`grid_export_limit_kw` sim-inject fields
      (confirmed to have zero consumers even before this change — only
      `capacity.export_limit_kw` was ever read, and only by the removed
      block) were **removed entirely** rather than left as unused dead
      code: `entities/sim_inject.rs`, `routes/sim.rs`
      (`PostSimInjectBody`, `merge_f64!`, `body_triggers_replan`), and
      `VEN/ui/src/api/types.ts`'s `SimInjectState` type. Also corrected
      two stale doc references to these fields in
      `docs/architecture/VEN_ARCHITECTURE.md` and
      `docs/architecture/asset_simulation.md` (which had documented the
      grid inject fields as if they worked, and PV curtailment as
      VTN-only/non-functional — both now describe the real, working
      mechanism).
- [x] 4.3 Add `effective_pv_export_ceiling_kw` unit tests in
      `tick_tests.rs` (tighter-of-two-sources, single-source fallback,
      neither-set) covering the VTN+operator combination logic.

## 5. PV control schema (backend)

- [x] 5.1 Add a third `ControlDescriptor` for `pv_export_limit_kw` to
      `PvInverter::control_schema()` (`assets/pv.rs`, alongside
      `pv_irradiance`/`pv_irradiance_alpha`), unit `kW`, min `0`, max
      `self.rated_kw`. Updated the `schema_snapshot.json` golden fixture
      accordingly.
- [x] 5.2 Unit test `control_schema_includes_export_limit_bounded_by_rated_kw`
      in `assets/pv.rs` confirms the descriptor's kind/bounds/unit.

## 6. VEN UI — control + display

- [x] 6.1 Add `pv_export_limit_kw` to the `SimInjectState` type
      (`VEN/ui/src/api/types.ts`), replacing the removed
      `grid_import_limit_kw`/`grid_export_limit_kw` fields (dead, see §4).
- [x] 6.2 Wire a persistent-override fallback for `pv_export_limit_kw` in
      `AssetRightSection.tsx::getValue`, following the `heater_temp_min_c`/
      `heater_temp_max_c` pattern (not `DECAY_CONTROLS`) — falls back to
      the live sim's (signed) `export_limit_kw`, abs'd for the
      positive-magnitude slider, so it reflects whichever of VTN/operator
      is currently binding. The slider itself needed no new rendering code
      — schema-driven `DynamicControl` already picks up the new descriptor
      automatically.
- [x] 6.3 Confirmed: `Dashboard.tsx`'s existing "Export limit" PV display
      (`Dashboard.tsx:326`) now shows real values once the backend sets
      `export_limit_kw` in PV `state_values()` — no code change needed.
- [x] 6.4 UI unit tests added in `AssetRightSection.test.tsx`
      (`describe("AssetRightSection — PV export limit control")`): shows
      no ceiling when unset, shows live effective ceiling when one is
      active, drag+commit posts positive magnitude and persists locally.

## 7. Verification

- [x] 7.1 `wsl cargo check` and `wsl cargo test -p ven-app` (wsl_lock.sh
      acquired/released around both runs): 789 unit/integration tests +
      1 architecture-invariant test, all passing.
- [x] 7.2 `cargo fmt --check` clean; `cargo clippy --all-targets
      --all-features -- -D warnings` clean.
- [x] 7.3 `cd VEN/ui && npm test` — 409 tests passing (38 files);
      `npm run build` succeeds (tsc + vite); `npx eslint .` — 0 errors (9
      pre-existing warnings, none in touched files).
- [x] 7.4 `scripts/audit_file_sizes.py` — passed after two fixes found
      during verification: (a) `simulator/mod.rs` exceeded the 500-line
      cap by a few lines once the new parameter/doc/assignment were added
      (file was already at 499/500 pre-change) — trimmed doc comments to
      fit; (b) `simulator/tests.rs` exceeded the cap once the new
      `pv_export_limit_tests` module was added — restructured into
      `simulator/tests/mod.rs` (a directory, matching the existing
      `controller/milp_planner/tests/` convention), which the audit script
      exempts by path regardless of size, same as that precedent.
- [x] Architecture invariants (grep checks from `.claude/CLAUDE.md`) — all
      clean: no `use crate::profile` in entities/controller/routes, no
      `use crate::assets::` in milp_planner production code, no
      `use crate::assets::` in entities, `serde_json::Value` in `vtn.rs`
      internal-only.
- [x] Logged `docs/reference/TECHNICAL_DEBTS.md` R-58: the planner's PV
      forecast input is not ceiling-aware yet (only live simulator physics
      respects the ceiling immediately) — Tier 2 scope, not fixed here.
- [x] 7.5 Manual verification on Pi4 (deployed 040-pv-export-curtailment,
      built + `docker compose up -d`, all 4 containers healthy): via
      direct API against ven-1 (port 8211) — `POST /sim/inject
      {"pv_export_limit_kw": 1.0}` while natural PV output was -2.32 kW
      clamped `power_kw` to exactly -1.0 kW within one tick, and
      `export_limit_kw: -1.0` appeared in the asset snapshot. **This
      surfaced a real, pre-existing bug**: clearing via
      `{"pv_export_limit_kw": null}` did not restore natural output —
      traced to serde_json's `Option<T>` null-handling (see §8 below) and
      fixed. Re-verified post-fix: set → clamps, clear via null → natural
      output restored, confirmed both via direct API and will be
      re-confirmed via `POST /sim/inject/reset` equivalence.
- [x] 7.6 Full E2E suite on Pi4 (`bash run_all_tests.sh --e2e`, pi4_lock
      acquired/auto-released): 268 scenarios, 0 failed, 0 skipped,
      including the full resilience suite (VTN restart, VEN restart,
      exponential backoff during a sustained VTN outage) and the
      browser-driven Controller V2 UI scenarios.
- [x] 7.7 Found post-deployment by user testing the live UI: the
      Controller tab's Export Limit slider showed "0.00 kW" when no
      ceiling was active — read as "no export allowed" instead of the
      intended "no limit set". Root cause: `DynamicControl`'s generic
      slider fallback defaults an unset value to `min` (0);
      `AssetRightSection.tsx::getValue("pv_export_limit_kw")` returned
      `null` in that case with no override applied. Fixed by falling back
      to the control's `max` (`rated_kw`) instead — a ceiling at rated_kw
      is non-binding, so the slider now correctly reads as unrestricted.
      Updated the corresponding UI test to assert on 8 (rated_kw), not 0,
      and added a regression assertion that "0.00" is never shown when no
      ceiling is set. Verified: 21/21 `AssetRightSection.test.tsx` tests
      pass, full UI suite 409/409 passing, ESLint clean on both changed
      files.

## 8. Fix: `POST /sim/inject` null-clearing was broken for every field

Discovered during §7.5 manual verification, not part of the original scope
— logged here rather than silently folded into §1-2 since it's a
pre-existing defect affecting all ~13 `PostSimInjectBody` fields, not just
`pv_export_limit_kw`.

- [x] 8.1 Root cause: `serde_json`'s `Option<T>` deserializer intercepts a
      literal JSON `null` and always produces `None` before `T` (here
      `serde_json::Value`) is ever constructed — so `Some(Value::Null)`
      (what the old `merge_f64!`'s `is_null()` check expected) is
      unreachable via real HTTP requests. Confirmed live on Pi4:
      `pv_irradiance` (pre-existing field) exhibited the identical symptom.
- [x] 8.2 Fix: added a shared `double_option` deserializer
      (`routes/sim.rs`) and changed every `PostSimInjectBody` field from
      `Option<serde_json::Value>` to `Option<Option<T>>` (`f64` or `bool`),
      so absent/null/value become distinguishable
      (`None`/`Some(None)`/`Some(Some(v))`). Rewrote `merge_f64!`/
      `merge_bool!` into a single `merge!` macro matching on the new shape;
      `pv_irradiance_alpha`/`base_load_alpha`'s "reset to default on null"
      branches updated the same way.
- [x] 8.3 Added JSON-deserialization-boundary regression tests
      (`json_absent_key_deserializes_to_outer_none`,
      `json_explicit_null_deserializes_to_some_none`,
      `json_value_deserializes_to_some_some`,
      `json_null_round_trip_actually_clears_the_field`,
      `json_bool_field_null_round_trip_actually_clears`) — these use
      `serde_json::from_str` on real JSON strings rather than constructing
      `PostSimInjectBody` directly, specifically to close the test gap that
      let the original bug ship (existing/prior tests all constructed the
      struct directly, never exercising the actual deserialization
      boundary).
- [x] 8.4 Full local verification re-run after the fix: 794 Rust tests
      (was 789 + 5 new), `cargo fmt`/`clippy -D warnings` clean, file-size
      audit clean.
- [x] 8.5 Redeployed to Pi4 (rebuilt ven-1/2/3 + restarted ui) and
      re-confirmed live: `POST /sim/inject {"pv_export_limit_kw": 1.5}`
      clamped `power_kw` to exactly -1.5 kW within one tick; `POST
      /sim/inject {"pv_export_limit_kw": null}` fully restored natural
      output (-4.386 kW, matching pre-test baseline) and removed
      `export_limit_kw` from the asset snapshot — the null-clear path now
      genuinely works end-to-end over the real API, not just in unit
      tests that bypass JSON deserialization.
