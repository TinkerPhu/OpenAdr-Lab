## Context

Unparked from `docs/plans/forecast-accuracy-idea.md` (parked 2026-08-04). That doc's v2 design
landed on "persist the forecast as it's actually made, then reconcile with the actual once that
moment arrives," wanting both a long-lead-time series (first forecast made for a far-future
instant) and a short-lead-time series (freshest forecast made just before an instant elapsed), but
stalled on: what fixed grid to key persisted forecasts on, and whether base_load should ship with
PV or be deferred. Both are resolved in this discussion.

## Goals / Non-Goals

**Goals:**
- Persist, per plan cycle, the plan's nearest- and farthest-lead forecast for each of PV,
  base_load, and site-residual.
- Reconcile each persisted forecast with the real value once its target instant elapses.
- Expose both series (near, far) alongside the actual for query and UI overlay.

**Non-Goals:**
- No fixed canonical grid across the full planning horizon, and no mid-horizon (e.g. 12h/24h-ahead)
  tracking — deliberately narrowed to exactly two lead-time buckets per the resolved design below.
- No tracking for dispatchable/controllable assets (battery, EV, heater) — see Decision 5.
- No revival of `plan_snapshots` (dropped as dead code in R-63) or any retrospective plan-replay
  mechanism — same "capture at the moment of truth" precedent `pv-curtailment-history` already
  established for a structurally similar problem.
- No new fields on `Plan`/`PlanTimeSlot` — see Decision 2.

## Decisions

**1. Track only two points per cycle — the plan's nearest and farthest slot — not a fixed grid.**
Rejected during this discussion: a fixed `target_ts` grid spanning the whole horizon (the original
open question). A canonical grid was only needed to answer "first + last forecast for every future
instant"; restricting to exactly two lead-time buckets removes the need for a grid entirely — each
row's `target_ts` is just whatever the current plan's own slot-1 or slot-last start time is, no
snapping or cross-cycle alignment required. Write volume drops from ~horizon/grid_step rows/cycle
to a flat 2 rows/asset/cycle (6 rows total for the three tracked assets, every `replan_interval_s`
— ~1,728 rows/day at the 300s default).

Because slot 1's `target_ts` (`now + cum_s[1]`) and the last slot's `target_ts` (`now + horizon`)
both advance by roughly `replan_interval_s` each cycle (since `now` itself advances by that much),
both series naturally land a new row every ~5 minutes at the default cadence, giving a dense,
continuous near-term trace and a dense far-horizon trace — without ever comparing two different
cycles' grids against each other.

**2. Skip `plan.slots[0]` for the "near" point — use `plan.slots[1]`.**
`slot[0]` spans `[now, now + step_s)`: it's the window currently being commanded, not a forecast
about to be tested against reality later. Scoring it would mostly measure "did the dispatcher
execute what it just decided," not forecast quality — the same objection that killed the parked
idea's rejected v1 design (querying the live model for the current instant is a near-tautological
nowcast). `slot[1]` is the first slot that is genuinely still in the future relative to what's
being enacted this cycle.

**3. No new `Plan`/`PlanTimeSlot` fields — reuse what the planner already computes and exposes.**
- PV: `PlanTimeSlot.pv_forecast_kw` already carries exactly "planned PV power for this slot,
  independent of any curtailment decision" (`milp_planner/results.rs`) — the same value
  `pv-curtailment-history` and the live `/forecast` route already treat as PV's forecast. Read
  directly, no new field.
- base_load / site-residual: sampled directly via `AssetHeuristics::sample_kw(target_ts)` for
  `crate::ids::ASSET_BASE_LOAD` and `crate::controller::residual::SITE_RESIDUAL_ASSET_ID` — the
  exact call `services::forecast::build_heuristic_forecasts` already makes to build the live
  `/forecast` API's heuristic-sourced entries. `PlanTimeSlot.baseline_kw` was considered and
  rejected as the source for this: it's `p_base_kw[t] + p_residual_kw[t]` combined
  (`milp_planner/results.rs`), but actuals are recorded per-asset separately in `tick_samples`
  (`base_load` and `site-residual` as distinct `asset_id`s) — sampling each heuristic independently
  keeps predicted and actual comparable per asset, with no decomposition step needed.

**4. Reconciliation reuses `history_sampler`'s existing per-minute flush — no new task.**
`tasks/history_sampler/mod.rs::write_window` already receives one `TickSample` per asset for each
flushed 1-minute window (`ts`, `power_kw`), right where it calls `history.append_tick_samples`.
Extending that same call to also invoke `HistoryPort::reconcile_forecast_actuals(&ticks)` — one
`UPDATE forecast_accuracy_samples SET actual_kw = ?, actual_at = ? WHERE asset_id = ? AND actual_kw
IS NULL AND target_ts >= ? AND target_ts < ?` per tick row, using the same window bounds — closes
the loop with no new background task, no polling, and no separate "has this elapsed yet" check:
reconciliation happens exactly when the real data that would satisfy it already exists.

**5. Scope: PV, base_load, site-residual only — not battery/EV/heater.**
The three tracked assets are precisely the ones that already receive a `ForecastSource::
WeatherModel`/`Heuristic`-tagged entry in the live `/forecast` API (`services/forecast.rs`) rather
than `ForecastSource::Optimization`. Dispatchable assets' "planned" value is a command the VEN
itself issues and (mostly) executes — there's no independent external prediction being tested,
just execution fidelity, a different and already-observable question (dispatch logs, plan
adoption). Extending this table to dispatchable assets was not requested and would conflate two
different kinds of "planned vs. actual."

**6. Written unconditionally every cycle — no change-gating.**
The original design considered gating writes on "did the predicted value change." Superseded: since
both `target_ts` values already advance every cycle (there is no stable identity to compare a new
value against), gating adds complexity for no volume benefit — 6 rows/cycle at the default 300s
cadence is already small enough that unconditional writes need no throttling.

**7. `forecast_accuracy_samples` is pruned by `target_ts`, mirroring `tick_samples`' `prune_before`
semantics.** A row becomes safe to discard once the instant it predicts is old, whether or not it
was ever reconciled (an unreconciled row past the retention window indicates a gap — e.g. the VEN
was offline when that instant elapsed — and there's nothing left to do with it).

## Risks / Trade-offs

- [No mid-horizon (e.g. 12h/24h-ahead) accuracy visibility] → Accepted scope cut, explicit per
  Decision 1 — the two-point design answers exactly the short-lead/long-lead questions that
  motivated this work, not a general lead-time-bucketed accuracy curve.
- [`reconcile_forecast_actuals` runs once per minute per asset regardless of whether any forecast
  row is actually open for that window] → A single indexed `UPDATE ... WHERE actual_kw IS NULL AND
  target_ts BETWEEN ...` against a table with ~1,728 rows/day is inexpensive; no batching or
  skip-if-empty optimization needed at this volume.
- [Reused heuristic sampling for base_load/site-residual duplicates the exact call already made in
  `build_heuristic_forecasts`] → Acceptable duplication of a cheap, pure sampling call, not
  duplicated *modeling* logic; consistent with how `pv-curtailment-history` treated similar
  read-only re-derivation as acceptable when it avoids a new stored/duplicated field.

## Open Questions

None blocking.
