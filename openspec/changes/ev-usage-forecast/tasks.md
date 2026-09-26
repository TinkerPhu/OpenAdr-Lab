# Tasks

## 1. Baseline safety net

- [x] 1.1 Before any other change, run and record green the existing `EvSession`-deadline pinned
      tests (`controller/milp_planner/tests/basic.rs::ev_mask_plugged_with_session_deadline`,
      `ev_mask_unplugged_all_false`, `ev_mode_must_run_for_firm_deadline_session`,
      `ev_mode_may_run_for_soft_deadline_session`) and the full VEN Rust suite, establishing the
      exact baseline this change must not disturb.

## 2. Profile schema and params (test-first)

- [x] 2.1 Write a test that a profile YAML declaring both `usage_sim` and `usage_forecast` on the
      same EV fails validation, naming both sections. Confirm it fails (validation doesn't exist
      yet), then add `EvUsageForecastConfig`/`EvUsageForecastDayConfig` to
      `VEN/src/profile/schema.rs` (mirroring `EvUsageSimConfig`/`EvUsageDayConfig` field-for-field
      per design.md Decision 4 — resolve there whether to share the underlying params type via a
      mode enum or define a separate type), `EvConfig.usage_forecast: Option<...>`, default
      functions in `profile/defaults.rs`, and the mutual-exclusion check in `profile/validate.rs`
      until the test passes.
- [x] 2.2 Mirror into `EvUsageForecastParams` in `VEN/src/entities/asset_params.rs` and wire
      (resolved per design.md Decision 4: no parallel type — `EvUsageMode { Simulated, Forecast }`
      rides on the existing `EvUsageSimParams`, so `EvCharger.usage_sim` and `from_params` needed no
      structural change and no profile using only `usage_sim` is affected)
      `EvConfig`'s conversion (`AssetProfile::Ev` arm in `schema.rs`) and `EvCharger::from_params`
      (`VEN/src/assets/ev.rs`) to carry it; verify `cargo check -p ven-app` passes and an existing
      profile using only `usage_sim` is byte-for-byte unaffected.

## 3. Per-slot availability via the existing `a_ev` mask (test-first)

Revised per design.md Decision 2 (verified in code): `a_ev: Vec<bool>` is already a per-slot mask,
already consumed at `ev_milp.rs:98` and `:41`. No new primitive, field, or trait method — this task
populates an existing array truthfully instead of uniformly.

- [x] 3.1 Write unit tests for a helper (in `assets/ev_schedule.rs`, beside the other prediction
      functions) that, given the usage schedule config, a seed tag, `now`, and per-slot offsets
      (`cum_s`), returns `Vec<bool>` per-slot availability: `false` inside any predicted trip's
      `[leave_at, return_at)`, `true` outside. Cover a single trip in the horizon, no trip at all,
      and multiple trips in one horizon (pin design.md's "multi-trip comes free" claim with a real
      test). Confirm failing, then implement.
- [x] 3.2 Write a test asserting the helper's per-slot result equals calling `is_away_at` directly
      at each slot's timestamp — a direct regression guard against a second copy of the prediction
      logic (one-concept-one-function).
- [x] 3.3 AND the helper's output into `a_ev` in `EvMilpContext::from_state` for every branch that
      builds a mask, so `usage_forecast` availability composes with (never replaces) the existing
      plugged/session/deadline logic. Verify the task-1.1 baseline tests still pass unchanged — with
      `usage_forecast` absent the AND must be a provable no-op.

## 4. SoC drop in the post-solve projected trajectory (test-first)

Revised per design.md Decision 3 (verified in code): the EV MILP has no SoC-balance constraint and
no per-slot SoC variable; the projected SoC curve is built after the solve by `ev_soc_trajectory`
(`controller/milp_planner/results.rs:332`). The drop is applied there.

- [x] 4.1 Consolidate R-73 first, before adding behaviour (project `refactoring` rule): `EvCharger::
      soc_trajectory` (`assets/ev.rs:229`, `#[allow(dead_code)]`, flagged R-73) and
      `ev_soc_trajectory` (`controller/milp_planner/asset_port.rs:274`) are duplicates. Consolidate
      to one function with the existing `soc_trajectory_charges_monotonically` /
      `soc_trajectory_clamps_at_one` tests kept green unchanged; note the differing `dt_h` signature
      (scalar vs slice) that must be reconciled. Remove R-73 from
      `docs/reference/TECHNICAL_DEBTS.md` if this closes it.
- [x] 4.2 Write unit tests for an exogenous per-slot SoC delta input to the surviving trajectory
      function: zero everywhere except the first slot whose start is at-or-after a predicted trip's
      `return_at`, where the projected SoC drops by `soc_drop_pct/100`, floored so it never falls
      below `min_soc_after_drop_pct`. Cover the exact-boundary case, the floor-clamping case, and
      no-trip-ending-in-horizon (all zero). Confirm failing, then implement.
