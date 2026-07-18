## Context

`routes/reports.rs`'s `post_reports`/`put_report` already call
`ctx.vtn.upsert_report`/`ctx.vtn.update_report` and know synchronously
whether the VTN accepted the submission (HTTP 2xx vs. an error mapped to
`BAD_GATEWAY`). Today that outcome is discarded after the HTTP response is
sent — the only durable trace is the aggregate `reports_sent_total` counter
(incremented on success only, no failure counter), invisible in the UI
outside `/metrics`.

Separately, `GET /reports` (`state.reports()`) is populated by
`tasks/poll_reports.rs` polling the VTN's `GET /reports?clientName=...`,
which echoes back the VEN's own previously-accepted reports. That list is
fully replaced on every poll cycle — it is not a place to attach transient
submission-attempt metadata without it being wiped on the next poll.

`entities/history.rs::ReportSent` + `HistoryPort::append_report_sent` exist
and are wired to a real SQLite table + `/history/reports` route, but no
production call site ever calls `append_report_sent` today (grep confirms
it's only exercised in tests). Wiring the actual periodic measurement-report
path (`tasks/sim_tick/publish.rs::run_measurement_reports`,
`services/obligation.rs`) into that store is a separate, larger piece of
work — out of scope here (recorded as new debt, see Non-Goals).

## Goals / Non-Goals

**Goals:**
- Make the outcome of a VEN-initiated report submission (via `POST /reports`
  / `PUT /reports/:id`) visible after the fact, not just at the moment of
  the request.
- Keep the mechanism in-memory and bounded, consistent with design principle
  4 of `docs/plans/ven-ui-transparency.md` (no new persistent store where
  in-memory suffices).
- Surface it on the existing Reports page as a per-row chip.

**Non-Goals:**
- Wiring `append_report_sent`/`/history/reports` to real call sites (the
  periodic measurement-report path in `sim_tick/publish.rs` and
  `services/obligation.rs`). That path never increments
  `reports_sent_total` either today and is a materially bigger piece of
  work (multiple call sites, persistence semantics). Recorded as new debt
  in `docs/reference/TECHNICAL_DEBTS.md`.
- Any change to the VTN, BFF, or openleadr-rs.
- Persisting submission history across a VEN restart — this is operational,
  same-process-lifetime state, same as WP-T1's `VtnConnectionStatus`.

## Decisions

1. **New `state/report_submissions.rs` module, not a field bolted onto
   `PollingState`.** Mirrors `state/obligations.rs`/`state/heuristics.rs` —
   keeps `state/mod.rs` under its 500-production-line cap without another
   file-size-driven refactor mid-WP (WP-T1 already hit this once). Alternative
   considered: extend `PollingState.reports` in place with a
   `vtn_accepted` field merged in at read time — rejected because the whole
   `reports` vec is overwritten wholesale by every poll cycle
   (`state.set_reports`), so any locally-attached annotation would be lost
   within one poll interval regardless of the submission's real outcome.

2. **Bounded `VecDeque<ReportSubmissionRecord>` ring (cap 100), not a
   `HashMap` keyed by report identity.** A ring keeps the most recent N
   attempts regardless of identity collisions or repeated resubmission of
   the same `reportName`, and mirrors the existing notification ring
   (`NOTIFICATION_RING_CAP` pattern in `state/mod.rs`) rather than
   introducing a new eviction strategy. The UI cross-references by
   `reportName`/`eventID`, taking the newest matching entry.

3. **New standalone `GET /reports/submissions` route, not merged into `GET
   /reports`.** Keeps the existing `GET /reports` contract (a straight
   VTN-echo pass-through, already asserted on by BDD/UI tests) completely
   unchanged, and keeps this WP's blast radius to additive-only. Alternative
   considered: enrich each `OadrReport` in the existing response with
   `vtnAccepted` — rejected as a needless coupling between two conceptually
   different things (what the VTN currently has on file vs. what our last
   submission attempt did) and it would require correlating differently
   depending on which came first (poll vs. submission).

4. **Record on both success and failure branches**, with an `error: Option
   <String>` field on failure (using the same `format!("{e:#}")` rendering
   already used for the log line), so the UI can show a reason on hover
   without a second round-trip.

5. **Entity struct lives in `entities/report_submission.rs`**, mirroring
   `entities/notification.rs::UserNotification` — a plain, serializable
   domain type with a `new()` constructor, not `serde_json::Value`.

## API contract

```
GET /reports/submissions
[
  {
    "reportName": "report-evt-1-2026-07-18",
    "eventID": "evt-1",
    "clientName": "ven-1",
    "vtnAccepted": true,
    "submittedAt": "2026-07-18T10:03:00Z",
    "error": null
  },
  ...
]
```
Newest first, capped at 100 entries.

## Risks / Trade-offs

- [Risk] A resubmission under the same `reportName` produces two ring
  entries; the UI must pick the newest match, not assume uniqueness. →
  Mitigation: UI cross-reference explicitly sorts/filters for the latest
  `submittedAt` per key; documented in the UI code comment.
- [Risk] Ring cap (100) could evict a submission before the resident checks
  the chip on a low-traffic Reports page. → Acceptable: matches this WP's
  effort tag (S) and the plan's explicit "no new persistent store"
  principle; a persisted variant is available later by wiring into the
  existing dormant `ReportSent`/`history_store` path (see Non-Goals) if this
  proves insufficient in practice.

## Migration Plan

Additive only: new module, new route, new UI hook + chip. No existing route
response shape changes, no schema migration, no config/env var changes.
Deploy via the standard WP workflow (branch → PR → merge → Pi4 rebuild).
