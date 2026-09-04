# Tasks: Asset Dispatch — Closed Enum to Trait Objects

## 1. Survey

- [x] 1.1 Re-run `grep -rlnE "\bAssetConfig\b" VEN/src --include="*.rs"` (the
      broad bare-type pattern, not just `AssetConfig::` — the narrower pattern
      misses struct-field/signature usages like `SimState.asset_configs:
      Vec<AssetConfig>`, per design.md D3's correction) and confirm the file
      list still matches proposal.md's Impact section (files may have changed
      since drafting).
- [ ] 1.2 For each file in that list, classify every hit as **construction**
      (`AssetConfig::Battery(...)` building a value), **storage** (a struct
      field or method signature typed `AssetConfig`/`&AssetConfig`/
      `&mut AssetConfig`/`Vec<AssetConfig>` — e.g. `SimState.asset_configs` and
      its `find_asset`/`find_asset_mut`/`iter_assets` accessors in
      `VEN/src/simulator/mod.rs`), **dispatch** (a `match` on `AssetConfig`
      outside `delegate_asset!`/`delegate_asset_state!`), or **comment-only**
      (the type name appears only in a doc comment, e.g. the mentions already
      found in `VEN/src/assets/grid.rs`, `VEN/src/controller/simulator_port.rs`,
      `VEN/src/profile/schema.rs` — no functional change needed, just update
      the comment during migration). Record the classification inline as a
      checklist here before starting Phase 2 work, per design.md Decision D3.
- [ ] 1.3 If any dispatch-site match is found outside `assets/mod.rs`, add a
      dedicated migration task for it in section 5 below (task 5.3; not covered
      by the generic per-asset-type tasks in section 4, since it implies
      `delegate_asset!` isn't the sole dispatch point after all — a design
      assumption worth flagging back to the user if it happens).
- [ ] 1.4 Confirm `AssetState`/`delegate_asset_state!` call sites are untouched
      by this survey — cross-check that the files above don't conflate
      `AssetConfig` and `AssetState` matches when classifying.

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
- [ ] 4.2 EV: implements `MilpParticipant` + `RequestResolvable`. Same,
      including `VEN/src/controller/milp_planner/tests/mod.rs` if it
      constructs `AssetConfig::Ev(...)` fixtures.
- [ ] 4.3 Heater: implements `MilpParticipant` + `Thermostat` (per D4's table —
      no `RequestResolvable`). Same.
- [ ] 4.4 PV: implements none of D4's three capability traits (inherits all
      three `as_*` `None` defaults) — universal `Asset` methods only. Same,
      paying particular attention to `VEN/src/simulator/pv_preview.rs` and
      `VEN/src/simulator/tests/peek_pv_kw_tests.rs`.
- [ ] 4.5 BaseLoad: implements none of D4's three capability traits, same as
      PV — universal `Asset` methods only. Same, paying particular attention
      to `VEN/src/simulator/base_load_preview.rs`.

Run the full test suite (per `docs/guidelines/TESTING.md`) after each of 4.1–4.5
— never leave two types' trait impls half-written.

## 5. Storage cutover (Phase 2b of design.md's Migration Plan — one atomic commit)

**Do not start until all of section 4 is complete.** `SimState.asset_configs`
is one homogeneous `Vec<AssetConfig>` — its element type changes in a single
commit, not incrementally per asset type.

- [ ] 5.1 Change `SimState.asset_configs`'s type from `Vec<AssetConfig>` to
      `Vec<Box<dyn Asset>>` (`VEN/src/simulator/mod.rs`), and update
      `find_asset`/`find_asset_mut`/`iter_assets`' return types to match.
