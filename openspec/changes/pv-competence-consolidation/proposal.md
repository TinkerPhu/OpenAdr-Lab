## Why

PV currently has four independent implementations of "PV's achievable power," confirmed by
reading each: `Pv::forecast()` (sin-model only), `Pv::step_inner`/`max_effort_setpoint`
(live, weather-aware, already correct for "now"), `entities::solar::pv_ceiling_kw` (a third
formula, called directly from `milp_planner/inputs.rs` on raw snapshot values), and
`pv_frames` (weather-MQTT-driven, used by `capacity_headroom.rs`, confirmed to never call
into any of PV's own methods). This is Phase 1 of
`docs/plans/asset-competence-assurance-master-plan.md`, closing the largest and
highest-value violation of the `asset-competence-assurance` rule
(`.claude/CLAUDE.md`, `VEN_ARCHITECTURE.md` §3.0d).

## What Changes

- `PvInverter` gains a `max_effort_schedule` override (currently absent — PV inherits the
  generic trait default, which has no weather awareness for future points). The override
  reads a per-tick-injected weather-forecast series from the asset's own state/config —
  reusing the exact same `TickOverrides`/`TickOverridable` per-tick data-injection channel
  already used for `weather_power_kw` (today's single "now" value) — rather than fetching
  weather itself. `t1`'s own point matches `max_effort_setpoint`'s existing, already-correct
  live answer exactly (no change there); points beyond `t1` now genuinely track future
  weather instead of a flat sin-model.
- `entities::solar::pv_ceiling_kw`'s external call site in `milp_planner/inputs.rs` is
  retired — `p_pv_kw` is built by calling into `Pv`'s own (now future-weather-aware) method
  instead of re-deriving the ceiling from raw snapshot values.
- `pv_frames`'s special-cased existence in `capacity_headroom.rs` is retired — PV flows
  through the same generic per-asset loop `compute_site_headroom_forecast` and
  `compute_site_capacity_curve` already use for every other asset kind, instead of a
  site-level exception.
- `Pv::forecast()` is reconciled with the above (either upgraded to use the same weather
  series `max_effort_schedule` now uses, or left as a defined lesser-tier estimate if it
  turns out to have a distinct caller with different needs — decided in design.md, not
  assumed).
- **Root-cause fix for the decaying-offset bug found during design (D7):** the manual
  irradiance-override fade (`pv_alpha`/reference-step encoding) is replaced with a single
  time constant `τ` (seconds) — the same physical quantity, currently smeared across two
  independently-sourced numbers (a hardcoded `300` in `simulator/pv_smoothing.rs`, and a
  planner-config-sourced `zone_a_step_s` in `entities::solar::pv_ceiling_kw`) that only
  coincide today by default. Storing `τ` directly removes the redundant parameter that made
  that divergence possible, not just today's specific instance of it. The "Blend-back Speed"
  UI slider is re-modeled to represent `τ` in seconds directly, per explicit instruction.

**No behavior change to PV's live dispatch power output** — `step()`/`capability()`/
`max_effort_setpoint` at `t1` compute the same export values as today. The one deliberate,
explicit exception is D7's manual-override fade, whose *default* is preserved exactly
(`τ≈2848.5s`, equivalent to today's `alpha=0.1`) but whose *control surface* (the slider) and
*internal parameterization* change as described above.

## Capabilities

### New Capabilities

(none — this is a consolidation of existing behavior under an existing capability, not a new
user-facing capability)

### Modified Capabilities

- `asset-competence-assurance`: PV becomes a confirmed-compliant instance of the rule
  established in Phase 0 — adds a requirement that per-asset forecast data flows in via the
  established per-tick injection channel (`TickOverrides`), not a bespoke site-level
  parameter, generalizing the pattern for future phases (base load, Phase 2) to follow.

## Impact

- **Modified (core consolidation)**: `VEN/src/assets/pv.rs` (new `max_effort_schedule`
  override, new weather-forecast-series field on `PvInverter`),
  `VEN/src/assets/asset_trait.rs` / `TickOverrides` (new field for the injected weather
  series), `VEN/src/simulator/mod.rs` (populate the new `TickOverrides` field each tick),
  `VEN/src/controller/milp_planner/inputs.rs` and `VEN/src/simulator/forecast.rs`
  (`insert_pv_points` — both of `pv_ceiling_kw`'s two real call sites, corrected count per
  design.md D4), `VEN/src/controller/capacity_headroom.rs` (remove PV's site-level
  special-casing in both `compute_site_headroom_forecast` and the shared capacity-curve
  aggregator).
- **Modified (D7, τ reparametrization)**: `VEN/src/simulator/pv_smoothing.rs`
  (`PvSmoothingState` — new `decayed_offset_after`, `alpha`→`tau_s` throughout),
  `VEN/src/assets/pv.rs`/`asset_trait.rs`/`mod.rs` (`pv_alpha`→`tau_s` field/param
  renames), `VEN/src/controller/dispatcher.rs`, `VEN/src/simulator/plan_context.rs`,
  `VEN/src/simulator/pv_preview.rs`, `VEN/src/controller/simulator_port.rs`,
  `VEN/src/services/test_support/mock_simulator_port.rs` (every other `pv_alpha` reference
  found via grep), `VEN/ui/src/api/types.ts` (the inject/override payload type and the
  "Blend-back Speed" control's range).
- **Read, not modified**: `VEN/src/entities/solar.rs`'s `weather_pv_kw_for_slots`/
  `weather_pv_forecast_series` (reused, not duplicated, by PV's new method);
  `VEN/src/services/planning/mod.rs`'s own separate `zone_a_step_s` use (switch-cost
  weighting — confirmed unrelated, untouched); weather.rs's MQTT infrastructure (unchanged).
- **Removed**: `entities::solar::pv_ceiling_kw`/`PvCeilingParams` (once both real call sites
  are replaced); `pv_frames`'s PV-specific plumbing in `capacity_headroom.rs`/
  `arbiter_glue.rs::resolve_weather_pv_kw_for_tick`/`simulator/forecast.rs`'s
  `insert_pv_points` (kept only if a non-PV consumer is found — confirmed in design.md
  before removal).
- **Tests**: `VEN/src/assets/pv.rs`'s and `VEN/src/simulator/pv_smoothing.rs`'s own suites
  gain coverage for the new methods and the τ rename; `milp_planner/inputs.rs`'s and
  `capacity_headroom.rs`'s existing PV-related tests need updating to the new call shape,
  plus a numeric-equivalence check against today's `pv_ceiling_kw`-derived `p_pv_kw` values
  (live MILP planning input, not just reporting); `VEN/ui`'s
  `AssetRightSection.test.tsx`/`pv_irradiance_one_shot.test.ts`/`useSetSimInject.test.tsx`
  need updating for the renamed field and re-scaled slider.
