## 1. Deviation signal (no new lag)

- [ ] 1.1 `simulator/base_load_preview.rs` (new): `SimState::peek_base_load_kw(now, dt_s,
      base_load_kw_override, base_load_alpha) -> Option<f64>`, mirroring `pv_preview.rs`'s
      structure exactly — same override/decay precedence as `tick()`'s `AssetConfig::BaseLoad`
      branch. Add an equivalence test (`peek_base_load_kw_matches_tick_output_for_same_now`),
      mirroring `peek_pv_kw_matches_tick_output_for_same_now`.
- [ ] 1.2 `arbiter.rs`: `fn projected_net_kw(sim: &SimSnapshot, base_setpoints: &HashMap<String,
      f64>, live_pv_kw: Option<f64>, live_base_load_kw: Option<f64>) -> f64` — generalizes
      `apply_surplus_ev_overlay`'s existing `net_other_kw` calculation (reuse its filter/fallback
      logic, don't reinvent it), preferring `live_pv_kw`/`live_base_load_kw` over the
      necessarily-stale `SimSnapshot` for those two inputs, and `base_setpoints` (this tick's
      plan-allocated setpoint) for battery/EV/heater.
- [ ] 1.3 `fn deviation_kw(plan_slot: &PlanTimeSlot, projected_net_kw: f64) -> f64` —
      `projected_net_kw - (plan_slot.net_import_kw - plan_slot.net_export_kw)`. Unit tests: zero
      when projection matches plan; positive sign = importing more than planned.
- [ ] 1.4 `tasks/sim_tick/tick.rs`: thread `live_base_load_kw` through the same lock scope as
      `live_pv_kw`, computed via `sim_guard.peek_base_load_kw(...)` right next to the existing
      `peek_pv_kw` call.

## 2. Lever model

- [ ] 2.1 `arbiter.rs`: `struct Lever { asset_id: &'static str, available_capacity_kw: f64,
      marginal_cost_eur_per_kwh: f64, apply: fn(&mut HashMap<String,f64>, f64) }` (or an enum of
      lever kinds if a trait object proves awkward — decide at implementation time; keep it plain
      data, not a new trait, since no asset-type polymorphism is needed beyond what
      `SimSnapshot`/`AssetSnapshot` already expose).
- [ ] 2.2 Battery lever: capacity from `AssetSnapshot.cap_max_import_kw`/`cap_max_export_kw` and
      `available_charge_kwh`/`available_discharge_kwh` (already precomputed per tick in
      `SimSnapshot` — do not re-derive via `AssetConfig`, that would break the
      `SimulatorPort`/`SimSnapshot` layering the existing dispatcher functions already respect).
      Cost = `plan_slot.marginal_cost_import_eur_per_kwh` (deviation > 0, need to reduce import →
      discharge) or `marginal_cost_export_eur_per_kwh` (deviation < 0 → charge). `apply` reuses
      `apply_battery_correction_overlay`'s dead-beat formula (moved from `dispatcher.rs`, its
      `#[allow(dead_code)]` removed) — see task 3a before treating this as safe to reuse verbatim.
- [ ] 2.3 EV lever: capacity/direction reuse `apply_surplus_ev_overlay`'s exact plugged/soc-target/
      min-charge-rate (BL-12) gating (moved from `dispatcher.rs`, not duplicated). Marginal cost =
      flat `0.0`, only available when `plan_has_ev_allocation == false` (opportunistic regime) —
      the plan's own EV allocation is never second-guessed, matching the existing rule that this
      overlay never fires when a plan-level EV allocation exists.
