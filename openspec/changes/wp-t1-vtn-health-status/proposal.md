## Why

`GET /health` (`VEN/src/routes/system.rs`) is a hardcoded `"ok"` string regardless of
real state — the VEN UI's health chip is actively misleading, and there is no
endpoint anywhere that tells a resident/operator whether the VEN can actually reach
its VTN, what its current retry/backoff state is, or when its OAuth token expires.
This is WP-T1 of `docs/plans/ven-ui-transparency.md`, second in the build order after
WP-T2 (which this change reuses: the `planner` health component reads WP-T2's
`Plan.solve_status`).

## What Changes

- Add a small in-memory `VtnConnectionStatus` (connected, last_success_ts, last_error,
  current_backoff_s) to `AppState`, written from `poll_events.rs` — the loop already
  designated as the canonical VTN-reachability signal in this codebase (it already
  drives `notify_outage_edge`). No new persistence; process-lifetime state only.
- Add a minimal `storage_ok: bool` to `AppState`, written from `tasks/state_persist.rs`
  on each write attempt's success/failure (today only logged, never queryable).
- Add `VtnClient::token_expires_at()` — a wall-clock `DateTime<Utc>` derived from the
  existing `Token { acquired_at: Instant, expires_in_secs }`.
- New route `GET /vtn/status` → `{connected, last_success_ts, last_error,
  current_backoff_s, token_expires_at}`.
- Rewrite `GET /health` from a plain string to `{status: "ok"|"degraded",
  components: {ven_process, vtn_connection, storage, planner}}`. `status` is the
  worst of the four components; HTTP stays 200 unless `ven_process` itself is down
  (it never is, by construction — the handler running proves the process is up), so
  existing `curl --fail` healthchecks keep passing unchanged during a VTN outage the
  poll loop is already retrying through.
- UI: Dashboard connection widget (green/amber/red) reading the new shape; replace
  the existing misleading health chip's assumption that `/health` is always `"ok"`.

## Capabilities

### New Capabilities
- `ven-health-status`: the VEN reports its own process/VTN-connection/storage/planner
  health as a structured, componentised status instead of a hardcoded string, and
  exposes VTN-connection detail (backoff, last error, token expiry) on a dedicated
  endpoint.

### Modified Capabilities
(none — this is new observability surface; no existing spec governs `/health`'s
current trivial behavior)

## Impact

- **VEN** (Rust): `state/mod.rs` (new `VtnConnectionStatus` struct + fields/methods),
  `tasks/poll_events.rs` (write connection status at the existing success/failure
  branches), `tasks/state_persist.rs` (write storage_ok), `vtn.rs` (new
  `token_expires_at` accessor), `routes/system.rs` (rewritten `health` handler, new
  `vtn_status` handler), `routes/mod.rs` (register `/vtn/status`).
- **VEN UI**: Dashboard connection widget; `api/types.ts`/`api/client.ts` additions.
- **Non-goals**: no change to the poll loop's actual retry behavior, no change to
  `poll_programs.rs`/`poll_reports.rs` (per-resource connection tracking is out of
  scope — `poll_events.rs` is the existing single canonical reachability signal in
  this codebase; splitting per-resource is a future WP if ever needed, not invented
  here speculatively). No Dashboard rebuild (WP-T8) — this WP only makes the data
  available and updates the existing health chip's rendering, not the full
  traffic-light redesign from the plan doc's §3.3.
- No VTN, BFF, or openleadr-rs changes.
