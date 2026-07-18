## Why

WP-T5 of `docs/plans/ven-ui-transparency.md` (gap G-5): the VEN's Reports page
shows report definitions polled from the VTN, but whether the VEN's *own*
POST/PUT submission was actually accepted is only visible momentarily via the
submit form's pending/error state, or buried in the raw `reports_sent_total`
Prometheus counter on `/metrics`. There is no per-report, persisted-through-a-
page-refresh signal of "was this submission accepted by the VTN."

## What Changes

- `AppState` gains a small bounded in-memory record of recent report
  submission outcomes (accepted/rejected, timestamp, report identity), keyed
  by the same identity fields already on `OadrReportBody`
  (`reportName`/`eventID`/`clientName`). Mirrors the existing
  `state/obligations.rs` extraction pattern — no new persistent store.
- `routes/reports.rs`'s `post_reports`/`put_report` record an entry on both
  the success and failure branch (alongside the existing `reports_sent_total`
  counter increment).
- New route `GET /reports/submissions` — recent submission outcomes, newest
  first.
- UI: Reports page (`VEN/ui/src/pages/Reports.tsx`) renders a per-row status
  chip (Accepted / Rejected / — no submission yet) by cross-referencing the
  new submissions list against the existing `useReports()` table, by
  `reportName` (falling back to `eventID` when `reportName` is absent).

## Capabilities

### New Capabilities
- `report-submission-status`: tracking and surfacing whether a VEN's report
  submission to the VTN was accepted, per report identity, beyond the
  aggregate `reports_sent_total` counter.

### Modified Capabilities
(none — no existing spec's requirements change; this only adds new surface)

## Impact

- `VEN/src/state/` — new `report_submissions.rs` module, one new field on
  `AppState`.
- `VEN/src/routes/reports.rs` — record submission outcome on both branches.
- `VEN/src/routes/mod.rs` — new route registration.
- `VEN/ui/src/api/{types.ts,client.ts,hooks.ts}` — new type + fetch hook.
- `VEN/ui/src/pages/Reports.tsx` — status chip per row.
- No VTN, BFF, or openleadr-rs change. No OpenADR 3.1 spec constraint — this
  is VEN-local operational visibility, not wire protocol.
