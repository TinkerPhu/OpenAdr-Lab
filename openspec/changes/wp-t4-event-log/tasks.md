## 1. `AppState`: event log ring + broadcast

- [x] 1.1 Added `EventLogEntry { id, created_at, category, message }` and
      `EVENT_LOG_RING_CAP = 200` in a new `VEN/src/state/event_log.rs`
      submodule (mirrors `state/task_status.rs`).
- [x] 1.2 Added `event_log: Arc<RwLock<VecDeque<EventLogEntry>>>` and
      `event_log_tx: broadcast::Sender<EventLogEntry>` fields to `AppState`
      (channel capacity 64, matching `Notifier`'s).
- [x] 1.3 Added `record_event(now, category, message)`,
      `event_log_snapshot()`, `subscribe_event_log()`.
- [x] 1.4 Unit tests: `record_event_appends_and_broadcasts`,
      `record_event_evicts_oldest_beyond_ring_cap`,
      `event_log_snapshot_returns_oldest_first`.

## 2. Wire the three producer sites

- [x] 2.1 `tasks/backoff.rs::record_fail_sleep` gains the sibling
      `state.record_event(now, "vtn_connection", message).await;` call —
      confirmed necessary: `poll_events.rs` was exactly 200/200 lines with
      zero headroom, so this could not be added at its own call site.
- [x] 2.2 `tasks/state_persist.rs`'s two `Err` branches each gained a
      `state.record_event(..., "storage", ...).await;` call.
- [x] 2.3 `tasks/mod.rs::supervised_spawn`'s two completion branches each
      gained a `state.record_event(..., "task_supervisor", ...).await;` call.
- [x] 2.4 Unit tests: extended `supervised_spawn_restarts_after_panic` to
      assert a `task_supervisor` event log entry naming `test-task`; added
      `record_fail_sleep_records_connection_failure_and_event_log_entry` in
      `backoff.rs`.

## 3. Routes: `GET /events/log` + `GET /events/log/events`

- [x] 3.1 New `VEN/src/routes/event_log.rs`: `get_event_log` (snapshot) and
      `get_event_log_events` (SSE, copying `routes/notifications.rs`'s exact
      broadcast → bounded mpsc → `Sse::new(...)` bridge pattern).
- [x] 3.2 Registered both routes in `VEN/src/routes/mod.rs`.
- [x] 3.3 No separate handler-core unit test added — `get_event_log` is a
      direct passthrough to `ctx.state.event_log_snapshot()` with no
      branching logic, so §1.4's `state/event_log.rs` tests already cover
      the only real logic (ring behavior). Matches the plan's own fallback
      clause ("rely on the state/event_log.rs unit tests instead").

## 4. VEN Rust suite gate

- [x] 4.1 Used `bash scripts/wsl_lock.sh acquire`/`release` around all WSL
      cargo commands in this WP.
- [x] 4.2 `wsl cargo fmt --check` and `wsl cargo clippy --all-targets --all-features -- -D warnings` clean (clippy performed a full recompile after `cargo fmt` reformatted 3 files, confirming the formatting-only changes — reordered imports, line wrapping — introduced no logic regressions).
- [x] 4.3 `wsl cargo test -p ven-app` green: 700/700 passed, run *before*
      `cargo fmt`. **Note**: the post-fmt full re-run was interrupted
      deliberately — see §8 below — rather than re-confirmed end-to-end.
      Treated as sufficient given fmt's changes were whitespace/import-order
      only (confirmed by reading the diffs) and clippy's clean full
      recompile after those changes is strong independent evidence nothing
      broke; flagging the gap explicitly rather than silently treating it as
      equivalent to a full green re-run.
- [x] 4.4 `scripts/audit_file_sizes.py` passed; `tasks/poll_events.rs`
      untouched, still exactly 200/200 as intended.

## 5. UI: Event Log page

- [x] 5.1 Added `EventLogEntry` type to `api/types.ts` and `eventLog()` to
      `api/client.ts`.
- [x] 5.2 New `pages/EventLog.tsx` — table (category chip, message,
      timestamp), newest-first. **Scope decision**: polling via
      `useEventLog()` (10s refetch, matching Metrics/Tasks), not SSE — the
      backend's `/events/log/events` SSE route exists and works, but wiring
      it into the UI is left as a follow-up; polling is consistent with
      every other Diagnostics page in this codebase and avoids adding a new
      client-side streaming pattern for a first cut.
- [x] 5.3 Component test (`__tests__/EventLog.test.tsx`): heading/empty
      state, one row per entry with category+message, and newest-first
      ordering.

## 6. UI suite gate

- [x] 6.1 `cd VEN/ui && npm test` green (370/370 passed).
- [x] 6.2 ESLint zero new errors (one pre-existing, unrelated warning);
      `npx tsc --noEmit` clean.

## 7. Bookkeeping

- [x] 7.1 Marked WP-T4 as done in `docs/plans/ven-ui-transparency.md` §4/§7.
- [x] 7.2 Noted in `docs/history/project_journal.md`: the no-persistence and
      no-`detail`-field scope decisions, the `poll_events.rs` zero-headroom
      constraint, and the SSE-not-wired-into-UI-yet decision.

## 8. Incident during this WP (unplanned)

- [x] 8.1 A second, unrelated resource-contention incident with the same
      other worktree (`.claude/worktrees/034-vtn-report-status`) as WP-T3's
      §8.1 — this time it was building `HiGHS` (a C++ MILP solver) from
      scratch concurrently with this session's `cargo test`/`cargo clippy`,
      despite this session correctly holding the newly-added `wsl_lock` for
      the whole sequence. Free host memory dropped to 1.0 GB (the exact
      floor, not yet critical). Rather than wait for it to worsen, killed
      this session's own redundant post-fmt re-verification test run
      (already had a full green result pre-fmt + a clean clippy recompile
      post-fmt as corroborating evidence) to free memory promptly. Confirms
      the other worktree's session is not honoring `wsl_lock` yet — worth
      the user's attention if it recurs, since the lock's whole point is to
      prevent exactly this.
