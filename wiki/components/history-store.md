---
title: History Store (VEN local persistence + VTN recorder)
type: component
created: 2026-07-10
updated: 2026-10-21
synced_commit: 650b626
sources: [VEN/src/history_store/, VEN/src/controller/history_port.rs, VEN/src/entities/history.rs, VEN/src/tasks/history_sampler/, VEN/src/services/history_sampling.rs, VEN/src/services/forecast.rs, VEN/ui/src/pages/PlanHistory.tsx, VEN/src/routes/hems/history.rs, VEN/ui/src/pages/History.tsx, VTN/bff/src/recorder.rs]
tags: [history, sqlite, persistence, vtn, recorder, ports]
---

# History Store (VEN local persistence + VTN recorder)

The VEN keeps a durable, queryable record of its own operation, and the VTN-side BFF keeps a Postgres
recorder of programs/events/reports/VEN snapshots. Both close the same gap: everything [[ven-ui]] and
[[vtn-stack]] show live is in-memory and lost on restart, and the [[simulator]] ring buffers only hold
about an hour — neither is a true operational log.

## VEN side: HistoryPort + SqliteHistoryStore

`HistoryPort` (`VEN/src/controller/history_port.rs`) is the domain-facing trait, with the same port
discipline as `VtnPort`/`SolverPort` in [[ven-hexagonal-architecture]]: `append_*`/`query_*` method
pairs for ticks, grid samples, events received, reports sent, ledger periods, notifications,
forecast-accuracy samples and plan-quality history, plus `prune_before` for retention. The newer
families (forecast accuracy, plan history) default to a no-op so a history-less test double keeps
compiling. All methods are **synchronous/blocking** — `rusqlite` is not async; callers on the async side
(routes, tasks) go through `tokio::task::spawn_blocking`. `SqliteHistoryStore`
(`VEN/src/history_store/`: `mod.rs`, `schema.rs`, and one file per family — `ticks`, `events`, `reports`,
`notifications`, `settings`, `forecast_accuracy`, `plan_history`) is the sole real implementation:
bundled SQLite (`rusqlite` `bundled`, no system libsqlite), WAL mode, schema versioned by
`PRAGMA user_version` and applied stepwise (`migrate` runs each `SCHEMA_Vn` above the stored version;
currently **v12**). A full in-memory fake (`VEN/src/services/test_support/mock_history_port.rs`) with
real filtering backs the unit tests.

Entity shapes (`VEN/src/entities/history.rs`) follow the project's unit-suffix convention
throughout (`power_kw`, `soc_pct`, `co2_g_kwh`, …), matching `TariffSnapshot` in
[[tariffs-and-capacity]].

## Write path: the history sampler

The 1-minute downsampling itself is a pure, clock-injected accumulator in the application ring
(`VEN/src/services/history_sampling.rs`: it aggregates port snapshots into `TickSample`/`GridSample`
rows, with no scheduling in it). `VEN/src/tasks/history_sampler/` keeps the loop, the
`spawn_blocking` writes and the boundary checks, matching the determinism rule in `.claude/CLAUDE.md`
(no wall-clock coupling, no sleeps in tests). Two boundary helpers, `day_boundary_crossed` and
`month_boundary_crossed`, deliberately differ in their first-call semantics: day-boundary pruning is
idempotent so it may fire on the very first call after a restart, but month-boundary ledger rollover
(`close_ledger_period`, `rollover_ledger`) must **not** fire on first call — that would truncate an
in-progress ledger period merely interrupted by a restart. The rollover is what
`GET /ledger?asset_id=` serves as `{ current, closed_periods }` (`VEN/src/routes/hems/misc.rs`; the
Dashboard lists an asset's closed periods). A write failure is log-and-continue, and a `StorageError`
additionally raises one deduplicated ALERT (`dedup_key` "storage-error") so the resident sees it once,
not per flush.

Retention is a separate, disableable concern: `HistoryConfig { enabled, retention_days }`
(`VEN/src/profile/schema.rs` + `defaults.rs`) gates whether `main.rs` constructs a
`SqliteHistoryStore` and spawns the sampler at all, and drives `prune_before`'s cutoff.

## Read path: history routes + VEN UI

`GET /history/{ticks,grid,events,reports,forecast-accuracy,plans}` (`VEN/src/routes/hems/history.rs`)
share one `resolve_range()` validator (a request spanning more than a bounded number of days is
rejected) and a `history_range_route!` macro, and serve the SQLite archive — distinct from
`/history/:asset_id` (`routes/assets.rs`), which serves the live in-memory ring buffer. Axum registers
literal path segments before named-parameter routes with the same prefix, or the router never reaches the
literal branch. `GET /history/events` returns a row for every newly-seen OpenADR event
(`spawn_event_poll` in `tasks/poll_events/` writes an `EventReceived` row, [[openadr-interface]]).
`GET /history/plans` serves the per-plan-cycle solve-quality rows described below; `PlanSnapshot` (a full
plan blob nothing ever read back) is not stored.

## EV plugged fraction and numeric curtailment source (schema v12)

`tick_samples.plugged` is the mean of an asset's
`plugged` state value over the 1-minute window — for the EV the fraction of the minute it was
plugged in, `NULL` for assets that report no such state and for every older row (deliberately
never 0, which would read as "away"). The same migration turns `curtailment_source` from the
names `'plan'`/`'capacity'` into the live `PvCurtailmentSource::as_f64()` code (1 plan …
5 comms-loss), the vocabulary the live timeline already used: a TEXT column would store a code
as text, so v12 renames the old column, adds a REAL one, converts and drops. The sampler now
keeps the arbiter, manual and comms-loss sources too, so a limit is always stored with its source. The
History page draws both from `GET /history/ticks` (see [[ven-ui]]).

## Forecast accuracy tracking (schema v8)

`forecast_accuracy_samples` persists how well the planner's own forecast held up against what
actually happened, for the two assets whose power is forecast rather than user-commanded —
PV and base_load. Every plan cycle, `services/forecast.rs::
record_forecast_accuracy_samples` (called from `finish_plan_cycle`) writes a *near*
(`plan.slots[1]`) and a *far* (`plan.slots.last()`) sample per tracked asset through
`HistoryPort::append_forecast_samples`; `history_sampler`'s existing 1-minute tick flush then
reconciles each open sample's `actual_kw`/`actual_at` once its `target_ts` has elapsed
(`reconcile_forecast_actuals`). Both are best-effort, log-and-continue writes off the async
runtime via `spawn_blocking`, and both default to a no-op on `HistoryPort` so a history-less
test double keeps compiling without overriding them. `GET /history/forecast-accuracy?from=&
to=&asset_id=&lead_kind=` serves the result; [[ven-ui]]'s History page overlays near (fine
dotted)/far (coarse dashed) forecast lines on those three assets' `AssetTimelineChart` cells,
via the same merged-array mechanism every multi-series chart in the UI is built on (see
[[ven-ui]]'s chart-kit section). Full mechanism: `docs/architecture/VEN_ARCHITECTURE.md`
§4.9a.

**Capacity envelope and headroom columns.** `grid_samples` also carries the Dynamic Operating
Envelope's `import_limit_kw`/`export_limit_kw` (v9; the *tightest* value seen in the minute, `NULL` =
no capacity event applied, not zero) and the site flexibility headroom `up_kw`/`down_kw` (v11; a minute
mean like `import_kw`, `NULL` = no dispatcher tick yet).

