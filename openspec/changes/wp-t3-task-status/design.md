## Context

`supervised_spawn` (`VEN/src/tasks/mod.rs`) already restarts a panicked task and
logs the event, but tracks nothing outside that log line — no counter, no
timestamp, nothing on `AppState`. Confirmed via code investigation: every one of
the 9 top-level tasks (`poll_events`, `poll_programs`, `poll_reports`, `sim_tick`,
`planning`, `obligation`, `state_persist`, `history_sampler`, `heuristics_job`) is
registered through `supervised_spawn` in `main.rs`, each already passing
`state.clone()` into its own closure — so `AppState` is already in scope at every
call site, just not threaded into `supervised_spawn` itself.

`progress_ticker` is excluded from this WP: it's not a top-level supervised task —
it's spawned/cancelled per plan-solve-cycle inside `spawn_planning`'s own loop with
a cancel-and-await lifecycle, not a restart lifecycle. It doesn't fit the
"is this background task alive" question this WP answers.

## Goals / Non-Goals

**Goals:**
- Make `supervised_spawn`'s restart behavior observable: last (re)start time,
  whether the most recent run completed normally or panicked, and how many times
  it's been restarted — per task, on a route.
- Reflect only tasks actually spawned (some are conditional on config —
  `state_persist` needs `persist_path`; `history_sampler`/`heuristics_job` need a
  configured history store) rather than a hardcoded list of 9.

**Non-Goals:**
- No change to restart/cooldown *behavior* (governed by the existing
  `task-supervisor` capability spec) — purely additive observability.
- No `progress_ticker` entry (see Context) — it's a different lifecycle, not a
  "background task" in the sense a resident/operator would look for one.
- No Dashboard summary line — that's WP-T8's rebuild; this WP delivers the Tasks
  page as a Diagnostics-group destination.

## Decisions

**D1 — Semantics of `last_success`/`restart_count` for infinite-loop tasks.**
Every one of these 9 tasks is, by design, an infinite loop that only returns to
`supervised_spawn`'s `await` when it panics (or, unusually, if the loop body itself
returns — not expected under normal operation). So:
- `last_run_ts`: updated every time `supervised_spawn` (re)spawns the task —
  i.e., "when did this task's current run start."
- `last_success`: `None` until the task's future completes once. Stays `None` for
  the entire healthy lifetime of a normally-behaving task (it never returns).
  Flips to `Some(false)` after a panic, `Some(true)` in the unusual case the loop
  returned `Ok(())` (itself worth surfacing — these tasks aren't supposed to exit).
- `restart_count`: incremented each time the wrapped future completes (panic or
  return) and `supervised_spawn` loops back to respawn it. Starts at 0 for a task
  that has never needed a restart.

This shape reads directly off `supervised_spawn`'s actual control flow — no
polling, no separate heartbeat, just recording at the two points that already
exist (before spawning, after the `JoinHandle` resolves).

**D2 — New `state/task_status.rs` submodule, not inline in `state/mod.rs`.**
`state/mod.rs` has only ~32 lines of headroom under its 500-line cap (confirmed via
`scripts/audit_file_sizes.py`) — too little for a new struct + `HashMap`-backed
field + 3 methods. Follows the existing `state/connection.rs`/`heuristics.rs`/
`obligations.rs` extraction pattern exactly: `mod task_status;` +
`pub use task_status::TaskStatus;` in `mod.rs`, everything else in the submodule.

**D3 — `supervised_spawn` takes `AppState` directly, not a separate registration
call.** Alternative considered: a `TaskRegistry::register(name)` called once at
startup, kept separate from `supervised_spawn`. Rejected — `supervised_spawn` is
already the single place that knows a task's name and lifecycle; threading
`AppState` through it (one new parameter, one `state.clone()` at each of the 9
call sites, all of which already have `state` in scope) is less code than
introducing a second registration mechanism that could drift out of sync with
which tasks actually get spawned.

**D4 — Response is an array keyed by name that was actually spawned, not a fixed
9-entry list.** `state_persist`/`history_sampler`/`heuristics_job` are
conditionally spawned. Since entries are created lazily on first
`record_task_started` call (not pre-populated), the response naturally reflects
only tasks that exist in this deployment — no need for the route to know the
config-conditional spawn logic itself.

## Risks / Trade-offs

- **[Risk] A task that has been running healthily since startup and never
  panicked shows `last_success: null`, `restart_count: 0` — could look
  ambiguous ("is null good or bad?") in the UI.** → Mitigation: the UI page (§7)
  renders `restart_count == 0` as the "healthy" signal, not `last_success` —
  `last_success` is specifically the *last completion* outcome, which is
  legitimately absent for a task still on its first, ongoing run. Document this
  in the UI code comment, not just here.
- **[Risk] Adding an `AppState` parameter to `supervised_spawn` is a signature
  change touching 9 call sites plus the existing unit test.** → Mitigation:
  mechanical, low-risk change — every call site already has `state` in scope
  (it's cloned into the task closure right next to the `supervised_spawn` call),
  so each site gains exactly one `state.clone()` argument.

## Migration Plan

Additive route + new `AppState` field; no persistence, no data migration. The
`supervised_spawn` signature change is a breaking change to that function's
callers, but all 9 are in this same crate and updated in this change — no external
consumer.

## Open Questions

None.