- [ ] 2.4 Heater lever, part A (pause-within-comfort-band): available whenever the heater's
      plan-allocated setpoint > 0 this slot; marginal cost = flat `0.0` (§5.4 scenario D: "not
      because a static rule ranked it third but because its marginal cost is genuinely zero").
      `apply` sets the heater setpoint toward 0 within `available_capacity_kw` — an ordinary
      setpoints-map write, no `HeaterEmergencyMode` change.
- [ ] 2.5 Heater lever, part B (`HeaterEmergencyMode::Curtail`/`Absorb`): a new profile-
      configurable `heater_comfort_override_eur_per_kwh` threshold (§5.4 scenario H — the
      obligation breach penalty must exceed ordinary comfort value before invading the safety
      envelope). Only offered as a lever when the relevant directional marginal cost exceeds this
      threshold; capacity = whatever headroom `flexibility_floor()`/`capability()` indicate before
      `temp_safety_max_c`/ambient. `apply` does **not** write to the setpoints map — it produces a
      separate `HeaterEmergencyModeDecision` field on `ArbiterOutcome` (task 3.1), since
      `HeaterEmergencyMode` is applied via `Heater::apply_tick_overrides` at the `SimState::tick()`
      boundary, not via the setpoints map.
- [ ] 2.6 PV curtailment lever: only offered in the export-excess direction (deviation < 0).
      Capacity = `plan_slot.pv_used_kw` (everything currently exported can in principle be
      curtailed further). Cost = `plan_slot.export_tariff_eur_kwh` (forgone revenue) — naturally
      ranks it after every other lever, the backstop for an export-cap breach with battery/EV/
      heater already exhausted. `apply` does not write to the setpoints map — it produces a
      `pv_export_limit_tighten_kw` field on `ArbiterOutcome` (task 3.1).
- [ ] 2.7 `fn rank_and_apply(levers, deviation_kw, dt_s) -> ArbiterOutcome` — the greedy loop:
      filter zero-or-below-capacity levers out entirely (not merely deprioritize — §5.3's explicit
      requirement), sort by `marginal_cost_eur_per_kwh` ascending, consume `remaining_kw` lever by
      lever, record `absorbed_kwh_by_asset` for task 4's accumulator, stop early once
      `remaining_kw` is within the dead-band. **Do not apply this ranking bare — task 4a's
      preemption margin must gate lever switches before this is wired into production.**
- [ ] 2.8 Unit tests reconstructing §5.4's four worked examples verbatim as table tests: scenario A
      (EV picked over battery, no battery movement), scenario D (battery covers base-load step,
      heater pause used opportunistically when mid-cycle and available), scenario H (heater
      `Curtail` invoked only once the obligation-penalty-inflated marginal cost crosses
      `heater_comfort_override_eur_per_kwh`, not below it), and a PV-curtailment-as-backstop case
      (battery+EV+heater all at capacity, export cap threatened → PV curtailed). Reuse the existing
      `battery_entry`/`ev_entry`/`pv_entry`/`heater_entry`/`base_entry` test fixtures from
      `dispatcher.rs`'s test module (factor into a shared `#[cfg(test)]` fixtures module if
      duplication becomes unwieldy — implementation-time call).

## 3. Wiring into the tick

- [ ] 3.1 `struct ArbiterOutcome { setpoints: HashMap<String, f64>, heater_emergency_mode:
      Option<(bool /* curtail */, bool /* absorb */)>, pv_export_limit_tighten_kw: Option<f64>,
      absorbed_kwh_by_asset: HashMap<String, f64> }`.
- [ ] 3.2 `tasks/sim_tick/helpers.rs::build_tick_setpoints`: after `dispatcher::build_setpoints`
      and before `apply_dispatch_override`, call `arbiter::reconcile(...)` when
      `deviation_arbiter_enabled` is true; otherwise fall back to today's exact call to
      `apply_surplus_ev_overlay` inline (rollout gate, task 6).
- [ ] 3.3 `resolve_pv_export_limit_kw` (`dispatcher.rs`): extend to accept an optional third,
      arbiter-sourced tightening value and fold it into the existing tighter-wins comparison
      (generalizes the existing two-source match to three sources); add
      `PvCurtailmentSource::Arbiter` to `entities/asset_params.rs` (update its `as_f64` encoding
      and the mirrored VEN UI TS enum/history feature — check `pv-curtailment-history` for
      exhaustive matches on this enum first).