- [ ] 5.2 In the same commit, migrate every remaining consumer: the
      `AssetConfig::<Variant>` construction sites from section 1's survey not
      already covered by section 4 (`VEN/src/routes/debug.rs`,
      `VEN/src/simulator/persist.rs`, `VEN/src/simulator/plan_context.rs`,
      `VEN/src/simulator/snapshot.rs`, `VEN/src/simulator/tests.rs`,
      `VEN/src/controller/capacity_forecast.rs`), plus
      `VEN/src/routes/hems/sessions.rs` (the bare-type usage the narrower
      `AssetConfig::` survey missed — see `design.md` D3's correction note).
      Any caller of `build_milp_context` also needs to switch from
      `AssetConfig::build_milp_context(...)` to going through the asset's
      `as_milp_participant()` accessor.
- [ ] 5.3 Confirmed dispatch site (design.md D5): rewrite `SimState::tick()`'s
      `match cfg { AssetConfig::Pv(pv) => ..., ... }`
      (`VEN/src/simulator/mod.rs:231-289`) as a `TickOverridable` capability
      trait. Both the trait definition and its implementations happen here
      (not in sections 2/4 — see section 2's note on why this was deferred):
      1. Hoist BaseLoad's `natural_base_kw`/`self.base_load_smoothing.update(...)`
         resolution out of the match arm and before the per-asset loop,
         mirroring how PV's `self.pv_smoothing.update(...)` is already hoisted
         today — this is the prerequisite that makes BaseLoad's override
         handling self-contained.
      2. Define `TickOverridable::apply_tick_overrides(&mut self, overrides:
         &TickOverrides)` and `TickOverrides`, carrying **pre-resolved**
         values (PV's resolved `irradiance`/`offset`, BaseLoad's resolved
         `baseline_kw`) rather than the raw override/alpha inputs `tick()`
         takes as bare arguments — each asset's impl should only need to
         assign fields, not recompute smoothing.
      3. Implement it for Pv/Heater/BaseLoad/Ev (Battery declines — no arm in
         `tick()`'s current match); add the `as_tick_overridable()` accessor
         to `Asset` in the same commit as the other three (retroactively
         completing task 2.3 for this fourth trait).
      4. Replace the match with a loop over `as_tick_overridable()`.
- [ ] 5.4 Update the three comment-only mentions of `AssetConfig` found by the
      broadened survey (`VEN/src/assets/grid.rs`,
      `VEN/src/controller/simulator_port.rs`, `VEN/src/profile/schema.rs`) —
      no functional change, just keep the doc comments accurate.
- [ ] 5.5 Run the full test suite once this single commit is complete.

## 6. Cleanup (Phase 3 of design.md's Migration Plan)

- [ ] 6.1 Re-run task 1.1's broadened grep
      (`grep -rlnE "\bAssetConfig\b" VEN/src --include="*.rs"`); confirm zero
      hits outside `assets/mod.rs`.
- [ ] 6.2 Delete the `AssetConfig` enum and the `delegate_asset!` macro from
      `VEN/src/assets/mod.rs`. Confirm every one of its 15 former
      non-trait-mirrored methods now has a home per D4's table (9 on `Asset`,
      6 across the three capability traits) — none silently dropped.
- [ ] 6.3 Confirm `AssetState` and `delegate_asset_state!` are untouched (per
      design.md D1) — diff `VEN/src/assets/mod.rs` against its pre-change state
      to confirm only `AssetConfig`-related code was removed.
- [ ] 6.4 Delete the now-redundant regression test from task 3.2 if it no longer
      has two paths to compare (or keep it if `AssetHandle`'s old
      borrowed-reference test coverage still benefits from the comparison —
      judgment call at cleanup time).

## 7. Verification

- [ ] 7.1 UI unit tests: `cd VEN/ui && npm test` (unaffected by this change but
      part of the required full-suite pass per `docs/guidelines/TESTING.md`).
- [ ] 7.2 Rust unit + integration: `wsl cargo test -p ven-app` (acquire
      `wsl_lock.sh` first per this repo's CLAUDE.md; check free RAM before
      starting).
- [ ] 7.3 E2E BDD: `bash run_all_tests.sh --e2e` on Node1 (acquire
      `docker_host_lock.sh` first).
- [ ] 7.4 Resilience: `bash run_all_tests.sh --resilience` on Node1.
- [ ] 7.5 `cargo fmt --check` and
      `cargo clippy --all-targets --all-features -- -D warnings`.
- [ ] 7.6 `python scripts/audit_file_sizes.py` — confirm `VEN/src/assets/mod.rs`,
      `VEN/src/assets/asset_trait.rs` (or wherever the capability traits land),
      and any file touched during migration stay within the 500-production-line
      limit.
- [ ] 7.7 Re-run this repo's `ven-architecture` invariant greps from the
      project's own CLAUDE.md (`use crate::profile` absence in
      entities/controller/routes; `use crate::assets::` absence in
      milp_planner/entities outside `cfg(test)`/`tests/`) — none of these should
      be affected by this change, but confirm rather than assume.
- [ ] 7.8 Once all suites are green: delete
      `openspec/changes/asset-dispatch-trait-objects/` per this repo's workflow
      rule (wave anything durable into `docs/architecture/VEN_ARCHITECTURE.md`
      or `docs/reference/KEY_LEARNINGS.md` first — e.g. the
      closed-enum-vs-trait-object dispatch tension noted in the master plan, the
      capability-trait-split pattern from D4, and the atomic-storage-cutover
      lesson from section 5 as reusable precedent for future asset-shaped work).
