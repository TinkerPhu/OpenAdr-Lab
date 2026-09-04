# Tasks: Asset Dispatch — Closed Enum to Trait Objects

## 1. Survey

- [x] 1.1 Re-run `grep -rlnE "\bAssetConfig\b" VEN/src --include="*.rs"` (the
      broad bare-type pattern, not just `AssetConfig::` — the narrower pattern
      misses struct-field/signature usages like `SimState.asset_configs:
      Vec<AssetConfig>`, per design.md D3's correction) and confirm the file
      list still matches proposal.md's Impact section (files may have changed
      since drafting).
- [x] 1.2 Classification happened implicitly through direct migration rather
      than as an upfront checklist: every storage site (`SimState.asset_configs`
      + its three accessors) was retyped in task 5.1; every construction/
      dispatch site was migrated in tasks 4.1-4.5, 5.2, and 6.4 (downcast via
      `as_any()`/`as_any_mut()` or `asset_type_str()` matching, per the user's
      confirmed choice of Any-based downcasting); the three comment-only sites
      were updated in task 5.4. Task 6.1's final grep confirms nothing was
      missed.
- [x] 1.3 One dispatch site outside `assets/mod.rs` surfaced during migration
      that the original per-asset-type tasks (section 4) didn't anticipate:
      `SimState::tick()`'s own match arm (task 5.3c) and the six call sites
      needing `Any`-based downcasting to recover a concrete type from
      `Box<dyn Asset>` (`pv_preview.rs`, `base_load_preview.rs`, `forecast.rs`,
      `plan_context.rs`, `routes/debug.rs`, `routes/hems/sessions.rs`) — raised
      to the user explicitly (not decided unilaterally) via AskUserQuestion;
      user selected Any-based downcasting over reintroducing enum matching.
- [x] 1.4 Confirmed: `AssetState` and its own dispatch (`delegate_asset_state!`
      was `AssetConfig`-only, not shared) were never touched by any of the
      migration's edits — see 6.3.

## 2. Trait design (Phase 0 of design.md's Migration Plan)

- [x] 2.1 Add the 9 universal methods (`default_setpoint`, `control_schema`,
      `update_config`, `default_comfort_rates`, `default_completion_policy`,
      `default_post_deadline_comfort_bid`, `state_values`, `reset`, `forecast`)
      to the `Asset` trait (`VEN/src/assets/asset_trait.rs`). **Correction
      found during implementation:** "no default bodies" (this task's original
      wording) doesn't compile — `Grid` also implements `Asset` outside
      `AssetConfig` and has no sim-inject/MILP/forecast concept, so `Grid`'s
      existing `impl Asset for Grid` would break. Gave all 9 the same
      panicking-default pattern already used for `id`/`current_state`/
      `history` instead (real bodies wired per-type in Phase 2a, section 4;
      `Grid` inherits the default and is never called this way in practice).
      The three `default_comfort_rates`/`default_completion_policy`/
      `default_post_deadline_comfort_bid` methods belong here, not on
      `MilpParticipant`, despite sounding MILP-specific — see D4's correction
      note.
- [x] 2.2 Define the three D4 capability traits (`MilpParticipant` —
      `build_milp_context` only; `RequestResolvable` — `resolve_request_target`,
      `surplus_charge_kw`, `available_storage_kwh`; `Thermostat` —
      `plan_trajectory`, `thermostat_setpoint_kw`). Place them in
      `VEN/src/assets/asset_trait.rs` or a new sibling file (check the
      500-production-line budget via `scripts/audit_file_sizes.py` before
      deciding). **`TickOverridable` (D5) is deferred to task 5.3, not defined
      here** — see the note below.
- [x] 2.3 Add three `as_milp_participant`/`as_request_resolvable`/
      `as_thermostat` accessor methods to `Asset`, each defaulting to `None`.
      Confirmed compiles standalone (no asset type implements any override
      yet), full test suite green (1196 passed), fmt/clippy clean, file-size
      audit passes — zero behavior change, as expected (nothing calls the new
      trait surface yet).