- [ ] 3.4 `tasks/sim_tick/tick.rs`: combine `ArbiterOutcome.heater_emergency_mode` with
      `inject.heater_emergency_curtail`/`heater_emergency_absorb` before the `sim_guard.tick(...)`
      call — manual sim-inject (testing/demo) wins over the arbiter, mirroring the existing
      "manual override wins while decaying" precedent for PV smoothing.
- [ ] 3.5 `dispatcher.rs`: delete `apply_surplus_ev_overlay` and
      `apply_battery_correction_overlay` (moved into `arbiter.rs` in task 2, not duplicated);
      `build_setpoints` no longer calls either.

## 3a. Battery corrector stability re-verification (must land before 2.2/3.5 are considered done)

- [ ] 3a.1 Confirm (already done during design review, re-verify at implementation time) that
      `loops.rs`/`prev_correction_kw` — the "holding" mechanism `apply_battery_correction_overlay`'s
      own doc comment says its caller must provide — no longer exists anywhere in the codebase.
- [ ] 3a.2 Write a multi-tick convergence test (not a single-call assertion): drive the moved
      battery lever for several consecutive simulated ticks under a *stationary* disturbance (e.g.
      a constant unplanned base-load step) and assert the applied setpoint converges and stays
      converged — no ringing, no sign reversal once converged.
- [ ] 3a.3 If 3a.2 rings: determine why the missing holding mechanism mattered under the old
      architecture and whether the new arbiter's unconditional-every-tick execution (using
      `AssetSnapshot.setpoint_kw`, the actually-applied value, as the dead-beat's integrator state)
      already supplies an equivalent guarantee, or whether an explicit holding/latch needs to be
      rebuilt. Do not ship 2.2/3.5 until this test passes.

## 4. Residual escalation (§5.5)

