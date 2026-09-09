//! Unified capacity/headroom engine (`unified-capacity-envelope-engine`,
//! Spec E of the asset-max-power-forecast master plan). Replaces
//! `capacity_forecast.rs`/`envelope_forecast.rs`'s two independent
//! computations with one, built on Spec C's `Asset::max_effort_setpoint`/
//! `assets::asset_max_power_series` and Spec D's
//! `simulator::forecast::simulated_trajectory` — the *forecast* half of the
//! Controller/History Site Headroom UI panel and the Diagnostics Capacity
//! Forecast panel are fixed-axis slices of the same underlying
//! `(t1, t2, direction, tier)` domain:
//!
//! - **Capacity Forecast** — `t1 = now` fixed, sweep `t2` from 0 to the
//!   plan's own horizon (`compute_site_capacity_curve`).
//! - **Site Headroom (forecast)** — `t2 = 0` fixed, sweep `t1` across the
//!   plan's remaining slots (`compute_site_headroom_forecast`).
//!
//! A third fixed slice of the same domain — `t1 = now` only, `t2 = 0` — feeds
//! the *history/live* half of the same Site Headroom UI panel:
//! `controller::site_headroom::compute_site_headroom`. It was originally a
//! separate, differently-named function (`compute_envelope`) left on a stale
//! relative-delta model after this module's own Spec E rewrite — a real,
//! user-reported bug (see that module's doc comment and
//! `docs/reference/KEY_LEARNINGS.md`). Naming it in this domain's own family
//! (not "envelope") is exactly what surfaced the gap; don't let a future
//! addition to this same UI panel go unnoticed the same way again.
//!
//! **PV is asset-kind-and-direction-special, by design** (design.md D1, not
//! an oversight): PV's Import contribution goes through
//! `max_effort_setpoint` like every other asset (a trivial, always-correct
//! `0.0`). PV's Export contribution keeps using the existing weather-driven
//! `pv_frames`/`pv_ceiling_kw` resolution unchanged from today — Spec C/D
//! never modeled PV's time-varying weather forecast inside the `Asset`
//! trait (a deliberate, documented scope limit in both), so routing PV's
//! Export through the trait-based primitives would flatten its ceiling to a
//! constant across the whole horizon, a real regression, not a unification.
//!
//! **`CapacityCurve`/`CapacityCurveStep::power_kw` is SIGNED** (positive =
//! import, negative = export — the same convention `Asset::max_effort_setpoint`/
//! `capability()` use everywhere else), not the unsigned magnitude the
//! deleted `capacity_forecast.rs` originally used. The unsigned convention
//! is external — it applies only where an actual external consumer needs it
//! (OpenADR's `STORAGE_MAX_CHARGE_POWER`/`STORAGE_MAX_DISCHARGE_POWER`
//! report payloads are direction-tagged by name and want a magnitude — see
//! `report_intervals.rs::build_capacity_forecast_intervals`); the UI
//! (`CapacityForecastChart.tsx`) needed no such conversion, confirmed by
//! reading its formatters and energy calc rather than assumed.
//! `SiteFlexibilityForecastSlot::up_kw`/`down_kw`, by contrast, stay unsigned
//! magnitudes (two separate always-non-negative fields by design, not one
//! bidirectional field) — see `magnitude_kw`'s doc comment for the full
//! reasoning on why these two types ended up with different conventions.
//!
//! The Controller's Site Headroom chart (`SiteHeadroomChart.tsx`) overlays
//! both — the headroom band and the capacity curves — in one view. They
//! answer different questions (per-instant snapshot along the plan's own
//! trajectory vs. a single continuous full-effort commitment starting now),
//! so the capacity curve legitimately sitting inside the band, or an Export
//! curve swinging positive past the band's usual scale (this module's own
//! `merge_events` doc, below), is expected — not a wiring bug.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};

use crate::assets::asset_max_power_series;
use crate::controller::simulator_port::{AssetForecastFrame, SimSnapshot};
use crate::entities::capacity_curve::{
    CapacityCurve, CapacityCurveStep, CommitmentDirection, LimitTier,
};
use crate::entities::device_session::EvSession;
use crate::entities::plan::{Plan, PlanTimeSlot, SiteFlexibilityForecastSlot};
use crate::ids::ASSET_PV;
use crate::simulator::forecast::simulated_trajectory;
use crate::simulator::SimState;

/// One (elapsed_s, delta_kw) breakpoint. Multiple events at the same
/// `elapsed_s` are summed by `merge_events`. Ported verbatim from the
/// deleted `capacity_forecast.rs` — this sweep-line merge technique is
/// unchanged by this refactor, only how each asset's events are produced is.
type Event = (i64, f64);

// ─── Capacity Forecast (t1 = now, sweep t2) ────────────────────────────────