- [x] 4.3 Wire it at the `results.rs:332` call site so a solved plan's projected SoC reflects the
      drop. Write a test solving a small plan across a predicted return instant, asserting the
      projected SoC drops by the expected amount independent of the charging scheduled that slot.

## 5. `engage_charge_planning` without a session (test-first)

- [x] 5.1 Write tests for `usage_forecast` + `engage_charge_planning: true`: the plan targets the
      EV's configured SoC by the next predicted leave instant within the horizon, with no
      `EvSession` created or altered (assert `AppState.ev_session()` stays `None` throughout).
      Confirm failing, then implement by populating `EvMilpContext`'s existing deadline/target
      fields directly from the predicted trip when no real session governs them.
- [x] 5.2 Write a test for `engage_charge_planning: false` under `usage_forecast`: no deadline/
      target is introduced, but the availability bounds from task 3 are still asserted (the plan
      still can't charge during a predicted-away window, it just isn't pushed to hurry beforehand).

## 6. Precedence with a real session (test-first)

- [x] 6.1 Write a test: a real user/VTN `EvSession` is active for an EV that also has
      `usage_forecast` + `engage_charge_planning: true` — the plan targets the real session's SoC
      and deadline, not `usage_forecast`'s. Implement the precedence check inside
      `EvMilpContext::from_state`.
- [x] 6.2 Write a test: a real session's deadline falls within (or after) a slot the EV is
      predicted unavailable for — that slot still reports zero feasible power in the plan (the real
      session cannot make a physically-unavailable slot available). This is a genuinely new
      infeasibility shape per design.md's risk section — verify the existing solver slack absorbs
      it without a hard failure. Finding: there is NO EV slack — `solve_ev_must_run_core_energy_
      beyond_what_the_available_slots_can_deliver` (`tests/solver.rs`) pins that an unreachable core
      equality is genuinely infeasible, which is why task 7.1 clamps rather than relies on slack.

## 7. Infeasible-deadline warning (test-first)

- [x] 7.1 Write a test: the target can't be reached given the predicted available time before the
      deadline — assert the plan still solves (best feasible SoC, no rejection) and a warning fires.
      Implemented as `EvMilpContext::clamp_core_to_reachable_energy`
      (`assets/ev_usage_forecast.rs`): the core energy is clamped to what the unmasked pre-deadline
      slots can deliver and `core_unmet_warning` travels EvScalars -> MilpInputs ->
      `ev_diagnostics::ev_warnings` as an existing `WarningKind::EvCoreEnergyUnmet` plan warning
      (same stable-text/dedup contract as the MAX_COST budget warning). The clamp applies to a real
      session's target too, since masking can strand that one identically.

## 8. UI transparency

- [ ] 8.1 Extend the existing `/ev-usage-sim`-equivalent diagnostics (or add a sibling route if
      `usage_forecast` needs its own, per whether the two config classes can share one response
      shape) so a `usage_forecast`-configured EV's next predicted trip and
      `engage_charge_planning` state are visible in the VEN UI, following the exact pattern
      `ev-usage-simulation` already established (`EvCard.tsx` chips, `api/types.ts`, `api/hooks.ts`)
      — no new UI paradigm.

## 9. BDD scenario and full verification

- [ ] 9.1 Add a BDD scenario demonstrating the actual user-visible outcome this change exists for:
      a profile-configured EV with `usage_forecast` + `engage_charge_planning: true` gets a
      charging plan built before its predicted departure — something `usage_sim` alone cannot show
      for a departure it hasn't reached yet. Reuse the `usage_sim_test`-style dedicated test
      profile/docker-compose-service pattern already established by `ev-usage-simulation`, adapted
      for `usage_forecast`.
- [ ] 9.2 Run the full VEN Rust suite, `cargo fmt --check`, `cargo clippy --all-targets
      --all-features -- -D warnings`, `scripts/audit_file_sizes.py`, and the full E2E suite on
      Node2 (`bash run_all_tests.sh --e2e`) — confirm the task-1.1 baseline tests are still green
      alongside everything new, using `wsl_lock`/`node2-lock` as required.

## 10. Documentation

- [ ] 10.1 Update `docs/architecture/VEN_ARCHITECTURE.md`'s `ev-usage-simulation` note (§EV
      departure handling) with `usage_forecast`'s addition, add a `docs/history/project_journal.md`
      entry, and record the deferred multi-deadline generalization (design.md Non-Goals) in
      `docs/reference/TECHNICAL_DEBTS.md` if it isn't already tracked from the prior change. Once
      implemented and tested, delete `openspec/changes/ev-usage-forecast/` per workflow rule 3 (do
      not archive).
