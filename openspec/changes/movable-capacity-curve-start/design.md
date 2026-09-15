## Context

The sustained-commitment capacity engine (`controller/capacity_headroom.rs`,
`docs/architecture/VEN_ARCHITECTURE.md` §3.0c) treats the capacity question as one
`(t1, t2, direction, tier)` domain and exposes fixed slices of it:

| Slice | Function | Consumer |
|---|---|---|
| `t1 = now`, sweep `t2` | `compute_site_capacity_curve` | dashed curves on Site Headroom, Diagnostics → Capacity Forecast, OpenADR reporter |
| `t2 = 0`, sweep `t1` | `compute_site_headroom_forecast` | Site Headroom band (future half) |
| `t1 = now`, `t2 = 0` | `compute_site_headroom` | Site Headroom band (live/history half) |

This change adds the fourth slice: **arbitrary future `t1`, sweep `t2`**.

Current facts this design builds on:

- `compute_site_capacity_curve` reads each asset's **live** `entry.state` from `SimState` and runs
  `asset_max_power_series` (60 s steps) out to `t2_max` = remaining plan horizon (48 h fallback).
  Base load is added from `BaseLoad::forecast_kw_at(t1 + elapsed)`, hourly. The result is clamped by
  the site's physical grid rating (`profile.grid.max_import_kw` / `max_export_kw`).
- Both directions are recomputed every sim tick (`tick_s: 1` in every profile) in
  `tasks/sim_tick/forecast_wiring.rs`, stored in `AppState`, and served by
  `GET /flexibility/capacity`. The UI polls it every 10 s (`useCapacityCurves`).
- `resolve_plan_state_at(sim, plan, t1, now)` (`simulator/forecast.rs`) returns each asset's
  plan-forecasted state at `t1`. It snaps down to the latest remaining slot boundary. For `t1 <= now`
  it returns the live states, and PV always keeps its live state (curtailment decision not
  forecast). It is unit-tested but `#[allow(dead_code)]`, with no production caller.
- `AppCtx` holds `sim: Arc<Mutex<SimState>>` (tokio mutex, shared with the 1 s tick) and
  `state.active_plan()`. It does **not** hold the grid physical ratings; those are threaded
  only into the tick/planning tasks.
- `TimeSeriesChart` (Recharts) renders a tooltip but exposes no cursor-position callbacks.
- Plan slots come from `plan_zones` (e.g. ven-1: 5 min × 8 h, 10 min × 16 h, 15 min × 24 h).
  The UI already has every remaining slot's start time via `GET /flexibility/forecast`.

## Goals / Non-Goals

**Goals:**
- Show the Import/Export commitment curves starting at a user-chosen future plan slot, driven by
  cursor hover (with double-click to pin), on the Controller's Site Headroom chart.
- Keep a single curve computation: the `t1 = now` path and the future-`t1` path go through the
  same code, differing only in where the per-asset starting states come from.
- Keep the seam property the chart relies on today: a curve starting at `t1` touches the band's
  forecast value at that same slot.
- Bounded, predictable compute: at most one curve pair computed per distinct slot the user visits,
  never a precomputed full `(t1, t2)` grid.

**Non-Goals:**
- Starting a curve in the **past** (no stored per-asset state history to start from).
- Forecasting PV curtailment decisions (inherits `resolve_plan_state_at`'s documented PV scope
  limit).
- Interpolating between plan slots (snaps to slot boundaries, like the resolver).
- Changing the per-tick curves, the Diagnostics Capacity Forecast panel, or the OpenADR reporter.
  All stay anchored at `now`.
- The History page's Site Headroom chart (it has no future window, `hoursForward = 0`).
- A VTN-restricted / contractual tier (still `LimitTier::Physical` only).

## Decisions

### D1. On-demand endpoint, not a precomputed triangle

`GET /flexibility/capacity?start=<RFC3339>` computes both curves when the request arrives.

- *Alternative: precompute a curve per remaining slot each tick.* ~288 × the per-tick curve work,
  every second, on every VEN. Rejected outright.