/// "If the site committed now to sustained max `direction`, how does
/// achievable power step down over elapsed time, and how much energy is
/// behind it." `t2_max` bounds the sweep (the plan's own horizon in
/// production — see `tasks/sim_tick/forecast_wiring.rs`).
pub fn compute_site_capacity_curve(
    direction: CommitmentDirection,
    now: DateTime<Utc>,
    t2_max: Duration,
    sim: &SimState,
    pv_frames: &[AssetForecastFrame],
    snapshot: &SimSnapshot,
) -> CapacityCurve {
    let mut events: Vec<Event> = Vec::new();

    for (entry, cfg) in sim.iter_assets() {
        match cfg.asset_type_str() {
            "pv" => continue, // handled separately below (design.md D1).
            "base_load" => {
                events.extend(base_load_capacity_events(&entry.state));
                continue;
            }
            _ => {}
        }
        let series = asset_max_power_series(
            cfg,
            &entry.state,
            now,
            t2_max,
            direction,
            LimitTier::Physical,
        );
        events.extend(series_to_events(&series));
    }
    events.extend(pv_capacity_events(direction, now, pv_frames));

    let import_limit_kw = snapshot.grid.import_limit_kw.max(0.0);
    let export_limit_kw = (-snapshot.grid.export_limit_kw).max(0.0);

    CapacityCurve {
        direction,
        start: now,
        steps: merge_events(events, direction, import_limit_kw, export_limit_kw),
    }
}

/// Converts one `Asset`-convention signed power reading (positive = import,
/// negative = export — the convention `TrajectoryPoint`/`AssetCapability`/
/// `max_effort_setpoint` all use) into `SiteFlexibilityForecastSlot`'s
/// unsigned achievable-power *magnitude* convention — `up_kw`/`down_kw` are
/// two separate always-non-negative fields by design (matching the
/// pre-existing `SiteFlexibilityEnvelope` convention), not a single
/// bidirectional signed field, so they still need this conversion.
///
/// `CapacityCurve`/`CapacityCurveStep::power_kw`, by contrast, is now signed
/// (matching the internal `Asset`-trait convention throughout) — the
/// external, unsigned-magnitude convention there applies only at the actual
/// OpenADR reporting boundary (`report_intervals.rs::build_capacity_forecast_intervals`),
/// per this session's design discussion: `CapacityCurve`'s two consumers
/// (the OpenADR reporter and `CapacityForecastChart.tsx`) turned out to need
/// this conversion at genuinely different points — the reporter needs it
/// because OpenADR's own payload types are direction-tagged by name and want
/// a magnitude; the UI chart needs no conversion at all (its formatters and
/// energy calc already handle negative values correctly, confirmed by
/// reading the code rather than assumed).
///
/// Asserts the sign actually matches `direction` rather than silently
/// trusting it, so a genuine convention violation panics in dev/test builds
/// instead of producing a silently wrong number — this assertion is what's
/// left of an earlier version of this function that also converted
/// `CapacityCurve`'s own producers, after a failing test caught exactly this
/// class of bug once already.
///
/// `pub(crate)`: also reused by `controller::site_headroom::compute_site_headroom`,
/// the `t1 = now`-only sibling of `compute_site_headroom_forecast` below.
pub(crate) fn magnitude_kw(power_kw: f64, direction: CommitmentDirection) -> f64 {
    debug_assert!(
        match direction {
            CommitmentDirection::Import => power_kw >= -1e-9,
            CommitmentDirection::Export => power_kw <= 1e-9,
        },
        "power_kw {power_kw} violates {direction:?}'s signed-power convention"
    );
    power_kw.abs()
}

/// Converts a dense `asset_max_power_series` output into sparse delta events
/// (design.md D3: dense compute, sparse output) — one event per point where
/// power actually changes, matching `CapacityCurveStep`'s existing
/// "ordered by elapsed_s ascending" breakpoint contract. Uses the series'
/// own raw signed `power_kw` directly — `CapacityCurve` is signed now (see
/// `magnitude_kw`'s doc comment), so no conversion happens here.
fn series_to_events(series: &[(i64, f64, f64)]) -> Vec<Event> {
    let mut events = Vec::new();
    let mut prev_power = 0.0_f64;
    for &(elapsed_s, power_kw, _) in series {
        let delta = power_kw - prev_power;
        if delta != 0.0 {
            events.push((elapsed_s, delta));
        }
        prev_power = power_kw;
    }
    events
}

