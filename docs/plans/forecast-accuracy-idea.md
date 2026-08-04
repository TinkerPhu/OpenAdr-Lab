# Forecast-vs-Actual Accuracy Tracking — Parked Idea

> **Status: PARKED, unfinished.** This is a design discussion with open
> questions, not a scoped plan ready to implement. Do not treat this as
> authoritative or start building from it without re-opening the discussion.
> Parked 2026-08-04 to first pursue a codebase-wide dead-code audit (see
> `docs/reference/TECHNICAL_DEBTS.md`/BACKLOG.md going forward for that
> effort's outcome) — this idea surfaced that `plan_snapshots` is dead code,
> which raised the question of how much more of that exists.

## Origin

Grew out of two earlier discussions on the same day:
1. **BL-40** (`docs/BACKLOG.md`) — should a stale real-measurement feed fall
   back to the learned heuristic instead of the synthetic spike model?
2. The weekday/weekend (2-bucket) vs. per-day (7-bucket) heuristic
   granularity question (`docs/reference/TECHNICAL_DEBTS.md`).

Both discussions were entirely theoretical — no one could actually see how
accurate the learned base-load heuristic or the weather-based PV forecast
are against reality. The user's framing: store the model's *prediction* for
a future moment, then later pair it with the *actual* measured/simulated
value once that moment arrives and is recorded, so accuracy becomes
directly queryable instead of argued from first principles.

## Design evolution during discussion

**v1 (rejected)**: stamp `predicted_kw` onto each `tick_samples` row at
write time, computed by re-querying the *currently live* model
(`AssetHeuristics::sample_kw(ts)` for base_load/site-residual,
`PvInverter.weather_power_kw` for PV) for that same instant.

Problem raised: this only ever measures near-zero-lead-time "nowcast"
accuracy — it can't distinguish "how good is our forecast 2 minutes before
the fact" from "how good is our forecast 48 hours ahead," which is the more
interesting planning-quality question. It also duplicates forecast
computation that already exists elsewhere (`pv_forecast_kw` on
`PlanTimeSlot`) rather than persisting what the system already produces.

**v2 (where discussion stopped)**: persist the forecast *as it's actually
made*, at the time it's made, then reconcile it with the actual once that
moment arrives — mirroring a real forecast-verification workflow. The user
asked for **both**:
- **long-lead-time**: the *first* forecast ever made for a given future
  instant (e.g. up to ~48h ahead, whatever the planning horizon allows).
- **short-lead-time**: the *most recent* forecast made for that instant
  before it elapsed (closest to the moment it actually mattered for
  dispatch).

## What investigation found

- `pv_forecast_kw` (the value already shown alongside actual PV power in the
  History/Controller UI's `AssetTimelineChart` for future points) is
  computed **live, per-API-request**, from the currently-adopted in-memory
  `Plan` (`controller/timeline.rs::build_asset_timeline`, reading
  `PlanTimeSlot.pv_forecast_kw`, `entities/plan.rs`). Nothing is persisted
  today.
- `plan_snapshots` (`history_store/schema.rs`) — a table that would carry
  the *entire* plan, including every slot's `pv_forecast_kw`, at
  `created_at` — **exists in the schema but is dead code**: its only writer
  (`HistoryPort::append_plan_snapshot`) is called from a unit test and the
  mock port, nowhere in the real replan/adoption path
  (`tasks/planning/cycle.rs`, `services/planning.rs`). This alarmed the user
  and triggered the dead-code-audit tangent below.
- Direct precedent against reviving `plan_snapshots` for retrospective
  replay: `openspec/changes/pv-curtailment-history/proposal.md` explicitly
  rejected a plan-snapshot-replay design for a structurally similar problem
  (explaining historical PV curtailment), choosing instead to tag the
  relevant fact live at resolution time and write it straight into
  `tick_samples`. The established pattern in this codebase is "capture at
  the moment of truth," not "reconstruct from replayed snapshots."
- **No existing `base_load_forecast_kw` equivalent** — only PV has a
  forecast value surfaced on `PlanTimeSlot` today. Base-load's heuristic
  forecast is consumed internally by `milp_planner/inputs.rs` as a solver
  input but isn't echoed back out per-slot. Adding "first + last forecast"
  tracking for base_load would need this new field first.
- **Plan slot boundaries are not a stable join key across cycles.** Every
  replan (`replan_interval_s`, default 300s) re-solves from a new `now`,
  and zone widths vary (300s/600s/900s), so the slot covering a given
  absolute timestamp `T` shifts cycle to cycle. A `(asset_id, target_ts)`
  table needs `target_ts` on a **fixed, canonical grid** (e.g. every 5
  minutes), with each plan cycle's covering-slot value decimated across
  however many grid points it spans — not the plan's own variable slot
  grid.

## Open questions, unresolved

1. **Fixed grid granularity** for `target_ts` — 5 minutes? Coarser?
2. **Add `base_load_forecast_kw` to `PlanTimeSlot`** (and the timeline
   route) before base_load can participate at all, or scope v1 to PV only?
3. **Table shape** — sketch discussed but not committed: a table keyed by
   `(asset_id, target_ts)` with `predicted_kw_first`/`predicted_at_first`
   (written once, on first sighting), `predicted_kw_last`/`predicted_at_last`
   (overwritten every cycle while still future), and `actual_kw`/`actual_at`
   (filled in by `tasks/history_sampler` once the real tick lands, matched
   by asset + grid timestamp).
4. **Write volume/retention** — up to ~288 slots × N forecastable assets
   every 300s scanning the full horizon; needs the same kind of retention
   pruning `tick_samples` already has (`prune_before`), not unbounded
   growth.
5. **UI**: user confirmed this should overlay onto the *existing* History
   page's `AssetTimelineChart` (two curves, one diagram) rather than a new
   page — this part of the design is not in question, only the backend
   shape feeding it.

## Next step, when resumed

Re-open discussion at question 1 (fixed grid) and question 2 (base_load
field) — both need a decision before a real implementation plan can be
written. Do not silently pick defaults for either.
