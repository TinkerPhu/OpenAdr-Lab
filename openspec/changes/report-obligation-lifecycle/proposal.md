## Why

Four open backlog/debt items (GB-23, GB-21, R-43, and the investigation half of R-41) all sit
in or immediately around the VEN's OpenADR report-obligation and report-submission code
(`services/obligation.rs`, `tasks/sim_tick/publish.rs`, `controller/reporter.rs`,
`tasks/poll_events/`). GB-23 was found live on Node1's production `ven-1` on 2026-08-11: once
a VTN 404s an obligation whose source event/program was deleted, the obligation checker retries
it every ~5s forever, spamming ERROR logs until the process is restarted. GB-21 was found the
same day, one code path away, while scoping BASELINE report work: two capacity-reservation
payload-type strings don't match the openleadr-rs wire schema, so they'd be rejected outright by
a spec-strict VTN. R-43 leaves `GET /history/reports` permanently empty because no real
submission call site ever calls `append_report_sent`. R-41 is longstanding E2E flakiness
correlated with the same "VEN keeps reporting against a VTN object that's gone" failure class as
GB-23.

## What Changes

- `services::obligation::ObligationService::check_and_report` translates a VTN 404 on
  `upsert_report` into a domain-level "obligation source gone" case and drops the obligation
  from `AppState`'s in-memory `report_obligations`, instead of leaving `due_at` unchanged so the
  same obligation is retried on the next 5s tick.
- `vtn.rs`'s `http_error` helper carries the numeric HTTP status through the `anyhow::Error`
  chain (via a small downcastable error type) so callers above the HTTP boundary can distinguish
  "not found" from other failure classes without parsing message strings.
- `HistoryPort::append_report_sent` is called from every real report-submission call site:
  `tasks/sim_tick/publish.rs::run_measurement_reports`, `services/obligation.rs::check_and_report`,
  and `routes/reports.rs`'s `post_reports`/`put_report` handlers — so `GET /history/reports`
  reflects real submissions instead of always returning empty.
- `controller/reporter.rs`'s `IMPORT_CAPACITY_RESERVATION`/`EXPORT_CAPACITY_RESERVATION` payload-type
  string literals (and the obligation payload-type match arms that key off them) are corrected to
  `IMPORT_RESERVATION_CAPACITY`/`EXPORT_RESERVATION_CAPACITY`, matching openleadr-wire's `ReportType`
  enum. All tests/BDD scenarios asserting the old (wrong) names are updated to match.
- Investigation task (not a guaranteed fix): after the GB-23 fix lands, re-run the full E2E suite
  (`bash run_all_tests.sh --e2e` on Node1/Node2) with the pre-existing R-41 warn-storm scenario and
  record whether obligation-404-drop resolves or measurably reduces the 409-churn/event-visibility
  degradation. If it does not fully resolve R-41, the remaining gap (VEN event-cache invalidation
  on upstream-deleted objects, or cleanup draining VEN caches) stays open in
  `docs/reference/TECHNICAL_DEBTS.md` as its own item — this change does not commit to closing R-41
  outright.

## Capabilities

### New Capabilities
- `report-obligation-lifecycle`: how the VEN keeps its in-memory report-obligation state
  consistent with the VTN's actual event/program state — specifically, dropping an obligation
  once its source is confirmed gone (VTN 404), rather than retrying indefinitely.
- `report-submission-history`: the VEN's persisted record of report submissions it initiated
  (`ReportSent` rows via `HistoryPort::append_report_sent`), queryable through
  `GET /history/reports`, covering all real submission call sites.

### Modified Capabilities
(none — no existing `openspec/specs/` capabilities to modify; `openspec/specs/` does not yet
exist in this repo)

## Impact

- Affected code: `VEN/src/services/obligation.rs`, `VEN/src/vtn.rs`,
  `VEN/src/controller/reporter.rs`, `VEN/src/controller/vtn_port.rs`,
  `VEN/src/tasks/sim_tick/publish.rs`, `VEN/src/tasks/sim_tick/post_lock.rs`,
  `VEN/src/tasks/obligation.rs`, `VEN/src/routes/reports.rs`,
  `VEN/src/services/test_support/mock_vtn.rs`, `VEN/src/state/obligations.rs`.
- Affected tests: unit tests in each touched file (test-first), plus BDD scenarios under
  `tests/features/` covering obligation reporting, capacity-reservation payload types, and
  `GET /history/reports`.
- No breaking API changes for VTN-facing wire behavior other than fixing the two wrong payload-type
  strings (GB-21) — this is a correctness fix, not a new contract; any consumer relying on the
  previously-wrong string was already broken against a spec-strict VTN.
- No new external dependencies.