- [ ] 4.1 New `entities/arbiter_residual.rs` (fresh type, not a repurposing of the dead
      `DispatchState` — its shape is whole-site scalar, not per-asset, and doesn't fit):
      `HashMap<asset_id, { absorbed_kwh: f64, capacity_kwh_at_last_plan: f64 }>` for battery + EV
      only.
- [ ] 4.2 `state/mod.rs`: `AppState` gains the residual-state field + `residual_state()`/
      `accumulate_residual(asset_id, kwh)`/`reset_residual(new_capacities)` accessors, following
      the exact async-lock pattern of `active_plan()`/`capacity_state()`.
- [ ] 4.3 `tasks/sim_tick/tick.rs`: after `arbiter::reconcile` returns `absorbed_kwh_by_asset`,
      call `state.accumulate_residual(...)`; check each asset's fraction against a new
      profile-configurable `residual_threshold_fraction` (illustrative default ~0.2 — needs a real
      value chosen, no default exists in the design doc); on breach, send
      `PlanTrigger::ResidualThreshold` via the existing `trigger_tx` watch channel — **gated by the
      cooldown from task 4.4, not fired unconditionally on every breach check**.
- [ ] 4.4 Add an explicit minimum-interval cooldown between `PlanTrigger::ResidualThreshold`
      firings (e.g. a `last_residual_trigger_at: Option<DateTime<Utc>>` on `AppState`, checked
      before sending). **Open design question carried from review, needs confirmation before/at
      implementation**: should `ResidualThreshold` also route through
      `evaluate_acceptance_gate`'s cost-improvement gating instead of being an unconditional hard
      trigger (today `is_hard_trigger = !matches!(trigger, PlanTrigger::Periodic)` adopts every
      non-`Periodic` trigger unconditionally, with no rate-limiting anywhere) — a cooldown alone
      may not be sufficient if the underlying cause is persistent, not transient.
- [ ] 4.5 `entities/asset.rs`: add `PlanTrigger::ResidualThreshold` to the enum. Grep for any
      exhaustive `match trigger { ... }` beyond `evaluate_acceptance_gate` (wildcard-safe via
      `!matches!(.., Periodic)`) and update mechanically — none expected to need special-casing
      beyond a `trigger_reason` string for logging/`PlannerEvent`.
- [ ] 4.6 `services/planning.rs`: at the existing plan-adoption call site, reset the residual
      accumulator and re-snapshot `capacity_kwh_at_last_plan` from the freshly-adopted plan's/
      current `AssetSnapshot`'s `available_charge_kwh`/`available_discharge_kwh` — regardless of
      what triggered this adoption (periodic or hard), so periodic replans also clear accumulated
      debt.
- [ ] 4.7 Unit test reconstructing §5.5's worked example: four small battery absorptions each
      individually under threshold, cumulative total crossing it → `ResidualThreshold` fires (once,
      respecting the task-4.4 cooldown); a single large absorption that itself exceeds the fraction
      fires immediately too.

## 4a. Lever-switching and heater-mode hysteresis (must land before 2.7 is wired into production)

- [ ] 4a.1 Add a configurable lever-preemption margin: a challenger lever must be cheaper than the
      currently-active lever by more than this margin to take over — a nominal tie (or near-tie)
      does not cause a switch. Applies inside `rank_and_apply` (task 2.7).
- [ ] 4a.2 Add a minimum dwell time (or equivalent hysteresis) for `HeaterEmergencyMode`
      transitions specifically, so a marginal cost hovering near `heater_comfort_override_eur_per_kwh`
      cannot flip `Curtail`/`Absorb` on and off every tick.
- [ ] 4a.3 Unit test: two levers with near-equal marginal cost under a sustained deviation — assert
      the arbiter does not swap the active lever every tick; it stays on the incumbent until the
      margin is clearly exceeded. Unit test: heater mode does not chatter when marginal cost
      oscillates narrowly around the threshold.

## 5. Regression safeguard (feature 017 oscillation — both shapes)

- [ ] 5.1 Feature-017-shape: a fresh BDD/integration scenario (feature 017's own `.feature` files
      were deleted with `absorber.rs` — check `git log --diff-filter=D -- '**/absorber*.feature'` /
      commit `7aa84a3` for wording to ground it conceptually, but write new steps) reproducing the
      failure shape: a PV step change that would, under the old two-loop architecture, cause the
      overlay (stale PV) and a reactive corrector (live PV) to push the battery in opposite
      directions on consecutive ticks. Assert the new arbiter's battery setpoint moves
      monotonically toward convergence across at least 3 consecutive ticks.
- [ ] 5.2 Lever-switching-chatter shape (from task 4a.3, promoted to an integration-level test):
      two near-equal-cost levers under sustained deviation across several real `tick_once` calls —
      assert no more than one lever switch across the whole run.
- [ ] 5.3 Add both as integration-level tests in `tasks/sim_tick/tick_tests.rs` running real
      `tick_once` calls, not just unit-level function calls.

## 6. Rollout gate

- [ ] 6.1 New `deviation_arbiter_enabled: bool` (default `false`), profile-configurable, following
      the `EvSettings.opportunistic_charging_enabled` pattern — plumb through profile YAML loading,
      `AppState`, and the `tick.rs` branch added in task 3.2.
- [ ] 6.2 Regression-run existing `apply_surplus_ev_overlay`/`apply_battery_correction_overlay`
      unit tests unchanged against their new home in `arbiter.rs` (same assertions, same fixtures)
      to confirm the move preserved behavior exactly.

## 7. Verification and bookkeeping

- [ ] 7.1 Full VEN Rust test pyramid + architecture test (`wsl cargo test -j 2 -p ven-app`).
- [ ] 7.2 `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`.
- [ ] 7.3 `scripts/audit_file_sizes.py`.
- [ ] 7.4 VEN UI: `PvCurtailmentSource` TS enum + any `pv-curtailment-history` UI matching updated
      for the new `Arbiter` variant; `tsc --noEmit`, ESLint, full UI test suite.
- [ ] 7.5 Record in `docs/history/project_journal.md`; update
      `docs/architecture/VEN_ARCHITECTURE.md` §2.1's "GAP" note (BL-22 resolved) and the Dispatcher
      description to reflect the new arbiter stage.
