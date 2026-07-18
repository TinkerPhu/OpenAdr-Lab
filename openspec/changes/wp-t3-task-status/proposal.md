## Why

None of the VEN's 9 supervised background tasks (poll_events, poll_programs,
poll_reports, sim_tick, planning, obligation, state_persist, history_sampler,
heuristics_job) expose whether they're actually running. `supervised_spawn`
(`VEN/src/tasks/mod.rs`) already restarts a task on panic and logs it, but tracks
no state anywhere reachable outside the log line — if a task silently crash-loops,
nothing in the UI would ever show it. WP-T3 of `docs/plans/ven-ui-transparency.md`.

## What Changes

- Add `TaskStatus { last_run_ts, last_success, restart_count }` tracked per task
  name on `AppState` (new `state/task_status.rs` submodule — `state/mod.rs` has
  only ~32 lines of headroom under its file-size cap, too little for this inline).
- `supervised_spawn` gains an `AppState` parameter and records a start on each
  (re)spawn and a completion (success/panic) + restart-count increment each time
  the wrapped future exits. All 9 call sites in `main.rs` pass their already-in-scope
  `state.clone()`.
- New route `GET /tasks/status` → `[{name, last_run_ts, last_success,
  restart_count}]`, one entry per task that has actually been spawned (not a fixed
  list of 9 — `state_persist`/`history_sampler`/`heuristics_job` are conditionally
  spawned depending on config, so the response reflects only what's really running).
- UI: new Tasks page (Diagnostics group, per the plan doc's nav redesign — added
  without waiting for WP-T8's full nav rebuild, since it needs to exist somewhere).

## Capabilities

### New Capabilities
- `task-supervision-status`: each supervised background task reports its last
  (re)start time, whether its most recent run completed normally or panicked, and
  how many times it has been restarted, on a dedicated endpoint.

### Modified Capabilities
(none — `task-supervisor` capability, per `openspec/specs/task-supervisor/spec.md`,
governs the panic-restart *behavior*; this change adds observability into that
existing behavior, not new restart semantics)

## Impact

- **VEN** (Rust): `state/task_status.rs` (new), `state/mod.rs` (one `mod`/`pub use`
  line pair + one `AppState` field), `tasks/mod.rs` (`supervised_spawn` signature
  change), `main.rs` (9 call sites each gain one `state.clone()` arg),
  `routes/system.rs` (new handler), `routes/mod.rs` (new route registration).
- **VEN UI**: new Tasks page; `api/types.ts`/`api/client.ts` additions.
- **Non-goals**: `progress_ticker` is excluded — it's not a top-level supervised
  task; it's spawned/cancelled per plan-solve-cycle inside `spawn_planning`'s own
  loop with a cancel-and-await lifecycle, not a restart lifecycle, so it doesn't fit
  this shape and isn't a task a resident/operator would look for in a "background
  tasks" list. No change to `supervised_spawn`'s restart/cooldown *behavior* — purely
  additive observability. No Dashboard summary line (WP-T8) — this WP delivers the
  Tasks page as a Diagnostics-group destination, not the Dashboard's status row.
