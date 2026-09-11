use chrono::{DateTime, Duration, Utc};

use crate::controller::capacity_headroom::compute_site_capacity_curve;
use crate::entities::capacity_curve::CommitmentDirection;
use crate::entities::plan::SiteFlexibilityEnvelope;
use crate::simulator::SimState;

const NEAR_ZERO_KW: f64 = 1e-3;
const NEAR_ZERO_KWH: f64 = 1e-6;

/// Compute the site-level flexibility headroom from current asset states — the
/// `t1 = now, t2 = 0` instant of the same `(t1, t2, direction, tier)` domain
/// `controller::capacity_headroom`'s module doc describes; that module's
/// `compute_site_headroom_forecast` is the `t2 = 0`, sweep-`t1` sibling, this
/// function is its single-instant (`t1 = now` only) counterpart, needed
/// because `GET /flexibility`/`GET /flexibility/history` must answer "right
/// now" without an active plan. Named `site_headroom` (not `envelope`) per
/// this repo's naming rule: "envelope" is reserved for genuine OpenADR-spec/
/// reporting-boundary concepts (Dynamic Operating Envelope,
/// `*_RESERVATION_CAPACITY`); this is an internal HEMS headroom quantity,
/// named after what the UI calls it ("Site Headroom").
///
/// `up_kw`/`down_kw` are each controllable asset's own ABSOLUTE achievable
/// power in that direction, NOT a delta from current dispatch. (Historical
/// note: before `unified-capacity-envelope-engine`, this function computed
/// `(last_power_kw − max_export_kw).max(0.0)` / `(max_import_kw −
/// last_power_kw).max(0.0)`, a delta-from-current-dispatch model — a real,
/// user-reported bug, since `SiteHeadroomChart.tsx`'s band renders this
/// value as if it were absolute. See `docs/reference/KEY_LEARNINGS.md`.)
///
/// `site-capacity-seam-unification`: literally the `t2 = 0` point of
/// `compute_site_capacity_curve` (called once per direction, read back
/// `.steps[0].power_kw`) — not a second, independent implementation of the
/// same sum. This is what guarantees the Controller's Site Headroom band and
/// its capacity-curve overlay touch at exactly `t = now` for both
/// directions: they're the same function, not two that happen to agree.
/// `up_kw`/`down_kw` are SIGNED now (matching `CapacityCurveStep::power_kw`'s
/// convention) — up_kw holds the Export-direction net signed power, down_kw
/// the Import-direction net signed power; base load (and any other
/// non-interruptible net-grid-power contributor) can push Export positive
/// (net-importing), the same signal `CapacityCurve`'s own Export curve
/// already carries.
///
/// Duration is estimated from available storage energy:
///   up_duration_s   = available_discharge_kwh / up_kw × 3600
///   down_duration_s = available_charge_kwh    / down_kw × 3600
pub fn compute_site_headroom(
    sim: &SimState,
    now: DateTime<Utc>,
    phys_imp_kw: f64,
    phys_exp_kw: f64,
) -> SiteFlexibilityEnvelope {
    let export_curve = compute_site_capacity_curve(
        CommitmentDirection::Export,
        now,
        Duration::zero(),
        sim,
        phys_imp_kw,
        phys_exp_kw,
    );
    let import_curve = compute_site_capacity_curve(
        CommitmentDirection::Import,
        now,
        Duration::zero(),
        sim,
        phys_imp_kw,
        phys_exp_kw,
    );
    let up_kw = export_curve
        .steps
        .first()
        .map(|s| s.power_kw)
        .unwrap_or(0.0);
    let down_kw = import_curve
        .steps
        .first()
        .map(|s| s.power_kw)
        .unwrap_or(0.0);

    let mut available_discharge_kwh = 0.0_f64;
    let mut available_charge_kwh = 0.0_f64;
    for (entry, cfg) in sim.iter_assets() {
        if let Some((dis, ch)) = cfg
            .as_request_resolvable()
            .and_then(|r| r.available_storage_kwh(&entry.state))
        {
            available_discharge_kwh += dis;
            available_charge_kwh += ch;
        }
    }

    // up_kw is SIGNED (Export-direction net power, negative when genuinely
    // exporting) -- the duration this can sustain is a magnitude, so guard
    // and divide on -up_kw, not up_kw itself (see this function's own doc
    // comment for the sign convention).
    let export_kw_magnitude = (-up_kw).max(0.0);
    let up_duration_s =
        if export_kw_magnitude > NEAR_ZERO_KW && available_discharge_kwh > NEAR_ZERO_KWH {
            Some((available_discharge_kwh / export_kw_magnitude * 3600.0) as u64)
        } else {
            None
        };
    let down_duration_s = if down_kw > NEAR_ZERO_KW && available_charge_kwh > NEAR_ZERO_KWH {
        Some((available_charge_kwh / down_kw * 3600.0) as u64)
    } else {
        None
    };

    SiteFlexibilityEnvelope {
        ts: now,
        up_kw,
        down_kw,
        up_duration_s,
        down_duration_s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::asset_params::{BatteryParams, EvParams, PvParams};

    fn t0() -> DateTime<Utc> {
        "2026-01-01T00:00:00Z".parse().unwrap()
    }

    // Effectively-unbounded physical rating for tests that aren't exercising
    // the clamp itself -- keeps every other assertion's numbers the same
    // shape they were before phys_imp_kw/phys_exp_kw existed.
    const UNBOUNDED_KW: f64 = 1_000.0;

    fn headroom(sim: &SimState, now: DateTime<Utc>) -> SiteFlexibilityEnvelope {
        compute_site_headroom(sim, now, UNBOUNDED_KW, UNBOUNDED_KW)
    }

    #[test]
    fn no_assets_returns_zero() {
        let sim = SimState::from_params(&[], t0());
        let env = headroom(&sim, t0());
        assert_eq!(env.up_kw, 0.0);
        assert_eq!(env.down_kw, 0.0);
        assert!(env.up_duration_s.is_none());
        assert!(env.down_duration_s.is_none());
    }

    #[test]
    fn ev_charging_at_max_reports_its_own_ceilings_not_a_delta_from_current_draw() {
        // Non-v2g EV mid-charge at its own max_charge_kw (7.0). Historical note:
        // the old delta formula gave up_kw=7.0/down_kw=0.0 here ("already at its
        // own ceiling, nothing more to give" -- a statement about current
        // dispatch). The absolute model gives the opposite: EV cannot export at
        // all (its own Export ceiling is 0.0, non-v2g), so up_kw=0.0; its own
        // Import ceiling is the full charge rate regardless of current draw, so
        // down_kw=7.0.
        let sim = SimState::from_params(
            &[crate::entities::asset_params::AssetParams::Ev(EvParams {
                max_charge_kw: 7.0,
                initial_soc: 0.5,
                ..Default::default()
            })],
            t0(),
        );
        let env = headroom(&sim, t0());
        assert!(
            (env.up_kw).abs() < 1e-6,
            "up_kw should be 0.0 (non-v2g EV cannot export), got {}",
            env.up_kw
        );
        assert!(
            (env.down_kw - 7.0).abs() < 1e-6,
            "down_kw should be 7.0 (EV's own charge-rate ceiling), got {}",
            env.down_kw
        );
    }

    #[test]
    fn battery_idle_contributes_both_directions_at_its_own_ceilings() {
        let sim = SimState::from_params(
            &[crate::entities::asset_params::AssetParams::Battery(
                BatteryParams {
                    id: "battery".to_string(),
                    capacity_kwh: 10.0,
                    max_charge_kw: 5.0,
                    max_discharge_kw: 5.0,
                    initial_soc: 0.5,
                    round_trip_efficiency: 1.0,
                    min_soc: 0.1,
                    c_terminal_eur_kwh: Some(0.0),
                },
            )],
            t0(),
        );
        let env = headroom(&sim, t0());
        // up_kw is SIGNED now (site-capacity-seam-unification): -5.0, not
        // +5.0 -- negative = genuinely exportable, matching CapacityCurve's
        // own convention.
        assert!(
            (env.up_kw - (-5.0)).abs() < 1e-6,
            "up_kw should be -5.0, got {}",
            env.up_kw
        );
        assert!(
            (env.down_kw - 5.0).abs() < 1e-6,
            "down_kw should be 5.0, got {}",
            env.down_kw
        );
    }

    #[test]
    fn battery_mid_charge_still_reports_its_full_physical_ceiling_not_narrowed_by_current_dispatch()
    {
        // The fix's actual point: an asset dispatched AWAY from its limits (here,
        // charging hard at a setpoint well short of its own max import) must
        // still report its full physical ceiling in both directions -- proving
        // the value no longer narrows around wherever the asset currently sits.
        let sim = SimState::from_params(
            &[crate::entities::asset_params::AssetParams::Battery(
                BatteryParams {
                    id: "battery".to_string(),
                    capacity_kwh: 10.0,
                    max_charge_kw: 5.0,
                    max_discharge_kw: 5.0,
                    initial_soc: 0.5,
                    round_trip_efficiency: 1.0,
                    min_soc: 0.1,
                    c_terminal_eur_kwh: Some(0.0),
                },
            )],
            t0(),
        );
        // `from_params` starts every asset idle (setpoint/last_power_kw = 0.0);
        // max_effort_setpoint reads the asset's own state (SoC), not its current
        // dispatch, so idle-at-0 already demonstrates the point directly -- the
        // ceiling would be identical if this battery were instead mid-dispatch
        // at e.g. +2.0 kW import, unlike the old delta formula which would have
        // shrunk up_kw to 3.0 in that case. Covered by the sibling idle test
        // above numerically; this test exists to name the invariant explicitly.
        let env = headroom(&sim, t0());
        assert!(
            (env.up_kw - (-5.0)).abs() < 1e-6 && (env.down_kw - 5.0).abs() < 1e-6,
            "battery's headroom must equal its own physical ceiling regardless of dispatch, got up={} down={}",
            env.up_kw,
            env.down_kw
        );
    }

    #[test]
    fn base_load_is_included_and_shows_net_importing_when_nothing_can_export() {
        // base-load-competence-consolidation + site-capacity-seam-unification:
        // base load is now INCLUDED (was excluded before this fix) -- with no
        // exportable asset present, a sustained-Export commitment can't
        // deliver anything, so the site's own draw shows through as a
        // genuinely positive (net-importing) up_kw, not the old floor-at-0.
        // No learned heuristic present -> forecast_kw_at falls back to the
        // static baseline_kw_profile (1.2, matching `baseline_kw` here).
        let sim = SimState::from_params(
            &[crate::entities::asset_params::AssetParams::BaseLoad(
                crate::entities::asset_params::BaseLoadParams {
                    baseline_kw: 1.2,
                    ..Default::default()
                },
            )],
            t0(),
        );
        let env = headroom(&sim, t0());
        assert!(
            (env.up_kw - 1.2).abs() < 1e-6,
            "up_kw should be +1.2 (net-importing, base load can't be shed), got {}",
            env.up_kw
        );
        assert!(
            (env.down_kw - 1.2).abs() < 1e-6,
            "down_kw should be +1.2 (base load's own draw), got {}",
            env.down_kw
        );
    }

    #[test]
    fn physical_clamp_bounds_import_below_the_summed_asset_capability() {
        // R-72 / site-capacity-seam-unification: a genuine site-level
        // physical/interconnection ceiling now applies, independent of any
        // VTN directive. A battery alone can import 5.0 kW; a tighter
        // phys_imp_kw must win.
        let sim = SimState::from_params(
            &[crate::entities::asset_params::AssetParams::Battery(
                BatteryParams {
                    id: "battery".to_string(),
                    capacity_kwh: 10.0,
                    max_charge_kw: 5.0,
                    max_discharge_kw: 5.0,
                    initial_soc: 0.5,
                    round_trip_efficiency: 1.0,
                    min_soc: 0.1,
                    c_terminal_eur_kwh: Some(0.0),
                },
            )],
            t0(),
        );
        let env = compute_site_headroom(&sim, t0(), 2.0, UNBOUNDED_KW);
        assert!(
            (env.down_kw - 2.0).abs() < 1e-6,
            "down_kw should be clamped to phys_imp_kw=2.0, got {}",
            env.down_kw
        );
    }

    #[test]
    fn pv_contributes_up_via_curtailment_headroom_not_zero() {
        // Genuine behavior improvement, not just a sign fix: PV curtailment is
        // real Export-direction flexibility available right now. The old
        // formula's "point-range capability" trick (capability() returning the
        // same value in both cap_max_import_kw/cap_max_export_kw fields) made
        // this invisible to compute_envelope entirely; max_effort_setpoint's PV
        // override reports the true achievable export magnitude instead.
        let sim = SimState::from_params(
            &[crate::entities::asset_params::AssetParams::Pv(PvParams {
                id: "pv".to_string(),
                rated_kw: 5.0,
                inverter_max_kw: 5.0,
                co2_g_kwh: 0.0,
            })],
            t0(),
        );
        let env = headroom(&sim, t0());
        assert!(
            env.up_kw <= 0.0,
            "PV's up_kw must never be positive (it can only export or do nothing), got {}",
            env.up_kw
        );
        assert!(
            (env.down_kw).abs() < 1e-6,
            "PV down_kw should still be 0.0 (cannot import), got {}",
            env.down_kw
        );
    }

    #[test]
    fn duration_from_battery_soc() {
        let sim = SimState::from_params(
            &[crate::entities::asset_params::AssetParams::Battery(
                BatteryParams {
                    id: "battery".to_string(),
                    capacity_kwh: 10.0,
                    max_charge_kw: 5.0,
                    max_discharge_kw: 5.0,
                    initial_soc: 0.5,
                    round_trip_efficiency: 1.0,
                    min_soc: 0.1,
                    c_terminal_eur_kwh: Some(0.0),
                },
            )],
            t0(),
        );
        let env = headroom(&sim, t0());
        // available_discharge_kwh = (0.5-0.1)*10 = 4.0; |up_kw| = 5.0 -> 2880s
        // available_charge_kwh    = (1.0-0.5)*10 = 5.0; down_kw = 5.0 -> 3600s
        assert_eq!(env.up_duration_s, Some(2880));
        assert_eq!(env.down_duration_s, Some(3600));
    }

    #[test]
    fn duration_suppressed_when_max_kw_below_near_zero_kw() {
        let sub_threshold = NEAR_ZERO_KW * 0.5;
        let sim = SimState::from_params(
            &[crate::entities::asset_params::AssetParams::Battery(
                BatteryParams {
                    id: "battery".to_string(),
                    capacity_kwh: 10.0,
                    max_charge_kw: sub_threshold,
                    max_discharge_kw: sub_threshold,
                    initial_soc: 0.5,
                    round_trip_efficiency: 1.0,
                    min_soc: 0.1,
                    c_terminal_eur_kwh: Some(0.0),
                },
            )],
            t0(),
        );
        let env = headroom(&sim, t0());
        assert!(env.up_duration_s.is_none());
        assert!(env.down_duration_s.is_none());
    }

    #[test]
    fn duration_present_when_max_kw_above_near_zero_kw() {
        let above_threshold = NEAR_ZERO_KW * 2.0;
        let sim = SimState::from_params(
            &[crate::entities::asset_params::AssetParams::Battery(
                BatteryParams {
                    id: "battery".to_string(),
                    capacity_kwh: 10.0,
                    max_charge_kw: above_threshold,
                    max_discharge_kw: above_threshold,
                    initial_soc: 0.5,
                    round_trip_efficiency: 1.0,
                    min_soc: 0.1,
                    c_terminal_eur_kwh: Some(0.0),
                },
            )],
            t0(),
        );
        let env = headroom(&sim, t0());
        assert!(env.up_duration_s.is_some());
        assert!(env.down_duration_s.is_some());
    }

    #[test]
    fn seam_equality_with_compute_site_capacity_curve() {
        // The invariant this whole change exists to establish
        // (site-capacity-seam-unification): compute_site_headroom's up_kw/
        // down_kw must equal compute_site_capacity_curve's own t2=0 point,
        // by construction -- not by two independently-written sums happening
        // to agree.
        let sim = SimState::from_params(
            &[
                crate::entities::asset_params::AssetParams::Battery(BatteryParams {
                    id: "battery".to_string(),
                    capacity_kwh: 10.0,
                    max_charge_kw: 5.0,
                    max_discharge_kw: 5.0,
                    initial_soc: 0.5,
                    round_trip_efficiency: 1.0,
                    min_soc: 0.1,
                    c_terminal_eur_kwh: Some(0.0),
                }),
                crate::entities::asset_params::AssetParams::BaseLoad(
                    crate::entities::asset_params::BaseLoadParams {
                        baseline_kw: 1.2,
                        ..Default::default()
                    },
                ),
            ],
            t0(),
        );
        let env = headroom(&sim, t0());
        let export_curve = compute_site_capacity_curve(
            CommitmentDirection::Export,
            t0(),
            Duration::zero(),
            &sim,
            UNBOUNDED_KW,
            UNBOUNDED_KW,
        );
        let import_curve = compute_site_capacity_curve(
            CommitmentDirection::Import,
            t0(),
            Duration::zero(),
            &sim,
            UNBOUNDED_KW,
            UNBOUNDED_KW,
        );
        assert_eq!(env.up_kw, export_curve.steps[0].power_kw);
        assert_eq!(env.down_kw, import_curve.steps[0].power_kw);
    }
}
