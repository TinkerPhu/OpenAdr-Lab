## Why

The Controller's Site Headroom chart shows the sustained-commitment capacity curves ("Import
commitment" / "Export commitment", red/green dashed) only for a commitment starting **now**. A
user planning ahead ("if the VTN asked for max export at 18:00, how long could we hold it?") has
no way to see that answer, even though the engine already models the full `(t1, t2, direction)`
domain and today only exposes two fixed slices of it (`t1 = now` sweep `t2`; `t2 = 0` sweep `t1`).
The missing arbitrary-`t1` slice is cheap: one curve for a future start costs about as much as the
per-tick curve every VEN already recomputes every second. The `planState(t1)` resolver it needs
(`resolve_plan_state_at`) is built and unit-tested but has no production caller yet.

## What Changes

- Add a cursor mode switch to the Controller's Site Headroom chart: **"Values"** (today's
  tooltip behavior, the default) vs. **"Move commitment start"**. In the second mode the dashed
  Import/Export commitment curves start at the hovered future time instead of `now`, and follow
  the cursor as it moves.
- The hovered time is snapped down to the active plan's slot boundary (5/10/15-min grid per
  plan zone), so the curve moves in slot-sized jumps. Requests are debounced and cached per
  slot, so moving back and forth over the same slots doesn't refetch.
- Double-click pins the commitment start at the clicked slot (the curve stays there while the
  cursor moves on); double-click again, or switching back to "Values", unpins it and the
  curves return to `now`.
- The cursor left of `now` (or with no active plan) keeps the curves at `now`. There is no past
  state to start from, and without a plan there is no forecast state for a future `t1`.
- New endpoint `GET /flexibility/capacity?start=<RFC3339>` computes both curves on demand from
  the plan-forecasted asset states at the snapped `t1`. Without `start` (or with `start <= now`)
  it behaves exactly like today's `GET /flexibility/capacity`, returning the per-tick curves.
  The response also carries the snapped `start` actually used.
- `compute_site_capacity_curve` gains a variant that starts from given per-asset states at a
  given `t1`, instead of reading the live `SimState`. The `t1 = now` path is expressed through it, so
  there is still exactly one curve computation. `resolve_plan_state_at` gets its first production
  caller and loses its `#[allow(dead_code)]`.
- The OpenADR reporter (`STORAGE_MAX_*_POWER`, `*_RESERVATION_CAPACITY`) and the per-tick curves
  are unchanged: they stay anchored at `now`.

## Capabilities

### New Capabilities
- `capacity-curve-at-future-start`: on-demand sustained-commitment capacity curves anchored at a
  user-chosen future plan slot (backend endpoint + the resolver/engine composition behind it),
  and the Controller Site Headroom chart's "Move commitment start" cursor mode that drives it.

### Modified Capabilities
<!-- None: openspec/specs/ holds no capability specs in this repo; current-state behavior is
     documented under docs/ (docs/architecture/VEN_ARCHITECTURE.md §3.0b/§3.0c,
     docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md). -->

## Impact

- **VEN backend**: `controller/capacity_headroom.rs` (state-parameterized curve entry point),
  `simulator/forecast.rs` (`resolve_plan_state_at` wired, dead-code allowance removed), a new
  service function composing the two, `routes/hems/sessions.rs` (`get_capacity_curves` accepts an
  optional `start` query), `main.rs` `AppCtx` (grid physical import/export ratings made
  available to the route).
- **VEN UI**: `components/controller/charts/SiteHeadroomChart.tsx`, `GridHeadroomCell.tsx`
  (mode switch + pinned state), `components/charts/TimeSeriesChart.tsx` (expose hover /
  double-click x-position), `api/client.ts` + `api/hooks.ts` (start-parameterized query),
  `api/types.ts` (response's snapped `start`).
- **API**: additive only. The existing `GET /flexibility/capacity` response is unchanged when
  `start` is absent. Nothing is breaking.
- **Compute**: one extra curve computation per distinct hovered slot (cached client-side),
  computed under the shared `SimState` lock. It's the same order of work as the existing
  per-second tick curve and must be measured on Node1 hardware before merge (see design).
- **Docs/tests**: `docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md` (new "future start"
  paragraph under "View sustained-commitment capacity forecast"), `docs/architecture/VEN_ARCHITECTURE.md`
  §3.0b/§3.0c (the resolver's first caller, the fourth domain slice), a new BDD scenario in
  `tests/features/isolated/capacity_envelope_absolute_quantities.feature`.
