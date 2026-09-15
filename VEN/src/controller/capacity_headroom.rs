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
//! - **Capacity Forecast from a future start** — `t1` = one remaining plan
//!   slot boundary, sweep `t2` (`compute_site_capacity_curves_at`, the
//!   Controller's "Move commitment start" mode).
//!
//! All of them go through one computation, `site_capacity_curve_from`; they
//! differ only in `t1`, `t2_max`, and whether each asset starts from its live
//! or its plan-forecasted state.
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
//! **No PV special-casing** (`pv-competence-consolidation` section 5,
//! `movable-capacity-curve-start`). PV flows through the same
//! `asset_max_power_series`/`simulated_trajectory` primitives every other
//! asset kind uses, backed by `PvInverter`'s own weather/decay-aware
//! `max_effort_schedule`/`simulate_forward` overrides, which measure elapsed
//! time from the instant PV's live inputs were captured
//! (`PvInverter::live_inputs_at`) — so a projection starting at a future slot
//! reads the forecast there, not the live measurement.
//!
//! **`CapacityCurve`/`CapacityCurveStep::power_kw` is SIGNED** (positive =
//! import, negative = export — the same convention `Asset::max_effort_setpoint`/
//! `capability()` use everywhere else), not the unsigned magnitude the
//! deleted `capacity_forecast.rs` originally used. The unsigned convention
//! is external — it applies only where an actual external consumer needs it
//! (OpenADR's `STORAGE_MAX_CHARGE_POWER`/`STORAGE_MAX_DISCHARGE_POWER`/
//! `*_RESERVATION_CAPACITY` report payloads are direction-tagged by name and
//! want a magnitude — see `report_intervals.rs::build_capacity_forecast_intervals`
//! and `reporter.rs`'s `IMPORT`/`EXPORT_RESERVATION_CAPACITY` arms); the UI
//! (`CapacityForecastChart.tsx`, `SiteHeadroomChart.tsx`) needs no such
//! conversion, confirmed by reading its formatters and energy calc rather
//! than assumed. **`SiteFlexibilityEnvelope`/`SiteFlexibilityForecastSlot`'s
//! `up_kw`/`down_kw` are signed too now** (`site-capacity-seam-unification`)
//! — they're literally this module's own `t2 = 0` point / `t1`-sweep, not a
//! separately-converted magnitude (see those structs' own doc comments in
//! `entities/plan.rs`).
//!
//! The Controller's Site Headroom chart (`SiteHeadroomChart.tsx`) overlays
//! both — the headroom band and the capacity curves — in one view. A curve
//! touches the band exactly at its own start, whether that is now or a
//! future slot (the band's value there is the same function's `t2 = 0`
//! point, not an independent computation that happens to agree). Past the
//! start they legitimately diverge — different questions (per-instant
//! snapshot along the plan's own trajectory vs. a single continuous
//! full-effort commitment from the start) — so the capacity curve sitting
//! inside the band there, or an Export curve swinging positive past the
//! band's usual scale (this module's own `merge_events` doc, below), is
//! still expected past the seam.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};

use crate::assets::{asset_max_power_series, AssetState, Trajectory};
use crate::entities::capacity_curve::{
    CapacityCurve, CapacityCurveStep, CapacityCurves, CommitmentDirection, LimitTier,
};
use crate::entities::plan::{Plan, SiteFlexibilityForecastSlot};
#[cfg(test)]
use crate::ids::ASSET_PV;
use crate::simulator::forecast::{
    plan_state_boundary_at, remaining_slots, resolve_plan_state_at, simulated_trajectory,
};
use crate::simulator::{AssetEntry, SimState};

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
    phys_imp_kw: f64,
    phys_exp_kw: f64,
) -> CapacityCurve {
    site_capacity_curve_from(
        direction,
        now,
        t2_max,
        sim,
        |_, entry| &entry.state,
        phys_imp_kw,
        phys_exp_kw,
    )
}

