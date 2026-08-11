## Why

A Layer-1 reactive battery/EV/heater/PV correction (`controller::arbiter::reconcile`, gated by
`deviation_arbiter_enabled`) currently has exactly one visible surface: the Planner tab's
`CorrectionBanner`, which only renders while that page happens to be mounted (`usePlannerEvents`
in `VEN/ui/src/pages/Planner.tsx` subscribes to the planner SSE stream for the component's
lifetime only). A correction firing while the resident is on the Dashboard, Devices, or any other
tab is invisible in the moment and leaves no durable record anywhere. This is a real trust/visibility
gap: the arbiter is actively overriding the plan's setpoints, and the only affected user has no way
to know unless they happen to be looking at one specific tab. Investigation for this change also
found that `CorrectionBanner` itself is currently dead UI — it listens for SSE events
(`correction_active`/`correction_cleared`) that no backend code has ever constructed (see
`wiki/components/deviation-arbiter.md`'s DRIFT note) — so today the gap is total, not merely
tab-scoped. This change closes it by routing the arbiter's edge signal through the existing
backend `Notifier` (ring + SSE + persistence), which already fans out to the global
`NotificationsBell` on every tab regardless of which page is mounted.

## What Changes

- Add an edge-triggered notification producer that fires when the arbiter's per-tick active-lever
  state transitions from inactive → active (a correction starts) or active → inactive (a correction
  clears), following the established `notify_outage_edge` producer pattern in `services/notify.rs`.
- Wire the producer into the sim-tick loop at the point that already records the arbiter's per-tick
  outcome (`tasks::sim_tick::arbiter_glue::record_arbiter_outcome`), threading a `Notifier` handle
  through `spawn_sim_tick` → `tick_once` → `record_arbiter_outcome` (mirrors how `Notifier` is
  already threaded into `poll_events`, `poll_signals`, and `planning`).
- Extract a shared `RingBuffer<T>` helper (push-and-evict-oldest-at-capacity) and use it at its
  three current near-identical call sites: `state/mod.rs`'s notification ring
  (`AppState::push_notification`), `state/event_log.rs`'s `record_event`, and
  `state/report_submissions.rs`'s `record_report_submission`. Pure refactor — no observable
  behavior change, existing eviction-order tests continue to assert the same outcomes.
- No UI changes: the consumer side (`NotificationsBell`, `GET /notifications`,
  `GET /notifications/history`) already exists and already renders any `UserNotification` — this
  change only supplies the missing producer.
- Does **not** fix `CorrectionBanner`'s dead SSE wiring (`PlannerEvent::CorrectionActive`/
  `CorrectionCleared` variants still don't exist) — that is a separate, already wiki-documented
  drift item and out of scope here; this change's producer reads the arbiter's state directly
  rather than depending on that broken path.

## Capabilities

### New Capabilities
- `reactive-correction-notifications`: an edge-triggered producer that emits exactly one
  info/warning `UserNotification` when a reactive (Layer-1) correction starts, and at most one
  follow-up when it clears, reaching every tab via the existing notification ring/SSE/history path.

### Modified Capabilities
(none — the `RingBuffer<T>` extraction is an internal refactor with no change to any existing
capability's observable requirements)

## Impact

- `VEN/src/services/notify.rs` — new producer function + tests (Application ring).
- `VEN/src/tasks/sim_tick/arbiter_glue.rs`, `mod.rs`, `tick.rs` — thread `Notifier` through and
  call the new producer at the existing arbiter-outcome recording point (Adapter ring).
- `VEN/src/main.rs` — pass the already-constructed `notifier` into `spawn_sim_tick`.
- `VEN/src/entities/` — new `ring_buffer.rs` module (Domain ring, zero-dependency generic type).
- `VEN/src/state/mod.rs`, `state/event_log.rs`, `state/report_submissions.rs` — refactored to use
  `RingBuffer<T>`; existing unit tests must keep passing unmodified in behavior.
- No route/API surface change, no UI change, no schema/persistence migration.
