## 1. Interpolation helper

- [x] 1.1 Write failing unit tests in `VEN/src/entities/asset.rs` (near `ComfortRate`) for a
      new `ComfortRate::value_at_fill(rates: &[ComfortRate], fill: f64) -> f64`: exact
      breakpoint lookup, mid-curve linear interpolation, and out-of-range clamping (per the
      `session-comfort-curve-planning` spec's interpolation requirement).
- [x] 1.2 Confirm the tests fail (function doesn't exist yet).
- [x] 1.3 Implement `value_at_fill`; confirm the tests pass. (4/4 green, incl. a 3-point curve
      bracket-interpolation test beyond the spec's 2-point minimum.)

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
- [x] 2.2 Confirmed: task 3.3's field population only needed `services/user_request.rs`'s
      `create_ev`/`create_heater` (single construction site per asset). `routes/hems/ev.rs`/
      `routes/hems/heater.rs` were given `comfort_rates: vec![]` (required field, no curve
      concept there — unchanged behavior). Also found and fixed a third `UserRequest`
      construction site not in the original trace: `routes/hems/sessions.rs`'s shiftable-load
      fast-path (duplicates `UserRequestService::create_shiftable` inline) — out of scope for
      the curve mechanism (shiftable loads aren't part of this change) but needed the new
      required field.
- [ ] 2.3 Resolve the design's open question on `HeaterTarget`'s autonomous (no-session)
      MayRun path: confirm `comfort_full_reward_eur_kwh` stays `0.0` there (no behavior
      change), matching D5's MustRun/no-session handling.

## 3. Carry the curve through the entities

- [x] 3.1 Add `pub comfort_rates: Vec<ComfortRate>` to `UserRequest`
      (`entities/user_request.rs`, `#[serde(default)]` for back-compat).
- [x] 3.2 Fix `create_from_body`: stop binding to `_comfort_rates`; store the resolved value on
      the constructed `UserRequest`.
- [x] 3.3 Add `pub comfort_rates: Vec<ComfortRate>` to `EvSession` and `HeaterTarget`
      (`entities/device_session.rs`, `#[serde(default)]`); populate from
      `UserRequest.comfort_rates` in `services/user_request.rs::create_ev`/`create_heater`.
      Compiles clean (`cargo check --tests`) — all other construction sites (legacy direct
      routes, poll_signals.rs VTN-commanded sessions, ~20 test fixtures) updated to pass
      `comfort_rates: vec![]` where no curve concept applies.

## 4. EV — repoint reward sourcing (D3)

- [ ] 4.1 Write a failing planner test in `VEN/src/controller/milp_planner/tests/` (new or
      existing EV test file): two EV sessions with `mode: ByDeadline, soft_deadline: true`
      (`MayRun`), identical state/tariffs, differing only in `comfort_rates`, must produce
      different results in the solved plan.
      **Revised during implementation**: the original plan to assert on `e_ev_extra_kwh`
      differing doesn't hold — `e_ev_extra` is only bounded *above* by
      `e_extra_max_kwh * z_ev_core` (`ev_milp.rs::constraints`), nothing lower-bounds it by
      real charged power, so its reward is a documented no-op for driving allocation. This is
      an already-known limitation, tracked as **R-18** in `docs/reference/TECHNICAL_DEBTS.md`
      (found independently again here — same root cause, same fix noted there: couple
      `e_ev_extra` from below or move to per-slot reward form). Verified empirically
      (binary-search probe on the reward coefficient) before locking in the test. Switched to
      asserting on `z_ev_core`
      commitment (whether the session charges its core target at all) instead — the
      correctly-wired mechanism (`ev_milp.rs::constraints`'s `ev_energy >= e_core_kwh *
      z_ev_core`). Test written in `tests/modes.rs`, pinning the fill=1.0 price to `0.0` in
      both curves to avoid the banked-extra-reward confound cross-contaminating the
      core-price comparison.
- [x] 4.2 Confirmed the test fails pre-fix (both sessions charge exactly the core amount,
      curve has zero effect).
- [x] 4.3 In `ev_milp.rs::from_state`, inside the `UserRequestMode::ByDeadline |
      UserRequestMode::Asap` match arm only, compute `v_core_eur_kwh`/`v_extra_eur_kwh` via
      `ComfortRate::value_at_fill` at `fill=0.0`/`1.0` from `session.comfort_rates` — falling
      back to the passed-in global defaults when `comfort_rates` is empty (legacy
      `/ev-session` route, VTN-commanded sessions never resolve a curve). All other match arms
      unchanged, confirmed in design D3.
- [x] 4.4 Confirmed the test passes.
- [x] 4.5 Added two regression tests: `from_state_by_deadline_empty_curve_falls_back_to_global_defaults`
      (unit-level, `ev_milp.rs`) confirms the empty-curve fallback reproduces the old
      global-parameter behavior exactly; `test_by_deadline_soft_no_curve_override_uses_default_reward`
      (`tests/modes.rs`) confirms the EV's real `default_comfort_rates()` values commit as
      expected, not a no-op zero reward.

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
- [ ] 6.8 Per the `workflow` rule in `.claude/CLAUDE.md` (no-lingering-plans, added on `main`
      after this change was proposed): once implemented and tested, delete this openspec
      change directory (`openspec/changes/comfort-curve-milp-constraints/`, including its
      `specs/`) rather than leaving or archiving it — its content is git-recoverable via
      history. Fold anything durable (the phase-gating/mode-scope decisions) into
      `docs/reference/KEY_LEARNINGS.md` if they'd help future MILP objective work; the
      day-to-day narrative doesn't need to survive.
