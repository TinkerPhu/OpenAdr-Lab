## Context

Investigation found the plan doc's assumption — "read live from the poll tasks'
shared state" — does not hold today: `Backoff` (`tasks/backoff.rs`) and the `vtn_ok`
flag (`poll_events.rs:147`) are stack-local variables captured in each poll loop's
closure, never written to `AppState`. Similarly `tasks/state_persist.rs` only logs
write failures (`error!("persist write failed: {e:#}")`), with no queryable state.
`VtnClient.token` (`vtn.rs:27`) is a private field on a struct that *is* reachable
from a route handler (`AppCtx.vtn: VtnClient` is concrete, not a trait object), but
has no accessor. So this WP necessarily adds new in-memory shared state — not
persistence (nothing survives restart, matching the plan doc's "no new persistent
stores" principle), just moving state that already conceptually exists from a local
variable to somewhere a route handler can read it.

## Goals / Non-Goals

**Goals:**
- Make `/health` reflect real state instead of a hardcoded string, without breaking
  any existing consumer (`fleet.sh`, Docker healthchecks — all check HTTP status only).
- Expose VTN connection detail (backoff, last error, token expiry) on `/vtn/status`.
- Reuse WP-T2's `Plan.solve_status` for the `planner` health component — no new
  state needed there, it already exists.

**Non-Goals:**
- No per-resource (events/programs/reports) connection tracking — `poll_events.rs`
  is designated the single canonical signal, matching this codebase's existing
  precedent (`notify_outage_edge` is only wired from `poll_events.rs` today).
  Splitting further is a future WP if a concrete need arises, not built speculatively.
- No Dashboard traffic-light redesign (WP-T8, plan doc §3.3) — this WP only makes the
  `/health`/`/vtn/status` data correct and updates the existing health-chip rendering
  to use it, not the full status-row rebuild.
- No change to backoff/retry *behavior* — only visibility into it.

## Decisions

**D1 — `VtnConnectionStatus` lives on `AppState`, written by `poll_events.rs` only.**
```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VtnConnectionStatus {
    pub connected: bool,
    pub last_success_ts: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub current_backoff_s: f64,
}
```
`connected` defaults to `true` (optimistic-until-first-poll, matching the existing
`let mut vtn_ok = true;` convention at `poll_events.rs:147`). Two new `AppState`
methods: `vtn_connection_status()` (read) and a pair of narrow writers
(`record_vtn_poll_success`/`record_vtn_poll_failure`) rather than one generic setter,
so the call sites at `poll_events.rs`'s existing `Ok`/`Err` branches stay a single
line each and can't accidentally set an inconsistent combination (e.g. `connected:
true` with a `last_error` still populated).

Alternative considered: track connection status per-resource (events/programs/
reports) as three separate statuses. Rejected for this WP — no other poll loop
drives outage notifications today, so a single canonical signal matches existing
precedent; three near-identical structs would be speculative symmetry, not a
concrete need.

**D2 — `storage_ok: Arc<RwLock<bool>>` on `AppState`, written by `state_persist.rs`.**
A bare bool, not a struct — `/health`'s `storage` component only needs ok/degraded,
and `state_persist.rs` already logs the detailed error via `tracing::error!`; no
value in duplicating that detail into a second queryable field for this WP's scope.

**D3 — `VtnClient::token_expires_at()` derives wall-clock time from the existing
monotonic `Instant`.**
```rust
pub async fn token_expires_at(&self) -> Option<DateTime<Utc>> {
    let guard = self.token.read().await;
    let token = guard.as_ref()?;
    let remaining = Duration::from_secs(token.expires_in_secs)
        .saturating_sub(token.acquired_at.elapsed());
    Some(Utc::now() + remaining)
}
```
Returns `Some(<now>)`-ish (zero remaining) rather than `None` when already expired —
"expires_at is in the past" is still meaningful information for the UI, and hiding it
behind `None` would be indistinguishable from "no token acquired yet."

**D4 — `/health` response shape, exactly as scoped in
`docs/plans/ven-ui-transparency.md` §4.1:**
```json
{
  "status": "ok",
  "components": {
    "ven_process": { "status": "ok" },
    "vtn_connection": { "status": "degraded", "detail": "backoff 45.2s; last error: connection refused" },
    "storage": { "status": "ok" },
    "planner": { "status": "ok" }
  }
}
```
`ven_process` is always `{"status": "ok"}` — the handler executing at all proves the
process is responsive; kept as its own key (not omitted) so a future finer-grained
check (event-loop lag, memory pressure) has somewhere to report without a shape
change. `vtn_connection` is `degraded` when `!connected`; `detail` is only present
when degraded (backoff value + last error, joined). `storage` mirrors `storage_ok`.
`planner` reads `state.active_plan().await` — `degraded` when
`Some(plan) if plan.solve_status == Infeasible`, `ok` otherwise (including "no plan
yet," which is not a degraded state, just an unstarted one).

Top-level `status` is `"degraded"` if any component is degraded, else `"ok"`. HTTP
status code is **always 200** — confirmed in WP-T2's era of planning (plan doc §5 Q2)
that `fleet.sh`/Docker healthchecks only check the HTTP status code, never the body,
so this is safe; keeping 200 during a VTN outage the poll loop is already retrying
through avoids Docker/`fleet.sh` treating a recoverable, already-being-handled
condition as a reason to restart the VEN container (restarting doesn't fix a VTN
outage).

**D5 — `/vtn/status` is additive, not a duplicate of `/health`'s `vtn_connection`.**
`/health` answers "is anything wrong" (glance-and-go, matches Dashboard use);
`/vtn/status` answers "what exactly, in detail" (`token_expires_at` has no place in
the terse `/health` shape). Both read the same underlying `VtnConnectionStatus`.

## Risks / Trade-offs

- **[Risk] `poll_events.rs` failing silently for its own resource doesn't mean
  `poll_programs`/`poll_reports` are failing too — `vtn_connection` could read `ok`
  while another resource's poll is actually broken.** → Mitigation: acceptable for
  this WP's scope (matches existing `notify_outage_edge` precedent, which has the
  same limitation today); documented as a Non-Goal, not silently assumed away. A
  future WP can extend to a per-resource breakdown if this proves insufficient in
  practice.
- **[Risk] Adding fields to `AppState`/`HemsState`-adjacent structs is a common
  place for lock-ordering bugs (per the file's own INVARIANT comment: never hold two
  locks, never cross an `.await` with a guard held).** → Mitigation: `vtn_connection`
  and `storage_ok` are independent `Arc<RwLock<_>>` fields, each acquired and
  released within a single method body, mirroring the existing pattern for
  `capacity_state`/`planned_tariffs` exactly — no new lock-ordering surface.
- **[Risk] `token_expires_at`'s `Instant`→`DateTime<Utc>` conversion drifts slightly
  under system clock changes (NTP adjustments) since `Instant` is monotonic but
  `Utc::now()` is wall-clock.** → Mitigation: acceptable for a UI-facing
  approximate-expiry display; the actual token refresh logic (`ensure_token`) still
  uses the monotonic `Instant` comparison internally and is unaffected — this
  accessor is read-only observability, not a behavior-affecting path.

## Migration Plan

Additive fields/routes; `/health`'s response shape changes from a bare string to a
JSON object, which is a breaking shape change for any consumer parsing the body as
text — confirmed (plan doc §5 Q2) the only such consumer is
`tests/features/ven_health.feature`'s literal-`"ok"` assertion, updated as part of
this change. No other migration steps; no data migration (nothing persists);
deploy order irrelevant.

## Open Questions

None — the plan-level open questions in `docs/plans/ven-ui-transparency.md` §5 were
resolved before this WP was scoped; the "no other poll loop tracks reachability"
finding above is a scoping decision (D1's alternative-considered), not an open
question.