/// The one capacity-curve computation every slice of the `(t1, t2)` domain
/// goes through (`compute_site_capacity_curve`, `compute_site_capacity_curves_at`,
/// `compute_site_headroom_forecast`): a sustained `direction` commitment
/// starting at `t1`, each asset starting from `state_of(asset_index, entry)`
/// — its live state for a commitment starting now, its plan-forecasted state
/// for a later `t1`. Asset configs (PV's weather series, the EV's departure,
/// base load's heuristic) always come from the live `sim`; each is already
/// time-aware by timestamp, so only the states differ between slices.
fn site_capacity_curve_from<'s>(
    direction: CommitmentDirection,
    t1: DateTime<Utc>,
    t2_max: Duration,
    sim: &'s SimState,
    state_of: impl Fn(usize, &'s AssetEntry) -> &'s AssetState,
    phys_imp_kw: f64,
    phys_exp_kw: f64,
) -> CapacityCurve {
    let mut events: Vec<Event> = Vec::new();

    for (asset_index, (entry, cfg)) in sim.iter_assets().enumerate() {
        // pv-competence-consolidation section 5a: PV is no longer special-
        // cased here — asset_max_power_series already calls PvInverter's own
        // max_effort_schedule (weather/decay-aware), which is now
        // authoritative for this the same way it is for every other asset
        // kind (see this module's own doc comment, revised).
        if cfg.asset_type_str() == "base_load" {
            continue;
        }
        let series = asset_max_power_series(
            cfg,
            state_of(asset_index, entry),
            t1,
            t2_max,
            direction,
            LimitTier::Physical,
        );
        events.extend(series_to_events(&series));
    }
    events.extend(base_load_capacity_events(sim, t1, t2_max));

    CapacityCurve {
        direction,
        start: t1,
        steps: merge_events(events, direction, phys_imp_kw, phys_exp_kw),
    }
}

// ─── Capacity Forecast from a future start (t1 = a plan slot, sweep t2) ────

/// Both capacity curves for a commitment starting at a future plan slot
/// boundary instead of now (the Controller's "Move commitment start" mode,
/// `GET /flexibility/capacity?start=`). `start` snaps down to a remaining
/// slot boundary (`plan_state_boundary_at`), clamped to the last remaining
/// slot's start; each asset starts from its plan-forecasted state there
/// (`resolve_plan_state_at`) and the sweep runs to the plan's horizon end.
///
/// `None` wherever the curve would start at now — `start <= now`, no
/// remaining slot, or `start` before the first remaining boundary — so the
/// caller serves the per-tick curves instead of recomputing them.
pub fn compute_site_capacity_curves_at(
    sim: &SimState,
    plan: &Plan,
    start: DateTime<Utc>,
    now: DateTime<Utc>,
    phys_imp_kw: f64,
    phys_exp_kw: f64,
) -> Option<CapacityCurves> {
    let last_slot_start = remaining_slots(plan, now).last()?.start;
    let t1 = plan_state_boundary_at(plan, start.min(last_slot_start), now)?;
    let states = resolve_plan_state_at(sim, plan, t1, now);
    let t2_max = (plan.horizon.end_time - t1).max(Duration::zero());
    let curve = |direction| {
        site_capacity_curve_from(
            direction,
            t1,
            t2_max,
            sim,
            |_, entry| states.get(&entry.id).unwrap_or(&entry.state),
            phys_imp_kw,
            phys_exp_kw,
        )
    };
    Some(CapacityCurves {
        start: t1,
        import: curve(CommitmentDirection::Import),
        export: curve(CommitmentDirection::Export),
    })
}

