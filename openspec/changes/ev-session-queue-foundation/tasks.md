# Tasks

Branch: `NNN-ev-session-queue-foundation` (take the next free openspec feature ID
for `NNN` at branch-creation time).

Every Rust task is written test-first (`test-first` rule): add the failing test,
run it selectively to confirm it fails, then implement until green.

## 1. The queue type and its invariant

- [ ] 1.1 Add `window_start: DateTime<Utc>` to `EvSession` (`VEN/src/entities/device_session.rs`) and verify `cargo check -p ven-app` names every construction site that must now state it (the compiler is the inventory — expect `routes/hems/sessions.rs`, `services/user_request.rs`, `tasks/poll_signals.rs`, `tasks/sim_tick/usage_sim_plan_ahead.rs` and their test fixtures). In this change the user-request and VTN producers set it to `now`, which is today's implicit semantics made explicit; only the simulated producer derives it from the preceding trip's return (task 5.2). Letting a *user* state it is the follow-up change's job
- [ ] 1.2 Write the failing unit tests for `EvSessionQueue` overlap semantics in `device_session.rs`: `conflicts_reports_overlapping_session`, `conflicts_allows_touching_windows` (one session's `departure_time` == the next's `window_start`), `insert_orders_by_window_start`, `insert_rejects_overlap_naming_conflicts`; verify they fail to compile/run before any implementation
- [ ] 1.3 Implement `EvSessionQueue` (newtype over `Vec<EvSession>`) with `conflicts`, `insert -> Result<(), EvSessionConflict>`, `remove(id)`, `current(now)`, `expire(now)`, `iter` — half-open `[window_start, departure_time)` per design Decision 2 — and verify the 1.2 tests pass
- [ ] 1.4 Add a property test asserting the ordered/non-overlapping invariant holds after arbitrary interleavings of `insert`/`remove`/`expire`, and verify it passes (design Risks: three producers, one invariant)
- [ ] 1.5 Add `EvSessionConflict` as a `DomainError` variant carrying the conflicting session ids as typed fields per `docs/guidelines/ERROR_HANDLING.md`, and verify `cargo clippy --all-targets --all-features -- -D warnings` is clean

## 2. State storage and accessors

- [ ] 2.1 Write failing `AppState` tests for `insert_ev_session`/`ev_sessions`/`current_ev_session`/`remove_ev_session`/`expire_ev_sessions` in `VEN/src/state/mod.rs`, including `expire_ev_sessions_drops_several_passed_sessions`; confirm they fail
- [ ] 2.2 Replace `HemsState.ev_session: Option<EvSession>` with `ev_sessions: EvSessionQueue` and implement the accessors; delete `ev_session()`/`set_ev_session()` so no caller can bypass the checked insert, and verify the 2.1 tests pass
- [ ] 2.3 Write a failing test that cancelling the request owning the *second* of three queued sessions leaves the other two queued, then fix `AppState::cancel_request` to remove by `session_id` instead of clearing the slot, and verify it passes (spec: "Cancelling a request removes only its own session")
- [ ] 2.4 Run `cargo check -p ven-app` and move every remaining caller of the deleted accessors (`routes/hems/ev.rs`, `routes/hems/sessions.rs`, `tasks/planning/cycle_state.rs`, `tasks/sim_tick/context.rs`, `tasks/sim_tick/arbiter_glue.rs`, `tasks/poll_signals.rs`) onto the queue API, keeping each one's existing behaviour for a single-session queue; verify `cargo test -p ven-app` is green
- [ ] 2.5 Make the user-request producer's conflict policy explicit (design Decision 3): `routes/hems/sessions.rs` asks `conflicts`, removes exactly those sessions, then inserts — today's silent overwrite concentrated at one site. Verify with a test that submitting an overlapping EV session still displaces the old one (unchanged observable behaviour) and that a non-overlapping one now leaves both queued
- [ ] 2.6 Verify the VTN producer (`tasks/poll_signals.rs`) replaces only the session it created, by id, and that a withdrawn charge signal removes that session and no other (spec: "A withdrawn VTN charge signal removes only the session it created")

## 3. Expiry and the "active session" derivation

- [ ] 3.1 Write failing tests in `tasks/sim_tick/arbiter_glue.rs` that `paused_by_active_session` is true only while a session's window contains `now` — specifically `not_paused_between_two_queued_sessions` and `not_paused_when_only_future_sessions_are_queued`; confirm they fail against today's `is_some()` test
- [ ] 3.2 Change `resolve_overlay_enabled` to use `current_ev_session(now)` for the pause flag and `expire_ev_sessions(now)` for expiry (design Decision 6), and verify the 3.1 tests pass plus the existing arbiter_glue tests stay green
- [ ] 3.3 Feed `EvCharger.departure_time` / `TickOverrides.ev_departure_time` from the current session, falling back to the queue head (design Decision 8), and verify the existing `simulate_forward` departure tests in `assets/ev_schedule.rs` still pass

## 4. The planner obligation list

