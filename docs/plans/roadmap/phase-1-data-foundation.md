# Phase 1 — Data Foundation

> **Goal:** persistent usage history in every VEN (SQLite) and a report/event archive
> on the VTN side (Postgres). Everything in Phases 3–5 reads from these two stores.
> **Items:** NEW A-1 (VEN history store), BL-“log for past” (history UI), BL-16
> (AssetLedger), NEW A-2 (VTN recorder).
> **Exit demonstration:** restart the VEN container, open the UI, and browse
> yesterday's per-asset power, grid exchange, tariffs, received events and sent
> reports. VTN side: query `lab_recorder.reports_received` for the same day.
> **Total effort:** ~3–4 weeks. **Start WP1.1+WP1.2 first — Phase 5 heuristics need
> multi-week accumulated history, so the write path should be live ASAP.**

## Design decisions (fixed for this phase)

- **VEN store:** SQLite via `rusqlite` with the `bundled` feature (vendored C sqlite —
  no cmake/system dependency issue on WSL or Pi4 ARM). Pin semver range; MIT licence OK.
  Rejected: `sqlx-sqlite` (large async dep tree for a background write path),
  CSV/Parquet (no ad-hoc SQL for KPI analysis).
- **Access pattern:** `HistoryPort` trait following the `SolverPort` precedent
  (`VEN/src/controller/solver_port.rs`): trait defined in the inner ring, adapter in
  infra, mock in `services/test_support/mock_history_port.rs`. All writes go through a
  single background task on a dedicated thread (rusqlite is blocking — never call it
  from an async context directly; use `tokio::task::spawn_blocking` or a channel to a
  worker thread).
- **Resolution:** 1-second monitor ticks are downsampled to **1-minute means** before
  insert (~650 k rows/90 days vs. 39 M raw). Raw ticks stay in the existing in-memory
  ring buffers; the DB is for history beyond process lifetime.
- **Location:** one file per VEN, `/data/history.sqlite`, docker volume per VEN service
  so it survives rebuilds. Schema versioning via `PRAGMA user_version` + idempotent
  migration function at startup.
- **VTN store:** background task **inside the existing Rust BFF** (`VTN/bff/`) — it
  already holds the `any-business` credential and polls the VTN; no new container, no
  fork changes. Writes to the *existing* Postgres instance, separate schema
  `lab_recorder`.

## WP1.1 — `HistoryPort` + SQLite adapter + schema v1 (L)

