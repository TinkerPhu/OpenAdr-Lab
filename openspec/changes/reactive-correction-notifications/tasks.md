## 1. RingBuffer<T> extraction (R-46)

- [ ] 1.1 Write failing unit tests for a new `VEN/src/entities/ring_buffer.rs` `RingBuffer<T>`:
      constructed with a capacity, `push` evicts the single oldest entry once length would exceed
      capacity, `len`, `iter`/oldest-first ordering, and a capacity-0 edge case.
- [ ] 1.2 Implement `RingBuffer<T>` (wraps `VecDeque<T>`, infallible `push`, no `DomainError` — see
      design.md D5) until the 1.1 tests pass. Export it from `entities/mod.rs`.
- [ ] 1.3 Refactor `VEN/src/state/mod.rs`'s `AppState::push_notification` (`notifications` ring,
      `NOTIFICATION_RING_CAP`) to use `RingBuffer<T>`. Re-run the existing
      `test_notification_ring_bounded_evicts_oldest` (services/notify.rs) unchanged and confirm it
      still passes.
- [ ] 1.4 Refactor `VEN/src/state/event_log.rs`'s `record_event` (`EVENT_LOG_RING_CAP`) to use
      `RingBuffer<T>`. Re-run `record_event_evicts_oldest_beyond_ring_cap` unchanged and confirm it
      still passes.
- [ ] 1.5 Refactor `VEN/src/state/report_submissions.rs`'s `record_report_submission`
      (`REPORT_SUBMISSION_RING_CAP`) to use `RingBuffer<T>`. Re-run `ring_evicts_oldest_past_cap`
      unchanged and confirm it still passes.
- [ ] 1.6 `grep -r "VecDeque" VEN/src/state/mod.rs VEN/src/state/event_log.rs VEN/src/state/report_submissions.rs`
      — confirm no hand-rolled evict-at-capacity loop remains at any of the three sites (reads that
      merely construct/iterate a `VecDeque` inside `RingBuffer<T>` are fine).

## 2. Reactive-correction notification producer (BL-37)

- [ ] 2.1 Write failing unit tests in `VEN/src/services/notify.rs` for a new
      `notify_correction_edge` producer (mirrors `notify_outage_edge`): `None → Some` emits exactly
      one notification (severity per design.md — Warn for active); `Some → Some` (same lever) emits
      nothing; `Some(a) → Some(b)` (lever handoff, no `None` in between) emits nothing and does not
      alter the earlier notification's text; `Some → None` emits exactly one "cleared" notification
      (severity Info); `None → None` emits nothing. Cover the dedup-key behavior (D6) directly via
      `Notifier::notify`'s existing dedup mechanism — no new dedup machinery needed.
- [ ] 2.2 Implement `notify_correction_edge` (and a small pure `correction_transition` helper
      mirroring `outage_transition`'s shape, if useful for testability without a `Notifier`) until
      2.1 passes. Use fixed dedup keys `"arbiter-correction-active"` / `"arbiter-correction-cleared"`
      and the lever-agnostic message text from design.md D1.

## 3. Wire the producer into the sim-tick loop

- [ ] 3.1 Add a `notifier: crate::services::notify::Notifier` parameter to
      `tasks::sim_tick::arbiter_glue::record_arbiter_outcome`, and call `notify_correction_edge`
      between the existing `arbiter_active_lever()` read and `set_arbiter_active_lever()` write
      (design.md D2 — the previous-tick value must be read before it's overwritten).
- [ ] 3.2 Thread `notifier` through `tasks::sim_tick::tick::tick_once` (new parameter) and
      `tasks::sim_tick::mod::spawn_sim_tick` (new parameter, cloned into the loop like `state`/
      `vtn`/`weather` already are).
- [ ] 3.3 Update `VEN/src/main.rs`'s `spawn_sim_tick` call site to pass the already-constructed
      `notifier` (defined earlier in `main.rs`, currently only passed to `poll_events`/
      `poll_signals`/`planning`).
- [ ] 3.4 Add an integration-level test (adapter-contract layer, `tasks/sim_tick` test module or
      equivalent) driving `record_arbiter_outcome` across a `None → Some → Some(different lever) →
      None` sequence and asserting exactly two notifications land in `AppState`'s ring (one active,
      one cleared), matching the spec's "lever handoff" scenario.

## 4. BDD coverage (use-case visibility)

- [ ] 4.1 Add a scenario to `tests/features/ven_notifications.feature` (or a new feature file if
      that one's scope note no longer fits) that: enables the arbiter (`PUT /arbiter-settings`),
      injects a sustained base-load deviation via `/sim/inject` (reusing the existing
      `I inject base_load_kw {kw} with alpha {alpha} via sim inject` step in
      `tests/features/steps/dispatcher_steps.py`), polls `GET /notifications` for a correction-start
      notification, clears the inject, polls for the "cleared" follow-up, then resets
      `deviation_arbiter_enabled` back to its default so later scenarios start clean.
- [ ] 4.2 Add/confirm any missing step definitions needed for 4.1 (e.g. a step to
      `PUT /arbiter-settings`, a step asserting a notification with given content appears within
      the ring) — check `tests/features/steps/` for existing equivalents before writing new ones.

## 5. Verification

- [ ] 5.1 `wsl cargo fmt --check` and `wsl cargo clippy --all-targets --all-features -- -D warnings`
      (per wsl-lock: acquire the lock first).
- [ ] 5.2 `python scripts/audit_file_sizes.py` — confirm `entities/ring_buffer.rs` and all touched
      files stay within the 500-production-line cap (VEN/src/) / 200-line cap (tasks/).
- [ ] 5.3 Run the four verifiable-invariant greps from `.claude/CLAUDE.md` (`use crate::profile` in
      entities/controller/routes; `use crate::assets::` in milp_planner or entities) — none of this
      change's files should trip them, but confirm since `entities/` gained a new file.
- [ ] 5.4 `wsl cargo test -p ven-app` (Rust unit + integration suites, per wsl-lock).
- [ ] 5.5 `bash run_all_tests.sh --e2e` on the preferred host (Node2 unless unavailable — see
      test-host-preference) to exercise the new BDD scenario end-to-end.
- [ ] 5.6 Update `docs/BACKLOG.md` (remove/resolve BL-37) and `docs/reference/TECHNICAL_DEBTS.md`
      (remove/resolve R-46) once implemented and tested, per the project's no-lingering-plans
      workflow rule; note the resolution in `docs/history/project_journal.md`.
