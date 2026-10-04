# Design

## Context

See proposal.md for motivation. Where the concepts live today:

| Concept | Where it lives today | Used by this change as |
|---|---|---|
| Live plugged state | `EvCharger::state_values` → `"plugged"` 1.0/0.0 (`VEN/src/assets/ev.rs`) | The single source for measured values. The EV asset is the authority. |
| Past timeline values | `simulator/snapshot.rs::to_timeline_snapshot` copies `state_values`; `controller/timeline.rs::resample_to_grid` / `locf_weighted_mean` time-weights every key per bucket; the now-point carries the current values | Unchanged. `plugged` already arrives as a 0..1 fraction. |
| Predicted presence | `EvMilpContext::a_ev`. Since 049/051 it is built from presence facts only: all-false when unplugged with no forecast, all-true when plugged without a deadline session, and otherwise `ev_trip_series::plan_inputs(..).available_per_slot` (the union of the stated sessions' windows plus the tail after a known final return), ANDed with the usage forecast's availability. → `EvMilpParams.a_ev` → `MilpInputs.a_ev` → solver bound `p_ev[t]=0` | Exposed as-is (D1). No second mask. |
| Planned EV state per slot | `planned_state.rs::fill_planned_state` → `asset_port::ev_future_state_at(soc)` → `PlanTimeSlot.planned_state_by_asset["ev"]` → merged verbatim into future timeline points (`controller/timeline.rs`), no other consumer | Extended with `plugged`. |
| Persisted history | `services/history_sampling.rs` accumulator reads `snap.val("soc")` / `"temp_c"` and writes window means → `entities/history.rs::TickSample` → `history_store` `tick_samples` | Gains a `plugged` mean, the same shape as `soc_pct`. |
| Time-range shading | Three run-to-`ReferenceArea` builders: `ui-charts/src/ZoneShading.tsx` (plan zones), `ui-charts/src/DayNightShading.tsx` (proportional alpha, painted via `backgroundAreas`), and `AssetTimelineChart.tsx`'s `classifyPvPoint` + `buildCurtailmentZones` (painted via `extraReferenceAreas`, enabled by `pvCurtailment={assetId === "pv"}`) | The curtailment builder becomes a generic helper in `ui-charts` (D5). Zones and day/night are not point-classified and stay as they are. |
| Value envelopes | `TimeSeriesChart`'s `bands` / `TimeSeriesBandSpec` (min/max y-range `Area`) | Not used. The new helper must not be called "bands" in code. |
| Which state line an asset draws | Two rules: `AssetMidSection.tsx` branches on `assetId`, `History.tsx` infers it from data presence | Consolidated into the per-asset chart declaration. |

## Goals / Non-Goals

**Goals:**
- One function per concept: the measured value from the EV asset, the predicted value from
  the EV's own availability mask, one point-classified shading builder in the chart package.
- Planner behaviour stays byte-identical: `a_ev` is read, not changed.
- Additive wire changes only (nullable column, optional key).

**Non-Goals:**
- An "unknown" style for points without a `plugged` value. They draw no band (accepted).
- PV curtailment shading on the History page. History rows do carry
  `generation_limit_kw`/`curtailment_source`, but `curtailment_source` is a string
  (`"plan"`/`"capacity"`) there and a numeric code in the live timeline. That mismatch gets
  recorded in TECHNICAL_DEBTS.md. The PV declaration finds no data on History and draws
  nothing, which matches today.
- Showing session deadlines or departure times as shading.
- Reconciling slot 0 of the forecast with the live plug state. Today a forecast that says
  "home" keeps slot 0 chargeable even if the car is unplugged right now. That is existing
  planner behaviour and stays.

## Decisions

**D1: The planner's `a_ev` is the predicted presence; expose it, don't add a second mask.**
The earlier draft of this design split a new presence mask off `a_ev`, because on the main
of that day `a_ev` was a charging window that closed at a session deadline. 049 and 051
changed the model before implementation started: a session now states an expected vehicle
use (window, departure, optional return), and `ev_trip_series::plan_inputs` turns the series
into `available_per_slot`, documented there as fact ("may the charger draw power at all").
After a stated departure the car is away, and its SoC drop is booked at the stated return.
A separate mask that stayed "plugged" after a departure would contradict that SoC curve and
duplicate the derivation. So `a_ev` is exposed unchanged.

**D2: The predicted value rides on `planned_state_by_asset`.** `results.rs` passes
`inputs.a_ev` to `fill_planned_state`, which calls `ev_future_state_at(soc, plugged)` and
gets `{"soc", "plugged"}`. The timeline already merges that map into future points.
*Alternative:* a dedicated `PlanTimeSlot` field. Rejected: EV-specific, needs its own merge
path. Old snapshots are fine because the map is open-ended.

**D3: One wire name, `plugged` (1 = plugged), past and future.** Past and future points are
told apart by `ts` vs `now`, the same way PV curtailment tells forecast from history. The
inversion to "unplugged" happens only in the chart's classifier. Storing or sending an
inverted `unplugged` would be a second word for the EV's own state.

**D4: R-73 is already resolved on main.** The dead `future_state_values*` duplicates were
deleted there before this work started, so only `asset_port::ev_future_state_at` exists.

**D5: Persist the window mean as `plugged REAL NULL` (schema v12).** It accumulates exactly
like `soc_pct`: sum plus count for samples where `snap.val("plugged")` is present. `None`
when the asset reports no such key. A fraction, not a boolean, to feed the proportional
shading. v12 is unused on main and on 049.

**D6: One generic point-classified shading helper in `ui-charts`.** New file
`ui-charts/src/StateShading.tsx`, next to `ZoneShading`/`DayNightShading`:
- `StateShadingSpec = { classify(values, isFuture) → { kind, weight } | null, styles: Record<kind, { rgb, alpha, dashedOutline? }>, layer: "background" | "overlay" }`.
- A pure builder turns classified rows into runs. A run splits when kind or weight changes,
  and when the spacing to the next point exceeds a multiple of the typical spacing (gap
  break). The last run extends one step, as curtailment does today.
- A render function returns `ReferenceArea` elements with the weight folded into the rgba
  alpha and `fillOpacity={1}` (recharts defaults to 0.5, which would halve every alpha; same
  handling as `DayNightShading`).
`AssetTimelineChart` takes `shadings?: StateShadingSpec[]` and passes `layer: "background"`
runs to `backgroundAreas` and `layer: "overlay"` runs to `extraReferenceAreas`.
- PV declaration: `classifyPvPoint` and `CURTAILMENT_COLORS` move into it unchanged,
  weight 1, `overlay` (today's stacking).
- EV declaration: `{ kind: isFuture ? "predicted_away" : "unplugged", weight: 1 − plugged }`
  for `plugged < 1`, `null` when `plugged` is 1 or absent; `background`. Colour: the EV blue
  from `ASSET_COLORS.ev`, which stays distinct from the neutral-grey plan-zone shading on
  the future side. Predicted gets a lower alpha plus a dashed outline. If the blue reads
  poorly against the EV's own blue lines in the visual check, switch to a strong navy.
Declarations live in a per-asset chart table next to `ASSET_COLORS`/`ASSET_LABELS` in
`components/controller/types.ts`, together with each asset's `stateKey`. `AssetMidSection`
and `History.tsx` look the asset up there. Neither branches on `assetId` any more
(`declare-dont-branch`). The `pvCurtailment` prop is deleted.

**D7: History page passes `plugged` through.** `HistoryTickSample` gets
`plugged: number | null`, and `ticksByAsset` copies it into `values.plugged` when it isn't
null. Same pass-through as `soc_pct`. No new endpoint.

## Risks / Trade-offs

- [Missing data reads as "plugged"] Pre-migration rows, old plan snapshots and VEN downtime
  draw no band → accepted by the user. The gap break at least stops a band from spanning
  downtime.
- [No usage forecast] The future is all-or-nothing: unplugged now shades the whole horizon,
  plugged now shades nothing → correct consequence of what the VEN knows. Documented in the
  use-case doc.
- [Non-deadline sessions] Opportunistic and budget sessions keep `a_ev` all-true even past
  their stated departure, so no "Predicted away" band appears there → existing planner
  behaviour, recorded in TECHNICAL_DEBTS.md.
- [Run count] A 1-minute grid over 24 h is at most 1440 areas, the bound PV curtailment
  already has. Runs merge equal neighbours, so the usual count is a handful.
- [Variable-step plan zones] A predicted band edge is only as precise as its slot →
  acceptable, that is the plan's actual precision.

## Migration Plan

Schema v12 `ALTER TABLE tick_samples ADD COLUMN plugged REAL;` runs through the existing
versioned migration path. No backfill: old rows read `null`. Rollback: older binaries
ignore the extra column. Deploy order doesn't matter, because the UI tolerates a missing
key.
