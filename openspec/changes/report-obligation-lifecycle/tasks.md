## 1. GB-23 — Drop report obligations on confirmed VTN 404 (D1, D2)

- [x] 1.1 Write a failing unit test in `VEN/src/vtn.rs` asserting `http_error`'s returned
      `anyhow::Error` downcasts to a `VtnHttpError` (or equivalent) exposing the numeric HTTP
      status, for at least a 404 and a 409 case.
- [x] 1.2 Implement `VtnHttpError` in `VEN/src/vtn.rs` and change `http_error` to construct it
      (keep the existing `Display` message format so all current assertions on error text keep
      passing) — per design.md D1.
- [x] 1.3 Write a failing unit test in `VEN/src/services/test_support/mock_vtn.rs` /
      `MockVtn` allowing an injected error that carries a specific HTTP status (not just an
      opaque string), so obligation-service tests can simulate a 404 specifically (distinct from
      today's generic `with_upsert_error(&str)`).
- [x] 1.4 Implement the `MockVtn` status-carrying error injection from 1.3.
- [x] 1.5 Write a failing unit test in `VEN/src/services/obligation.rs`: a due obligation whose
      `upsert_report` call returns a 404 is removed from `AppState`'s report-obligation state
      (not present in `state.report_obligations()` afterward), and no error-level retry state
      (`due_at` unchanged) persists.
- [x] 1.6 Write a failing unit test: two obligations sharing an `event_id`, only one 404s — the
      other remains due, unaffected (per design.md D2 / spec scenario "A 404 on one obligation
      does not remove sibling obligations").
- [x] 1.7 Write a failing unit test: a non-404 failure (e.g. connection error, 500) leaves the
      obligation in state with `due_at` unchanged — confirms this change does not alter existing
      non-404 retry behavior.
- [x] 1.8 Add `AppState::remove_obligation(id: Uuid)` to `VEN/src/state/obligations.rs`
      (mirrors `rearm_obligation`'s existing shape) with its own unit test.
- [x] 1.9 Implement the 404-detection branch in
      `ObligationService::check_and_report` (`VEN/src/services/obligation.rs`): on a 404
      (downcast per 1.2), log at `info!` (not `error!`) with `obligation_id`, `event_id`,
      `program_id`, call `remove_obligation`, and skip the existing `error!`+propagate path for
      that case only. All tests from 1.5–1.7 pass.
- [x] 1.10 Run `wsl cargo test -j 2 -p ven-app` under `wsl_lock` (see CLAUDE.md `wsl-lock`) —
      full green before moving on. 971 passed, 0 failed.

## 2. R-43 — Wire `append_report_sent` into real report-submission call sites (D3)

- [x] 2.1 Write a failing integration test: submitting a report via `run_measurement_reports`
      results in a `ReportSent` row visible through `HistoryPort::fetch_reports_sent` (or the
      equivalent existing read method backing `GET /history/reports` —confirm exact method name
      in `controller/history_port.rs` during implementation).
- [x] 2.2 Implement the call: `tasks/sim_tick/publish.rs::run_measurement_reports` accepts
      `Option<Arc<dyn HistoryPort>>` (threaded from `tasks/sim_tick/post_lock.rs`'s
      `maybe_run_measurement_reports`, which already has access to it via the same wiring used
      by `poll_events`/`planning`), and calls `append_report_sent` via `spawn_blocking` after
      each successful `vtn.upsert_report`.
- [x] 2.3 Write a failing integration test: a due obligation's report submission via
      `ObligationService::check_and_report` results in a `ReportSent` row.
- [x] 2.4 Implement the call: `check_and_report` accepts `Option<Arc<dyn HistoryPort>>`
      (threaded from `tasks/obligation.rs::spawn_obligation_check`'s caller in `main.rs`, same
      `history_port` value already constructed there), calls `append_report_sent` after each
      successful `upsert_report` in the existing per-obligation loop.
- [x] 2.5 Write a failing test in `VEN/src/routes/reports.rs`: a successful `POST /reports` (and
      separately, a successful `PUT /reports/{id}`) results in a `ReportSent` row via the
      already-available `ctx.history`.
- [x] 2.6 Implement the calls in `post_reports` and `put_report` (`VEN/src/routes/reports.rs`),
      guarded on `ctx.history.is_some()` like other optional-history call sites in this file's
      neighboring routes.
- [x] 2.7 Confirm (unit or route test) that all three call sites are no-ops — not errors — when
      `history` is `None`, per the spec's "No HistoryPort configured" scenario.
- [x] 2.8 Confirm end-to-end that `GET /history/reports` returns non-empty after a real
      submission through at least one of the three paths — as a BDD scenario in
      `tests/features/` if practical (extend an existing reports/obligation feature file rather
      than creating a new one, per this project's convention of extending nearby coverage).
      Written (`tests/features/ven_reporting_out.feature`'s new `@r-43` scenario) but NOT yet
      run — E2E access unavailable this pass (Node1/Node2 occupied); unverified until 3.7/4.
- [x] 2.9 Run `wsl cargo test -j 2 -p ven-app` under `wsl_lock` — full green (see 1.10's run;
      same pass covers all three call sites' unit/integration tests).

## 3. GB-21 — Fix capacity-reservation payload-type strings (D4)

- [x] 3.1 Grep the repo for every occurrence of `IMPORT_CAPACITY_RESERVATION` and
      `EXPORT_CAPACITY_RESERVATION` (source, tests, BDD features, docs, wiki) to get the
      authoritative edit list — design.md's Open Questions section lists the files found during
      design (`VEN/src/controller/reporter.rs`, `VEN/src/controller/openadr_interface.rs`,
      `tests/features/ven_reporting_out.feature`, `tests/features/ven_capacity_reservation.feature`,
      plus docs/wiki) — confirm it's still current, since code may have moved since design time.
      **Finding**: re-verified against `docs/openadr_3_1_specs/2_OpenADR 3.1.0_Definition
      _20250801.md` — `IMPORT_/EXPORT_CAPACITY_RESERVATION` and `IMPORT_/EXPORT_RESERVATION
      _CAPACITY` are two *distinct* spec payload-type enums, not one enum with a typo:
      `*_CAPACITY_RESERVATION` is the *event* payload type (VTN→VEN, "capacity granted"),
      `*_RESERVATION_CAPACITY` is the *report* payload type (VEN→VTN, "capacity requested").
      `openadr_interface.rs::parse_capacity_state` and `ven_capacity_reservation.feature`
      correctly use the event-side name and are NOT part of GB-21's bug — scope narrowed to
      `reporter.rs` (report-payload construction) and the report-context doc/wiki/BDD mentions
      only. See `docs/reference/KEY_LEARNINGS.md`'s new entry for the full reasoning.
- [x] 3.2 Write/update failing unit tests in `VEN/src/controller/reporter.rs` asserting the
      import/export capacity-reservation obligation report payload `type` is exactly
      `IMPORT_RESERVATION_CAPACITY` / `EXPORT_RESERVATION_CAPACITY` (update
      `test_reporter_import_capacity_reservation_from_envelope`,
      `test_reporter_export_capacity_reservation_from_envelope`,
      `test_reporter_capacity_reservation_no_envelope_returns_zero`, and the match-arm string
      literals in `build_measurement_report_for_obligation`).
- [x] 3.3 Update `VEN/src/controller/openadr_interface.rs` if it also matches/constructs these
      strings during obligation extraction — add/update its unit test(s) accordingly.
      **No change needed** — see 3.1's finding; `parse_capacity_state` matches the correctly-
      named event-side enum, not the report-side one GB-21 is about.
- [x] 3.4 Update `tests/features/ven_reporting_out.feature` and
      `tests/features/ven_capacity_reservation.feature` (and their step definitions in
      `tests/features/steps/`, if any hardcode the old strings) to assert the corrected payload
      type strings. Only `ven_reporting_out.feature` changed (report-context scenario);
      `ven_capacity_reservation.feature` is event-context and correctly unchanged (per 3.1).
- [x] 3.5 Update the doc/wiki occurrences found in 3.1
      (`docs/architecture/VEN_ARCHITECTURE.md`, `docs/REQUIREMENTS.md`,
      `docs/reference/FAQ.md`, `docs/plans/strategic_roadmap.md`,
      `wiki/components/openadr-interface.md`, `wiki/concepts/tariffs-and-capacity.md`) to the
      corrected strings — current-state documentation, not a separate follow-up.
      `docs/reference/FAQ.md` had no report-context occurrence (its table is event-context,
      per 3.1) — left unchanged; every other listed file had a report-context occurrence, fixed.
- [x] 3.6 Run `wsl cargo test -j 2 -p ven-app` under `wsl_lock` — full green; run
      `cargo fmt --check` and `cargo clippy --all-targets --all-features -- -D warnings`.
      All three green (971 tests; fmt clean; clippy clean after gating
      `VtnHttpError::new` with `#[cfg(test)]` — it's only called from the
      `#[cfg(test)]`-gated `mock_vtn.rs`, so it was flagged dead code in the
      non-test `ven-app` bin target). Also split `build_domain_params` out of
      `main.rs` into a new `VEN/src/domain_params.rs` — R-43's history-port
      wiring pushed `main.rs` to 507 production lines, 7 over the 500 cap;
      the extraction is a pure move (no behavior change) verified by the same
      test run.
- [ ] 3.7 Full E2E BDD (Node1 or Node2, per `test-host-preference`):
      `bash run_all_tests.sh --e2e`, confirming `ven_capacity_reservation.feature` and
      `ven_reporting_out.feature` pass with the corrected strings.
      **Still blocked (2026-08-12 resume pass)**: Node1/Node2 are now both free, but this
      session's instructions were explicit — commit only, do not push, do not merge to main.
      Both hosts are separate git clones (per `node-docker-hosts-separate-git-clones` memory)
      that only see pushed commits, so E2E can't run against this branch's code without
      pushing it. Left unchecked; not attempted this pass.

## 4. R-41 — Investigate whether the GB-23 fix resolves/de-risks the E2E warn-storm

- [ ] 4.1 With tasks 1–3 merged, run the complete E2E suite
      (`bash run_all_tests.sh --e2e`, under the appropriate `node1-lock`/`node2-lock`) at least
      twice to establish whether the historical 18-scenario-failure pattern (R-41,
      `docs/reference/TECHNICAL_DEBTS.md`) still reproduces.
- [ ] 4.2 While reproducing (or attempting to), capture whether the VTN 409 warn-storm on
      `report_report_name_uindex` still occurs at the same rate, and whether it's now
      accompanied by 404-triggered obligation drops (per GB-23's new `info!` log) rather than
      indefinite retries — correlate log timestamps between VEN and VTN for at least one
      affected scenario.
- [ ] 4.3 Record the finding in `docs/reference/TECHNICAL_DEBTS.md`'s R-41 entry: either (a) mark
      R-41 resolved if the GB-23 fix eliminates the degradation, (b) narrow R-41's remaining scope
      if it measurably reduces but doesn't eliminate it (e.g. down to VEN cache-invalidation-only,
      per its original "investigate... add VEN cache invalidation" note), or (c) leave R-41
      unchanged with a note that GB-23 did not help, citing the evidence from 4.1–4.2.
- [ ] 4.4 If R-41 is resolved or newly scoped, update its priority-queue entry accordingly (do not
      delete the whole `TECHNICAL_DEBTS.md` entry unless fully resolved, per this project's
      partial-completion documentation rule).

## 5. Change closeout

- [ ] 5.1 Confirm all four suites pass: UI unit (n/a — no UI touched by this change, confirm and
      note), `wsl cargo test -p ven-app`, `--e2e`, `--resilience`
      (`bash run_all_tests.sh` full run).
      UI unit: n/a, confirmed — no UI touched. `wsl cargo test -p ven-app`: green, 971/971
      (see 1.10/2.9/3.6). `--e2e`/`--resilience`: still outstanding — blocked by 3.7's
      no-push constraint this pass.
- [x] 5.2 Update `docs/BACKLOG.md` to remove the resolved GB-21 and GB-23 rows (and R-43's
      register line in `docs/reference/TECHNICAL_DEBTS.md`'s Implementation Task List, item 4,
      per its own "remove R-43 from this register" step).
      Done in commit d439d0c — verified still in place this pass (grep for GB-21/GB-23 in
      BACKLOG.md returns nothing; R-43's register line gone from TECHNICAL_DEBTS.md's
      Implementation Task List, item 4).
- [x] 5.3 Add a `docs/history/project_journal.md` entry summarizing what was done, why, and any
      key learnings (e.g. the `VtnHttpError` downcast pattern, if it proves reusable for future
      status-sensitive `VtnPort` call sites).
      Done in commit d439d0c; `docs/reference/KEY_LEARNINGS.md` also carries the GB-21
      capacity-reservation-vs-reservation-capacity distinction and the `VtnHttpError`
      downcast pattern (search "GB-21"/"GB-23" in that file).
- [ ] 5.4 Once merged and verified, delete this `openspec/changes/report-obligation-lifecycle/`
      change directory (including its `specs/`) per this project's no-lingering-plans workflow
      rule — its content has been waved into current-state docs by 5.2/5.3.
      Not yet — 3.7/4/5.1's E2E confirmation is still outstanding, and this branch hasn't
      merged to main. Per the partial-completion rule, only 5.2/5.3's now-done items were
      checked off; the directory stays until 3.7, 4, and the rest of 5.1 are actually verified.