**Why `TickOverridable` isn't defined in this phase (found during
implementation, 2026-09-04):** D5 assumed each asset's tick-override handling
is self-contained behind `apply_tick_overrides(&mut self, ...)`. Checking
`SimState::tick()`'s actual body: PV's cross-cutting smoothing
(`self.pv_smoothing.update(...)`) is *already* hoisted out of the per-asset
loop, computed once, then copied onto `pv`'s own fields inside the match arm —
so PV genuinely fits the self-contained shape once `TickOverrides` carries the
pre-resolved `irradiance`/`offset`, not the raw override/alpha inputs.
BaseLoad's `self.base_load_smoothing.update(...)` call is **not** yet
hoisted — it still runs inside the match arm using the `BaseLoad` config's own
fields (`baseline_kw_profile`, `appliance_noise_kw`) to compute
`natural_base_kw`. Making `TickOverridable` self-contained for BaseLoad
requires hoisting that resolution out of the loop the same way PV's already
is — genuine design work entangled with rewriting `tick()`'s control flow, not
a trait signature that can be responsibly predefined ahead of that rewrite.
Per design.md R3's own stated mitigation ("fix the trait boundaries when real
information arrives, rather than over-designing... now"), `TickOverridable`'s
exact shape (including whatever `TickOverrides` needs to carry) is decided in
task 5.3, where the `tick()` rewrite actually happens, not guessed at here.

## 3. Trait-object conversion (Phase 1 of design.md's Migration Plan)

- [x] 3.1 Introduced `AssetConfig::to_boxed_asset(&self) -> Box<dyn Asset>`
      (`VEN/src/assets/mod.rs`) as the trait-object construction path,
      alongside the existing enum — a temporary bridge (matches each variant,
      clones the concrete type into a box), not a new permanent wrapper type,
      consistent with D2's "extend, don't introduce a parallel type" (the
      bridge itself is deleted in Phase 3; `AssetHandle`'s own role is
      unrelated and untouched, see D2's note on what `AssetHandle` becomes).
- [x] 3.2 Added `assets::phase1_bridge_tests` (3 tests: `step`, `capability`,
      `flexibility_floor`) proving `Box<dyn Asset>` dispatch via the bridge
      produces bit-identical results to `AssetConfig`'s enum dispatch, for
      Battery. 1199/1199 tests pass (1196 + these 3), fmt/clippy/file-size
      audit clean.

## 4. Per-type trait implementation (Phase 2a of design.md's Migration Plan — incremental)

One task per asset type, in the stated order (Battery → EV → Heater → PV →
BaseLoad — Battery first as simplest, PV/BaseLoad last as most entangled with
forecast-frame code). Each task covers only that type's own trait impl: the 9
universal `Asset` methods (moving existing per-type logic verbatim) plus
whichever capability-trait override(s) design.md D4 assigns it, unit-tested for
equivalence against the still-live `AssetConfig`-dispatched behavior.
`SimState.asset_configs` itself is untouched in this section — see section 5.

`TickOverridable` (D5) is out of scope for this section — deferred to task 5.3
per the note in section 2.

- [x] 4.1 Battery: implements `MilpParticipant` + `RequestResolvable` (per D4's
      table — no `Thermostat`). `available_storage_kwh` had no prior
      `Battery`-only inherent method (was inline in `AssetConfig`'s own impl)
      — added directly on the trait impl. Full test suite green (1206
      passed = 1199 + 7 new equivalence tests), fmt/clippy/file-size-audit/
      ven-architecture-invariant checks all clean.
- [x] 4.2 EV: implements `MilpParticipant` + `RequestResolvable`. Same pattern
      as Battery, including `available_storage_kwh`/`surplus_charge_kw` moved
      verbatim from `AssetConfig`'s match arms (no prior `EvCharger`-only
      inherent methods existed for either). 7 new equivalence tests
      (`assets::phase2a_ev_tests`), covering plugged/unplugged branches.
      `VEN/src/controller/milp_planner/tests/mod.rs` needed no change (its
      `AssetConfig::Ev(...)` fixtures are unaffected — additive change only).
      Full suite green (1213 = 1206 + 7), fmt/clippy/file-size-audit clean.
- [x] 4.3 Heater: implements `MilpParticipant` + `Thermostat` (per D4's table —
      no `RequestResolvable`). `plan_trajectory` already took `&AssetState`
      directly (an associated fn, `Heater::plan_trajectory(cfg, live_state)`,
      not `&self`) — trait impl just delegates. `thermostat_setpoint_kw`
      simplified from the original's `Option<f64>` to plain `f64` (the
      `None` case was purely "not a Heater," now handled by
      `as_thermostat()`'s own `Option`). **File-size fix required:**
      `heater.rs` hit 574 lines (over the 500 cap) after the new trait impls
      — split `control_schema` into `heater_control_schema.rs` and
      `HeaterEmergencyMode` into `heater_emergency.rs` (both legitimate
      separate concerns, same pattern as `pv_preview.rs`/
      `base_load_preview.rs` splitting out of `simulator/mod.rs`) — the
      `control_schema` split alone (507 lines) wasn't enough on its own,
      both splits were needed to clear the cap (both new files register in
      `assets/mod.rs`). 6 new equivalence tests
      (`assets::phase2a_heater_tests`). Full suite green (1219 = 1213 + 6),
      fmt/clippy/file-size-audit clean.
