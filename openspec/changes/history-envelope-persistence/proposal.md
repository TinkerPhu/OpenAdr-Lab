## Why

The Controller-tab tariff/envelope split (2026-08-10) added `GET /capacity/schedule` — the
Dynamic Operating Envelope's per-interval import/export capacity limit schedule (OpenADR 3.1
User Guide §8.10.1), distinct from `GET /capacity`'s single current-value scalar — and a
`TariffEnvelopeChart`/`GridRatesChart` split that renders it live on the Controller page. But
`/capacity/schedule` only reflects *currently active* events; nothing persists what limit applied
at a past instant. The History tab's `tariffPoints` builder hardcodes
`importLimitKw`/`exportLimitKw` to `null` and still renders the old combined `TariffChart`, so a
capacity-limit event that has already ended is invisible after the fact — even though it may have
materially constrained what the plan could do during its window. Tariffs don't have this problem:
`grid_samples` (SQLite-backed archive) already carries `import_tariff_eur_kwh`/
`export_tariff_eur_kwh` per minute, fed by `HistorySampler`.

## What Changes

- Add `import_limit_kw`/`export_limit_kw` columns to `grid_samples` (schema v9) and extend
  `GridSample` (`entities/history.rs`) with the two new nullable fields.
- Wire `HistorySampler`/`GridAcc` to accumulate them from `state.planned_capacity_limits()` each
  tick (already populated by `poll_events` since the Controller-split work), using **tightest-value-
  wins within the minute window**, not the tariff mean pattern: a capacity limit is a ceiling that's
  usually absent (no active event), and averaging an active limit against an unlimited state would
  produce a meaningless number. This reuses the same accumulation shape `pv-curtailment-history`
  already established for `tick_samples`' curtailment fields — applied here to the site-wide
  `grid_samples` table instead of a per-asset one.
- `GET /history/grid` (`query_grid`) returns the two new fields once `GridSample` carries them — no
  route-shape change beyond the additional fields.
- VEN UI: `History.tsx` swaps its old combined `TariffChart` for `TariffEnvelopeChart` +
  `GridRatesChart` (both already built for the Controller tab) and maps real
  `row.import_limit_kw`/`export_limit_kw` instead of the hardcoded `null`.

## Capabilities

### New Capabilities
- `capacity-limit-history`: the VEN persists the Dynamic Operating Envelope's per-interval
  import/export capacity limit into long-term history (tightest-value-wins per minute window,
  `None` when no capacity event was active), serves it through the existing grid-history query
  path, and the History tab renders it with the same direct-signal/derived-signal chart split the
  Controller tab already uses.

### Modified Capabilities
(none — additive; `GET /history/grid`'s existing fields and shape are unchanged, only extended)

## Impact

- **VEN** (Rust): `history_store/schema.rs` (schema v9, two new columns on `grid_samples`),
  `history_store/mod.rs` (`migrate`, `append_grid_sample`'s INSERT, `query_grid`'s SELECT/mapping),
  `entities/history.rs` (`GridSample` gains the two fields), `tasks/history_sampler/accumulator.rs`
  (`GridAcc` gains tightest-value accumulation for the two fields, fed from
  `state.planned_capacity_limits()`), `tasks/history_sampler/mod.rs` (`record`/`flush` wiring).
- **VEN UI**: `pages/History.tsx` (swap `TariffChart` → `TariffEnvelopeChart` + `GridRatesChart`,
  map real capacity-limit fields instead of `null`).
- **Non-goals**: no change to the live `/capacity/schedule` endpoint or Controller-tab charts
  (already correct); no change to `grid_samples`' existing tariff/power columns or their
  mean-based accumulation; no retrofit of historical data prior to this change (columns are `NULL`
  for all rows written before schema v9, same as every prior schema-version column addition in
  this store).
- No VTN, BFF, or openleadr-rs changes.
