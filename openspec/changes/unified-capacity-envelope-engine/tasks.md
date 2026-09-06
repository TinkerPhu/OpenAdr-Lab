# Tasks: Unified Capacity/Envelope Engine (Spec E)

## 1. Survey (confirm design.md's claims against current code)

- [x] 1.1 Re-confirmed unchanged.
- [x] 1.2 Re-confirmed unchanged.
- [x] 1.3 Re-confirmed unchanged.
- [x] 1.4 Re-confirmed unchanged.

## 2. `asset_max_power_series` (D2)

- [x] 2.1 Test: `asset_max_power_series_last_point_matches_asset_max_power`.
- [x] 2.2 Implemented in `assets/max_power.rs`; `asset_max_power` redefined
      to call it.
- [x] 2.3 Test: `shiftable_loads_series_lands_on_the_same_60s_grid_as_a_continuous_asset`
      — confirms D2's "no resampling needed" claim held.
      Also added `asset_max_power_series_reports_every_60s_step_not_just_the_endpoint`
      (not originally listed) to pin the series' actual per-step shape.

## 3. Trajectory reuse for Site Headroom (D4)

- [x] 3.1 Widened to `pub(crate)`; existing `build_forecast_frames` tests
      confirmed unchanged.

## 4. `controller/capacity_envelope.rs` — Capacity Forecast half

- [x] 4.1 Ported worked examples for battery (export exhaustion timing) and
      base load (additive/subtractive constant), through the new engine.
      EV/heater/shiftable-load's underlying physics are already covered by
      Spec C/D's own primitive tests (`asset_max_power`/`max_effort_setpoint`
      per kind); re-deriving every exact numeric example a second time here
      would just re-test already-proven primitives, not the engine's own
      wiring/merge logic — scoped down accordingly.
- [x] 4.2 Tests: `pv_contributes_zero_to_a_sustained_import_commitment`,
      `heater_contributes_zero_to_a_sustained_export_commitment`.