1. Define the port (suggested: `VEN/src/services/history_port.rs`, mirroring how
   `SolverPort` is consumed by services):
   ```rust
   pub trait HistoryPort: Send + Sync {
       fn append_tick_samples(&self, rows: &[TickSample]) -> Result<(), DomainError>;
       fn append_grid_sample(&self, row: &GridSample) -> Result<(), DomainError>;
       fn append_plan_snapshot(&self, row: &PlanSnapshot) -> Result<(), DomainError>;
       fn append_event_received(&self, row: &EventReceived) -> Result<(), DomainError>;
       fn append_report_sent(&self, row: &ReportSent) -> Result<(), DomainError>;
       fn query_ticks(&self, from: DateTime<Utc>, to: DateTime<Utc>,
                      asset_id: Option<&str>) -> Result<Vec<TickSample>, DomainError>;
       fn query_grid(&self, from: DateTime<Utc>, to: DateTime<Utc>)
                      -> Result<Vec<GridSample>, DomainError>;
       // … query_events / query_reports / query_plans by time range
       fn prune_before(&self, cutoff: DateTime<Utc>) -> Result<u64, DomainError>;
   }
   ```
   Row structs live in `entities/` (they're domain vocabulary), unit suffixes per
   naming rule.
2. Schema v1 (adapter-internal SQL):
   ```sql
   tick_samples    (ts, asset_id, power_kw, soc_pct, temperature_c)
   grid_samples    (ts, import_kw, export_kw, tariff_eur_per_kwh,
                    export_tariff_eur_per_kwh, co2_g_per_kwh)
   plan_snapshots  (created_at, horizon_start, horizon_end, plan_json)
   events_received (received_at, event_id, event_type, payload_json)
   reports_sent    (sent_at, report_type, event_id, payload_json)
   ledger_periods  (asset_id, period_start, period_end, energy_kwh, cost_eur, co2_kg)
   ```
   Indexes on every `ts`/time column. `payload_json` is the raw wire object
   (DTO-passthrough rule) — typed columns only for filter fields.
3. Test-first at adapter-contract level: a temp-file SQLite adapter test per port
   method (`test_append_tick_samples_roundtrip`, `test_prune_before_deletes_only_older`,
   `test_migration_idempotent_on_existing_db`, `test_query_ticks_filters_by_asset`).
4. Implement `VEN/src/history_store.rs` (infra ring, like `vtn.rs`). Keep < 500 lines;
   split `history_store/schema.rs` if needed.
5. Add `MockHistoryPort` to `services/test_support/` (in-memory Vec, `#[cfg(test)]`).

## WP1.2 — Sampler task: 1-min downsampling write path (M)

1. New `VEN/src/tasks/history_sampler.rs` (≤ 200 lines): every second, read the same
   snapshot the monitor uses; accumulate per-asset sums; at each minute boundary
   (injectable clock — test with a stepped fake clock, no sleeps), compute means and
   batch-append via `HistoryPort`.
2. Hook event/report/plan appends at their natural choke points: where
   `openadr_interface` accepts a new event set, where `reporter` submits, where a plan
   cycle completes. Application layer calls the port — inner rings never import the
   adapter.
3. Use-case tests with `MockHistoryPort`: `test_sampler_emits_minute_means`,
   `test_sampler_flushes_partial_minute_on_shutdown`,
   `test_plan_cycle_appends_snapshot`.
4. Failure policy: history writes are best-effort — log-and-continue, never block or
   crash the control loop. Test: mock port returning `Err` doesn't stop the sampler.
5. docker-compose: add `/data` volume per VEN service; profile key
   `history.retention_days` (default 90), `history.enabled` (default true).

## WP1.3 — Retention pruning (S)

Daily (clock-injected) `prune_before(now - retention_days)` inside the sampler task,
plus `PRAGMA wal_checkpoint` after prune. Test: rows older than cutoff removed, newer
kept, called at day boundary exactly once.

## WP1.4 — History routes (M)

`VEN/src/routes/hems/history.rs`: `GET /history/ticks?from=&to=&asset_id=`,
`/history/grid`, `/history/events`, `/history/reports`, `/history/plans`. Time range
capped (e.g. 7 days/request) to bound response size; 1-min data passthrough, no
resampling (consistent with commit bb3bee3's real-slot philosophy). Adapter-contract
tests via the existing route-test pattern with `MockHistoryPort`.

## WP1.5 — VEN UI history view (M) — resolves BL-“log for past”

1. New "History" page: date picker + the existing controller-chart component family
   fed from `/history/*` instead of the live timeline (reuse, don't fork, the chart
   code — check `VEN/ui` timeline components first).
2. Overlay received events and sent reports as markers on the time axis.
3. UI unit tests (`cd VEN/ui && npm test`) for data mapping + date-window logic;
   eslint zero errors.

## WP1.6 — BL-16: AssetLedger rollup (M–L)

1. Domain: `AssetLedger` aggregation over `monitor::record_tick`'s accumulators with
   explicit `period_start`/`period_end` (monthly default, profile-configurable).
2. Rollover: at period end (injected clock), close ledger → `ledger_periods` row via
   `HistoryPort`, reset accumulators. Unit tests: accumulate-across-ticks, reset
   exactly at boundary, totals reconcile with raw `record_tick` sums (the BL-16
   verify condition).
3. Route `GET /ledger?asset_id=` (current open period + closed periods) and a simple
   UI cost table ("what did each device cost this month").

## WP1.7 — NEW A-2: VTN recorder in the BFF (M–L)

1. Read `VTN/bff/src` structure first; add a background poll task (30 s, configurable)
   using the BFF's existing VTN client/auth: fetch all reports + events (+ vens for
   health), upsert into Postgres schema `lab_recorder`:
   ```sql
   reports_received (received_at, report_id, ven_name, report_type, payload_json)
   events_published (seen_at, event_id, event_type, program_id, payload_json)
   ven_snapshots    (ts, ven_name, last_seen, report_lag_s)
   ```
   Dedupe on `(report_id, modificationDateTime)` so re-polls don't duplicate rows.
2. New crate dep in the BFF for Postgres (`tokio-postgres` or `sqlx` — match whatever
   the BFF already uses if anything; pin semver range, licence check).
3. **Pagination caveat:** until Phase 2's WP2.2 lands VEN-side, the recorder must do
   its own `skip`/`limit` loop against the VTN — collections will exceed one page
   quickly once recording runs for days. Build the pagination loop here first; Phase 2
   reuses the pattern.
4. Integration test against the Pi4 stack (behave or a BFF-level test): publish an
   event, submit a report, assert both rows appear in `lab_recorder.*`.
5. Migration: SQL file applied at BFF startup (`CREATE SCHEMA IF NOT EXISTS …`), never
   touching openleadr-rs's own tables.

## Order & risks

```
WP1.1 → WP1.2 → WP1.3   (write path live — let it start accumulating)
      ↘ WP1.4 → WP1.5   (read path + UI)
      ↘ WP1.6           (ledger, after port exists)
WP1.7                    (independent — can run in parallel from day 1)
```

Risks: (a) rusqlite blocking calls leaking into async context — enforce the
worker-thread rule in review; (b) SQLite on a Pi4 SD card — WAL mode + one batch
insert/minute keeps write amplification trivial; (c) BFF structure unknown until
WP1.7 step 1 — timebox a spike before committing the estimate.

Bookkeeping: register A-1/A-2 as BL-31/BL-32 in `docs/BACKLOG.md` (per
`strategic_roadmap.md` §9), mark BL-16 and the “log for past” line resolved when done;
journal + `/wiki-sync` (new component page: history-store).
