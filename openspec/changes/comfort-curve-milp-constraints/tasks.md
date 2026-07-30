## 1. Interpolation helper

- [ ] 1.1 Write failing unit tests in `VEN/src/entities/asset.rs` (near `ComfortRate`) for a
      new `ComfortRate::value_at_fill(rates: &[ComfortRate], fill: f64) -> f64`: exact
      breakpoint lookup, mid-curve linear interpolation, and out-of-range clamping (per the
      `session-comfort-curve-planning` spec's interpolation requirement).
- [ ] 1.2 Confirm the tests fail (function doesn't exist yet).
- [ ] 1.3 Implement `value_at_fill`; confirm the tests pass.

## 2. Trace and confirm the call chain to `EvSession`/`HeaterTarget`

- [x] 2.1 Confirmed: two independent construction paths exist.
      `routes/hems/sessions.rs::post_requests` (`POST /user-requests`) → `UserRequestService::
      create_ev`/`create_heater` (`services/user_request.rs:18,57`) → `create_from_body`
      (`controller/user_request.rs:75`, where the curve is resolved then dropped) — this is
      the path the VEN UI actually uses (`usePostRequest` in `Devices.tsx`, the only caller of
      that hook). Separately, `routes/hems/ev.rs::post_ev_session` (`POST /ev-session`) and
      `routes/hems/heater.rs` (`POST /heater-target`) build `EvSession`/`HeaterTarget` directly
      with no comfort-curve field/logic at all — these hooks (`usePostEvSession`,
      `usePostHeaterTarget`) exist in `api/hooks.ts` but are not called from any UI page/
      component. Scope this change to the `/user-requests` path only: the direct routes were
      already curve-blind before this change (never referenced `comfort_rates`), so leaving
      them unchanged is not a regression, just an existing dead/legacy surface. Locate the
      point where `build_asset_contexts` reads `EvSession`/`HeaterTarget` to call
      `EvMilpContext::from_state`/`HeaterMilpContext::from_state` (`simulator/plan_context.rs`)
      before starting task 3.
- [ ] 2.2 Confirm task 3.3's field population only needs to touch the `/user-requests`
      construction site(s) in `services/user_request.rs` (where `EvSession`/`HeaterTarget` are
      built from a `UserRequest`) — not `routes/hems/ev.rs`/`routes/hems/heater.rs`, which stay
      as they are.
- [ ] 2.3 Resolve the design's open question on `HeaterTarget`'s autonomous (no-session)
      MayRun path: confirm `comfort_full_reward_eur_kwh` stays `0.0` there (no behavior
      change), matching D5's MustRun/no-session handling.

## 3. Carry the curve through the entities

- [ ] 3.1 Add `pub comfort_rates: Vec<ComfortRate>` to `UserRequest`
      (`entities/user_request.rs`).
- [ ] 3.2 Fix `create_from_body`: stop binding to `_comfort_rates`; store the resolved value on
      the constructed `UserRequest`.
- [ ] 3.3 Add `pub comfort_rates: Vec<ComfortRate>` to `EvSession` and `HeaterTarget`
      (`entities/device_session.rs`); populate from `UserRequest.comfort_rates` at the
      construction site(s) found in 2.1.

## 4. EV — repoint reward sourcing (D3)

- [ ] 4.1 Write a failing planner test in `VEN/src/controller/milp_planner/tests/` (new or
      existing EV test file): two EV sessions with `mode: ByDeadline, soft_deadline: true`
      (`MayRun`), identical state/tariffs, differing only in `comfort_rates`, must produce
      different `e_ev_extra_kwh`/`z_ev_core` results in the solved plan.
- [ ] 4.2 Confirm the test fails (both sessions currently produce identical allocations).
- [ ] 4.3 In `ev_milp.rs::from_state`, inside the `UserRequestMode::ByDeadline |
      UserRequestMode::Asap` match arm (line ~336) only, compute `v_core_eur_kwh =
      ComfortRate::value_at_fill(&session.comfort_rates, 0.0)` and `v_extra_eur_kwh =
      ComfortRate::value_at_fill(&session.comfort_rates, 1.0)`, replacing the passed-in
      `v_ev_core_eur_kwh`/`v_ev_extra_eur_kwh` parameters for that arm's `v_core_eur`
      (`core_kwh * v_core_eur_kwh`, only when `soft_deadline`) and `e_ev_extra` reward. All
      other match arms (`Opportunistic | AsapFree`, `MaxCost`, `ByDeadlineFree`) are
      unchanged — confirmed in design D3 that their `v_extra_eur_kwh` overrides
      (`v_ev_free_charge_eur_kwh`, `BUDGET_CHARGE_REWARD_EUR_KWH`) are a different signal than
      comfort preference.
- [ ] 4.4 Confirm the 4.1 test passes.
- [ ] 4.5 Add a no-curve-session regression test: a session using `default_comfort_rates()`
      (no override) produces the same allocation as before this change (compare against a
      fixture solved with the old hardcoded `PlannerParams` values).

## 5. Heater — new reward term (D4)

- [ ] 5.1 Write a failing test in `VEN/src/assets/heater_milp.rs`'s existing
      `milp_context_trait_tests`/`milp_tests` modules (or
      `controller/milp_planner/tests/heater.rs`) asserting that two otherwise-identical
      `HeaterMilpContext`s with different `comfort_full_reward_eur_kwh` values produce
      different objective expressions (follow the existing `format!("{obj:?}")` comparison
      pattern used for terminal-reward tests), and/or different solved `z_heat_full`
      allocations end-to-end.
- [ ] 5.2 Confirm the test fails (field doesn't exist yet / has no effect).
- [ ] 5.3 Add `pub comfort_full_reward_eur_kwh: f64` to `HeaterMilpContext`; compute it in
      `from_state` via `ComfortRate::value_at_fill(&target.comfort_rates, 1.0)` (0.0 when no
      session/curve, preserving current behavior).
- [ ] 5.4 Add a new `comfort_full_reward_eur_kwh: f64` parameter to the inherent `objective()`
      signature (not a `self` read); add `obj -= comfort_full_reward_eur_kwh * dt *
      v.z_heat_full[t]` alongside the existing `w_tier_penalty_eur * v.z_heat_full[t]` term.
      In the `AssetMilpContext::objective()` trait impl (`asset_port.rs:385-410`), pass `0.0`
      in the Phase 1 branch (`c_startup_eur == 0.0`) and `self.comfort_full_reward_eur_kwh` in
      the Phase 2 branch — mirrors `w_tier_penalty_eur`'s existing phase-gating exactly (D4).
- [ ] 5.5 Confirm the 5.1 test passes: Phase 1's objective is unchanged when the new parameter
      is `0.0` (add an explicit assertion for this, not just "doesn't regress"); Phase 2's
      `z_heat_full` allocation responds to `comfort_full_reward_eur_kwh` changes.
- [ ] 5.6 Add a no-curve-session regression test confirming unchanged fallback behavior
      (`comfort_full_reward_eur_kwh == 0.0` reproduces pre-change allocations).

## 6. Cross-check and full verification

- [ ] 6.1 Manual UI check (or an E2E scenario if manual verification isn't practical this
      session): confirm moving a comfort-curve slider and creating a new session visibly
      changes the resulting plan for that asset.
- [ ] 6.2 Run `wsl cargo test -j 2 -p ven-app` under `wsl_lock` (acquire lock first per
      `wsl-lock` rule) — all tests green.
- [ ] 6.3 Run `cargo fmt --check` and `cargo clippy --all-targets --all-features -- -D
      warnings` — clean.
- [ ] 6.4 Run `scripts/audit_file_sizes.py` — confirm `heater_milp.rs`/`ev_milp.rs`/
      `user_request.rs` stay within the VEN/src/ 500 production-line limit (split further if
      any file grows past it).
- [ ] 6.5 Run `cargo audit` — no new advisories introduced (no new dependencies expected).
- [ ] 6.6 Update `docs/history/project_journal.md` with what changed, why, and any key
      learnings (e.g. anything discovered in task 2.1/2.2 that diverged from the design).
- [ ] 6.7 Remove the BL-34 entry (and its Implementation Task List section 1 checklist) from
      `docs/BACKLOG.md` once merged.