- [x] 4.3 Test: `pv_export_contribution_still_varies_with_the_weather_forecast`.
- [x] 4.4 Implemented `compute_site_capacity_curve`. **Corrected during
      implementation** (design.md's D1 note): PV's Export sweep reuses
      `pv_frames` verbatim (the default 48h plan horizon already matches the
      target sweep range) rather than a new `pv_ceiling_kw`-at-`t2`-samples
      mechanism.
- [x] **New finding, not foreseen in design.md**: `AssetCapability::max_export_kw`
      (and `max_effort_setpoint`'s Export return value) is signed negative —
      `series_to_events` needed an explicit `.abs()`, confirmed by a failing
      test before the fix (see the journal entry for the full story).
- [x] **New finding, not foreseen in design.md**: base load
      (`PowerAdjustability::None`) needed its own exception, third alongside
      PV — routing it through `max_effort_setpoint` naively reports `0.0`
      for Export, silently dropping the old constant net-grid-power offset
      (additive on Import, subtractive on Export) that was never a bug.
      Added `base_load_capacity_events`, ported from the deleted
      `capacity_forecast.rs::base_load_events` verbatim.
- [x] 4.5 Measured: a 4-asset (battery/EV/heater/base-load) 48h/60s sweep,
      both directions, took ~205ms in an unoptimized debug build (release
      will be faster). Fine for a once-per-dispatcher-tick computation — no
      coarser step needed. Temporary perf test removed after measuring.
- [x] **New finding, not foreseen in design.md, superseding the two findings
      above**: user asked to critically assess the `.abs()`/`magnitude_kw`
      patch — correctly identified it as three independent, ad hoc
      sign-conversion sites with no single point of truth. Resolved by
      making `CapacityCurve`/`CapacityCurveStep::power_kw` genuinely SIGNED
      (the internal `Asset`-trait convention), converting to the unsigned
      magnitude OpenADR wants only at the actual reporting boundary
      (`report_intervals.rs::build_capacity_forecast_intervals`). Checking
      `CapacityForecastChart.tsx` directly (not assumed) showed it needs no
      changes — its formatters and energy calc already handle negative
      values. This also surfaced a real, deliberate behavior refinement:
      `base_load_capacity_events` is now direction-independent (always
      `+actual_power_kw`), and `merge_events`'s Export clamp lost its
      artificial floor at `0.0` — a sustained Export commitment can now
      correctly report a positive (net-importing) result when base load's
      draw exceeds what's exportable, instead of the old unsigned-magnitude
      code's silent floor. `magnitude_kw` itself is retained (not removed)
      for `compute_site_headroom_forecast`'s use, since
      `SiteFlexibilityForecastSlot.up_kw`/`down_kw` are a genuinely
      different (two-separate-non-negative-fields) shape, unaffected by this
      change. See the journal entry for the full derivation.
- [x] **Independent review pass (requested before continuing to Section 6),
      found a real, reachable gap**: the "only base load can push a
      sustained-Export total positive" claim in `merge_events`'/
      `base_load_capacity_events`' doc comments was false —
      `ShiftableLoadAsset::max_effort_schedule`'s Export branch reports its
      own positive draw once running (it has no way to actually export,
      unlike every other asset kind's `capability()`-backed
      `max_effort_setpoint`), and `merge_events`' Export clamp had no upper
      bound at all, meaning an unrealistically large forced draw could
      report a value exceeding the site's real grid import hardware limit.
      Fixed: `merge_events` now takes both `import_limit_kw` and
      `export_limit_kw`, clamping Export to `[-export_limit_kw,
      import_limit_kw]` (symmetric with Import's own ceiling). Also fixed a
      related issue the same review surfaced in `report_intervals.rs`: a
      bare `.abs()` would have reported a legitimately-positive
      (net-importing) Export-curve value as if it were genuine discharge
      capability — now floored per-direction instead. Two new tests pin
      both fixes (`merge_export_ceiling_at_the_import_limit_when_non_exportable_contributions_overwhelm`,
      `shiftable_load_forced_to_run_under_export_reports_its_own_positive_draw`).

## 5. `controller/capacity_envelope.rs` — Site Headroom half

- [x] 5.1 Test: `a_fully_charged_battery_reports_zero_absolute_import_headroom`.
- [x] 5.2 Test: `shiftable_load_contributes_via_the_same_primitive_as_other_assets`.
- [x] 5.3 Structural: `compute_site_headroom_forecast` calls
      `simulated_trajectory` exactly once per asset inside the outer
      `sim.iter_assets()` loop, then iterates that single trajectory's
      points for every slot — confirmed by code inspection (the call site
      is outside the per-slot loop, not inside it) rather than a
      call-counting test double, which would need test-only instrumentation
      this codebase doesn't otherwise use for this kind of invariant.
- [x] 5.4 Implemented. Base load excluded entirely (matching
      `build_forecast_frames`'s own existing exclusion — zero flexibility,
      not the constant-offset treatment §4 needed for the *capacity*
      curve's different "net grid power" semantics).
- [x] 5.5 Updated `SiteFlexibilityForecastSlot`'s doc comment (D5).
- [x] **New finding, before wiring into production**: `simulated_trajectory`
      alone doesn't exclude a plugged-in EV past its live session's
      `departure_time` (`EvState::plugged` is never toggled by `step()`,
      matching `build_forecast_frames`'s own documented reason for its
      `include_at` closure) — without this, EV would keep contributing
      headroom at slots after it's actually departed.
      `compute_site_headroom_forecast` gained an `ev_session` parameter and
      the same exclusion; pinned by a new test
      (`ev_headroom_zeroes_out_past_the_live_sessions_departure`). Confirmed
      `compute_site_capacity_curve` does NOT need the equivalent — its
      sustained-commitment model never projected a future departure either,
      matching the deleted `capacity_forecast.rs`'s own unchanged scope
      (`EvCharger::capability_inner` already zeroes correctly for the LIVE
      `plugged` state, which is all that model ever used).

## 6. Wiring and deletion

- [x] 6.1 `tasks/sim_tick/forecast_wiring.rs::compute_tick_forecasts` now
      calls `capacity_envelope::compute_site_capacity_curve`/
      `compute_site_headroom_forecast`. `t2_max` for the capacity sweep is
      the plan's own remaining horizon (`plan.horizon.end_time - now`,
      falling back to 48h with no active plan) — matches `pv_frames`' own
      range (design.md D1's correction). Dropped the now-unused
      `shiftable_loads: &[ShiftableLoad]` parameter (shiftable loads are
      read directly via `sim.iter_assets()` now) — traced through and
      removed the now-dead `TickContext.shiftable_loads` field
      (`tasks/sim_tick/context.rs`) and its one populate site too, since
      nothing else read it.
- [x] 6.2 Deleted `controller/capacity_forecast.rs` and
      `controller/envelope_forecast.rs` (and their `pub mod` declarations).
      Reviewed every test in both — all of `envelope_forecast.rs`'s tests
      (PV up/down margin-to-ceiling, shiftable load plan-relative slack) test
      the relative-delta semantics the master plan explicitly says are
      overturned as incorrect (its own open point #2 resolution), so porting
      them would mean asserting behavior this change deliberately replaces;
      not ported, not silently dropped either — reviewed and recorded here.
      `capacity_forecast.rs`'s worked-numeric-example tests were already
      re-verified through the new engine in §4.
- [x] 6.3 Updated route doc comments (`get_flexibility_forecast` /
      `get_capacity_curves`) plus every other stale `capacity_forecast`/
      `envelope_forecast` doc-comment reference found across the codebase
      (`reporter.rs`, `assets/shiftable_load.rs`, `simulator/forecast.rs`,
      `tasks/sim_tick/publish.rs`) — none were real imports, all were doc
      mentions, confirmed via a full-codebase grep before deleting.
- [x] Full suite re-verified post-wiring: 1264/1264 Rust tests (was 1295;
      -33 deleted old-module tests +2 new ones = -31, confirmed exact, no
      unexpected loss), `cargo fmt`/`clippy -D warnings`, file-size audit,
      and `ven-architecture` invariant greps all clean. `cargo check`
      compiled clean on the first try after wiring + deletion — no dangling
      references.

## 7. UI: `SiteHeadroomChart` rework (D6)

- [x] 7.1 Confirmed with user: band between absolute limits (`-up_kw` to
      `down_kw`), alongside the existing grid-power line — same visual
      language as today's band, now anchored to absolute limits instead of
      the live grid-power line.
- [x] 7.2 Implemented: band's `lower`/`upper` accessors changed from
      `gridPowerKw ∓ up/down_kw` to `-up_kw`/`down_kw` directly — no longer
      depends on `gridPowerKw` being present on the same row at all. Grid
      power line itself unchanged.
- [x] 7.3 `SiteHeadroomChart.test.tsx`: 3/4 tests passed unchanged (their
      assertions only checked presence, not the old formula's specific
      values); one test's hardcoded expected value
      (`gridPowerKw - up_kw = 2.0-3.0`) updated to the new formula's
      `-up_kw = -3.0`, plus a stale docstring on another test describing the
      now-obsolete "needs gridPowerKw on the same row" premise. All 4 pass.
      `GridHeadroomCell.test.tsx`: 7/7 pass unchanged (renders the live
      instant `SiteFlexibilityEnvelope`, a separate, unaffected type).
- [x] 7.4 Confirmed: `CapacityForecastChart.test.tsx` (3/3) passes unchanged
      — its own mock data is independent of the backend, and its rendering
      (two positive-scale lines by label/color) needs no change for a
      signed `CapacityCurve`, as already established when designing the
      sign-convention fix. Full VEN UI suite: 627/627. ESLint: 0 errors on
      changed files.

## 8. BDD coverage (workflow rule 4)

- [ ] 8.1 Add or extend a scenario in `tests/features/` exercising the new
      absolute-quantity Site Headroom behavior end-to-end (e.g. drive a
      battery toward full, confirm the reported down_kw drops toward zero
      via the live API, not just a unit test on the engine).
- [ ] 8.2 Add or extend a scenario confirming the Capacity Forecast's
      PV-Import-is-zero / Heater-Export-is-zero fixes are visible via the
      live `GET /flexibility/capacity` endpoint.

## 9. Cross-cutting verification

- [ ] 9.1 `scripts/audit_file_sizes.py` — confirm `capacity_envelope.rs`
      stays under budget; split into sibling files if not (e.g. separate
      capacity-forecast-half and headroom-half modules from the start if it
      looks close).
- [ ] 9.2 Architecture invariants (`ven-architecture` in `.claude/CLAUDE.md`)
      — confirm no new cross-ring imports.
- [ ] 9.3 `cargo fmt --check` / `cargo clippy --all-targets --all-features -- -D warnings`.
- [ ] 9.4 Full Rust unit suite green.
- [ ] 9.5 UI unit suites green (VEN UI's updated chart tests included).
- [ ] 9.6 E2E + resilience suites on Node2 — this change has real
      user-visible behavior change (unlike Specs C/D), so E2E must actually
      exercise the new numbers, not just pass unchanged.

## 10. Documentation

- [ ] 10.1 `docs/history/project_journal.md` — narrative entry covering the
      engine design, the PV special-casing decision (D1), the performance
      measurement (task 4.5's result), and the UI rendering decision (task
      7.1's resolution).
- [ ] 10.2 `docs/reference/KEY_LEARNINGS.md` — durable lesson candidate, if
      one emerges (e.g. anything about the dense-compute/sparse-output
      pattern, or the PV-special-casing precedent, worth generalizing).
- [ ] 10.3 `docs/architecture/VEN_ARCHITECTURE.md` — replace the
      `capacity_forecast.rs`/`envelope_forecast.rs` write-up with the new
      unified engine's; fold §3.0a/§3.0b's "not yet called from production
      code" caveats into "now wired in via `capacity_envelope.rs`."
- [ ] 10.4 `docs/use-cases/*.md` — this change has real user-observable
      behavior (Site Headroom's numbers change meaning, Capacity Forecast's
      PV/Heater bugs are fixed) — update or add the relevant use-case
      document per `workflow` rule 4, pointing to the new BDD scenario(s)
      rather than restating their Given/When/Then prose.
- [ ] 10.5 `docs/plans/asset-max-power-forecast-master-plan.md` — mark Spec E
      complete. This closes the master plan: fold any durable lessons into
      `KEY_LEARNINGS.md` and delete the plan file itself, per the
      `workflow` no-lingering-plans rule (the plan document's own final
      instruction).
- [ ] 10.6 Delete this change directory once the above is done and all tests
      are green (do not archive).
