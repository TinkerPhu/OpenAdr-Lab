## 1. Schema migration

- [ ] 1.1 Add `SCHEMA_V9` DDL to `VEN/src/history_store/schema.rs`: `ALTER TABLE grid_samples ADD
      COLUMN import_limit_kw REAL; ALTER TABLE grid_samples ADD COLUMN export_limit_kw REAL;`
- [ ] 1.2 Bump `SCHEMA_VERSION` to 9; add the `if version < 9` branch in
      `SqliteHistoryStore::migrate` (`VEN/src/history_store/mod.rs`)
- [ ] 1.3 Migration-roundtrip test: a store created at v8 migrates cleanly to v9, new columns exist
      and default to `NULL` on pre-existing rows

## 2. `GridSample` + DB write/query path

- [ ] 2.1 Add `import_limit_kw: Option<f64>`, `export_limit_kw: Option<f64>` to `GridSample`
      (`VEN/src/entities/history.rs`)
- [ ] 2.2 Update `append_grid_sample`'s INSERT statement for the two new columns
      (`VEN/src/history_store/mod.rs`)
- [ ] 2.3 Update `query_grid`'s SELECT + row-mapping closure for the two new columns
- [ ] 2.4 Unit test: a `GridSample` with both fields set round-trips through
      `append_grid_sample`/`query_grid` unchanged; a `GridSample` with both `None` round-trips as
      `NULL`, not `0.0`

## 3. Accumulator: tightest-value-wins, not a mean

- [ ] 3.1 Add `import_limit_kw: Option<f64>`, `export_limit_kw: Option<f64>` to `GridAcc`
      (`VEN/src/tasks/history_sampler/accumulator.rs`) — tracks the tightest (minimum) value
      observed in the window, `None` if no capacity event was ever applicable
- [ ] 3.2 Add a `capacity_limits: &[CapacitySnapshot]` parameter to `HistorySampler::record`,
      parallel to the existing `tariffs: &[TariffSnapshot]` parameter; look up the applicable
      entry the same way tariffs do (`.find(|c| c.interval_start <= now && now <
      c.interval_end)`), update `GridAcc`'s tightest-value fields
- [ ] 3.3 Update `flush()` to emit the two new fields on `GridSample`
- [ ] 3.4 Unit tests (test-first, before wiring `record`'s new parameter through the caller):
      a window with one applicable limit persists that value; a window with no applicable limit
      persists `None` for both directions; a window where the applicable interval changes
      mid-window to a tighter value persists the tighter value, not the first-seen or a mean; a
      window with a looser-then-tighter sequence still persists the tightest, order-independent

## 4. Wire the sampler's call site

- [ ] 4.1 In `VEN/src/tasks/history_sampler/mod.rs`, pass `state.planned_capacity_limits()`
      alongside the existing `state.planned_tariffs()` call feeding `HistorySampler::record`
- [ ] 4.2 Confirm (via existing `spawn_history_sampler`-level test, extend if needed) that a
      capacity-limit change flows end-to-end: `state.set_planned_capacity_limits(...)` → next
      `record()` call → flushed `GridSample` carries it

## 5. UI: History tab chart swap

- [ ] 5.1 In `VEN/ui/src/pages/History.tsx`, replace the `TariffChart` import/usage with
      `TariffEnvelopeChart` + `GridRatesChart` (same split as `GridTariffCell`/`GridRatesCell` on
      the Controller page)
- [ ] 5.2 Map `row.import_limit_kw`/`row.export_limit_kw` from the `GET /history/grid` response
      into `TariffTimePoint.importLimitKw`/`exportLimitKw` in the `tariffPoints` builder, replacing
      the current hardcoded `null`
- [ ] 5.3 Update `History.test.tsx` for the two-chart layout; add a fixture asserting a non-null
      historical capacity limit renders on the envelope chart

## 6. Documentation & backlog

- [ ] 6.1 Update `docs/architecture/VEN_ARCHITECTURE.md`/`INTERFACES.md` if `GET /history/grid`'s
      documented response shape needs the two new fields listed
- [ ] 6.2 Append a `docs/history/project_journal.md` entry
- [ ] 6.3 Remove/resolve `docs/BACKLOG.md` BL-44 once implemented and tested (per this project's
      plan-to-docs waving convention)

## 7. Verification

- [ ] 7.1 `wsl cargo check` / `wsl cargo test -p ven-app` locally (`wsl_lock`)
- [ ] 7.2 `cargo fmt --check` and `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] 7.3 `scripts/audit_file_sizes.py`
- [ ] 7.4 VEN UI unit tests + `npm run build`
- [ ] 7.5 Manual check on Node1: a live Controller-tab capacity-limit event, once it rolls into the
      past, shows up correctly on the History tab