- [ ] 4.1 Write the failing MILP context tests in `assets/ev_milp.rs`: `two_queued_sessions_yield_two_obligations`, `energy_in_a_later_window_does_not_satisfy_an_earlier_obligation`, `obligation_beyond_the_horizon_is_dropped`; confirm they fail
- [ ] 4.2 Replace `EvMilpContext.t_dead_step` + `.e_required_kwh` with `obligations: Vec<EvObligation>` in `controller/milp_planner/asset_port.rs`, generalise `energy_expr`/`reachable_energy_kwh` to take `(first_step, last_step)`, and emit one energy-floor constraint per obligation with the whole-horizon `ev_energy == bought` equality retained (design Decision 4); verify 4.1 passes and every pre-existing single-session MILP test is still green
- [ ] 4.3 Write a failing test that the open-loop SoC chain sizes a later obligation correctly (`required_kwh` for session k includes the preceding trip's predicted drop, floored at `min_soc_after_drop_pct`), then implement the chaining in a new module beside `assets/ev_session_context.rs` sourcing the drop from `ev_schedule` only; verify it passes
- [ ] 4.4 Build the obligation list in `EvMilpContext::from_state` from the session queue, keeping the comfort-curve `segments` built from the head session alone (design Decision 5), and verify the existing `ev-comfort-piecewise-core` band tests are byte-identical in behaviour
- [ ] 4.5 Switch `assets/ev_usage_forecast.rs::target_next_predicted_departure` to push one `EvObligation` instead of writing `t_dead_step`/`e_required_kwh` (design Decision 7), and verify `tests/features/ev_usage_forecast.feature` behaviour is unchanged via the existing unit tests
- [ ] 4.6 Make `assets/ev_diagnostics.rs::firm_shortfall` report per obligation with its `session_id`, and verify a test asserting the shortfall names which queued session fell short
- [ ] 4.7 Verify no obligation can make the site solve infeasible: add a test where a queued session demands more than its window can deliver and assert a plan is still produced with a reported shortfall (spec: "An unreachable obligation degrades visibly, never infeasibly")

## 5. The simulated-usage producer

- [ ] 5.1 Write failing tests in `tasks/sim_tick/usage_sim_plan_ahead.rs`: `queues_a_session_for_each_predicted_trip_in_seven_days`, `tops_up_the_window_as_time_advances`, `skips_a_day_with_no_predicted_trip`, `is_idempotent_across_repeated_ticks`, `skips_a_trip_conflicting_with_a_user_session_and_keeps_the_rest`, `writes_nothing_under_the_forecast_class`; confirm they fail
- [ ] 5.2 Replace the single-trip lookahead with a rolling 7-day loop over `ev_schedule::next_trip_after`, deriving each session's `window_start` from the preceding trip's `return_at` (or `now` for the imminent one) and inserting via the checked `insert`, skipping conflicts; verify 5.1 passes
- [ ] 5.3 Verify the producer stays under the 200-line `tasks/` cap by running `python scripts/audit_file_sizes.py`, splitting the trip-to-session mapping into `assets/` if needed

## 6. Read surfaces and UI

- [ ] 6.1 Fix `GET /user-requests` (`routes/hems/sessions.rs:119-167`) to resolve each request's session by its own `session_id` against the queue instead of matching against one global session, and verify a test with two EV-owning requests shows each its own target and departure
- [ ] 6.2 Return the queue from `GET /ev-session` (`routes/hems/ev.rs`), resolving design Open Question 2 at this point, and verify a test asserting all queued sessions come back in window order
- [ ] 6.3 Update `VEN/ui/src/api/types.ts` (`EvSession` gains `window_start`; the EV read surface returns a list) and verify `cd VEN/ui && npm test` plus `npx eslint` are clean
- [ ] 6.4 Surface the queue in the VEN UI per `ui-transparency` — upcoming sessions visible alongside the current one in `components/sessions/SessionProgressBoard.tsx` — and verify a UI unit test renders two queued sessions with distinct departures
- [ ] 6.5 Verify in the running app (`ssh Node2` stack or local dev server) that a simulated 7-day queue is visible in the UI and that opportunistic charging is not permanently paused

## 7. BDD use-case coverage

- [ ] 7.1 Add a scenario to `tests/features/ev_usage_simulation.feature` asserting several simulated sessions are queued across a week and each predicted trip has its own session, and verify it fails before section 5 lands
- [ ] 7.2 Add a scenario asserting the plan charges for a deadline that lies behind an intervening departure, and reports a shortfall rather than promising an unreachable target (spec: "Pre-charging across an intervening departure")
- [ ] 7.3 Add a scenario asserting opportunistic charging resumes in the gap between two queued sessions
- [ ] 7.4 Extend `tests/features/ven_user_request.feature` so cancelling one request leaves the other queued sessions in place
- [ ] 7.5 Run `DOCKER_HOST=Node2 bash run_all_tests.sh --e2e` and verify all BDD suites pass

## 8. Verification and close-out

- [ ] 8.1 Run the full gate and verify green: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo audit`, `python scripts/audit_file_sizes.py`, the four VEN Rust pyramid layers, both UI suites
- [ ] 8.2 Verify the `ven-architecture` grep invariants are still empty (no `use crate::profile` in `entities/`/`controller/`/`routes/`, no `use crate::assets::` in `milp_planner` or `entities/`)
- [ ] 8.3 Re-benchmark EV phase-2 solve time against the R-97 battery+EV figures with a 7-session queue and record the delta in `docs/history/project_journal.md` (design Risks: solve time grows with queue length)
- [ ] 8.4 Run `DOCKER_HOST=Node2 bash run_all_tests.sh` and verify every suite passes
- [ ] 8.5 Record the accepted pre-charge limitation (design Decision 4) in `docs/reference/TECHNICAL_DEBTS.md`, naming the SoC-state-variable model as its fix
- [ ] 8.6 Wave the implemented behaviour into current-state docs per `workflow` rule 3: mechanism-level facts into `docs/architecture/VEN_ARCHITECTURE.md` (EV charger section and §3.0d asset-competence), user-observable behaviour into the relevant `docs/use-cases/*.md`, pointing at the test files by name rather than restating scenarios
- [ ] 8.7 Write the journal entry (what, why, key learnings) in `docs/history/project_journal.md` and add any durable lesson to `docs/reference/KEY_LEARNINGS.md`
- [ ] 8.8 Delete `openspec/changes/ev-session-queue-foundation/` once implemented and tested, and verify the follow-up change `ev-session-user-conflict-resolution` still stands on its own
