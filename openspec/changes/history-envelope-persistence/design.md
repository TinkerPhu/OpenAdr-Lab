## Context

`grid_samples` (SQLite-backed archive, `history_store/schema.rs`, currently `SCHEMA_VERSION = 8`)
already persists tariff/CO2 data per minute via `HistorySampler::record`/`flush`, fed from
`state.planned_tariffs()`. The equivalent for the Dynamic Operating Envelope
(`state.planned_capacity_limits()`, populated since the Controller-tab tariff/envelope split) has
no persistence path at all — `GET /capacity/schedule` only ever answers "what does the schedule
say right now," never "what did it say at 14:32 yesterday." The History tab therefore hardcodes
`importLimitKw`/`exportLimitKw` to `null`.

`pv-curtailment-history` solved a structurally similar problem one level down (per-asset
`tick_samples`, PV curtailment limit) and established the precedent this change follows: don't
average a categorical/intermittent limit, track the tightest value actually observed in the
window instead.

## Goals / Non-Goals

**Goals:**
- Persist the capacity-limit envelope (import/export) into `grid_samples`, one row per minute,
  alongside the tariff fields already there.
- Never let averaging hide a brief, real limit — a capacity event that's active for 10 seconds
  within an otherwise unconstrained minute must still show up in that minute's row.
- History tab renders it with the same `TariffEnvelopeChart`/`GridRatesChart` split the Controller
  tab already uses — one implementation of "how an envelope is drawn," reused by both live and
  historical views.

**Non-Goals:**
- No change to the live `GET /capacity/schedule` endpoint, `parse_capacity_schedule`, or the
  Controller-tab charts — all already correct as of the Controller-split work.
- No backfill of historical data prior to this change; rows written before schema v9 simply have
  `NULL` in the two new columns, same as every prior schema-version column addition in this store
  (e.g. `pv-curtailment-history`'s `tick_samples` columns).
- No per-asset attribution — the envelope is site-wide (matches tariffs; there's no "which asset
  caused this limit" question the way curtailment has a plan-vs-capacity source to distinguish).
- No priority/source tagging on the persisted value. Unlike PV curtailment (which resolves a real
  conflict between the plan's own target and a live capacity cap and needs to remember which one
  won), `parse_capacity_schedule` already resolves multi-event conflicts (strictest-wins,
  priority-ordered) *before* `planned_capacity_limits()` ever sees the data — by the time
  `HistorySampler` reads it, there is exactly one applicable value per instant, not several
  candidate sources to rank.

## Decisions

**1. Extend `grid_samples`, not `tick_samples`.**
The capacity-limit envelope is site-level, exactly like the tariff fields already on
`grid_samples` (`import_tariff_eur_kwh`/`export_tariff_eur_kwh`) — not per-asset like PV
curtailment's `tick_samples` fields. Same table family, same accumulator (`GridAcc`), same query
path (`query_grid`/`GET /history/grid`).
Alternative considered: a new dedicated table for capacity-limit history. Rejected — there's no
independent query pattern for it (always consumed alongside the rest of the grid row, same as
tariffs), so a separate table would only add a join for no benefit.

**2. Tightest-value-wins per window, not a mean — but simpler than curtailment's priority tiers.**
`GridAcc` tracks `import_limit_kw: Option<f64>`/`export_limit_kw: Option<f64>` as the minimum
value observed during the window (`None` if the schedule never had an applicable entry). This
mirrors `pv-curtailment-history`'s "don't average a categorical/intermittent limit" decision, but
without that change's priority-tier bookkeeping (`curtailment_priority`): there's only one source
here (`parse_capacity_schedule`'s own already-resolved schedule), so "tightest value across the
window" is sufficient — there's no second source to rank against it.
Alternative considered: last-value-wins (simplest). Rejected for the same reason
`pv-curtailment-history` rejected it — a brief capacity event firing and clearing within one
minute must not be masked by the rest of the window being unconstrained.

**3. Lookup by applicable interval, same pattern as tariffs.**
`record` gains a `capacity_limits: &[CapacitySnapshot]` parameter (parallel to the existing
`tariffs: &[TariffSnapshot]`), and does the same `.find(|c| c.interval_start <= now && now <
c.interval_end)` lookup tariffs already do — reusing the exact shape, not inventing a new lookup
strategy.

**4. `GridSample`'s new fields are plain `Option<f64>`, no source/reason field.**
Following from Goal/Non-Goal above: nothing else the VEN currently exposes distinguishes *why* a
persisted limit applied (which OpenADR event, what priority) — `OadrCapacityState` itself doesn't
track it either. Adding that here would be new scope beyond "persist the envelope," not implied by
it. If a future need for per-event provenance in history emerges, it's a separate change.

## Risks / Trade-offs

- [New rows accumulate two more `Option<f64>` lookups per 1s tick] → Same cost class as the three
  existing tariff lookups; `capacity_limits` is a small (\<10 typically) `Vec`, linear scan is
  already how tariffs work at this scale.
- [Schema migration on a live table] → `ALTER TABLE ... ADD COLUMN` with no default is a
  metadata-only change in SQLite (no table rewrite), same as every prior `SCHEMA_Vn` step in this
  store; no downtime, no backfill needed for the migration itself.
- [History tab's `TariffTimePoint` merge logic must treat these two new grid-row fields the same
  way it treats tariff fields (LOCF, carry-forward to window edges)] → Already solved once by
  `TariffEnvelopeChart`'s existing `carryForwardLastKnown` helper (built for the Controller tab);
  reused here, not reimplemented.

## Migration Plan

1. Add `SCHEMA_V9` DDL (`ALTER TABLE grid_samples ADD COLUMN import_limit_kw REAL; ALTER TABLE
   grid_samples ADD COLUMN export_limit_kw REAL;`) to `history_store/schema.rs`, bump
   `SCHEMA_VERSION` to 9, add the `if version < 9` branch in `SqliteHistoryStore::migrate`.
2. Extend `GridSample`, `append_grid_sample`, `query_grid` for the two new fields.
3. Extend `GridAcc`/`HistorySampler::record`/`flush` with the tightest-value accumulation,
   test-first (accumulator unit tests before wiring `record`'s new parameter through).
4. Wire the sampler's call site (`tasks/history_sampler/mod.rs`) to pass
   `state.planned_capacity_limits()` alongside the existing `state.planned_tariffs()` call.
5. Swap `History.tsx` to `TariffEnvelopeChart`/`GridRatesChart`, mapping the real fields.
6. Rollback: schema migrations in this store are additive/forward-only (no prior `SCHEMA_Vn` step
   in this codebase has a documented rollback path); reverting the code change leaves the two
   extra columns in place, unused and always `NULL` going forward — harmless, consistent with how
   this store has always handled it.

## Open Questions

None blocking.
