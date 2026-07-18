## 1. Domain + state

- [x] 1.1 Add `entities/report_submission.rs`: `ReportSubmissionRecord { report_name: Option<String>, event_id: Option<String>, client_name: String, vtn_accepted: bool, submitted_at: DateTime<Utc>, error: Option<String> }` with a `new()` constructor, serde-serializable (camelCase wire fields per DTO passthrough).
- [x] 1.2 Test-first: unit test for `ReportSubmissionRecord::new` (serde field names, defaults) in `entities/report_submission.rs`.
- [x] 1.3 Add `state/report_submissions.rs`: bounded `VecDeque<ReportSubmissionRecord>` (cap 100) on `AppState`, `record_report_submission(&self, record)` (evicts oldest past cap) and `report_submissions(&self) -> Vec<ReportSubmissionRecord>` (newest first). Mirror `state/obligations.rs` module style.
- [x] 1.4 Wire the new field into `AppState::new()` and `mod state;` module declarations in `state/mod.rs`.
- [x] 1.5 Test-first: unit tests for ring eviction (>100 entries keeps only the newest 100) and newest-first ordering.

## 2. Routes

- [x] 2.1 `routes/reports.rs::post_reports`: on success, call `record_report_submission` with `vtn_accepted: true`, `error: None`, alongside the existing `reports_sent_total` increment. On failure, call it with `vtn_accepted: false` and `error: Some(format!("{e:#}"))`.
- [x] 2.2 `routes/reports.rs::put_report`: same on both branches.
- [x] 2.3 New handler `get_report_submissions` in `routes/reports.rs` → `Json(ctx.state.report_submissions().await)`.
- [x] 2.4 Register `GET /reports/submissions` in `routes/mod.rs` (before/independent of `/reports/:id` — no path conflict since it's a distinct literal segment... verify ordering doesn't shadow `/reports/:id`).
- [x] 2.5 Test-first: `test_report_submission_marks_vtn_accepted_on_success_and_false_on_failure` — exercise `post_reports`/`put_report` against `MockVtnClient` configured to succeed then fail, assert `GET /reports/submissions` reflects both outcomes in the right order.

## 3. File-size check

- [x] 3.1 Run `python scripts/audit_file_sizes.py` after routes/state changes; if `routes/reports.rs` or `state/mod.rs` crosses the cap, extract further (routes/reports.rs is currently well under 200; unlikely but verify).

## 4. UI

- [x] 4.1 `VEN/ui/src/api/types.ts`: add `ReportSubmission` type (`reportName`, `eventID`, `clientName`, `vtnAccepted`, `submittedAt`, `error`).
- [x] 4.2 `VEN/ui/src/api/client.ts`: add `getReportSubmissions()` fetch fn for `GET /reports/submissions`.
- [x] 4.3 `VEN/ui/src/api/hooks.ts`: add `useReportSubmissions()` (react-query, same polling interval convention as `useReports()`).
- [x] 4.4 `VEN/ui/src/pages/Reports.tsx`: build a lookup (by `reportName`, fallback `eventID`) from `useReportSubmissions()` picking the newest `submittedAt` per key; render an "Accepted"/"Rejected" `Chip` in a new table column (or appended to an existing column) with a tooltip showing `error` on rejection; no chip when no match.
- [x] 4.5 UI unit test in `VEN/ui/src/__tests__/Reports.test.tsx`: accepted chip renders for a matching accepted submission, rejected chip for a matching rejected one, no chip when unmatched.

## 5. Verification

- [x] 5.1 `wsl cargo check -p ven-app` then `wsl cargo test -p ven-app` (new + existing tests green). 682 passed + 1 architecture test passed.
- [x] 5.2 `cd VEN/ui && npm test` (new + existing tests green). 363 passed (33 files).
- [x] 5.3 `cargo fmt --check` and `cargo clippy --all-targets --all-features -- -D warnings` (VEN). fmt found+fixed minor diffs; clippy clean.
- [x] 5.4 `cd VEN/ui && npx eslint .` zero errors (9 pre-existing-pattern warnings, consistent with other pages).
- [x] 5.5 Manual smoke: deployed to Pi4 (`ven-1/2/3` rebuilt + `ui` restarted via scp, per `deploy-pi4` skill) and curl'd both the direct backend port and the real nginx `ui` proxy path a browser uses. `POST /reports` without `eventID` was rejected by the VTN (400) and recorded `vtn_accepted:false` with the VTN's error text; a follow-up submission with `eventID` was accepted (201) and recorded `vtn_accepted:true`. `GET /reports/submissions` (both direct and via `/api/ven-1/...` proxy) returned both records newest-first. No headless-browser tool was available in this environment to screenshot the rendered chip itself, but the exact JSON contract the `useReportSubmissions()` hook consumes is now proven live end-to-end, and `Reports.test.tsx` already asserts deterministic chip rendering from this shape.

## 6. Bookkeeping

- [x] 6.1 Record the dormant `append_report_sent`/`/history/reports` finding (never called from production code) as new debt in `docs/reference/TECHNICAL_DEBTS.md` (R-43).
- [ ] 6.2 Update `docs/history/project_journal.md` with a WP-T5 entry.
- [ ] 6.3 Update `docs/plans/ven-ui-transparency.md` WP-T5 row/section to ✅ done, mirroring the WP-T1/WP-T2 write-up style.