## Plan quality history (schema v10, GB-25)

`plan_history` holds one narrow row per plan cycle — trigger, solver milliseconds, solve status,
objective and friction euros, the configured `mip_gap_target`, the cost-component split and the warning
kinds as a comma-joined TEXT column — written by `services/forecast.rs::build_plan_history_sample`
(best effort, off the async runtime) and served by `GET /history/plans`. It is deliberately typed
columns, not a revival of the dropped `plan_snapshots` blob table (`SCHEMA_V7` dropped it).
The VEN UI's **Plan History** page (`VEN/ui/src/pages/PlanHistory.tsx`, route `/plan-history`) plots solver
time and lists each cycle's status and warnings, with the same rolling-24 h/date-pick behaviour as History
and a day range taken from the server clock ([[milp-planner]] for the warnings).

**History page.** `VEN/ui/src/pages/History.tsx` (TanStack Query hooks, `TextField type="date"` range
picker, no date-picker dependency) reuses `AssetTimelineChart`/`TariffEnvelopeChart` from [[ven-ui]]
rather than introducing chart components. BDD coverage: `tests/features/ven_history.feature` (route
scenarios + a `@ven-ui` browser check via `go_history()` in `tests/features/helpers/ui.py`).

## VTN side: the BFF recorder

`VTN/bff/src/recorder.rs` is an independent piece: `spawn_recorder(database_url, business, poll_secs,
status)` polls the VTN (`openleadr-rs`, [[vtn-stack]]) for reports, events and VEN snapshots and writes
them into a `lab_recorder.*` Postgres schema (`reports_received`, `events_published`, `ven_snapshots`;
`sqlx` runtime query API, not compile-time `query!` macros, to avoid `.sqlx` offline-cache overhead). It
never touches openleadr-rs's own tables and is gated on `DATABASE_URL`: unset, the BFF runs unrecorded.
Collections are read through `VtnClient::get_all_pages`, the BFF's one paginated-fetch path; dedup is a
composite-PK `ON CONFLICT DO NOTHING`, since the recorder is a write-once log, not a live mirror. Every
call goes through the BFF's single `bl-client` credential, which also reads `/vens` ([[vtn-stack]]).

The recorder owns its whole lifecycle, including its first connection, so a database problem can never
take down the BFF: it connects with retry and reports `RecorderStatus` (connected, last poll, last
success, consecutive failures, last error) on `/api/health` (ui-transparency; a single failed startup
connection once left it silently dead for nine days). The fleet's per-VEN telemetry has its own table in
the same schema, `lab_recorder.fleet_telemetry` (1-minute rollups kept 90 days, raw 7 days, drops counted
on `/api/health`), described in [[fleet-monitoring]].

## Notifications and settings (schema v2–v4)

The migration is stepwise. v2 adds the `notifications` table ([[notifications]]; persistence in
`history_store/notifications.rs`), v4 its dedup columns (`dedup_key`, `count`, `last_seen_at`: repeats
inside the rolling window collapse into one row). v3 adds `user_settings` (`(key, asset_id) → value_json`)
behind **`SettingsPort`** (`controller/settings_port.rs`) — a deliberate *sibling* of `HistoryPort`:
settings are current state, not time series, and the same `SqliteHistoryStore` implements both
(`main.rs` hands out two `Arc<dyn …>` views of one store). First consumer: comfort-curve overrides,
re-seeded into an `AppState` hot map at startup.

## Why this shape

Both halves solve the same problem — "the live view disappears on restart, and nothing
before 'now' is queryable" — but stay decoupled: the VEN never talks to the VTN recorder's
Postgres schema, and the BFF recorder never talks to `HistoryPort`. Each closes its own
`docs/BACKLOG.md` item (VEN side: BL-31/A-1 and BL-16 rollup; VTN side: BL-32/A-2) without
either side depending on the other's storage engine or schema.

For the exact on-disk SQLite schema, time encoding, and docker file location, see
[[history-store-persistence-format]].