/// Converts a dense `asset_max_power_series` output into sparse delta events
/// (design.md D3: dense compute, sparse output) — one event per point where
/// power actually changes, matching `CapacityCurveStep`'s existing
/// "ordered by elapsed_s ascending" breakpoint contract. Uses the series'
/// own raw signed `power_kw` directly — `CapacityCurve` is signed (positive =
/// import, negative = export, the same convention `Asset::max_effort_setpoint`
/// uses), so no conversion happens here.
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
/// question to ask it. Its contribution is a NET-GRID-POWER offset instead,
/// exactly as `capacity_forecast.rs`'s own module doc explained — but that
/// offset is time-varying, not a flat snapshot: `site-capacity-seam-
/// unification` swapped the old single `actual_power_kw` read (held constant
/// across the whole `t2_max` sweep — a known-crude approximation once
/// `BaseLoad::forecast_kw_at` existed) for `BaseLoad`'s own learned-heuristic
/// forecast (`base-load-competence-consolidation`), sampled hourly — the
/// heuristic's own bucket resolution (weekday × hour, `AssetHeuristics::
/// sample_kw`), so finer sampling would only repeat the same value. `None`
/// live `"base_load"` asset → no contribution, matching every other producer
/// here's "absent asset contributes nothing" convention.
///
/// **Direction-independent**, unlike the deleted `capacity_forecast.rs`'s
/// own `base_load_events` (which negated for Export): base load's physical
/// contribution to net grid power doesn't care what direction the SITE is
/// committing to — it always draws `+forecast_kw` (positive, the universal
/// signed convention every other producer here now also uses directly).
/// Summed together with an exporting asset's own negative contribution, this
/// correctly and automatically produces a *smaller* magnitude net export
/// (the old behavior) — and, if base load's draw exceeds what's exportable,
/// a genuinely positive (net-importing) result instead of the old code's
/// artificial floor at `0.0`. That floor was an artifact of the old
/// unsigned-magnitude representation (a magnitude can't go below zero by
/// definition), not a real physical constraint — the site genuinely can
/// still be net-importing even while every exportable asset is maxed out,
/// and the old code was silently discarding that fact. (Base load isn't the
/// only contributor that can do this — see `merge_events`'s doc comment for
/// `ShiftableLoadAsset`'s own version of the same effect.)
fn base_load_capacity_events(sim: &SimState, t1: DateTime<Utc>, t2_max: Duration) -> Vec<Event> {
    let Some((_, cfg)) = sim.find_asset(crate::ids::ASSET_BASE_LOAD) else {
        return Vec::new();
    };
    let Some(bl) = cfg.as_any().downcast_ref::<crate::assets::BaseLoad>() else {
        return Vec::new();
    };
    let t2_max_s = t2_max.num_seconds().max(0);
    let mut events = Vec::new();
    let mut prev_kw = 0.0_f64;
    let mut elapsed_s = 0_i64;
    loop {
        let forecast_kw = bl.forecast_kw_at(t1 + Duration::seconds(elapsed_s));
        let delta = forecast_kw - prev_kw;
        if delta != 0.0 {
            events.push((elapsed_s, delta));
        }
        prev_kw = forecast_kw;
        if elapsed_s >= t2_max_s {
            break;
        }
        elapsed_s = (elapsed_s + 3600).min(t2_max_s);
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
/// above by `phys_imp_kw` — the site's genuine physical/interconnection
/// import rating (`profile.grid.max_import_kw`), not any VTN-imposed
/// directive (R-72: `site-capacity-seam-unification` replaced the old
/// `snapshot.grid.import_limit_kw`/`export_limit_kw` — the VTN's *current,
/// revocable* capacity-limit event — with this genuine physical ceiling; a
/// VTN-restricted tier layered on top is deferred to a future change).
/// Export's floor is `-phys_exp_kw` (the site's physical export rating) —
/// but Export ALSO needs an upper bound at `phys_imp_kw`, not left unbounded:
/// a sustained Export commitment's net total can legitimately swing positive
/// (net importing) when non-exportable contributions exceed what's
/// exportable — not just base load's draw, but also a non-interruptible
/// `ShiftableLoadAsset` whose deadline forces it to keep drawing `power_kw`
/// once started, regardless of the site's Export commitment (found via
/// review: `ShiftableLoadAsset::max_effort_schedule`'s Export branch places
/// the run as late as possible but still reports its own positive draw once
/// running — it has no way to actually export). Since the site's real grid
/// hardware still can't exceed `phys_imp_kw` even while "trying" to export,
/// that swing must be bounded by the same limit Import direction itself
/// uses, not left to grow arbitrarily.
fn merge_events(
    events: Vec<Event>,
    direction: CommitmentDirection,
    phys_imp_kw: f64,
    phys_exp_kw: f64,
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
                CommitmentDirection::Import => running.clamp(0.0, phys_imp_kw),
                CommitmentDirection::Export => running.clamp(-phys_exp_kw, phys_imp_kw),
            };
            CapacityCurveStep {
                elapsed_s,
                power_kw,
            }
        })
        .collect()
}

// ─── Site Headroom (t2 = 0, sweep t1 across plan slots) ────────────────────

