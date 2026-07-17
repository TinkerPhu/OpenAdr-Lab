# Tasks: ven-notification-dedup-viewer

Test-first throughout: each task writes the failing test before the implementation.
Local Rust via `wsl cargo test -p ven-app -j 2` (check free RAM first, one build at
a time); UI via `cd VEN/ui && npm test`; full E2E on Pi4 needs `docker compose build ven`.

## 1. Entity + schema (dedup foundations)

- [x] 1.1 Extend `UserNotification` (`VEN/src/entities/notification.rs`) with
      `dedup_key: Option<String>`, `count: u32`, `last_seen_at: DateTime<Utc>`;
      update constructor + serde; unit tests for serialization (additive fields,
      SCREAMING_SNAKE severities unchanged).
- [x] 1.2 SQLite migration in `history_store`: add `dedup_key`, `count` (default 1),
      `last_seen_at` (backfilled from `created_at`); adapter test proves a
      pre-migration DB migrates and backfills per spec scenario.
- [x] 1.3 Extend `history_store/notifications.rs` `append`/`query` to the new
      columns; add `update_notification_seen(id, count, last_seen_at)`; adapter
      tests (round-trip, update path). Watch the 500-line cap.
- [x] 1.4 Add `update_notification_seen` to `HistoryPort`
      (`VEN/src/controller/history_port.rs`) and to
      `test_support/mock_history_port.rs`.

## 2. Notifier dedup (application layer)

- [x] 2.1 Failing use-case tests in `services/notify.rs`: dedup hit within window
      (count/last_seen_at updated, no new row, SSE re-emit), miss outside window,
      `None` key never dedups, injected-clock determinism — the four
      notification-dedup spec scenarios.
- [x] 2.2 Implement: `notify` gains `dedup_key: Option<String>`; ring scan for a
      key within `DEDUP_WINDOW` (30 min const); update-in-place path
      (`AppState` ring mutation helper in `state/mod.rs`, broadcast re-emit,
      `update_notification_seen` via HistoryPort). All existing callers pass
      `None` — behaviour unchanged; existing tests stay green.
- [x] 2.3 Restart-survival test: ring seeded from store, then a keyed notify
      increments the persisted row (spec scenario "Dedup hit survives restart").

## 3. StorageError producer

- [x] 3.1 Failing test: hot-path history-store write failure (mock returning
      `DomainError::StorageError`) produces one ALERT with
      `dedup_key = "storage-error"`; repeated failures increment count; the
      persist step inside `notify` itself stays log-only.
- [x] 3.2 Wire the producer at the history-sampler/planner-cycle write boundary
      (thread `Notifier` in where needed; keep tasks/ files < 200 production lines).

## 4. History endpoint (adapter)

- [x] 4.1 Failing route tests for `GET /notifications/history`
      (`routes/notifications.rs`): rows beyond ring cap, `severity` filter,
      `400` on invalid severity, `limit` keeps newest rows oldest-first.
- [x] 4.2 Implement route + severity filter in the store query; register in
      `routes/mod.rs`.

## 5. VEN UI

- [x] 5.1 API types + `useNotificationHistory(severity?)` hook
      (`VEN/ui/src/api/`): additive `count`/`last_seen_at`/`dedup_key` fields;
      hook unit test.
- [x] 5.2 `Notifications.tsx` page + route + nav entry: severity filter chips,
      `message ×N` rendering (count > 1 only), first/last-seen timestamps;
      component tests for the four viewer spec scenarios.
- [x] 5.3 Bell: "view all" link in the popover; SSE/id reconciliation — incoming
      row with existing `id` replaces the entry (test: count 3 → 4, length
      unchanged). Update `NotificationsBell.test.tsx`.
- [x] 5.4 `npm test` + `npm run build` + eslint clean in `VEN/ui`.

## 6. E2E + quality gates

- [x] 6.1 BDD scenario "repeated storage failures appear once with a count"
      (inject storage failure via debug/sim hook; assert single ALERT with
      count > 1 via `/notifications/history`). Requires
      `ssh Pi4-Server … docker compose build ven` before running
      `bash run_all_tests.sh --e2e`.
- [ ] 6.2 `wsl cargo fmt --check`, `wsl cargo clippy --all-targets --all-features
      -- -D warnings -j 2`, `python scripts/audit_file_sizes.py`, full
      `wsl cargo test -p ven-app -j 2`.
- [x] 6.3 Docs: journal entry (`docs/history/project_journal.md`); note the new
      endpoint in `docs/architecture/INTERFACES.md` if it lists VEN routes;
      wiki-sync after merge.
