## 1. Entities

- [x] 1.1 Add `ForecastLeadKind` enum (`Near | Far`) to `VEN/src/entities/history.rs`, with
      `Display`/`FromStr` (or equivalent) for the `"near"`/`"far"` TEXT mapping, mirroring
      `curtailment_source`'s string convention
- [x] 1.2 Add `ForecastAccuracySample` struct: `asset_id: String`, `lead_kind: ForecastLeadKind`,
      `target_ts: DateTime<Utc>`, `predicted_kw: f64`, `predicted_at: DateTime<Utc>`,
      `actual_kw: Option<f64>`, `actual_at: Option<DateTime<Utc>>`
- [x] 1.3 Unit tests: `ForecastLeadKind` round-trips through its string form; both valid values
      parse, an invalid string is rejected

## 2. History store: schema + port

- [x] 2.1 Add schema v8 (`VEN/src/history_store/schema.rs`): `CREATE TABLE
      forecast_accuracy_samples (asset_id TEXT NOT NULL, lead_kind TEXT NOT NULL, target_ts
      INTEGER NOT NULL, predicted_kw REAL NOT NULL, predicted_at INTEGER NOT NULL, actual_kw REAL,
      actual_at INTEGER)` plus `CREATE INDEX idx_forecast_accuracy_target ON
      forecast_accuracy_samples(asset_id, target_ts)`; bump `SCHEMA_VERSION` to 8; add the
      `if version < 8` branch in `migrate()` (`VEN/src/history_store/mod.rs`)
- [x] 2.2 Add to `HistoryPort` trait (`VEN/src/controller/history_port.rs`):
      `append_forecast_samples(&self, rows: &[ForecastAccuracySample]) -> Result<(), DomainError>`,
      `reconcile_forecast_actuals(&self, ticks: &[TickSample], window_s: i64) -> Result<(),
      DomainError>` (per tick row, UPDATE any open sample for that `asset_id` with `target_ts` in
      `[ticks.ts, ticks.ts + window_s)`), `query_forecast_accuracy(&self, from: DateTime<Utc>, to:
      DateTime<Utc>, asset_id: Option<&str>, lead_kind: Option<ForecastLeadKind>) ->
      Result<Vec<ForecastAccuracySample>, DomainError>`. Default no-op / empty implementations for
      test-double compatibility, matching `append_notification`'s pattern
- [x] 2.3 Implement all three on `SqliteHistoryStore` (`VEN/src/history_store/mod.rs`)
- [x] 2.4 Extend `prune_before` to also delete `forecast_accuracy_samples` rows with `target_ts <
      cutoff`
- [x] 2.5 Implement on `MockHistoryPort` (`VEN/src/services/test_support/mock_history_port.rs`)
- [x] 2.6 Unit tests: append then query round-trips predicted-only rows; reconciliation fills
      `actual_kw`/`actual_at` for a matching open row and leaves a non-matching one untouched; a
      reconciled row is not overwritten by a second reconciliation call; `prune_before` deletes rows
      by `target_ts` regardless of reconciliation state; migration test asserts `SCHEMA_VERSION ==
      8`

## 3. Capture: near/far forecast recording per plan cycle

- [x] 3.1 In `VEN/src/services/forecast.rs`, add `record_forecast_accuracy_samples(plan: &Plan,
      heuristics: &HashMap<String, AssetHeuristics>, now: DateTime<Utc>) ->
      Vec<ForecastAccuracySample>`: for `plan.slots[1]` and `plan.slots.last()` (no-op if fewer than
      2 slots), build one sample per point for `ASSET_PV` (from `slot.pv_forecast_kw`),
      `ASSET_BASE_LOAD`, and `SITE_RESIDUAL_ASSET_ID` (each via `heuristics.get(id).map(|h|
      h.sample_kw(slot.start))`, skipped when no heuristic exists yet for that asset)
- [x] 3.2 Wire into `finish_plan_cycle`/`publish_post_cycle_state`: call the builder, then
      `tokio::task::spawn_blocking` to write through `state.history.clone()` when present
      (best-effort — log and continue on failure, same pattern as `history_sampler::write_window`)
- [x] 3.3 Unit tests: a 2+ slot plan yields exactly 6 samples (3 assets × 2 points); a
      single-slot plan yields zero; `plan.slots[0]`'s value is never used as the near point's
      source even when it differs from `slots[1]`; a missing base_load/site-residual heuristic
      omits that asset's samples without affecting PV's

## 4. Reconciliation: hook into the history-sampler flush

- [x] 4.1 In `tasks/history_sampler/mod.rs::write_window`, after `history.append_tick_samples`,
      call `history.reconcile_forecast_actuals(&ticks, window_s)` (best-effort, same log-and-continue
      handling as the existing calls in that function)
- [x] 4.2 Unit test: a flushed window containing an asset with an open forecast sample whose
      `target_ts` falls inside it results in that sample being reconciled after the flush

## 5. Query route

- [x] 5.1 Add `GET /history/forecast-accuracy?from=&to=&asset_id=&lead_kind=` to
      `VEN/src/routes/hems/history.rs`, reusing `resolve_range` and following
      `get_history_ticks`'s shape (asset_id filter) extended with an optional `lead_kind` filter
- [x] 5.2 Wire the route in `routes/mod.rs`
- [x] 5.3 Route tests: valid range returns rows; invalid `lead_kind` value returns 400; disabled
      history store returns 503, matching the existing routes' behavior — covered via
      `tests/features/ven_history.feature` BDD scenarios plus `ForecastLeadKind::from_str`'s entity
      unit tests, matching this route file's existing precedent (only `resolve_range` is unit-tested
      at the route layer)

## 6. VEN UI

- [x] 6.1 Add an API client method + hook for `GET /history/forecast-accuracy` (VEN UI's existing
      history-fetching pattern)
- [x] 6.2 In `AssetTimelineChart.tsx`, overlay the near and far series (two additional lines,
      visually distinct from the actual line and from each other) for the PV, base_load, and
      site-residual cells only
- [x] 6.3 UI unit tests: overlay renders both series when data is present; renders cleanly with no
      overlay when the query returns no rows (asset not yet reconciled, or endpoint unavailable)

## 7. Documentation & cleanup

- [x] 7.1 Update `DOCUMENTATION.md` (new history table, new route, schema version)
- [x] 7.2 Append `docs/history/project_journal.md` entry
- [x] 7.3 Delete `docs/plans/forecast-accuracy-idea.md` — superseded by this change (done when this
      openspec change was created, since its content was fully absorbed into `proposal.md`/`design.md`)

## 8. Verification

- [x] 8.1 `wsl cargo check` / `wsl cargo test -p ven-app` locally (wsl_lock) — 943/943 passing
- [x] 8.2 `cargo fmt --check` and `cargo clippy --all-targets --all-features -- -D warnings` — clean
- [x] 8.3 `scripts/audit_file_sizes.py` — passed
- [x] 8.4 VEN UI unit tests + `npm run build` — 443/443 passing, build + eslint clean
- [ ] 8.5 Node1 E2E + resilience suites green (docker_host_lock) before merge