- [x] 4.4 PV: implements none of D4's three capability traits (inherits all
      three `as_*` `None` defaults) — universal `Asset` methods only. No
      change needed to `pv_preview.rs`/`peek_pv_kw_tests.rs` — those consume
      `PvInverter::resolve_power_kw` (from `tick-physics-deduplication`),
      unrelated to this trait wiring. 3 new equivalence tests
      (`assets::phase2a_pv_tests`), including a check that all three
      capability accessors are `None`.
- [x] 4.5 BaseLoad: implements none of D4's three capability traits, same as
      PV — universal `Asset` methods only. 3 new equivalence tests
      (`assets::phase2a_base_load_tests`), same shape as PV's.

**Phase 2a complete.** Full test suite green (1225 = 1219 + 6), fmt/clippy/
file-size-audit clean. All 5 asset types now implement their full `Asset` +
capability-trait surface; `SimState.asset_configs` itself is still untouched
(next: section 5's atomic storage cutover).

## 5. Storage cutover (Phase 2b of design.md's Migration Plan — one atomic commit)

**Do not start until all of section 4 is complete.** `SimState.asset_configs`
is one homogeneous `Vec<AssetConfig>` — its element type changes in a single
commit, not incrementally per asset type.

- [x] 5.1 Changed `SimState.asset_configs`'s type from `Vec<AssetConfig>` to
      `Vec<Box<dyn Asset>>` (`VEN/src/simulator/mod.rs`), and updated
      `find_asset`/`find_asset_mut`/`iter_assets`' return types to
      `&dyn Asset`/`&mut dyn Asset` (not `&Box<dyn Asset>` — clippy's
      `borrowed_box` lint, correctly: the box indirection isn't part of the
      public contract). Hit three trait-object limitations not anticipated in
      design.md, all resolved pragmatically rather than reached for a crate:
      `Box<dyn Asset>` can't derive `Serialize`/`Deserialize` (no variant tag)
      — fixed via `#[serde(skip, default)]`, safe because
      `persist.rs::load_with_params` already unconditionally overwrote
      `loaded.asset_configs = fresh.asset_configs` after every load, so the
      field was already effectively discarded on the persistence round-trip;
      `Clone` isn't object-safe — added a `clone_box(&self) -> Box<dyn Asset>`
      trait method (no default body — a default doing `self` → `Self` requires
      `Self: Sized`, which breaks `dyn Asset` callability) plus a manual
      `impl Clone for Box<dyn Asset>`; `SimState` could no longer derive
      `Debug` (no `Asset: Debug` supertrait, and adding one would force the
      lifetime-bound `AssetHandle` to derive it too) — dropped `Debug` from
      `SimState`'s derive list after confirming via grep that nothing
      formats a whole `SimState` with `{:?}`. `as_any`/`as_any_mut`/
      `clone_box` also have no default bodies for the same object-safety
      reason; every implementor (5 asset types + `Grid` + `AssetHandle`)
      provides its own one-line body. `AssetHandle<'a>` (borrows, not
      `'static`) can't produce a real `'static` trait object for these three,
      so its bodies `unimplemented!()` — never called in practice, since the
      real construction path always holds an owned concrete type.
- [x] 5.2 Migrated every remaining consumer in the same commit: the
      `AssetConfig::<Variant>` construction/dispatch sites in
      `VEN/src/routes/debug.rs`, `VEN/src/simulator/persist.rs`,
      `VEN/src/simulator/plan_context.rs`, `VEN/src/simulator/snapshot.rs`,
      `VEN/src/simulator/tests.rs`,
      `VEN/src/simulator/tests/peek_pv_kw_tests.rs`,
      `VEN/src/simulator/pv_preview.rs`, `VEN/src/simulator/base_load_preview.rs`,
      `VEN/src/simulator/forecast.rs`, and
      `VEN/src/controller/milp_planner/tests/mod.rs`, plus
      `VEN/src/routes/hems/sessions.rs` (the bare-type usage the narrower
      `AssetConfig::` survey missed — see `design.md` D3's correction note).
      Each site either matches on the new `asset_type_str()` (where only the
      kind, not the concrete type, was needed) or recovers the concrete type
      via `as_any().downcast_ref::<T>()`/`as_any_mut().downcast_mut::<T>()`
      per the user's explicit choice of Any-based downcasting over
      reintroducing enum matching. Every `build_milp_context` caller switched
      from `AssetConfig::build_milp_context(...)` (returning `Option<...>`) to
      `cfg.as_milp_participant()?.build_milp_context(...)` (the `Option` now
      comes from the accessor, not the method itself, which always returns
      `Box<dyn AssetMilpContext>` for the three kinds that implement
      `MilpParticipant`). One real dedup win found in passing:
      `snapshot.rs::to_timeline_snapshot` had a *third* independent copy of
      the heater plan-trajectory logic (alongside `Heater::plan_trajectory`
      and `Thermostat::plan_trajectory`) — deleted in favor of
      `cfg.as_thermostat().and_then(|t| t.plan_trajectory(&entry.state))`.
- [x] 5.3a Hoisted BaseLoad's `natural_base_kw`/
      `self.base_load_smoothing.update(...)` resolution out of the match arm
      and before the per-asset loop, mirroring PV's already-hoisted
      `self.pv_smoothing.update(...)`. Verified behavior-preserving (all
      `peek_base_load_kw_matches_tick_output_*` equivalence tests still pass
      unchanged). Committed separately (`c13b60f4`).
- [x] 5.3b Defined `TickOverridable::apply_tick_overrides(&mut self, state:
      &mut AssetState, overrides: &TickOverrides)` and `TickOverrides` (in
      `asset_trait.rs`, alongside the other three capability traits) — note
      the signature grew a `state` param beyond design.md's original sketch:
      EV's plugged-state override writes to `AssetState`, not just config,
      so a `&mut self`-only signature couldn't have worked for it. Implemented
      for Pv/Heater/BaseLoad/Ev (Battery declines); added
      `as_tick_overridable()` (`&mut self` accessor, unlike the other three)
      to `Asset`. **Naming-collision finding:** Heater's trait method shares
      a name with its pre-existing inherent `apply_tick_overrides` (different
      arity) — dot-syntax on a concrete `Heater` always resolves to the
      inherent one; reaching the trait impl needs fully-qualified
      `TickOverridable::apply_tick_overrides(...)` syntax. Confirmed by a
      real compile error, not just reasoned about. Doesn't affect `tick()`'s
      planned rewrite (dispatches through `dyn TickOverridable`, which only
      exposes the trait's own methods). 6 new tests proving each impl matches
      `tick()`'s current match-arm behavior for the same inputs. Committed
      separately (`c7d4e7c6`). Still additive — `tick()` itself not yet
      rewired to call this.
- [x] 5.3c Replaced `tick()`'s `match cfg { AssetConfig::Pv(pv) => ..., ... }`
      (`VEN/src/simulator/mod.rs`) with: build one `TickOverrides` value before
      the per-asset loop (from the already-hoisted PV/BaseLoad resolution),
      then for each `(cfg, entry)` pair, `if let Some(overridable) =
      cfg.as_tick_overridable() { overridable.apply_tick_overrides(&mut
      entry.state, &tick_overrides); }` followed by the existing
      `cfg.step(...)` dispatch (now a direct `Asset` trait call, not a
      `delegate_asset!` macro expansion). `default_setpoint()` also dropped
      its now-redundant `&AssetState` parameter (the trait method never used
      it — an `AssetConfig`-era leftover from when `default_setpoint` was
      matched jointly with state, per `delegate_asset_state!`'s uniform
      shape).
- [x] 5.4 Updated the three comment-only mentions of `AssetConfig` found by the
      broadened survey (`VEN/src/assets/grid.rs`,
      `VEN/src/controller/simulator_port.rs`, `VEN/src/profile/schema.rs`) to
      reflect the trait-object world — no functional change.
- [x] 5.5 Ran the full test suite once this single commit was complete: 1236
      passed (was 1225 at end of Phase 2a; net +11 from new
      `phase2b_asset_type_and_downcast_tests` and
      `phase2b_tick_overridable_tests` modules, minus the retired
      `phase1_bridge_tests`), one pre-existing test needed updating (see 6.4)
      — not a regression, a contract change the test hadn't caught up to yet.

## 6. Cleanup (Phase 3 of design.md's Migration Plan)

- [x] 6.1 Re-ran task 1.1's broadened grep
      (`grep -rlnE "\bAssetConfig\b" VEN/src --include="*.rs"`); zero hits
      outside doc comments documenting migration history (e.g. "moved here
      verbatim from `AssetConfig::available_storage_kwh`'s Battery arm") —
      left in place as provenance, not functional code.
- [x] 6.2 Deleted the `AssetConfig` enum, `delegate_asset!`/
      `delegate_asset_state!` macros, and the `impl AssetConfig` block
      (including the Phase 1 `to_boxed_asset()` bridge) from
      `VEN/src/assets/mod.rs`. Confirmed every one of its former methods has a
      home per D4's table (9 on `Asset`, 6 across the three capability
      traits) — none silently dropped; the now-unused `BatteryMilpContext`/
      `EvMilpContext`/`HeaterMilpContext` imports and the top-level
      `chrono`/`HashMap` imports (only ever needed by the deleted impl block)
      were removed too, with each `#[cfg(test)]` module that relied on them
      via `use super::*` gaining its own explicit `use` instead.
- [x] 6.3 Confirmed `AssetState` (per design.md D1) is untouched: still the
      same `Battery/Ev/Heater/Pv/BaseLoad/Grid` enum, no variant renamed,
      added, or removed. (`delegate_asset_state!` was itself
      `AssetConfig`-only machinery and was removed along with it — it never
      had a life independent of the enum it dispatched.)
- [x] 6.4 The task 3.2 "enum vs boxed" regression tests were rewritten, not
      just deleted-or-kept — once `AssetConfig` was gone there was only one
      implementation left to test, so every `phase2a_*_tests`/
      `phase2a_trivial_delegation_smoke_tests` module was simplified from
      "assert boxed output equals enum output" to direct behavioral
      assertions on the boxed/trait path (e.g. `state_values_exposes_soc`
      replacing `state_values_boxed_matches_enum`). This surfaced one real
      pre-existing test needing a fix, unrelated to the rewrite itself:
      `simulator::persist::tests::save_then_load_round_trip_restores_mutable_state`
      called bare `persist::load()` (not `load_with_params`) and then
      `find_asset()`, which now legitimately returns `None` for every asset
      once `asset_configs` is `#[serde(skip)]`'d — bare `load()` was never a
      real production path (only `load_with_params` is called from
      `main.rs`), so the test was rewritten to look the entry up by iterating
      `loaded.assets` directly, matching what bare `load()`'s documented
      contract actually promises.

## 7. Verification

- [ ] 7.1 UI unit tests: `cd VEN/ui && npm test` (unaffected by this change but
      part of the required full-suite pass per `docs/guidelines/TESTING.md`).
- [x] 7.2 Rust unit + integration: `wsl cargo test -j 2` under `wsl_lock.sh` —
      1236 passed, 0 failed, 3 ignored.
- [ ] 7.3 E2E BDD: `bash run_all_tests.sh --e2e` on Node1 (acquire
      `docker_host_lock.sh` first).
- [ ] 7.4 Resilience: `bash run_all_tests.sh --resilience` on Node1.
- [x] 7.5 `cargo fmt --check` and
      `cargo clippy --all-targets --all-features -- -D warnings` — both clean.
      Clippy caught one real API smell: `find_asset`/`find_asset_mut`/
      `iter_assets` returning `&Box<dyn Asset>`/`&mut Box<dyn Asset>`
      (`clippy::borrowed_box`) — fixed to return `&dyn Asset`/`&mut dyn Asset`,
      since the `Box` indirection was never part of the intended contract.
- [x] 7.6 `python scripts/audit_file_sizes.py` — PASSED.
- [x] 7.7 Re-ran this repo's `ven-architecture` invariant greps — all clean
      (the milp_planner grep's own match is the doc-comment asserting the
      invariant, not a violation of it).
- [ ] 7.8 Once all suites are green: delete
      `openspec/changes/asset-dispatch-trait-objects/` per this repo's workflow
      rule (wave anything durable into `docs/architecture/VEN_ARCHITECTURE.md`
      or `docs/reference/KEY_LEARNINGS.md` first — e.g. the
      closed-enum-vs-trait-object dispatch tension noted in the master plan, the
      capability-trait-split pattern from D4, and the atomic-storage-cutover
      lesson from section 5 as reusable precedent for future asset-shaped work).
