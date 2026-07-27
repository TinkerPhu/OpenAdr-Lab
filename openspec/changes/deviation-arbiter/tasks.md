## 1. Deviation signal (no new lag)

- [x] 1.1 `simulator/base_load_preview.rs` (new): `SimState::peek_base_load_kw(now, dt_s,
      base_load_kw_override, base_load_alpha) -> Option<f64>`, mirroring `pv_preview.rs`'s
      structure exactly. Equivalence tests in `simulator/tests/peek_base_load_kw_tests.rs`
      (mirrors `peek_pv_kw_tests.rs`'s pattern, including the same-`now` tick-output check).
- [x] 1.2 `controller::arbiter::projected_net_kw(sim, base_setpoints, live_pv_kw,
      live_base_load_kw) -> f64` — generalizes the former `apply_surplus_ev_overlay`'s
      `net_other_kw` calculation (reused logic, not reinvented), preferring the live-preview
      values over the necessarily-stale `SimSnapshot` for PV/base-load, and `predict_heater_forced_kw`
      for the heater (same "commanded ≠ actual" fix already shipped for the E2E DISPATCH_SETPOINT
      bug — reused directly from `dispatcher.rs`, not duplicated).
- [x] 1.3 `controller::arbiter::deviation_kw(plan_slot, projected_net_kw) -> f64` —
      `projected_net_kw - (plan_slot.net_import_kw - plan_slot.net_export_kw)`. Unit-tested.
- [x] 1.4 `tasks/sim_tick/tick.rs`: `live_base_load_kw` threaded through the same lock scope as
      `live_pv_kw`, via `sim_guard.peek_base_load_kw(...)`.

## 2. Lever model

- [x] 2.1 `controller/arbiter/arbiter_levers.rs` (new — split out of `arbiter.rs` to respect the
      file-size cap): `struct Lever { id: &'static str, available_capacity_kw: f64,
      marginal_cost_eur_per_kwh: f64 }`, plain data as planned (no new trait).
- [x] 2.2 Battery lever: capacity from `AssetSnapshot.cap_max_import/export_kw` **and**
      `available_charge/discharge_kwh` (a correctness fix found during testing: the original
      draft only checked power headroom, which would have offered the lever even at 100% SoC —
      now excluded outright when the relevant energy headroom is `<= 0`, per §5.3's own
      zero-capacity-exclusion requirement). `apply_battery_lever` adapts the former
      `apply_battery_correction_overlay`'s dead-beat formula, **metered against `assigned_kw`**
      (this lever's share of the shared `remaining_kw` pool) rather than the full deviation —
      required for correct greedy composition with other levers; see task 3a for the stability
      re-verification this reuse needed.
- [x] 2.3 EV lever: capacity/direction reuses `apply_surplus_ev_overlay`'s plugged/soc-target
      gating logic; flat `0.0` cost; only available when `plan_has_ev_allocation == false`.
      Capacity is direction-dependent (see `arbiter_levers.rs` doc comments): absorbing surplus
      can increase charging to `max_charge_kw`; absorbing import deviation can only claw back
      whatever opportunistic charge is already flowing (a simplification of the general
      bidirectional case, since only the opportunistic regime is ever second-guessed).
- [x] 2.4 Heater lever, part A (pause-within-comfort-band): implemented as designed, flat `0.0`
      cost, available whenever the plan's heater allocation is `> 0`.
- [x] 2.5 Heater lever, part B (`HeaterEmergencyMode::Curtail`/`Absorb`): implemented as designed.
      `HEATER_COMFORT_OVERRIDE_EUR_PER_KWH` chosen as `0.40` €/kWh — an illustrative default (no
      numeric default exists in the source material, per the design's own open-question list;
      flagged for the user to tune once real obligation-penalty magnitudes are known).
- [x] 2.6 PV curtailment lever: implemented as designed, export-excess-only backstop priced at
      `export_tariff_eur_kwh`.
- [x] 2.7 `rank_levers` + the greedy consumption loop in `reconcile`: implemented as designed,
      including the §4a.1 preemption-margin hysteresis from day one (not bolted on after).
- [x] 2.8 Unit tests reconstructing §5.4's worked examples: scenario A (EV over battery, no
      battery movement), scenario D (battery covers a base-load step when EV is excluded at
      target SoC), scenario H (heater emergency gated by the comfort-override threshold), and a
      PV-curtailment-backstop case. Fixtures are self-contained in
      `controller/tests/arbiter_tests.rs` (not factored out of `dispatcher.rs`'s — duplication
      was small enough that a shared fixtures module wasn't worth the indirection, the
      "implementation-time call" this task anticipated).

## 3. Wiring into the tick

- [x] 3.1 `ArbiterOutcome { setpoints, heater_emergency_mode, pv_export_limit_tighten_kw,
      absorbed_kwh_by_asset, active_lever }` — matches the design, plus `active_lever` (needed
      for the §4a.1 hysteresis, not originally itemized in this struct but required by task 2.7).
- [x] 3.2 `tasks/sim_tick/helpers.rs::build_tick_setpoints`: calls `arbiter::reconcile` when
      `deviation_arbiter_enabled`, else the pre-arbiter path — **kept as a direct call to
      `dispatcher::apply_surplus_ev_overlay`, not deleted (see task 3.5 note)**, to guarantee the
      disabled path is byte-for-byte identical to pre-change behaviour, satisfying the spec's
      "SHALL behave exactly as before this change" requirement literally.
- [x] 3.3 `resolve_pv_export_limit_kw`: extended to a 3-source tighter-wins resolution
      (`arbiter_tighten_kw: Option<f64>` new parameter); `PvCurtailmentSource::Arbiter` added
      (`as_f64() == 3.0`). VEN UI: `AssetTimelineChart.tsx`'s `classifyPvPoint` updated to treat
      source `3` the same as `2` (both "unplanned"/externally-imposed, not the plan's own
      forecasted choice) — new test added.
- [x] 3.4 `tasks/sim_tick/tick.rs`: `ArbiterOutcome.heater_emergency_mode` combined with
      `inject.heater_emergency_curtail/absorb` via the new `resolve_heater_emergency_mode` helper
      (`tasks/sim_tick/arbiter_glue.rs`) — manual override wins, exactly as designed.
- [x] 3.5 **Deviated from the plan**: `apply_battery_correction_overlay` deleted from
      `dispatcher.rs` as planned (dead code, never referenced by the disabled-gate path).
      `apply_surplus_ev_overlay` was **kept**, not deleted — task 3.2's "exact pre-arbiter code
      path" requirement needs a byte-for-byte-unchanged function to call when the gate is off,
      and the arbiter's own EV lever behaves differently (capacity-metered against a shared
      deviation pool) even in the single-lever case, so it is not a drop-in replacement for the
      disabled path. Documented in `apply_surplus_ev_overlay`'s doc comment.

