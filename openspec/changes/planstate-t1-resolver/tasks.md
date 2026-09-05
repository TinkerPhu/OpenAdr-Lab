# Tasks: `planState(t1)` Resolver

## 1. Survey (confirm design.md's claims against current code)

- [x] 1.1 Re-confirmed unchanged.
- [x] 1.2 Re-confirmed: `PvState { actual_power_kw, generation_limit_kw,
      curtailment_source }`; no code anywhere forecasts `curtailment_source`.
- [x] 1.3 Re-confirmed R-69 still open (0/8 tasks done in
      `openspec/changes/battery-efficiency-model-reconciliation/tasks.md`).

## 2. Shared trajectory helper (D1)

- [x] 2.1 Extracted `simulated_trajectory(entry, cfg, future_slots) ->
      Trajectory`. Used the existing `battery_capability_evolves_across_slots_not_flat_copied`
      and other `build_forecast_frames` tests as the regression net (all
      still passing byte-for-byte) rather than writing a redundant duplicate
      test — they already assert exactly this behavior.
- [x] 2.2 `insert_simulated_points` now calls the shared helper.

## 3. `resolve_plan_state_at` (D2/D3)

- [x] 3.1 Test: `t1_at_or_before_now_returns_live_state_unchanged`.
- [x] 3.2 Test: `battery_state_at_a_future_slot_matches_direct_simulate_forward`.
- [x] 3.3 Test: `base_load_is_included_even_though_build_forecast_frames_skips_it`.
- [x] 3.4 Test: `pv_state_at_a_future_t1_equals_its_current_live_state`.
- [x] 3.5 Test: `t1_past_the_last_slot_returns_the_last_available_state`.
- [x] 3.6 Implemented in `VEN/src/simulator/forecast.rs`.

## 4. R-69 visibility check (design.md's Risks section)

- [x] 4.1 Test: `r69_partial_cycle_soc_disagrees_with_planned_state_by_asset_until_r69_lands`
      — confirmed the disagreement is currently real (resolver 0.405 vs.
      planner-believed 0.45 SoC for the worked 5 kWh/0.81-rte example); the
      test fails loudly (by design) once R-69 is resolved, prompting an
      update rather than silently passing either way.

## 5. Cross-cutting verification

- [x] 5.1 File-size audit PASSED.
- [x] 5.2 Architecture invariant greps clean.
- [x] 5.3 `cargo fmt`/`clippy -D warnings` clean.
- [x] 5.4 Full Rust unit suite green (1277/1277, up from 1271).
- [x] 5.5 UI unit suites green — no UI code touched.
- [ ] 5.6 E2E + resilience suites on Node2.

## 6. Documentation

- [ ] 6.1 `docs/history/project_journal.md` — narrative entry: the D1 shared-
      helper design, the D2 PV honest-scope decision, the R-69 visibility
      check's actual result (pass or documented-fail), and why this change
      deliberately does not touch `capacity_forecast.rs`/`envelope_forecast.rs`
      itself.
- [ ] 6.2 `docs/reference/KEY_LEARNINGS.md` — durable lesson candidate, if one
      emerges during implementation (e.g. if the R-69 check surfaces
      something not already captured by that debt entry).
- [ ] 6.3 `docs/architecture/VEN_ARCHITECTURE.md` — document
      `resolve_plan_state_at` next to `build_forecast_frames`'s existing
      writeup; explicitly note PV's honest scope limit so a future reader
      doesn't assume more precision exists than the model supports.
- [ ] 6.4 `docs/plans/asset-max-power-forecast-master-plan.md` — mark Spec D
      complete; note explicitly that Spec E still owns wiring this resolver
      (and Spec C's `asset_max_power`) into the unified engine.
- [ ] 6.5 No `docs/use-cases/*.md` update expected (no user-observable
      behavior changes yet) — confirm and record that conclusion rather than
      silently skipping, matching Spec B/C's precedent.
- [ ] 6.6 Delete this change directory once the above is done and all tests
      are green (do not archive).
