## Context

Investigation of `services/notify.rs` confirmed the shape to mirror (bounded ring
+ `broadcast::Sender` + optional persistence via `HistoryPort`) but also confirmed
it should **not** be extended — `Notifier`'s dedup window (30 min) and its role as
a resident-facing feed are exactly the properties an operational diagnostic trail
must not inherit (see `docs/plans/ven-ui-transparency.md` §5 Q1 for the full
frequency/dedup/retention/vocabulary/consumption-pattern reasoning that led to
"Option A: fully separate").

`state/mod.rs` has ~26 lines of headroom under its 500-line cap — enough for the
field additions, not for the accessor methods, so those go in a new
`state/event_log.rs` submodule (same pattern as `state/connection.rs`/
`task_status.rs`). `tasks/poll_events.rs` is at **exactly** 200/200 lines — zero
headroom — so its producer call cannot be added there directly; it's added inside
`tasks/backoff.rs::record_fail_sleep` instead (which already has headroom and is
already the single call site `poll_events.rs` uses for failure-path bookkeeping).

## Goals / Non-Goals

**Goals:**
- A diagnostic trail for VEN-operational failures, viewable as a snapshot and
  streamed live, structurally incapable of colliding with Notifications' ring,
  dedup window, or retention.

**Non-Goals:**
- No persistence (see proposal.md Impact) — in-memory ring only.
- No dedup — every occurrence recorded; bounded by ring capacity only.
- No `poll_programs.rs`/`poll_reports.rs` wiring — they don't use the
  `backoff.rs` helpers this WP hooks into (confirmed: each keeps its own local
  `Backoff` instance, untouched since WP-T1 only wired `poll_events.rs`).
- No UI redesign beyond a new page — full Diagnostics-group nav is WP-T8.

## Decisions

**D1 — Plain `AppState` methods, not a separate `EventLogger` service struct.**
Alternative considered: mirror `Notifier`'s exact shape as a standalone struct
(`EventLogger { tx: broadcast::Sender<EventLogEntry> }`) threaded through
`AppCtx` and every producer function's parameter list, the way `Notifier` is
threaded into `spawn_event_poll` etc. Rejected — every producer site already
receives `state: AppState` (threaded there by WP-T1/WP-T3 for the exact same
reason), so adding `AppState::record_event(...)` costs zero new parameters at any
call site, versus a new service struct requiring `AppCtx` field additions and
sixth/seventh clone captures at every `main.rs` spawn block. `Notifier` is a
separate struct because it predates this WP-T1/T3 pattern and is also used from
places that don't otherwise touch `AppState` as heavily; that historical reason
doesn't apply here.

**D2 — `record_fail_sleep` gains a sibling call, not a merged responsibility.**
`tasks/backoff.rs::record_fail_sleep` already calls
`state.record_vtn_poll_failure(...)`. Adding `state.record_event(...)` right next
to it is two independent calls at one call site (dictated by `poll_events.rs`'s
zero file-size headroom), not one method conflating "current connection status"
with "log of failures" — the distinction the plan doc's §5 Q1 reasoning is about
preserving is between *Notifications and the Event Log*, not between
*`record_vtn_poll_failure` and `record_event`* being called from the same
function body. `record_success` (the happy-path sibling) is **not** touched —
routine successes aren't loggable events.

**D3 — `EventLogEntry` shape: `{id, created_at, category, message}`, no
`detail` field.** The plan doc's original sketch included a `detail` field
alongside `message`. Investigation found every planned producer (connection
failure, storage error, task panic/exit) already has exactly one string worth
recording (`e.to_string()`, `e:?`, or a static description) — no site has a
separate structured "summary vs. detail" split the way `PlanWarning` does
(`message` + `suggested_action`). Splitting into two fields here would be
speculative — added because the sketch had it, not because a producer needs it.
Single `message: String` covers every real call site; extend later if a producer
that genuinely needs two fields shows up.

**D4 — `/events/log` returns the ring snapshot; no separate `/events/log/history`
route.** The plan doc's original sketch had both, mirroring Notifications'
`/notifications` (ring) + `/notifications/history` (persistent store, beyond the
ring). Since this WP ships no persistence (D-non-goal above), there is nothing
for a `/history` variant to read that the ring snapshot doesn't already have —
adding the route now would be a route that always returns the same data as
`/events/log`, which is dead API surface. Add `/events/log/history` only if/when
persistence is added.

## Risks / Trade-offs

- **[Risk] In-memory-only means an Event Log is empty after every restart, right
  when a crash-loop investigation would want history from before the restart.**
  → Mitigation: accepted for a first cut (see Non-Goals); the ring still captures
  everything from process start onward, which covers the common case (an ongoing
  issue, not a resolved one from a previous process lifetime). Persistence is a
  contained follow-up if this proves insufficient.
- **[Risk] `record_fail_sleep` gaining a second call makes it do two things.**
  → Mitigation: documented in D2 — this is a call-site consolidation forced by
  `poll_events.rs`'s file-size cap, not a design merger; the two calls remain
  independently testable and one could be removed without touching the other.

## Migration Plan

Additive fields/routes; no persistence, no data migration. Deploy order
irrelevant.

## Open Questions

None.