/// Each future plan slot's absolute achievable import/export power — NOT a
/// delta from the plan's own chosen dispatch (design.md D5: this is the
/// resolved absolute-vs-relative product decision). `up_kw` = absolute max
/// export achievable at this slot; `down_kw` = absolute max import
/// achievable at this slot (same direction mapping `CapacityCurve` uses).
///
/// Each slot is literally the `t2 = 0` point of `site_capacity_curve_from`,
/// started from that slot's plan-forecasted states — the same way
/// `compute_site_headroom` is its `t2 = 0` point at now, and the same
/// function `compute_site_capacity_curves_at` sweeps from that slot. That is
/// what makes a curve anchored at any slot touch this band there
/// (`movable-capacity-curve-start`), by construction rather than by two
/// computations happening to agree. It also retired this function's own
/// PV branch, which read the plan trajectory's PV power — capped by the
/// plan's own PV setpoint, so any planned curtailment narrowed the export
/// side, although curtailment can always be released (Physical tier).
///
/// `ev-departure-consolidation`: `EvCharger`'s own `simulate_forward` forces
/// `plugged=false` for any trajectory point at or after the live session's
/// `departure_time`, so an EV past departure contributes nothing — no
/// site-level exclusion here.
pub fn compute_site_headroom_forecast(
    sim: &SimState,
    plan: &Plan,
    now: DateTime<Utc>,
    phys_imp_kw: f64,
    phys_exp_kw: f64,
) -> Vec<SiteFlexibilityForecastSlot> {
    let future_slots = remaining_slots(plan, now);
    if future_slots.is_empty() {
        return Vec::new();
    }
    // One plan-driven trajectory per asset, walked once for the whole horizon
    // (design.md D4) -- NOT once per slot, which would redundantly re-walk the
    // same trajectory for every slot requested.
    let trajectories: Vec<Trajectory> = sim
        .iter_assets()
        .map(|(entry, cfg)| simulated_trajectory(entry, cfg, &future_slots))
        .collect();
    future_slots
        .iter()
        .enumerate()
        .map(|(slot_index, slot)| {
            let headroom_kw = |direction| {
                site_capacity_curve_from(
                    direction,
                    slot.start,
                    Duration::zero(),
                    sim,
                    |asset_index, entry| {
                        trajectories[asset_index]
                            .points
                            .get(slot_index)
                            .map_or(&entry.state, |point| &point.state)
                    },
                    phys_imp_kw,
                    phys_exp_kw,
                )
                .steps[0]
                    .power_kw
            };
            SiteFlexibilityForecastSlot {
                ts: slot.start,
                up_kw: headroom_kw(CommitmentDirection::Export),
                down_kw: headroom_kw(CommitmentDirection::Import),
            }
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
        let curve = compute_site_capacity_curve(
            CommitmentDirection::Export,
            now,
            Duration::hours(2),
            &sim,
            1_000.0,
            1_000.0,
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
        let import_curve = compute_site_capacity_curve(
            CommitmentDirection::Import,
            now,
            Duration::hours(1),
            &sim,
            1_000.0,
            1_000.0,
        );
        let export_curve = compute_site_capacity_curve(
            CommitmentDirection::Export,
            now,
            Duration::hours(1),
            &sim,
            1_000.0,
            1_000.0,
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
        // credit its own current generation as import headroom. Section 5a:
        // now routed through asset_max_power_series/max_effort_setpoint like
        // every other asset, so the live PvInverter config itself (not a
        // constructed AssetForecastFrame) drives this.
        let now = t0();
        let mut sim = SimState::from_params(
            &[AssetParams::Pv(PvParams {
                id: ASSET_PV.to_string(),
                rated_kw: 5.0,
                inverter_max_kw: 5.0,
                co2_g_kwh: 0.0,
            })],
            now,
        );
        // Force PV to be actively generating right now (capability_inner
        // reads the state's own actual_power_kw, not a live recompute).
        let (entry, _) = sim.find_asset_mut(ASSET_PV).unwrap();
        let AssetState::Pv(s) = &mut entry.state else {
            panic!("expected Pv state")
        };
        s.actual_power_kw = -4.0;

        let curve = compute_site_capacity_curve(
            CommitmentDirection::Import,
            now,
            Duration::hours(1),
            &sim,
            1_000.0,
            1_000.0,
        );
        assert!(
            curve.steps.iter().all(|s| s.power_kw == 0.0),
            "PV must contribute exactly 0.0 to the Import curve while generating, got {:?}",
            curve.steps
        );
    }

    #[test]
    fn pv_generation_leaves_the_import_curve_equal_to_the_battery_only_curve() {
        // PV-only tests are blind here (merge_events floors Import at 0.0); next
        // to a battery, any PV leak would shave the battery's own import steps.
        let battery = AssetParams::Battery(BatteryParams {
            id: ASSET_BATTERY.to_string(),
            capacity_kwh: 10.0,
            max_charge_kw: 5.0,
            max_discharge_kw: 5.0,
            initial_soc: 0.5,
            round_trip_efficiency: 1.0,
            min_soc: 0.1,
            c_terminal_eur_kwh: Some(0.0),
        });
        let pv = AssetParams::Pv(PvParams {
            id: ASSET_PV.to_string(),
            rated_kw: 5.0,
            inverter_max_kw: 5.0,
            co2_g_kwh: 0.0,
        });
        let now = t0(); // noon: sin-model PV fully generating
        let import_curve = |params: &[AssetParams]| {
            compute_site_capacity_curve(
                CommitmentDirection::Import,
                now,
                Duration::hours(2),
                &SimState::from_params(params, now),
                1_000.0,
                1_000.0,
            )
        };
        assert_eq!(
            import_curve(&[battery.clone(), pv]).steps,
            import_curve(&[battery]).steps
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
        let curve = compute_site_capacity_curve(
            CommitmentDirection::Export,
            now,
            Duration::hours(1),
            &sim,
            1_000.0,
            1_000.0,
        );
        assert!(
            curve.steps.iter().all(|s| s.power_kw == 0.0),
            "heater must contribute exactly 0.0 to the Export curve, got {:?}",
            curve.steps
        );
    }

    #[test]
    fn pv_export_contribution_still_varies_with_the_weather_forecast() {
        // Section 5a: PV's Export contribution now flows through
        // asset_max_power_series -> PvInverter::max_effort_schedule, which
        // samples the live PvInverter's own weather_forecast field -- must
        // still genuinely vary across the horizon, not flatten to a constant.
        let now = t0();
        let mut sim = SimState::from_params(
            &[AssetParams::Pv(PvParams {
                id: ASSET_PV.to_string(),
                rated_kw: 5.0,
                inverter_max_kw: 5.0,
                co2_g_kwh: 0.0,
            })],
            now,
        );
        {
            let (_, cfg) = sim.find_asset_mut(ASSET_PV).unwrap();
            let pv = cfg
                .as_any_mut()
                .downcast_mut::<crate::assets::PvInverter>()
                .unwrap();
            pv.weather_forecast = Some(vec![
                crate::entities::solar::WeatherPvForecastSlot {
                    valid_at: now,
                    forecast_ac_kw: 4.0,
                    snow_covered: false,
                },
                crate::entities::solar::WeatherPvForecastSlot {
                    valid_at: now + Duration::seconds(3600),
                    forecast_ac_kw: 0.0, // night -- ceiling drops to 0
                    snow_covered: false,
                },
            ]);
        }
        let curve = compute_site_capacity_curve(
            CommitmentDirection::Export,
            now,
            Duration::hours(2),
            &sim,
            1_000.0,
            1_000.0,
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
        let forecast = compute_site_headroom_forecast(&sim, &plan, now, 1_000.0, 1_000.0);
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

        let forecast = compute_site_headroom_forecast(&sim, &plan, now, 1_000.0, 1_000.0);
        assert!(
            forecast[0].down_kw >= 2.0,
            "an eligible-to-start shiftable load must contribute its power_kw \
             to the first slot's absolute import headroom, got {}",
            forecast[0].down_kw
        );
    }

    #[test]
    fn ev_headroom_zeroes_out_past_the_live_sessions_departure() {
        // ev-departure-consolidation: EvCharger::simulate_forward now forces
        // plugged=false for any trajectory point at/after the live
        // departure_time, so capability_inner correctly zeroes out headroom
        // past it -- no site-level exclusion needed any more (this test used
        // to pass an EvSession parameter to compute_site_headroom_forecast
        // directly; that parameter is gone, the live EvCharger's own
        // departure_time now carries the same fact).
        use crate::assets::EvCharger;
        use crate::entities::asset_params::EvParams;
        use crate::ids::ASSET_EV;

        let now = t0();
        let mut sim = SimState::from_params(
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
        {
            let (_, cfg) = sim.find_asset_mut(ASSET_EV).unwrap();
            let ev = cfg.as_any_mut().downcast_mut::<EvCharger>().unwrap();
            ev.departure_time = Some(now + Duration::minutes(30)); // departs after slot 1
        }
        let plan = make_plan(900, 4, now); // 4 x 15-min slots

        let forecast = compute_site_headroom_forecast(&sim, &plan, now, 1_000.0, 1_000.0);

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
    fn pv_site_headroom_forecast_varies_across_future_slots() {
        // Section 5b regression: PV's up_kw contribution must genuinely vary
        // with each slot's own time-of-day, not flatten to a constant (the
        // bug simulate_forward's override exists to fix).
        let now = t0(); // noon UTC
        let sim = SimState::from_params(
            &[AssetParams::Pv(PvParams {
                id: ASSET_PV.to_string(),
                rated_kw: 5.0,
                inverter_max_kw: 5.0,
                co2_g_kwh: 0.0,
            })],
            now,
        );
        let plan = make_plan(12 * 3600, 2, now); // noon slot, then midnight slot
        let forecast = compute_site_headroom_forecast(&sim, &plan, now, 1_000.0, 1_000.0);
        // up_kw is signed now (site-capacity-seam-unification): negative
        // while genuinely generating/exportable, not positive.
        assert!(
            forecast[0].up_kw < 0.0,
            "noon slot must show real PV headroom, got {}",
            forecast[0].up_kw
        );
        assert_eq!(
            forecast[1].up_kw, 0.0,
            "midnight slot must show zero PV headroom, got {}",
            forecast[1].up_kw
        );
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

        // Export placement: start as late as possible = latest_end - duration
        // = now+20min, running through now+30min.
        let curve = compute_site_capacity_curve(
            CommitmentDirection::Export,
            now,
            Duration::minutes(30),
            &sim,
            1_000.0,
            1_000.0,
        );
        assert!(
            curve.steps.iter().any(|s| s.power_kw > 0.0),
            "a non-interruptible shiftable load forced to run under a sustained \
             Export commitment must produce a genuinely positive (net-importing) \
             step, not silently 0.0 or negative, got {:?}",
            curve.steps
        );
    }

    #[test]
    fn base_load_contribution_varies_hourly_not_held_flat_across_the_sweep() {
        // site-capacity-seam-unification: base_load_capacity_events used to
        // read a single actual_power_kw snapshot and hold it constant across
        // the whole t2_max sweep. With a learned heuristic now driving it
        // (base-load-competence-consolidation), two hours 12h apart with very
        // different learned values must produce genuinely different steps.
        use crate::entities::design_vocabulary::AssetHeuristics;
        let now = Utc.with_ymd_and_hms(2026, 9, 6, 0, 0, 0).unwrap(); // midnight
        let mut sim = SimState::from_params(
            &[AssetParams::BaseLoad(BaseLoadParams {
                baseline_kw: 0.3,
                ..Default::default()
            })],
            now,
        );
        {
            let (_, cfg) = sim.find_asset_mut(crate::ids::ASSET_BASE_LOAD).unwrap();
            let bl = cfg
                .as_any_mut()
                .downcast_mut::<crate::assets::BaseLoad>()
                .unwrap();
            let mut daytime_profile_kw: [Vec<f64>; 7] = Default::default();
            for day in daytime_profile_kw.iter_mut() {
                *day = vec![0.2; 24]; // low overnight
                day[12] = 3.0; // spike at noon
            }
            bl.heuristic = Some(AssetHeuristics {
                asset_id: crate::ids::ASSET_BASE_LOAD.to_string(),
                daytime_profile_kw,
                seasonal_factor: 1.0,
                last_updated: None,
                recent_mean_abs_error_kw: None,
            });
        }
        let curve = compute_site_capacity_curve(
            CommitmentDirection::Import,
            now,
            Duration::hours(13),
            &sim,
            1_000.0,
            1_000.0,
        );
        let midnight_kw = curve.steps[0].power_kw;
        let noon_kw = curve
            .steps
            .iter()
            .find(|s| s.elapsed_s == 12 * 3600)
            .expect("a breakpoint at elapsed_s=12h")
            .power_kw;
        assert!(
            (midnight_kw - 0.2).abs() < 1e-6,
            "midnight step should be 0.2, got {midnight_kw}"
        );
        assert!(
            (noon_kw - 3.0).abs() < 1e-6,
            "noon step should be 3.0, got {noon_kw}"
        );
    }

    // ── compute_site_capacity_curves_at (movable-capacity-curve-start) ─────

    fn battery_params(initial_soc: f64) -> AssetParams {
        AssetParams::Battery(BatteryParams {
            id: ASSET_BATTERY.to_string(),
            capacity_kwh: 10.0,
            max_charge_kw: 5.0,
            max_discharge_kw: 5.0,
            initial_soc,
            round_trip_efficiency: 1.0,
            min_soc: 0.1,
            c_terminal_eur_kwh: Some(0.0),
        })
    }

    /// 8 x 15-min slots from `now`, the battery charging at `charge_kw` in every one.
    fn charging_plan(now: DateTime<Utc>, charge_kw: f64) -> Plan {
        let mut plan = make_plan(900, 8, now);
        for slot in &mut plan.slots {
            slot.planned_kw_by_asset
                .insert(ASSET_BATTERY.to_string(), charge_kw);
        }
        plan
    }

    fn curves_at(
        sim: &SimState,
        plan: &Plan,
        start: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Option<CapacityCurves> {
        compute_site_capacity_curves_at(sim, plan, start, now, 1_000.0, 1_000.0)
    }

    #[test]
    fn site_capacity_curve_from_given_live_states_reproduces_the_per_tick_curve() {
        // The future-start path looks states up by asset id; fed the live
        // states, it must be indistinguishable from the per-tick curve.
        let now = t0();
        let sim = SimState::from_params(
            &[
                battery_params(0.6),
                AssetParams::BaseLoad(BaseLoadParams {
                    baseline_kw: 0.5,
                    ..Default::default()
                }),
                AssetParams::Pv(PvParams {
                    id: ASSET_PV.to_string(),
                    rated_kw: 5.0,
                    inverter_max_kw: 5.0,
                    co2_g_kwh: 0.0,
                }),
            ],
            now,
        );
        let live_states: Map<String, AssetState> = sim
            .iter_assets()
            .map(|(entry, _)| (entry.id.clone(), entry.state.clone()))
            .collect();
        for direction in [CommitmentDirection::Import, CommitmentDirection::Export] {
            let from_states = site_capacity_curve_from(
                direction,
                now,
                Duration::hours(2),
                &sim,
                |_, entry| live_states.get(&entry.id).unwrap_or(&entry.state),
                1_000.0,
                1_000.0,
            );
            let per_tick = compute_site_capacity_curve(
                direction,
                now,
                Duration::hours(2),
                &sim,
                1_000.0,
                1_000.0,
            );
            assert_eq!(from_states, per_tick, "{direction:?}");
        }
    }

    #[test]
    fn capacity_curves_at_a_mid_slot_start_snap_down_to_the_slot_start() {
        let now = t0();
        let sim = SimState::from_params(&[battery_params(0.5)], now);
        let plan = charging_plan(now, 0.0);
        let at = curves_at(&sim, &plan, plan.slots[3].start + Duration::minutes(8), now)
            .expect("a future start inside the plan must produce curves");
        assert_eq!(at.start, plan.slots[3].start);
        assert_eq!(at.import.start, plan.slots[3].start);
        assert_eq!(at.export.start, plan.slots[3].start);
    }

    #[test]
    fn capacity_curves_at_sweep_no_further_than_the_plan_horizon_end() {
        let now = t0();
        let sim = SimState::from_params(&[battery_params(0.5)], now);
        let plan = charging_plan(now, 0.0);
        let at = curves_at(&sim, &plan, plan.slots[2].start, now).unwrap();
        let max_elapsed_s = (plan.horizon.end_time - at.start).num_seconds();
        for curve in [&at.import, &at.export] {
            let last = curve.steps.last().unwrap().elapsed_s;
            assert!(
                last <= max_elapsed_s,
                "{:?} ends at {last}s, past the horizon end ({max_elapsed_s}s)",
                curve.direction
            );
        }
    }

    #[test]
    fn capacity_curves_at_start_from_the_plan_forecasted_state_not_the_live_one() {
        // Live soc 0.5 -> 5 kWh of charge room. The plan charges 2.5 kW for
        // the 4 slots before slot 4 (4 x 0.625 kWh), leaving 2.5 kWh there.
        let now = t0();
        let sim = SimState::from_params(&[battery_params(0.5)], now);
        let plan = charging_plan(now, 2.5);
        let at_now = compute_site_capacity_curve(
            CommitmentDirection::Import,
            now,
            plan.horizon.end_time - now,
            &sim,
            1_000.0,
            1_000.0,
        );
        let at_slot_4 = curves_at(&sim, &plan, plan.slots[4].start, now).unwrap();
        assert!(
            (at_now.energy_kwh_total() - 5.0).abs() < 0.1,
            "import energy behind a commitment now: {}",
            at_now.energy_kwh_total()
        );
        assert!(
            (at_slot_4.import.energy_kwh_total() - 2.5).abs() < 0.1,
            "import energy behind a commitment at slot 4: {}",
            at_slot_4.import.energy_kwh_total()
        );
    }

    #[test]
    fn capacity_curves_at_clamp_a_start_past_the_last_slot_to_that_slots_start() {
        let now = t0();
        let sim = SimState::from_params(&[battery_params(0.5)], now);
        let plan = charging_plan(now, 0.0);
        let at = curves_at(&sim, &plan, plan.horizon.end_time + Duration::hours(4), now).unwrap();
        assert_eq!(at.start, plan.slots[7].start);
    }

    #[test]
    fn capacity_curves_at_are_none_wherever_the_curve_would_start_at_now() {
        let now = t0();
        let sim = SimState::from_params(&[battery_params(0.5)], now);
        let plan = charging_plan(now, 0.0);
        assert!(curves_at(&sim, &plan, now, now).is_none(), "start == now");
        assert!(
            curves_at(&sim, &plan, now - Duration::hours(1), now).is_none(),
            "start in the past"
        );
        let later = now + Duration::minutes(5); // plan's slot 0 already started
        assert!(
            curves_at(&sim, &plan, later + Duration::minutes(3), later).is_none(),
            "start before the first remaining slot boundary"
        );
        assert!(
            curves_at(&sim, &make_plan(900, 0, now), now + Duration::hours(1), now).is_none(),
            "no remaining slot"
        );
    }

    /// Battery (plan-charged), EV, heater, base load, a shiftable load, and PV
    /// with a live measurement, a weather forecast, and a plan that curtails it.
    fn mixed_fleet(now: DateTime<Utc>) -> (SimState, Plan) {
        use crate::assets::shiftable_load::ShiftableLoadAsset;
        use crate::assets::AssetHistoryBuffer;
        use crate::entities::asset_params::EvParams;
        use crate::ids::ASSET_EV;
        use crate::simulator::energy::EnergyCounter;
        use crate::simulator::AssetEntry;

        let mut sim = SimState::from_params(
            &[
                battery_params(0.5),
                AssetParams::Ev(EvParams {
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
                }),
                AssetParams::Heater(HeaterParams {
                    id: ASSET_HEATER.to_string(),
                    temp_initial_c: 20.0,
                    ..Default::default()
                }),
                AssetParams::BaseLoad(BaseLoadParams {
                    baseline_kw: 0.4,
                    ..Default::default()
                }),
                AssetParams::Pv(PvParams {
                    id: ASSET_PV.to_string(),
                    rated_kw: 8.0,
                    inverter_max_kw: 8.0,
                    co2_g_kwh: 0.0,
                }),
            ],
            now,
        );
        sim.add_asset(
            AssetEntry {
                id: "wm-1".to_string(),
                state: AssetState::ShiftableLoad(ShiftableLoadAsset::initial_state()),
                setpoint_kw: 0.0,
                last_power_kw: 0.0,
                energy: EnergyCounter::new(),
                history: AssetHistoryBuffer::new(3600),
            },
            Box::new(ShiftableLoadAsset {
                power_kw: 2.0,
                duration_min: 30,
                earliest_start: now + Duration::minutes(30),
                latest_end: now + Duration::hours(3),
            }),
        )
        .unwrap();
        {
            let (_, cfg) = sim.find_asset_mut(ASSET_PV).unwrap();
            let pv = cfg
                .as_any_mut()
                .downcast_mut::<crate::assets::PvInverter>()
                .unwrap();
            pv.weather_forecast = Some(
                (0..=3)
                    .map(|h| crate::entities::solar::WeatherPvForecastSlot {
                        valid_at: now + Duration::hours(h),
                        forecast_ac_kw: 6.0 - h as f64,
                        snow_covered: false,
                    })
                    .collect(),
            );
            pv.measured_power_kw = Some(1.5);
            pv.live_inputs_at = Some(now);
        }
        let mut plan = charging_plan(now, 2.5);
        for slot in &mut plan.slots {
            slot.planned_kw_by_asset.insert(ASSET_PV.to_string(), -1.0); // curtail to 1 kW
        }
        (sim, plan)
    }

    #[test]
    fn a_future_start_curve_touches_the_site_headroom_forecast_at_every_slot() {
        // The seam the chart relies on at `now` must hold at every slot a
        // curve can be anchored at: first step == that slot's band value.
        let now = t0();
        let (sim, plan) = mixed_fleet(now);
        let band = compute_site_headroom_forecast(&sim, &plan, now, 1_000.0, 1_000.0);
        for (i, slot) in plan.slots.iter().enumerate().skip(1) {
            let at = curves_at(&sim, &plan, slot.start, now).unwrap();
            assert_eq!(
                at.import.steps[0].power_kw, band[i].down_kw,
                "import seam at slot {i}"
            );
            assert_eq!(
                at.export.steps[0].power_kw, band[i].up_kw,
                "export seam at slot {i}"
            );
        }
    }

    #[test]
    fn site_headroom_forecast_reports_pv_uncurtailed_where_the_plan_curtails_it() {
        // Physical headroom: curtailment can always be released, so the plan's
        // own 1 kW PV cap must not narrow the export side. PV-only, so the
        // export side is PV's alone.
        let now = t0();
        let mut sim = SimState::from_params(
            &[AssetParams::Pv(PvParams {
                id: ASSET_PV.to_string(),
                rated_kw: 8.0,
                inverter_max_kw: 8.0,
                co2_g_kwh: 0.0,
            })],
            now,
        );
        {
            let (_, cfg) = sim.find_asset_mut(ASSET_PV).unwrap();
            let pv = cfg
                .as_any_mut()
                .downcast_mut::<crate::assets::PvInverter>()
                .unwrap();
            pv.weather_forecast = Some(vec![crate::entities::solar::WeatherPvForecastSlot {
                valid_at: now,
                forecast_ac_kw: 6.0,
                snow_covered: false,
            }]);
            pv.live_inputs_at = Some(now);
        }
        let mut plan = make_plan(900, 4, now);
        for slot in &mut plan.slots {
            slot.planned_kw_by_asset.insert(ASSET_PV.to_string(), -1.0);
        }
        let band = compute_site_headroom_forecast(&sim, &plan, now, 1_000.0, 1_000.0);
        assert!(
            band.iter().all(|s| (s.up_kw + 6.0).abs() < 1e-9),
            "every slot's export side must be the uncurtailed -6.0 kW, got {:?}",
            band.iter().map(|s| s.up_kw).collect::<Vec<_>>()
        );
    }

    #[test]
    fn physical_clamp_bounds_the_capacity_curve_below_summed_asset_capability() {
        // R-72 / site-capacity-seam-unification: phys_imp_kw/phys_exp_kw are
        // the site's genuine physical/interconnection rating, independent of
        // any VTN directive -- a tighter value than the summed asset
        // capability must still win.
        let now = t0();
        let sim = SimState::from_params(
            &[AssetParams::Battery(BatteryParams {
                id: ASSET_BATTERY.to_string(),
                capacity_kwh: 10.0,
                max_charge_kw: 5.0,
                max_discharge_kw: 5.0,
                initial_soc: 0.5,
                round_trip_efficiency: 1.0,
                min_soc: 0.1,
                c_terminal_eur_kwh: Some(0.0),
            })],
            now,
        );
        let curve = compute_site_capacity_curve(
            CommitmentDirection::Import,
            now,
            Duration::zero(),
            &sim,
            2.0,
            1_000.0,
        );
        assert_eq!(
            curve.steps[0].power_kw, 2.0,
            "import curve must be clamped to phys_imp_kw=2.0, not the battery's own 5.0 kW ceiling"
        );
    }
}