- *Alternative: precompute per slot once per replan and ship the whole grid to the UI.* Buys
  perfectly smooth sweeping, but costs ~288 curve pairs per replan per VEN (a 20-VEN fleet shares
  Node1/Node2), a large payload, and still goes stale between replans as live state drifts from the
  plan. Rejected: slot-snapping (D3) already makes the on-demand path feel continuous.

Cost estimate (to be confirmed by measurement, see Risks): one future-start curve pair = one plan
trajectory walk per asset (≤ ~288 slot steps) + one `asset_max_power_series` per asset per direction
over the remaining horizon (≤ 2 880 × 60 s steps). That is the same order as, or less than, the
curve pair every VEN already computes every second.

### D2. Same endpoint, optional `start` parameter

Extend `GET /flexibility/capacity` with an optional `start` query parameter rather than adding a
new route.

- Absent, or `start <= now`: return the per-tick stored curves exactly as today (no extra
  compute), plus `start` = the curves' own start.
- `start > now` with an active plan: compute on demand (D4).
- `start > now` without an active plan: return the per-tick `now` curves with `start` = their
  own start. The UI detects that `start` ≠ what it asked for and shows "no active plan, curves
  anchored at now" (see D7).
- `start` past the last remaining slot's start: clamped to that slot's start (the curve would
  otherwise have a zero-length horizon).
- Unparsable `start`: 400. That's a presentation-boundary validation error, not a domain failure.

Response shape stays `{ import: CapacityCurve, export: CapacityCurve }`, plus a top-level `start`
field (the snapped `t1` actually used). Each `CapacityCurve` already carries its own `start`, so
the top-level field is redundant but explicit. It's additive and non-breaking. Field names pass
through unchanged (`dto` rule).

- *Alternative: a separate `/flexibility/capacity/at` route.* Same data shape, same concept.
  A second route would split one UI label ("Import/Export commitment") across two backend
  names (`naming-transparency`). Rejected.

### D3. Snap `t1` to plan slot boundaries, authoritatively on the server

The server snaps via `resolve_plan_state_at`'s existing rule (latest remaining slot boundary
≤ `start`), and it is the only place that does. The UI sends the rested cursor time unsnapped
and renders the `start` the server returns. (A first version also snapped client-side, to key a
per-slot cache. That was a second copy of the same rule, removed under the
one-concept-one-function rule. The cost is one request per cursor rest instead of one per
slot, which the debounce bounds.)

Consequence: the curve moves in 5/10/15-min jumps depending on the plan zone. This matches the
resolution of the state the curve starts from. Finer movement would just redraw the same curve.

### D4. One curve computation, parameterized by starting states

Split `compute_site_capacity_curve` into a core that takes `t1`, `t2_max`, and a per-asset
starting-state source, plus the existing signature as a thin wrapper that passes live states.
The future-start path is a new function next to it:

```text
compute_site_capacity_curves_at(sim, plan, start, now, phys_imp_kw, phys_exp_kw)
    -> (snapped_t1, import_curve, export_curve)
  states  = resolve_plan_state_at(sim, plan, start, now)   // snapped, PV = live
  t2_max  = plan.horizon.end_time - snapped_t1
  core(Import, snapped_t1, t2_max, sim configs, states, ...)
  core(Export, snapped_t1, t2_max, sim configs, states, ...)
```

- Asset **configs** still come from the live `SimState`. That's where PV's weather forecast, the EV's
  departure time, and base load's heuristic live, and each of them is already time-aware by
  timestamp. Only the **states** are swapped. No site-level code re-derives any asset's future
  state; `resolve_plan_state_at` asks each asset's own `simulate_forward`
  (`asset-competence-assurance`).
- The snapped `t1` is returned from `resolve_plan_state_at`'s boundary pick, not recomputed a
  second way. The resolver gains a small sibling (or return value) exposing which boundary it
  picked. The implementation chooses the least invasive shape, as long as it doesn't
  re-implement the snapping.
- Lives in `controller/capacity_headroom.rs` (that module already depends on
  `simulator::forecast`). Production size stays well under 500 lines.