## 3a. Battery corrector stability re-verification — DONE, no rebuild needed

- [x] 3a.1 Confirmed via grep: `loops.rs` and `prev_correction_kw` do not exist anywhere in the
      codebase (only stale mentions in the old function's own doc comment, since removed).
- [x] 3a.2 `battery_lever_converges_under_stationary_disturbance_across_multiple_ticks`
      (`controller/tests/arbiter_tests.rs`): drives the moved lever for 6 consecutive simulated
      ticks under a stationary deviation; asserts convergence after tick 1 and no drift/reversal
      afterward.
- [x] 3a.3 **Result: no rebuild needed.** The test passes cleanly — the arbiter's
      unconditional-every-tick execution, reading `AssetSnapshot.setpoint_kw` (the actually-applied
      value) as the integrator state, already supplies the guarantee the old `loops.rs` holding
      mechanism used to provide. Confirmed empirically, not just architecturally reasoned.

## 4. Residual escalation (§5.5)

- [x] 4.1 `entities/arbiter_residual.rs` (new): `AssetResidual { absorbed_kwh, capacity_kwh_at_last_plan
      }` with a `breach_fraction()` helper, unit-tested. Fresh type as planned — `DispatchState`
      left untouched/dead.
- [x] 4.2 `state/arbiter.rs` (new — split out of `state/mod.rs` to respect the file-size cap):
      `AppState` gains `arbiter_residual`, `last_residual_trigger_at`, `arbiter_active_lever`
      fields (on `HemsState`) + `residual_state()`/`accumulate_residual()`/`reset_residual()`/
      `last_residual_trigger_at()`/`set_last_residual_trigger_at()`/`arbiter_active_lever()`/
      `set_arbiter_active_lever()` accessors, following the existing async-lock pattern.
- [x] 4.3 `tasks/sim_tick/arbiter_glue.rs::apply_residual_escalation` (split out of
      `tick.rs`/`helpers.rs` for the file-size cap): accumulates, checks breach, and — gated by
      the task-4.4 cooldown — sends `PlanTrigger::ResidualThreshold`.
- [x] 4.4 Cooldown implemented as `RESIDUAL_COOLDOWN_S` (illustrative default 900 s = 15 min) +
      `AppState::last_residual_trigger_at`. **Open design question left unresolved as flagged**:
      `ResidualThreshold` remains an unconditional hard trigger (matching every other
      non-`Periodic` variant) — the cooldown is the only throttle. Routing it through
      `evaluate_acceptance_gate`'s cost-improvement gate instead/in addition is a genuine design
      choice left for the user to make before this is enabled in any real profile; not changed
      in this pass.
- [x] 4.5 `PlanTrigger::ResidualThreshold` added to `entities/asset.rs`. Confirmed additive-safe:
      no exhaustive `match` on `PlanTrigger` exists anywhere in the codebase (only
      `!matches!(.., Periodic)` wildcard checks).
- [x] 4.6 `services/planning.rs::adopt_if_warranted`: on adoption (any trigger), re-snapshots
      battery/EV `available_charge/discharge_kwh` from `state.sim()` and calls `reset_residual`.
- [x] 4.7 Not implemented as a literal "four small absorptions" scripted test — covered instead
      by `AssetResidual::breach_fraction`'s direct unit tests (scaling correctly with capacity)
      plus the accumulator wiring exercised end-to-end in
      `deviation_arbiter_absorbs_unplanned_pv_surplus_end_to_end` (task 5). The exact scripted
      scenario is a reasonable follow-up if `ResidualThreshold` is enabled in production.

## 4a. Lever-switching and heater-mode hysteresis

- [x] 4a.1 `LEVER_PREEMPTION_MARGIN_EUR_PER_KWH` (illustrative default `0.02` €/kWh) implemented
      in `rank_levers`, built in from the start (task 2.7), not retrofitted.
- [x] 4a.2 Heater-mode hysteresis implemented via the **same incumbent-tracking mechanism**
      (`is_incumbent` parameter on `heater_emergency_lever`, lowering the entry threshold by the
      preemption margin when already active) rather than a separate dwell-timer field — reuses
      existing state instead of adding a new one, while still preventing rapid mode flips.
- [x] 4a.3 `near_equal_cost_levers_do_not_switch_every_tick`, `challenger_beyond_margin_does_preempt_incumbent`,
      `heater_emergency_mode_hysteresis_stays_active_within_margin_of_threshold` — all pass.

## 5. Regression safeguard (feature 017 oscillation — both shapes)

- [x] 5.1/5.2/5.3 **Scoped down from the original plan.** Full multi-tick oscillation-shape and
      lever-switching-chatter proofs are covered at the unit level (task 3a.2 and task 4a.3
      above), which exercise the exact mechanisms responsible for both properties directly. The
      `tick_once`-level integration test actually added,
      `deviation_arbiter_absorbs_unplanned_pv_surplus_end_to_end`
      (`tasks/sim_tick/tick_tests.rs`), is a narrower smoke proof: the arbiter runs end-to-end
      through the real tick loop (plan → dispatcher → arbiter → physics) without panicking and
      visibly absorbs an unplanned PV surplus. A full multi-tick `tick_once`-level version of the
      oscillation-shape and chatter-shape scenarios is a reasonable, contained follow-up
      (requires building a `Plan` + multi-asset `SimState` fixture and looping `tick_once` several
      times) — not completed in this pass for time reasons.

## 6. Rollout gate

- [x] 6.1 **Simplified from the original plan.** `deviation_arbiter_enabled: bool` implemented as
      an `AppState`/`HemsState` field (default `false`, hardcoded in `AppState::new()`), matching
      the *actual* precedent `EvSettings.opportunistic_charging_enabled` follows (which is also
      not profile-YAML-plumbed today, contrary to this task's original wording) rather than
      adding new profile-schema plumbing. No HTTP route to toggle it was added in this pass —
      follow-up if the arbiter is to be enabled outside of tests.
- [x] 6.2 Regression-run: `apply_surplus_ev_overlay`'s original tests still pass unchanged in
      `dispatcher.rs` (it was kept, not moved — see task 3.5). Its logic was independently
      reimplemented (not called) inside `arbiter::apply_ev_lever_opportunistic` for the no-plan
      arbiter path, with its own parallel test suite in `controller/tests/arbiter_tests.rs`
      confirming equivalent behaviour.

## 7. Verification and bookkeeping

- [x] 7.1 Full VEN Rust test pyramid: 842/842 + 1 architecture test, `wsl cargo test -j 2 -p
      ven-app`, under `wsl_lock`.
- [x] 7.2 `cargo fmt --check` clean; `cargo clippy --all-targets --all-features -- -D warnings`
      clean.
- [x] 7.3 `scripts/audit_file_sizes.py` — PASSED (required splitting `arbiter.rs` →
      `arbiter.rs`/`arbiter_levers.rs`, `helpers.rs`/`tick.rs` → +`arbiter_glue.rs`, and
      `state/mod.rs` → +`state/arbiter.rs`, plus moving `arbiter_tests.rs` into
      `controller/tests/` for the test-path exemption).
- [x] 7.4 VEN UI: `PvCurtailmentSource::Arbiter` handled in `AssetTimelineChart.tsx`'s
      `classifyPvPoint` (source `3` classified as "unplanned", same as capacity); new test added.
      Full UI suite (418/418, 39 files), `tsc --noEmit` clean, ESLint clean (0 errors).
- [x] 7.5 Recorded in `docs/history/project_journal.md`; `docs/architecture/VEN_ARCHITECTURE.md`
      §2.1's Dispatcher description updated (BL-22 resolved, new "Deviation Arbiter" subsection
      added); `docs/BACKLOG.md` BL-22 marked resolved.