/// Base load's Capacity Forecast contribution — a **second** asset-kind
/// exception, found during implementation (not foreseen in design.md,
/// documented here rather than silently patched): base load has
/// `PowerAdjustability::None` — it cannot be committed to any extreme at
/// all, so `max_effort_setpoint`/`asset_max_power_series` are the wrong
/// question to ask it. Its current draw is a constant NET-GRID-POWER
/// offset instead, exactly as `capacity_forecast.rs`'s own module doc
/// explained.
///
/// **Direction-independent**, unlike the deleted `capacity_forecast.rs`'s
/// own `base_load_events` (which negated for Export): base load's physical
/// contribution to net grid power doesn't care what direction the SITE is
/// committing to — it always draws `+actual_power_kw` (positive, the
/// universal signed convention every other producer here now also uses
/// directly). Summed together with an exporting asset's own negative
/// contribution, this correctly and automatically produces a *smaller*
/// magnitude net export (the old behavior) — and, if base load's draw
/// exceeds what's exportable, a genuinely positive (net-importing) result
/// instead of the old code's artificial floor at `0.0`. That floor was an
/// artifact of the old unsigned-magnitude representation (a magnitude can't
/// go below zero by definition), not a real physical constraint — the site
/// genuinely can still be net-importing even while every exportable asset is
/// maxed out, and the old code was silently discarding that fact. (Base load
/// isn't the only contributor that can do this — see `merge_events`'s doc
/// comment for `ShiftableLoadAsset`'s own version of the same effect.)
fn base_load_capacity_events(state: &crate::assets::AssetState) -> Vec<Event> {
    let crate::assets::AssetState::BaseLoad(s) = state else {
        return Vec::new();
    };
    if s.actual_power_kw <= 0.0 {
        return Vec::new();
    }
    vec![(0, s.actual_power_kw)]
}

/// PV's Capacity Forecast contribution (design.md D1): Import is the
/// trivial constant `0.0` (no event needed at all — `max_effort_setpoint`
/// already establishes this is correct, so there's nothing to emit).
/// Export reuses `pv_frames` verbatim, via the same per-frame delta
/// extraction the deleted `capacity_forecast.rs::pv_events`'s Export branch
/// used — PV's ceiling is weather-driven, not something this engine's
/// trait-based primitives can forecast (see module doc). Uses
/// `cap_max_export_kw` directly (already negative, the correct sign for
/// `CapacityCurve`'s now-signed convention) — `magnitude_kw` is called only
/// for its `debug_assert!` (a validation guard), with the result discarded
/// rather than substituted, since no conversion is needed.
fn pv_capacity_events(
    direction: CommitmentDirection,
    start: DateTime<Utc>,
    pv_frames: &[AssetForecastFrame],
) -> Vec<Event> {
    if direction == CommitmentDirection::Import {
        return Vec::new();
    }
    let mut events = Vec::new();
    let mut prev_value = 0.0_f64;
    for frame in pv_frames {
        let Some(point) = frame.assets.get(ASSET_PV) else {
            continue;
        };
        let _ = magnitude_kw(point.cap_max_export_kw, CommitmentDirection::Export); // sign-validates, result unused
        let value = point.cap_max_export_kw;
        let elapsed_s = (frame.ts - start).num_seconds();
        if elapsed_s < 0 {
            prev_value = value;
            continue;
        }
        let delta = value - prev_value;
        if delta != 0.0 {
            events.push((elapsed_s, delta));
        }
        prev_value = value;
    }
    events
}

/// Sweep-line merge of piecewise-constant contributions: sum deltas at each
/// distinct elapsed time, accumulate a running SIGNED total (positive =
/// import, negative = export — `CapacityCurve`'s internal convention).
/// Always includes an elapsed-0 step so the curve starts at the commitment
/// instant even if every contributor is silent there.
///
/// The clamp shape is direction-dependent, not the old code's symmetric
/// `[0, cap_kw]`: Import's floor at `0.0` is defensive/redundant (every
/// Import-direction contributor is already `>= 0` by construction), bounded
/// above by the grid's own hardware/contractual import limit
/// (`import_limit_kw`). Export's floor is `-export_limit_kw` (the grid's
/// export hardware limit) — but Export ALSO needs an upper bound at
/// `import_limit_kw`, not left unbounded: a sustained Export commitment's
/// net total can legitimately swing positive (net importing) when
/// non-exportable contributions exceed what's exportable — not just base
/// load's constant draw (as an earlier version of this comment claimed), but
/// also a non-interruptible `ShiftableLoadAsset` whose deadline forces it to
/// keep drawing `power_kw` once started, regardless of the site's Export
/// commitment (found via review: `ShiftableLoadAsset::max_effort_schedule`'s
/// Export branch places the run as late as possible but still reports its
/// own positive draw once running — it has no way to actually export). Since
/// the site's real grid hardware still can't exceed `import_limit_kw` even
/// while "trying" to export, that swing must be bounded by the same limit
/// Import direction itself uses, not left to grow arbitrarily.
fn merge_events(
    events: Vec<Event>,
    direction: CommitmentDirection,
    import_limit_kw: f64,
    export_limit_kw: f64,
) -> Vec<CapacityCurveStep> {
    let mut by_elapsed: BTreeMap<i64, f64> = BTreeMap::new();
    by_elapsed.insert(0, 0.0);
    for (elapsed_s, delta_kw) in events {
        *by_elapsed.entry(elapsed_s).or_insert(0.0) += delta_kw;
    }
    let mut running = 0.0_f64;
    by_elapsed
        .into_iter()
        .map(|(elapsed_s, delta_kw)| {
            running += delta_kw;
            let power_kw = match direction {
                CommitmentDirection::Import => running.clamp(0.0, import_limit_kw),
                CommitmentDirection::Export => running.clamp(-export_limit_kw, import_limit_kw),
            };
            CapacityCurveStep {
                elapsed_s,
                power_kw,
            }
        })
        .collect()
}