- `now` is an injected parameter (`determinism` rule). The route passes the wall clock.

### D5. Compute under the `SimState` lock, synchronously, no `.await` held

The route takes `ctx.sim.lock().await`, runs D4 synchronously, and drops the lock before
serializing. That's the same pattern `services::forecast::finish_plan_cycle` uses for
`compute_site_headroom`.

- *Alternative: clone the needed state out and compute off-lock (`spawn_blocking`).* `SimState`
  holds `Box<dyn Asset>` configs and isn't cheaply clonable. Adopt this only if the measurement
  (Risks) shows the lock hold would delay the 1 s tick noticeably.

### D6. Grid physical ratings on `AppCtx`

Add `grid_max_import_kw` / `grid_max_export_kw` to `AppCtx` (built once in `main.rs` from
`profile.grid`, as the tick/planning tasks already are), so the route can clamp the same way the
tick does without reading `Profile` (profile rule: no `use crate::profile` in `routes/`).

### D7. UI: a cursor mode, slot-snapped hover, double-click pin

- **Mode switch** in `GridHeadroomCell`'s header: "Values" (default, today's behavior) /
  "Move commitment start". Not persisted across reloads (per-viewer convenience at most).
- **`TimeSeriesChart` generic cursor hooks**: optional `onCursorMove(tsMs | null)` and
  `onCursorDoubleClick(tsMs)` props, forwarded from the Recharts chart's mouse events (x from the
  active label on the numeric time axis). They're generic primitives, not a Site-Headroom-only
  special case (`generic-over-bespoke`). Other charts ignore them.
- **Hover** (move mode, not held): future cursor ts → debounce 150 ms → `useCapacityCurvesAt(ts)`.
  Cursor at or left of `now`, or off-chart → curves fall back to the regular
  `useCapacityCurves()` data (anchored at now).
- **Double-click** holds the double-clicked time (the server snaps it to the same slot). While
  held, hovering doesn't move the curves. Double-click again or switching to "Values" releases
  it. On touch devices a tap moves the start and it stays until the next tap.
