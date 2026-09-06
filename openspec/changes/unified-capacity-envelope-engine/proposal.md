## Why

`capacity_forecast.rs` (`compute_capacity_curve`) and `envelope_forecast.rs`
(`compute_headroom_forecast`) are two independent, hand-written
implementations of "how much power could the site achieve, under some
commitment." Spec C (`asset_max_power`, `max_effort_setpoint`) and Spec D
(`resolve_plan_state_at`) built the shared per-asset primitives specifically
so these two modules — and the two confirmed bugs living inside one of them
(PV Import double-credits curtailment; Heater Export credits current draw
instead of reporting zero) — could be replaced by one engine instead of
patched separately. This change is that replacement.

## What Changes

- `capacity_forecast.rs` and `envelope_forecast.rs` are deleted; a new
  `controller/capacity_envelope.rs` provides both consumers' data through
  Spec C/D's shared primitives.
- **Capacity Forecast** (Diagnostics page): unchanged shape (`t1 = now`
  fixed, sweep `t2` from 0–48h) but computed through the unified engine —
  fixes the PV-Import bug by construction (PV's Import contribution becomes
  the trivial constant `0.0` from `max_effort_setpoint`, not a re-derived
  formula).
- **Site Headroom** (Controller/History): unchanged shape (`t2 = 0` fixed,
  sweep `t1` across plan slots) but the reported `up_kw`/`down_kw` become
  **absolute** achievable power at each slot (via `max_effort_setpoint`),
  not a delta from the plan's currently-chosen dispatch — fixes the
  Heater-Export bug by construction, and changes shiftable load's
  contribution from a plan-relative "is the current slot deferrable" check
  to the same absolute-capability call every other asset gets.
- `SiteHeadroomChart`'s rendering reworks from a relative band around the
  live grid-power line to displaying the absolute up/down capability
  directly — the old band shape (`gridPowerKw ± up/down`) only makes sense
  for a relative delta.
- PV keeps its existing weather-driven ceiling resolution
  (`entities::solar::pv_ceiling_kw`) for its Export contribution in both
  consumers — **not** routed through Spec C/D's trait-based primitives for
  Export, since those never modeled PV's time-varying weather forecast (a
  documented, deliberate scope limit from Specs C and D, not something this
  change can or should extend). Only PV's Import contribution (a trivial
  constant 0) goes through the unified primitive.

## Capabilities

### New Capabilities
- `unified-capacity-envelope-engine`: one shared computation, built on Spec
  C/D's per-asset primitives, backing both the Capacity Forecast and Site
  Headroom UI surfaces as two fixed-axis slices of the same underlying
  `(t1, t2, direction, tier)` domain.

### Modified Capabilities

(none tracked as formal `openspec/specs/` capabilities today — this repo's
current-state documentation for this area lives in
`docs/architecture/VEN_ARCHITECTURE.md` and `docs/use-cases/*.md`, updated
directly per this repo's `workflow` convention rather than through a
tracked `openspec/specs/` capability.)

## Impact

- `VEN/src/controller/capacity_forecast.rs`,
  `VEN/src/controller/envelope_forecast.rs` — deleted.
- `VEN/src/controller/capacity_envelope.rs` — new, replaces both.
- `VEN/src/assets/max_power.rs` — new primitive (`asset_max_power_series` or
  similar; exact shape decided in design.md) needed for the Capacity
  Forecast's dense `t2` sweep, since `asset_max_power` alone only returns
  one `(power, energy)` pair per call.
- `VEN/src/simulator/forecast.rs` — `simulated_trajectory` (Spec D) likely
  needs wider visibility (`pub(crate)`) so the new engine can reuse the
  already-computed per-slot trajectory for Site Headroom's sweep, rather
  than recomputing it once per slot.
- `VEN/src/tasks/sim_tick/forecast_wiring.rs` — call site update.
- `VEN/ui/src/components/controller/charts/SiteHeadroomChart.tsx` — rendering
  rework (relative band → absolute quantity display). `CapacityForecastChart.tsx`
  needs no rendering change (already displays an absolute step curve).
- `tests/features/` — BDD scenario(s) covering the new absolute-quantity
  behavior end-to-end, per this repo's `workflow` rule 4.
- Explicitly **not** in scope (per the master plan's own note): a
  future-anchored `t1` with its own UI control (the "what if the plan holds
  until 3pm" capability) — deferred as a clearly separate follow-on, not a
  blocking requirement for this change's landing.
