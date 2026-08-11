## Context

All four bundled items touch the same narrow slice of the VEN: `services/obligation.rs`
(application ring) drives `check_and_report` off `AppState`'s in-memory `report_obligations`
(populated/retired by `tasks/poll_events/mod.rs` via `add_obligations` /
`retire_obligations_not_in`, keyed by `event_id` against the VTN's `/events?active=true`
response); `controller/reporter.rs` (domain ring) builds the wire payload for each obligation
payload type; `vtn.rs` (infra ring) is the sole place that talks HTTP to the VTN and currently
folds every non-2xx response into a plain `anyhow::Error` string via `http_error()`
(`VEN/src/vtn.rs:85`); `VtnPort::upsert_report` (`VEN/src/controller/vtn_port.rs:25`) returns
`anyhow::Result<()>`, consumed identically by `services/obligation.rs`,
`tasks/sim_tick/publish.rs::run_measurement_reports`, and `routes/reports.rs::post_reports`.

`retire_obligations_not_in` already removes obligations whose `event_id` is absent from the
current `/events?active=true` fetch, on every ~30s poll tick. That is not sufficient by itself:
the obligation checker (`tasks/obligation.rs::spawn_obligation_check`) runs on its own faster
tick (~5s per GB-23's report), so there is always a window — and, per GB-23's live observation,
sometimes an unbounded one — between a VTN-side deletion and the next successful poll_events
cycle clearing the cache. GB-23's own fix direction (a `check_and_report`-local drop-on-404) is
therefore a *direct, unconditional* backstop: it does not depend on poll_events' timing, success,
or on `event_id`-only matching (a program-only deletion that leaves an orphaned event visible in
`/events` would not be caught by `retire_obligations_not_in` at all, only by a 404 on report
submission). R-41's investigation task exists because this same "act on a VTN 404 instead of
retrying blindly" idea is a plausible partial fix for the E2E 409-warn-storm, but that is not
proven and is treated as speculative in this change (see proposal.md).

`entities/error.rs`'s `DomainError` enum is the project's designated boundary type for
cross-layer failures (`error-handling` rule, `docs/guidelines/ERROR_HANDLING.md`), but
`vtn.rs`/`VtnPort` predate consistent use of it here and remain on `anyhow::Result` throughout —
changing every `VtnPort` method to return `Result<_, DomainError>` is out of scope for this
change (see Non-Goals).

## Goals / Non-Goals

**Goals:**
- `check_and_report` stops retrying an obligation forever once the VTN has confirmed (via 404)
  that its source event/program no longer exists.
- The 404 signal is detected structurally (HTTP status), not by string-matching the error
  message — keeps the fix robust to VTN problem-detail wording changes.
- Every real report-submission call site records a `ReportSent` row, so `GET /history/reports`
  reflects reality.
- `IMPORT_CAPACITY_RESERVATION`/`EXPORT_CAPACITY_RESERVATION` payload-type strings match
  openleadr-wire's `ReportType` enum exactly.
- R-41 gets a documented, repeatable verification step (not a promised fix).

**Non-Goals:**
- Rewriting `VtnPort` to return `Result<_, DomainError>` for every method — too large a ripple
  (touches every route/task/mock consuming any `VtnPort` method) for what this change needs; only
  the narrow "was this a 404" signal is added.
- Persisting `report_obligations` across restarts — GB-23's log-spam symptom is fully addressed
  by dropping stale obligations promptly; in-memory-only state remains acceptable (restart also
  clears it, just later and noisily).
- Fixing R-41 outright — scoped as investigation only; if the GB-23 fix doesn't resolve it, the
  remainder stays a separate `TECHNICAL_DEBTS.md` item.
- Any change to `retire_obligations_not_in`'s poll-tick-based cleanup — it stays as a second,
  independent safety net; this change adds a faster, more targeted one at the report-submission
  boundary.

## Decisions

### D1: Carry HTTP status through the error chain via a small downcastable type, not a full DomainError-typed `VtnPort`

`vtn.rs::http_error` currently returns `anyhow::anyhow!("{path} returned {status}: ...")` — a
plain string error with no structured status. Add a small `struct VtnHttpError { status:
StatusCode, path: String }` (implements `std::error::Error` + `Display`, matching the existing
message format) in `vtn.rs`, and construct `http_error` via `anyhow::Error::new(VtnHttpError {
.. })` (optionally `.context(...)` for any RFC7807 detail) instead of `anyhow::anyhow!`. Callers
that need to distinguish "not found" from other failures downcast:
`e.downcast_ref::<VtnHttpError>().map(|e| e.status) == Some(StatusCode::NOT_FOUND)`.

This is the boundary translation the `error-handling` rule calls for ("translate technical
errors to a domain variant at the boundary where they occur") applied narrowly: the *technical*
fact (HTTP 404) is captured structurally right where it's produced (`vtn.rs`), and
`services/obligation.rs` — the place that actually has domain context (an obligation, an
event_id) to decide "this obligation's source is gone" — performs the domain-level
interpretation itself, rather than `vtn.rs` guessing that every 404 anywhere means "drop an
obligation" (a 404 has different domain meaning depending on which endpoint produced it).

**Alternative considered**: change `VtnPort::upsert_report`'s signature to
`Result<(), DomainError>` with a new `DomainError::VtnResourceNotFound { path: String }` variant.
Rejected for this change's scope — it forces every `VtnPort` implementor (`VtnClient`, `MockVtn`,
any future adapter) and every call site (`services/obligation.rs`,
`tasks/sim_tick/publish.rs`, `routes/reports.rs`) to be touched for a fix that only one call site
(`services/obligation.rs`) actually needs to branch on. Left as a documented option if a second
call site later needs the same distinction — at that point promoting `VtnHttpError`'s status into
a real `DomainError` variant becomes worth the wider ripple.

### D2: Obligation removal happens in `check_and_report`, keyed by obligation id, not event_id

On a 404 from `vtn.upsert_report`, `check_and_report` removes that specific obligation (by
`ob.id`) from `AppState` via a new `AppState::remove_obligation(id: Uuid)` accessor in
`state/obligations.rs` (mirrors the existing `rearm_obligation`/`retire_obligations_not_in`
pattern), instead of calling `rearm_obligation` (which would keep it alive for another retry
cycle). This does not touch other obligations sharing the same `event_id` — a 404 on one
obligation's report submission does not by itself prove every obligation on that event is gone
(defense in depth: only remove what's confirmed, not what's inferred).

A 404 is logged once at `info` level (state transition, not an error) instead of `error` level,
since dropping the obligation is the designed response, not a failure needing operator
attention. Non-404 failures keep today's `error!`-level log and unchanged-`due_at` retry
behavior — this is unaffected by this change.

**Alternative considered**: have `check_and_report` also drop every other obligation with the
same `event_id`, on the reasoning that a 404 on one almost always means the whole event is gone.
Rejected: possible but unproven false-positive risk (e.g., a payload-type-specific rejection
unrelated to event deletion) is more consequential than one extra obligation retried for a bit
longer until it, too, hits its own 404. Each obligation is removed only by its own confirmed 404.

### D3: `append_report_sent` call sites pass `Option<Arc<dyn HistoryPort>>`, matching the existing pattern

`tasks/poll_events/mod.rs`, `tasks/planning/{mod,cycle}.rs`, and `services/notify.rs` already
thread `Option<Arc<dyn HistoryPort>>` through to their call sites and no-op when `None` (test/dev
runs without a store). `check_and_report`, `run_measurement_reports`, and
`routes/reports.rs::post_reports`/`put_report` follow the same convention: accept
`Option<Arc<dyn HistoryPort>>` (or thread it in from a caller that already has it — `AppCtx` in
`routes/reports.rs` already carries `ctx.history: Option<Arc<dyn HistoryPort>>`, no new plumbing
needed there), and call `append_report_sent` after a successful `upsert_report`/`update_report`
(blocking call → `tokio::task::spawn_blocking`, matching `history_sampler` and `poll_events`'s
existing pattern for `HistoryPort` calls off the async runtime).

`ReportSent.report_type` is populated from the obligation's `payload_type` /
`OadrReportBody.reportName` (best-available identifier at each call site — `reportName` for the
timer-driven and REST paths, `ob.payload_type` for the obligation path, consistent with what each
site already has in scope). `event_id` comes from the same body/obligation.

**Alternative considered**: add a single central "on any successful report submission" hook
inside `vtn.rs::upsert_report`/`update_report` instead of three separate call sites. Rejected:
`vtn.rs` is the infra ring and has no `HistoryPort` dependency today (a domain/infra
cross-dependency the `ven-architecture` port map doesn't list); the three call sites already sit
in the application/adapter rings where `HistoryPort` is legitimately available and already
threaded for other purposes.

### D4: GB-21 string fix is a pure literal correction, no schema/entity change

`IMPORT_CAPACITY_RESERVATION` → `IMPORT_RESERVATION_CAPACITY`,
`EXPORT_CAPACITY_RESERVATION` → `EXPORT_RESERVATION_CAPACITY` in
`controller/reporter.rs`'s `match payload_type.as_str()` arms (both the match key and the
payload's own `r#type` field it emits) and wherever `OadrReportObligation.payload_type` is
compared against these constants (`controller/openadr_interface.rs`'s obligation extraction, if
it pattern-matches these strings — verified during implementation, not assumed here). No wire
schema types change; `openleadr-wire`'s `ReportType` enum is external (upstream openleadr-rs) and
untouched.

## Risks / Trade-offs

- **[Risk]** Downcasting `anyhow::Error` to detect 404 is inherently coupled to `vtn.rs`
  constructing the error via `VtnHttpError` on every non-2xx path, including the existing 409
  upsert-conflict branch's `anyhow::bail!` calls (`vtn.rs:375-384`), which stay plain string
  errors (they're a different, already-handled case, not 404). If a future change adds another
  non-2xx path in `vtn.rs` using `anyhow::anyhow!`/`anyhow::bail!` directly instead of
  `http_error`, the downcast silently won't find a `VtnHttpError` and the 404 branch simply
  won't trigger (falls through to today's retry behavior — no crash, just missed detection).
  → Mitigation: keep `http_error` as the single non-2xx-response constructor (already true for
  every status-code path in `upsert_report` except the 409 upsert-conflict special case, which
  is intentionally distinct); add a unit test asserting `check_and_report` drops the obligation
  specifically when `MockVtn`'s injected error carries a 404 `VtnHttpError`, so a regression that
  breaks the downcast chain fails a test rather than silently degrading.
- **[Risk]** Obligation removal on 404 could mask a genuinely transient VTN misconfiguration that
  returns 404 in error (e.g., a routing bug) rather than "resource really deleted" — the VEN
  would silently stop reporting instead of alerting.
  → Mitigation: the `info!`-level log on drop still records `obligation_id`, `event_id`, and
  `program_id`, so this stays visible in normal log review (just not as ERROR-level spam); this
  matches how `retire_obligations_not_in` already treats a vanished event today (silent removal),
  so the risk profile is not new, only extended to the report-submission path.
- **[Risk/Trade-off]** R-41's investigation may find the GB-23 fix only partially helps (e.g., the
  warn-storm is dominated by `sim_tick/publish.rs`'s timer-driven reports, which have no
  obligation to drop — they key off `state.events()`, not `report_obligations`).
  → Mitigation: the proposal and tasks explicitly scope R-41 as investigate-and-report, not
  fix-and-close; a negative or partial finding is a valid, complete outcome for this change's
  R-41 task.
- **[Trade-off]** `HistoryPort` calls added to `check_and_report` and `run_measurement_reports`
  add one `spawn_blocking` per submitted report. Existing call sites (`history_sampler`,
  `poll_events`) already accept this cost for the same store; no new pattern, no measured
  concern in this codebase's existing usage (SQLite writes, 5-30s tick cadence).

## Migration Plan

No data migration — `report_obligations` is in-memory only (unaffected by schema), and
`ReportSent`/`append_report_sent`/the `/history/reports` route already exist end-to-end
(`history_store/mod.rs:186`, `routes/hems/history.rs`) — this change only adds call sites, no new
schema. Rollout is a normal code deploy: merge to `main`, deploy per `deploy-node1` skill. No
feature flag needed — GB-23's behavior change (drop on 404 instead of retry) is strictly safer
than today's indefinite retry, and R-43's history rows are purely additive (empty-until-now
endpoint starts returning data). GB-21's string fix could theoretically break a consumer that
depended on the old (wrong) string, but no such consumer exists in this codebase or is known
externally (BASELINE/capacity-reservation obligations against this repo's own openleadr-rs VTN
fork, which per GB-21 doesn't validate the enum strictly — so behavior against it is unaffected
either way). Rollback is a normal revert if `wsl cargo test -p ven-app`, `--e2e`, or
`--resilience` regress after deploy.

## Open Questions

- A repo-wide grep for `CAPACITY_RESERVATION` during this design pass confirms
  `VEN/src/controller/openadr_interface.rs` (obligation extraction) also references these
  strings, alongside `tests/features/ven_reporting_out.feature` and
  `tests/features/ven_capacity_reservation.feature` (BDD), plus several docs
  (`docs/architecture/VEN_ARCHITECTURE.md`, `docs/REQUIREMENTS.md`, `docs/reference/FAQ.md`,
  `docs/plans/strategic_roadmap.md`, `wiki/components/openadr-interface.md`,
  `wiki/concepts/tariffs-and-capacity.md`) — tasks.md's GB-21 task list covers source + BDD;
  doc/wiki occurrences are updated as part of the same task since they document the wire string
  directly (not a separate documentation pass). Cross-check while doing so: R-42
  (`docs/reference/TECHNICAL_DEBTS.md`) also touches `reports_steps.py` (fixed `reportName`
  "TELEMETRY_USAGE") — unrelated string, but same file; keep the GB-21 edit scoped to
  payload-type strings only, don't fold R-42 in.
- R-41's investigation task depends on Node1/Node2 E2E runtime access this change's author does
  not exercise directly (per this change's own constraints — no docker/wsl commands are run
  while authoring these artifacts); its task is written as a concrete, repeatable verification
  step for whoever implements/runs it, not something resolved by this design.