- **Caching**: react-query key `["capacity_curves_at", baseUrl, startMs]`, `staleTime` and
  `refetchInterval` 10 s (the per-tick curve's poll); the previous answer stays on screen while
  the next one loads.
- **Rendering**: same two dashed series, same `SiteHeadroomChart` sample-building code. Only the
  `CapacityCurvesResponse` passed in changes, plus a vertical reference line at the snapped start.
- **Transparency** (`ui-transparency`): a caption under the chart in move mode showing the
  snapped start ("Commitment start: 18:00, plan slot" / "pinned" / "no active plan, anchored at
  now"). When the server's `start` differs from the requested one, the caption shows the
  server's value.

### D8. The band's forecast half becomes the `t2 = 0` point of the same core (found during implementation)

Reading the PV code before writing the seam test showed that the band and a future-start curve
would compute PV differently in two ways:

1. **Where PV measures elapsed time from.** `PvInverter::uncurtailed_power_kw_at(ts, elapsed_s)`
   uses the live measurement when `elapsed_s == 0`, and decays the manual-inject offset over
   `elapsed_s`. Both of PV's projection methods (`max_effort_schedule_inner`,
   `simulate_forward_inner`) measure `elapsed_s` from their own schedule's first point, which
   they treat as "when the live inputs were captured". That's only true when the schedule starts
   at `now`. A curve starting at a future `t1` would reuse today's live measurement for `t1`.
   The band's slot 0 (the first remaining slot start, up to one slot after `now`) already had a
   smaller version of this. The planner's own PV input (`resolve_pv_forecast_kw`) measures
   from `now`, which is the correct convention.
2. **Curtailed vs. uncurtailed.** The band's PV branch read the plan trajectory's `power_kw`,
   which is capped by the plan's own PV setpoint. The band means "Physical" absolute export
   headroom, and PV curtailment can always be released, so that's wrong whenever the plan
   curtails PV.

Fixes:
- `PvInverter` gets `live_inputs_at: Option<DateTime<Utc>>`, set each tick via `TickOverrides`
  (the tick's own `now`, the same `now` the tick-path forecasts receive). Its projections
  measure `elapsed_s` from that instant, not from their schedule's first point, and fall back to
  the schedule origin only when it's unset (a PV constructed outside a tick, e.g. in unit tests).
  PV stays the sole authority on its own forecast (`asset-competence-assurance`).
- `compute_site_headroom_forecast` computes each slot as the `t2 = 0` point of the same core
  (D4), started from that slot's trajectory states, the same way `compute_site_headroom` is
  the `t2 = 0` point at `now`. Its PV-specific branch and its separate `max_effort_setpoint`
  reads are deleted. Band and future-start curve are then one function evaluated at the same
  point, so the seam requirement holds by construction and the seam test pins it.

User-visible effect: when the plan curtails PV in a future slot, the band's export side there now
shows the uncurtailed PV ceiling. The band's first remaining slot now uses the weather forecast
rather than the live PV measurement.

A third divergence surfaced once the band ran through the shared core (the seam test failed on it):
`ShiftableLoadAsset::max_effort_schedule` answered every zero-length (`t2 = 0`) window with 0 kW,
even when its own placement had the run starting at `t1`. The old band read the load's
capability (`power_kw`) instead; the live headroom at now already went through the zero-length
path, so band and live headroom already disagreed at now for shiftable loads. Fix: the
zero-length branch applies the same placement at `t1` (drawing if the run is placed at or
before `t1`). User-visible effect: the live Site Headroom's import side now includes a shiftable
load that could start now.

## Risks / Trade-offs

- **Lock hold delays the tick.** The compute is unmeasured. → Add a `debug`-level timing log
  around D4. Before merge, measure on Node1 hardware (production runs there). Acceptance:
  typical request ≤ ~20 ms. Above that, switch D5 to the off-lock alternative before shipping.
  Client debounce + per-slot cache bound the request rate to roughly one per slot crossing.
- **Seam at `t1` may not hold for every asset kind.** For battery/EV/heater/shiftable load, the
  curve's first step and the band's slot value both come from `max_effort_setpoint` on the same
  trajectory state, so they should agree. For **PV**, the band uses the plan trajectory's own
  `power_kw` at that slot, while the curve starts from PV's live state and its
  weather-driven `max_effort_schedule`. For **base load**, the band uses
  `resolve_base_load_forecast_kw` per slot and the curve uses `forecast_kw_at(t1)`. → A
  seam-at-`t1` unit test across all kinds is an early task. The spec requires equality. If PV or
  base load diverge, work out which side answers the question wrongly (it may be the band, not
  the curve) and fix that side. Don't paper over it with a tolerance. Relaxing the requirement
  instead is a spec change that needs the user's sign-off.
- **Meaning shifts with the plan.** A future-start curve means "the plan runs as intended until
  `t1`, then the site goes all-in." It inherits plan error, and its answer changes on every
  replan. → The caption labels it as plan-based. The 10 s `staleTime` keeps it no staler than
  the band it is drawn against.
- **Slot jumps, not continuous motion.** The curve moves in 5–15 min steps. → Deliberate (D3). It
  matches the resolution of the underlying state, and the reference line makes the snapping
  visible.
- **Mode conflicts with reading values.** → Explicit mode switch. The tooltip still works in both
  modes. Pinning frees the cursor to read values against a fixed curve.

## Migration Plan

Additive only: new optional query parameter, new response field, new UI mode defaulting to
today's behavior. Deploy VEN + VEN UI together (same image). Rollback = redeploy the previous
image. No persisted data or schema changes.

## Open Questions

- Does Recharts (the version pinned in `VEN/ui/package.json`) surface `onDoubleClick` on the
  chart root with an active label? If not, attach the double-click to the wrapper `div` and use
  the last `onCursorMove` value.
- Should the Diagnostics → Capacity Forecast panel get the same start picker later? Out of scope
  here. If it's wanted, it reuses the same endpoint unchanged.