// ─── Site Headroom (t2 = 0, sweep t1 across plan slots) ────────────────────

/// Each future plan slot's absolute achievable import/export power
/// (`max_effort_setpoint` at that slot's plan-forecasted state) — NOT a
/// delta from the plan's own chosen dispatch (design.md D5: this is the
/// resolved absolute-vs-relative product decision). `up_kw` = absolute max
/// export achievable at this slot; `down_kw` = absolute max import
/// achievable at this slot (same direction mapping `CapacityCurve` uses).
///
/// `ev_session` is used the same way `build_forecast_frames` used it: found
/// via review before wiring this into production that `simulated_trajectory`
/// alone doesn't exclude a plugged-in EV past its live session's
/// `departure_time` — `EvState::plugged` is never toggled by `step()` (the
/// physics model doesn't know a car left), so without this exclusion the EV
/// would keep contributing headroom at slots after it's actually gone,
/// exactly the gap `build_forecast_frames`'s own `include_at` closure exists
/// to close for the capability-frame path. `compute_site_capacity_curve`
/// does NOT need the equivalent — its sustained-commitment model never
/// projected a future departure either, matching the deleted
/// `capacity_forecast.rs`'s own (unchanged) scope.
pub fn compute_site_headroom_forecast(
    sim: &SimState,
    plan: &Plan,
    ev_session: Option<&EvSession>,
    pv_frames: &[AssetForecastFrame],
    now: DateTime<Utc>,
) -> Vec<SiteFlexibilityForecastSlot> {
    let future_slots: Vec<&PlanTimeSlot> = plan.all_slots().filter(|s| s.start >= now).collect();
    if future_slots.is_empty() {
        return Vec::new();
    }

    let mut up_kw = vec![0.0_f64; future_slots.len()];
    let mut down_kw = vec![0.0_f64; future_slots.len()];

    for (entry, cfg) in sim.iter_assets() {
        let asset_kind = cfg.asset_type_str();
        match asset_kind {
            "pv" => continue, // handled separately below (design.md D1).
            // Base load has zero controllable degrees of freedom
            // (`PowerAdjustability::None`) -- it contributes no *flexibility*
            // of either kind, matching `build_forecast_frames`'s own existing
            // exclusion of base_load from capability frames. Its live draw
            // is a real number, but "how much MORE could this asset do" is
            // always zero for it, unlike `max_effort_setpoint`'s Import
            // answer (its current draw) would naively suggest.
            "base_load" => continue,
            _ => {}
        }
        // Computed once per asset (design.md D4) -- NOT once per slot, which
        // would redundantly re-walk this same trajectory for every slot
        // requested (the mistake `resolve_plan_state_at`'s per-call design
        // would make if called in a loop here).
        let traj = simulated_trajectory(entry, cfg, &future_slots);
        for (i, point) in traj.points.iter().enumerate() {
            if i >= future_slots.len() {
                break; // the trailing sentinel point (Spec D) — no matching slot.
            }
            if asset_kind == "ev" {
                if let Some(session) = ev_session {
                    if future_slots[i].start >= session.departure_time {
                        continue; // EV has departed by this slot -- exclude.
                    }
                }
            }
            let export_kw = cfg.max_effort_setpoint(
                &point.state,
                CommitmentDirection::Export,
                LimitTier::Physical,
            );
            let import_kw = cfg.max_effort_setpoint(
                &point.state,
                CommitmentDirection::Import,
                LimitTier::Physical,
            );
            up_kw[i] += magnitude_kw(export_kw, CommitmentDirection::Export);
            down_kw[i] += magnitude_kw(import_kw, CommitmentDirection::Import);
        }
    }

    for (i, slot) in future_slots.iter().enumerate() {
        let Some(pv_point) = pv_frames
            .iter()
            .find(|f| f.ts == slot.start)
            .and_then(|f| f.assets.get(ASSET_PV))
        else {
            continue;
        };
        // Import: PV's max_effort_setpoint is always 0.0 -- nothing to add.
        up_kw[i] += magnitude_kw(pv_point.cap_max_export_kw, CommitmentDirection::Export);
    }

    future_slots
        .iter()
        .enumerate()
        .map(|(i, slot)| SiteFlexibilityForecastSlot {
            ts: slot.start,
            up_kw: up_kw[i],
            down_kw: down_kw[i],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::AssetState;
    use crate::entities::asset::PlanTrigger;
    use crate::entities::asset_params::{
        AssetParams, BaseLoadParams, BatteryParams, HeaterParams, PvParams,
    };
    use crate::entities::plan::{Plan, PlanTimeSlot, PlanZone, PlanningHorizon, SolveStatus};
    use crate::entities::planner_params::PlannerObjective;
    use crate::ids::{ASSET_BATTERY, ASSET_HEATER};
    use chrono::TimeZone;
    use std::collections::HashMap as Map;
    use uuid::Uuid;

    fn t0() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 6, 12, 0, 0).unwrap()
    }

    fn make_plan(step_s: u64, slots: usize, start: DateTime<Utc>) -> Plan {
        let horizon = PlanningHorizon {
            start_time: start,
            end_time: start + Duration::seconds((step_s * slots as u64) as i64),
            step_size_s: step_s,
            num_steps: slots,
            far_horizon: start + Duration::seconds((step_s * slots as u64) as i64),
            zones: vec![PlanZone { step_s, slots }],
        };
        let plan_slots: Vec<PlanTimeSlot> = (0..slots)
            .map(|i| PlanTimeSlot {
                slot_index: i,
                start: start + Duration::seconds((step_s * i as u64) as i64),
                end: start + Duration::seconds((step_s * (i + 1) as u64) as i64),
                import_tariff_eur_kwh: 0.25,
                export_tariff_eur_kwh: 0.08,
                co2_g_kwh: 300.0,
                grid_effective_cost: 0.25,
                marginal_cost_import_eur_per_kwh: 0.25,
                marginal_cost_export_eur_per_kwh: 0.25,
                rate_estimated: false,
                import_cap_kw: 25.0,
                export_cap_kw: 10.0,
                allocations: vec![],
                pv_forecast_kw: 0.0,
                pv_used_kw: 0.0,
                baseline_kw: 0.0,
                surplus_available_kw: 0.0,
                net_import_kw: 0.0,
                net_export_kw: 0.0,
                import_flexibility_kw: 0.0,
                export_flexibility_kw: 0.0,
                planned_kw_by_asset: Map::new(),
                planned_state_by_asset: Map::new(),
                bat_charge_kw: 0.0,
                bat_discharge_kw: 0.0,
            })
            .collect();
        Plan {
            id: Uuid::new_v4(),
            created_at: start,
            trigger: PlanTrigger::Periodic,
            objective: PlannerObjective::MinCost,
            horizon,
            slots: plan_slots,
            objective_eur: 0.0,
            friction_eur: 0.0,
            cost_breakdown: Default::default(),
            soc_trajectory_kwh: vec![],
            summary: Default::default(),
            envelopes: vec![],
            warnings: vec![],
            solve_status: SolveStatus::Optimal,
            penalty_rules_active: vec![],
            solver_ms: None,
            mip_gap_target: None,
        }
    }

    // ── merge_events ─────────────────────────────────────────────────────

    #[test]
    fn merge_clips_combined_total_to_cap() {
        // Import direction: two 6 kW positive contributors = 12 kW combined,
        // clamped to the 10 kW grid import limit.
        let events = vec![(0, 6.0), (0, 6.0)];
        let steps = merge_events(events, CommitmentDirection::Import, 10.0, 10.0);
        assert_eq!(
            steps,
            vec![CapacityCurveStep {
                elapsed_s: 0,
                power_kw: 10.0
            }]
        );
    }

    #[test]
    fn merge_import_never_goes_negative() {
        // Every Import-direction contributor is >= 0 by construction, so this
        // floor is defensive/redundant in practice -- still verified directly.
        let events = vec![(0, 2.0), (0, -5.0)];
        let steps = merge_events(events, CommitmentDirection::Import, 100.0, 100.0);
        assert_eq!(
            steps,
            vec![CapacityCurveStep {
                elapsed_s: 0,
                power_kw: 0.0
            }]
        );
    }

    #[test]
    fn merge_export_can_go_positive_when_base_load_exceeds_exportable_capacity() {
        // Deliberate behavior refinement (see base_load_capacity_events' doc
        // comment): unlike the old unsigned-magnitude code, which floored
        // this at 0.0, an Export commitment whose net signed total is
        // positive (base load's constant draw exceeding what's exportable)
        // must report that positive (net-importing) value, not silently
        // floor it to "0 kW export capacity".
        let events = vec![(0, -2.0), (0, 5.0)]; // -2 kW export capacity + 5 kW base load draw
        let steps = merge_events(events, CommitmentDirection::Export, 100.0, 100.0);
        assert_eq!(
            steps,
            vec![CapacityCurveStep {
                elapsed_s: 0,
                power_kw: 3.0
            }]
        );
    }

    #[test]
    fn merge_export_floors_at_the_negative_grid_limit() {
        let events = vec![(0, -15.0)]; // requests more export than the grid allows
        let steps = merge_events(events, CommitmentDirection::Export, 100.0, 10.0);
        assert_eq!(
            steps,
            vec![CapacityCurveStep {
                elapsed_s: 0,
                power_kw: -10.0
            }]
        );
    }

    #[test]
    fn merge_export_ceiling_at_the_import_limit_when_non_exportable_contributions_overwhelm() {
        // Found via review: base load isn't the only contributor that can
        // push a sustained-Export total positive -- a non-interruptible
        // ShiftableLoadAsset forced to keep drawing at its deadline can too
        // (`ShiftableLoadAsset::max_effort_schedule`'s Export branch reports
        // its own positive draw once running, since it has no way to
        // actually export). Without an upper clamp, an unrealistically large
        // forced draw could report a CapacityCurveStep exceeding the site's
        // real grid import hardware limit. This pins the fix: Export is
        // bounded above by import_limit_kw, exactly like Import direction's
        // own ceiling.
        let events = vec![(0, 50.0)]; // e.g. a large shiftable load forced to run, nothing exporting
        let steps = merge_events(events, CommitmentDirection::Export, 25.0, 100.0);
        assert_eq!(
            steps,
            vec![CapacityCurveStep {
                elapsed_s: 0,
                power_kw: 25.0
            }]
        );
    }

    #[test]
    fn merge_always_includes_an_elapsed_zero_step() {
        let steps = merge_events(vec![], CommitmentDirection::Import, 100.0, 100.0);
        assert_eq!(
            steps,
            vec![CapacityCurveStep {
                elapsed_s: 0,
                power_kw: 0.0
            }]
        );
    }

    // ── compute_site_capacity_curve ─────────────────────────────────────────

    #[test]
    fn battery_export_worked_example_matches_the_old_closed_form_answer() {
        // 10 kWh capacity, soc=0.6, min_soc=0.1 -> 5.0 kWh available, 5 kW
        // discharge rate -> 3600s duration. Same numeric example
        // capacity_forecast.rs's own battery_export_energy_and_duration test
        // used, now verified through the new engine end-to-end. Export is
        // signed negative now (CapacityCurve's internal convention) -- the
        // OLD unsigned-magnitude test expected +5.0; this expects -5.0.
        let now = t0();
        let sim = SimState::from_params(
            &[AssetParams::Battery(BatteryParams {
                id: ASSET_BATTERY.to_string(),
                capacity_kwh: 10.0,
                max_charge_kw: 5.0,
                max_discharge_kw: 5.0,
                initial_soc: 0.6,
                round_trip_efficiency: 1.0,
                min_soc: 0.1,
                c_terminal_eur_kwh: Some(0.0),
            })],
            now,
        );
        let snapshot = sim.to_sim_snapshot();
        let curve = compute_site_capacity_curve(
            CommitmentDirection::Export,
            now,
            Duration::hours(2),
            &sim,
            &[],
            &snapshot,
        );
        assert_eq!(
            curve.steps,
            vec![
                CapacityCurveStep {
                    elapsed_s: 0,
                    power_kw: -5.0
                },
                CapacityCurveStep {
                    elapsed_s: 3600,
                    power_kw: 0.0
                },
            ]
        );
    }

    #[test]
    fn base_load_is_additive_on_import_and_can_flip_a_sustained_export_commitment_net_positive() {
        let now = t0();
        let sim = SimState::from_params(
            &[AssetParams::BaseLoad(BaseLoadParams {
                baseline_kw: 0.5,
                ..Default::default()
            })],
            now,
        );
        let snapshot = sim.to_sim_snapshot();
        let import_curve = compute_site_capacity_curve(
            CommitmentDirection::Import,
            now,
            Duration::hours(1),
            &sim,
            &[],
            &snapshot,
        );
        let export_curve = compute_site_capacity_curve(
            CommitmentDirection::Export,
            now,
            Duration::hours(1),
            &sim,
            &[],
            &snapshot,
        );
        assert_eq!(import_curve.steps[0].power_kw, 0.5);
        // Deliberate behavior refinement (base_load_capacity_events' doc
        // comment): with nothing in the sim able to export at all, a
        // sustained Export commitment's true net grid power is base load's
        // own draw, POSITIVE (net importing) -- not the old unsigned-
        // magnitude code's artificial floor at 0.0, which silently hid the
        // fact that the site is still net-importing despite the commitment.
        assert_eq!(
            export_curve.steps[0].power_kw, 0.5,
            "with no exporting asset, a sustained Export commitment's net grid power is base load's own draw (net importing), not a floored 0.0"
        );
    }

    #[test]
    fn pv_contributes_zero_to_a_sustained_import_commitment() {
        // The confirmed PV-Import bug (design.md Context): PV must never
        // credit its own current generation as import headroom.
        let now = t0();
        let sim = SimState::from_params(
            &[AssetParams::Pv(PvParams {
                id: ASSET_PV.to_string(),
                rated_kw: 5.0,
                inverter_max_kw: 5.0,
                co2_g_kwh: 0.0,
            })],
            now,
        );
        // Force PV to be actively generating right now.
        let (entry, cfg) = sim.find_asset(ASSET_PV).unwrap();
        let pv = cfg
            .as_any()
            .downcast_ref::<crate::assets::PvInverter>()
            .unwrap();
        let inputs = crate::assets::PvPowerInputs {
            measured_power_kw: None,
            weather_power_kw: Some(4.0), // positive generation magnitude input
            irradiance: 0.0,
            irradiance_offset: 0.0,
            irradiance_forced: false,
        };
        let generating_kw = pv.resolve_power_kw(&inputs);
        assert!(generating_kw < 0.0, "fixture must actually be generating");
        let frames = vec![AssetForecastFrame {
            ts: now,
            assets: Map::from([(
                ASSET_PV.to_string(),
                crate::controller::simulator_port::AssetForecastPoint {
                    planned_kw: generating_kw,
                    cap_max_import_kw: 0.0,
                    cap_max_export_kw: generating_kw,
                },
            )]),
        }];
        let _ = entry; // silence unused warning if downcast path changes later

        let snapshot = sim.to_sim_snapshot();
        let curve = compute_site_capacity_curve(
            CommitmentDirection::Import,
            now,
            Duration::hours(1),
            &sim,
            &frames,
            &snapshot,
        );
        assert!(
            curve.steps.iter().all(|s| s.power_kw == 0.0),
            "PV must contribute exactly 0.0 to the Import curve while generating, got {:?}",
            curve.steps
        );
    }

    #[test]
    fn heater_contributes_zero_to_a_sustained_export_commitment() {
        // The confirmed Heater-Export bug (design.md Context): a heater
        // cannot export at all, so its current draw must not be credited.
        let now = t0();
        let sim = SimState::from_params(
            &[AssetParams::Heater(HeaterParams {
                id: ASSET_HEATER.to_string(),
                temp_initial_c: 20.0,
                draw_kw: 1.25, // actively drawing right now
                ..Default::default()
            })],
            now,
        );
        let snapshot = sim.to_sim_snapshot();
        let curve = compute_site_capacity_curve(
            CommitmentDirection::Export,
            now,
            Duration::hours(1),
            &sim,
            &[],
            &snapshot,
        );
        assert!(
            curve.steps.iter().all(|s| s.power_kw == 0.0),
            "heater must contribute exactly 0.0 to the Export curve, got {:?}",
            curve.steps
        );
    }

    #[test]
    fn pv_export_contribution_still_varies_with_the_weather_forecast() {
        // PV's Export contribution must keep reflecting pv_frames (design.md
        // D1) -- not flatten to a constant across the horizon.
        let now = t0();
        let sim = SimState::from_params(
            &[AssetParams::Pv(PvParams {
                id: ASSET_PV.to_string(),
                rated_kw: 5.0,
                inverter_max_kw: 5.0,
                co2_g_kwh: 0.0,
            })],
            now,
        );
        let frames = vec![
            AssetForecastFrame {
                ts: now,
                assets: Map::from([(
                    ASSET_PV.to_string(),
                    crate::controller::simulator_port::AssetForecastPoint {
                        planned_kw: -4.0,
                        cap_max_import_kw: 0.0,
                        cap_max_export_kw: -4.0,
                    },
                )]),
            },
            AssetForecastFrame {
                ts: now + Duration::seconds(3600),
                assets: Map::from([(
                    ASSET_PV.to_string(),
                    crate::controller::simulator_port::AssetForecastPoint {
                        planned_kw: 0.0,
                        cap_max_import_kw: 0.0,
                        cap_max_export_kw: 0.0, // night -- ceiling drops to 0
                    },
                )]),
            },
        ];
        let snapshot = sim.to_sim_snapshot();
        let curve = compute_site_capacity_curve(
            CommitmentDirection::Export,
            now,
            Duration::hours(2),
            &sim,
            &frames,
            &snapshot,
        );
        assert_eq!(curve.steps[0].power_kw, -4.0); // Export is signed negative now.
        assert_eq!(
            curve.steps.last().unwrap().power_kw,
            0.0,
            "PV's export ceiling must drop at the night frame, not stay flat at -4.0"
        );
    }

    // ── compute_site_headroom_forecast ──────────────────────────────────────

    #[test]
    fn a_fully_charged_battery_reports_zero_absolute_import_headroom() {
        let now = t0();
        let sim = SimState::from_params(
            &[AssetParams::Battery(BatteryParams {
                id: ASSET_BATTERY.to_string(),
                capacity_kwh: 10.0,
                max_charge_kw: 5.0,
                max_discharge_kw: 5.0,
                initial_soc: 1.0,
                round_trip_efficiency: 1.0,
                min_soc: 0.1,
                c_terminal_eur_kwh: Some(0.0),
            })],
            now,
        );
        let plan = make_plan(900, 2, now);
        let forecast = compute_site_headroom_forecast(&sim, &plan, None, &[], now);
        assert!(
            forecast.iter().all(|s| s.down_kw == 0.0),
            "a fully-charged battery must report 0.0 absolute import headroom at every slot"
        );
    }

    #[test]
    fn shiftable_load_contributes_via_the_same_primitive_as_other_assets() {
        use crate::assets::shiftable_load::ShiftableLoadAsset;
        use crate::assets::AssetHistoryBuffer;
        use crate::simulator::energy::EnergyCounter;
        use crate::simulator::AssetEntry;

        let now = t0();
        let plan = make_plan(900, 2, now); // 2 x 15-min slots
        let mut sim = SimState::from_params(&[], now);
        let entry = AssetEntry {
            id: "wm-1".to_string(),
            state: AssetState::ShiftableLoad(ShiftableLoadAsset::initial_state()),
            setpoint_kw: 0.0,
            last_power_kw: 0.0,
            energy: EnergyCounter::new(),
            history: AssetHistoryBuffer::new(3600),
        };
        let config: Box<dyn crate::assets::Asset> = Box::new(ShiftableLoadAsset {
            power_kw: 2.0,
            duration_min: 10,
            earliest_start: now,
            latest_end: now + Duration::minutes(30),
        });
        sim.add_asset(entry, config).unwrap();

        let forecast = compute_site_headroom_forecast(&sim, &plan, None, &[], now);
        assert!(
            forecast[0].down_kw >= 2.0,
            "an eligible-to-start shiftable load must contribute its power_kw \
             to the first slot's absolute import headroom, got {}",
            forecast[0].down_kw
        );
    }

    #[test]
    fn ev_headroom_zeroes_out_past_the_live_sessions_departure() {
        // Found before wiring into production: EvState::plugged is never
        // toggled by step() (the physics model doesn't know a car left), so
        // without this exclusion the EV would keep contributing headroom at
        // slots after it's actually departed -- the same gap
        // build_forecast_frames' own `include_at` closure exists to close
        // for the capability-frame path (simulator/forecast.rs's
        // ev_zeroes_out_past_the_live_sessions_departure test).
        use crate::entities::asset_params::EvParams;
        use crate::ids::ASSET_EV;
        use uuid::Uuid;

        let now = t0();
        let sim = SimState::from_params(
            &[AssetParams::Ev(EvParams {
                id: ASSET_EV.to_string(),
                max_charge_kw: 7.0,
                max_discharge_kw: 0.0,
                initial_soc: 0.5,
                battery_kwh: 60.0,
                soc_target: 0.8,
                default_charge_kw: 0.0,
                min_charge_kw: 1.4,
                response_delay_s: 0.0,
                v2g_capable: false,
            })],
            now,
        );
        let plan = make_plan(900, 4, now); // 4 x 15-min slots
        let session = EvSession {
            id: Uuid::new_v4(),
            target_soc: 0.8,
            departure_time: now + Duration::minutes(30), // departs after slot 1
            soft_deadline: false,
            mode: Default::default(),
            budget_eur: None,
            comfort_rates: vec![],
            created_at: now,
            updated_at: now,
        };

        let forecast = compute_site_headroom_forecast(&sim, &plan, Some(&session), &[], now);

        assert!(
            forecast[0].down_kw > 0.0,
            "EV should still contribute before its session's departure"
        );
        assert!(
            forecast[1].down_kw > 0.0,
            "EV should still contribute right up to (not including) departure"
        );
        assert_eq!(
            forecast[2].down_kw, 0.0,
            "EV must not contribute past its live session's departure_time"
        );
        assert_eq!(forecast[3].down_kw, 0.0);
    }

    #[test]
    fn shiftable_load_forced_to_run_under_export_reports_its_own_positive_draw() {
        // Found via review: unlike every other asset kind, ShiftableLoadAsset's
        // max_effort_schedule Export branch reports its own POSITIVE draw once
        // running (it has no way to actually export) -- confirms this real
        // physics correctly flows through compute_site_capacity_curve's
        // generic asset_max_power_series path, not just merge_events' own
        // unit tests, closing the untested combination the review flagged.
        use crate::assets::shiftable_load::ShiftableLoadAsset;
        use crate::assets::AssetHistoryBuffer;
        use crate::simulator::energy::EnergyCounter;
        use crate::simulator::AssetEntry;

        let now = t0();
        let mut sim = SimState::from_params(&[], now);
        let entry = AssetEntry {
            id: "wm-1".to_string(),
            state: AssetState::ShiftableLoad(ShiftableLoadAsset::initial_state()),
            setpoint_kw: 0.0,
            last_power_kw: 0.0,
            energy: EnergyCounter::new(),
            history: AssetHistoryBuffer::new(3600),
        };
        let config: Box<dyn crate::assets::Asset> = Box::new(ShiftableLoadAsset {
            power_kw: 2.0,
            duration_min: 10,
            earliest_start: now,
            latest_end: now + Duration::minutes(30),
        });
        sim.add_asset(entry, config).unwrap();
        let snapshot = sim.to_sim_snapshot();

        // Export placement: start as late as possible = latest_end - duration
        // = now+20min, running through now+30min.
        let curve = compute_site_capacity_curve(
            CommitmentDirection::Export,
            now,
            Duration::minutes(30),
            &sim,
            &[],
            &snapshot,
        );
        assert!(
            curve.steps.iter().any(|s| s.power_kw > 0.0),
            "a non-interruptible shiftable load forced to run under a sustained \
             Export commitment must produce a genuinely positive (net-importing) \
             step, not silently 0.0 or negative, got {:?}",
            curve.steps
        );
    }
}
