---
title: History Store (VEN local persistence + VTN recorder)
type: component
created: 2026-07-10
updated: 2026-07-10
synced_commit: c5a1d03
sources: [VEN/src/history_store/, VEN/src/controller/history_port.rs, VEN/src/entities/history.rs, VEN/src/tasks/history_sampler/, VEN/src/routes/hems/history.rs, VEN/ui/src/pages/History.tsx, VTN/bff/src/recorder.rs, docs/plans/roadmap/phase-1-data-foundation.md]
tags: [history, sqlite, persistence, vtn, recorder, ports]
---

# History Store (VEN local persistence + VTN recorder)

Phase 1 ("Data Foundation", `docs/plans/roadmap/phase-1-data-foundation.md`) gave the VEN a
durable, queryable record of its own operation, and gave the VTN-side BFF a Postgres
recorder for programs/events/reports/VEN snapshots. Both close the same gap: before this
phase, everything [[ven-ui]] and [[vtn-stack]] showed was either live-only (in-memory,
lost on restart) or resampled forecast/history from [[simulator]] ring buffers, never a
true operational log.

## VEN side: HistoryPort + SqliteHistoryStore

`HistoryPort` (`VEN/src/controller/history_port.rs`) is the domain-facing trait — following
the same port discipline as `SimulatorPort`/`VtnPort`/`SolverPort` in
[[ven-hexagonal-architecture]]: six `append_*` methods (ticks, grid samples, plan
snapshots, events received, reports sent, ledger periods), six matching `query_*` methods,
and `prune_before` for retention. All methods are **synchronous/blocking** — a deliberate
consequence of `rusqlite` not being async; callers on the async side (routes, tasks) go
through `tokio::task::spawn_blocking`. `SqliteHistoryStore`
(`VEN/src/history_store/mod.rs` + `schema.rs`) is the sole real implementation: bundled
SQLite (`rusqlite` `bundled` feature, no system libsqlite dependency), WAL mode, schema
versioned via `PRAGMA user_version` with a v1 migration. A full in-memory fake
(`VEN/src/services/test_support/mock_history_port.rs`) with real filtering backs the unit
tests — the same mock-per-port pattern as the simulator/VTN mocks already in that
directory.

Entity shapes (`VEN/src/entities/history.rs`) follow the project's unit-suffix convention
throughout (`power_kw`, `soc_pct`, `co2_g_kwh`, …), matching `TariffSnapshot` in
[[tariffs-and-capacity]].

## Write path: the history sampler

`VEN/src/tasks/history_sampler/` (split into `mod.rs` + `accumulator.rs` after the combined
file exceeded the 200-line `tasks/` cap — see [[ven-hexagonal-architecture]]'s file-size
section) is a pure, clock-injected accumulator: it buffers per-tick samples and downsamples
to 1-minute resolution before writing, matching the determinism rule in `.claude/CLAUDE.md`
(no wall-clock coupling, no sleeps in tests). Two boundary-crossing helpers,
`day_boundary_crossed` and `month_boundary_crossed`, deliberately differ in their
first-call semantics: day-boundary pruning is idempotent so it may fire on the very first
call after a restart, but month-boundary ledger rollover (`close_ledger_period`,
`rollover_ledger`) must **not** fire on first call — doing so would truncate an
in-progress ledger period that was simply interrupted by a restart, not actually
completed. This rollover is also how BL-16 (asset ledger, `docs/BACKLOG.md`) got resolved:
`GET /ledger?asset_id=` now returns `{ current, closed_periods }` from
`VEN/src/routes/hems/misc.rs`.

Retention is a separate, disableable concern: `HistoryConfig { enabled, retention_days }`
(`VEN/src/profile/schema.rs` + `defaults.rs`) gates whether `main.rs` constructs a
`SqliteHistoryStore` and spawns the sampler at all, and drives `prune_before`'s cutoff.

## Read path: history routes + VEN UI

`GET /history/{ticks,grid,events,reports,plans}` (`VEN/src/routes/hems/history.rs`) share
one `resolve_range()` validator and a `history_range_route!` macro to avoid repeating
query-param parsing five times. One Axum-specific gotcha: literal path segments must be
registered before named-parameter routes with the same prefix, or the router never reaches
the literal branch — `/history/ticks` has to precede `/history/:asset_id`-shaped routes.

`VEN/ui/src/pages/History.tsx` is a new page (TanStack Query hooks, `TextField
type="date"` range picker — no new date-picker dependency) added to the router in
`App.tsx`; it reuses `AssetTimelineChart`/`TariffChart` from [[ven-ui]] rather than
introducing new chart components, since those were already pure-props. BDD coverage:
`tests/features/ven_history.feature` (10 route scenarios + a `@ven-ui` browser check via
`go_history()` in `tests/features/helpers/ui.py`).

## VTN side: the BFF recorder

`VTN/bff/src/recorder.rs` is a new, independent piece: `spawn_recorder(pool, business,
ven_mgr, poll_secs)` polls the VTN (`openleadr-rs`, [[vtn-stack]]) for programs, events,
reports, and VEN snapshots and writes them into a `lab_recorder.*` Postgres schema
(`sqlx`, runtime query API — not compile-time `query!` macros, to avoid `.sqlx`
offline-cache overhead). It is gated on `DATABASE_URL` being set (`VTN/bff/src/config.rs`,
`main.rs`) — if unset, the BFF runs exactly as before, unrecorded. Pagination follows
openleadr-rs's `skip`/`limit` convention (max 50/page,
`fetch_all_pages`); deduplication is a composite-PK `ON CONFLICT DO NOTHING` rather than
upsert logic, since the recorder is a write-once log, not a live mirror.

> **Fixed during Pi4 verification**: `record_ven_snapshots` initially ran on the `business`
> `VtnClient`, which gets a 403 from `/vens` — that endpoint requires the `ven-manager`
> role specifically ([[vtn-stack]]'s dual-credential pattern). Found only by a live
> curl+psql check against the running stack, not by unit tests (the recorder's own tests
> mock the client entirely). Fixed by threading `ven_mgr` through `spawn_recorder`'s
> signature.

## Phase 4 additions: schema v2/v3 + SettingsPort

The one-shot v1 migration became stepwise (`PRAGMA user_version`, apply each
`SCHEMA_Vn` in order — `history_store/mod.rs::migrate`). v2 adds the
`notifications` table ([[notifications]]; persistence code split to
`history_store/notifications.rs` for the 500-line cap). v3 adds `user_settings`
(`(key, asset_id) → value_json`), behind the new **`SettingsPort`** trait
(`controller/settings_port.rs`) — a deliberate *sibling* of `HistoryPort`
rather than an extension: settings are current-state, not time-series, and the
same `SqliteHistoryStore` implements both (`main.rs` hands out two `Arc<dyn …>`
views of one store). First consumer: WP4.2 comfort-curve overrides, re-seeded
into an `AppState` hot map at startup.

## Why this shape

Both halves solve the same problem — "the live view disappears on restart, and nothing
before 'now' is queryable" — but stay decoupled: the VEN never talks to the VTN recorder's
Postgres schema, and the BFF recorder never talks to `HistoryPort`. Each closes its own
`docs/BACKLOG.md` item (VEN side: BL-31/A-1 and BL-16 rollup; VTN side: BL-32/A-2) without
either side depending on the other's storage engine or schema.

For the exact on-disk SQLite schema, time encoding, and docker file location, see
[[history-store-persistence-format]].
