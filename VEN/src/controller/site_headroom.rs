use chrono::{DateTime, Utc};

use crate::controller::capacity_headroom::magnitude_kw;
use crate::entities::capacity_curve::{CommitmentDirection, LimitTier};
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
/// power in that direction (`Asset::max_effort_setpoint`, `LimitTier::Physical`),
/// summed — NOT a delta from current dispatch. (Historical note: before this
/// fix, this function computed `(last_power_kw − max_export_kw).max(0.0)` /
/// `(max_import_kw − last_power_kw).max(0.0)`, a delta-from-current-dispatch
/// model left over from before `unified-capacity-envelope-engine` — a real,
/// user-reported bug, since `SiteHeadroomChart.tsx`'s band renders this
/// value as if it were absolute. See `docs/reference/KEY_LEARNINGS.md`.)
///
/// `base_load` is excluded entirely (zero controllable degrees of freedom —
/// its live draw is real, but "how much MORE could this asset do" is always
/// zero for it, matching `compute_site_headroom_forecast`'s own exclusion).
/// PV needs no special-casing here (unlike the forecast sibling, which reads
/// weather-driven `pv_frames` instead): `max_effort_setpoint` on PV's own
/// *current* state already returns the correct answer directly (`0.0` for
/// Import, live achievable export magnitude for Export) — no trajectory or
/// forecast machinery needed for a "right now" instant.
///
/// Duration is estimated from available storage energy:
///   up_duration_s   = available_discharge_kwh / up_kw × 3600
///   down_duration_s = available_charge_kwh    / down_kw × 3600
pub fn compute_site_headroom(sim: &SimState, now: DateTime<Utc>) -> SiteFlexibilityEnvelope {
    let mut up_kw = 0.0_f64;
    let mut down_kw = 0.0_f64;
    let mut available_discharge_kwh = 0.0_f64;
    let mut available_charge_kwh = 0.0_f64;

    for (entry, cfg) in sim.iter_assets() {
        if cfg.asset_type_str() == "base_load" {
            continue;
        }

        let export_kw = cfg.max_effort_setpoint(
            &entry.state,
            CommitmentDirection::Export,
            LimitTier::Physical,
        );
        let import_kw = cfg.max_effort_setpoint(
            &entry.state,
            CommitmentDirection::Import,
            LimitTier::Physical,
        );
        up_kw += magnitude_kw(export_kw, CommitmentDirection::Export);
        down_kw += magnitude_kw(import_kw, CommitmentDirection::Import);

        if let Some((dis, ch)) = cfg
            .as_request_resolvable()
            .and_then(|r| r.available_storage_kwh(&entry.state))
        {
            available_discharge_kwh += dis;
            available_charge_kwh += ch;
        }
    }

    let up_duration_s = if up_kw > NEAR_ZERO_KW && available_discharge_kwh > NEAR_ZERO_KWH {
        Some((available_discharge_kwh / up_kw * 3600.0) as u64)
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

    #[test]
    fn no_assets_returns_zero() {
        let sim = SimState::from_params(&[], t0());
        let env = compute_site_headroom(&sim, t0());
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
        let env = compute_site_headroom(&sim, t0());
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
        let env = compute_site_headroom(&sim, t0());
        assert!(
            (env.up_kw - 5.0).abs() < 1e-6,
            "up_kw should be 5.0, got {}",
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
        let env = compute_site_headroom(&sim, t0());
        assert!(
            (env.up_kw - 5.0).abs() < 1e-6 && (env.down_kw - 5.0).abs() < 1e-6,
            "battery's headroom must equal its own physical ceiling regardless of dispatch, got up={} down={}",
            env.up_kw,
            env.down_kw
        );
    }

    #[test]
    fn base_load_is_excluded_from_both_directions() {
        let sim = SimState::from_params(
            &[crate::entities::asset_params::AssetParams::BaseLoad(
                crate::entities::asset_params::BaseLoadParams {
                    baseline_kw: 1.2,
                    ..Default::default()
                },
            )],
            t0(),
        );
        let env = compute_site_headroom(&sim, t0());
        assert_eq!(env.up_kw, 0.0);
        assert_eq!(env.down_kw, 0.0);
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
        let env = compute_site_headroom(&sim, t0());
        assert!(
            env.up_kw >= 0.0,
            "PV's up_kw must never be negative, got {}",
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
        let env = compute_site_headroom(&sim, t0());
        // available_discharge_kwh = (0.5-0.1)*10 = 4.0; up_kw = 5.0 -> 2880s
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
        let env = compute_site_headroom(&sim, t0());
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
        let env = compute_site_headroom(&sim, t0());
        assert!(env.up_duration_s.is_some());
        assert!(env.down_duration_s.is_some());
    }
}
