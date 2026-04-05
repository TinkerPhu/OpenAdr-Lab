use crate::controller::thresholds::{NEAR_ZERO_KW, NEAR_ZERO_KWH};
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::assets::{AssetConfig, AssetState};
use crate::controller::openadr_interface::parse_firm_reservations;
use crate::controller::reservation::ReservationLayer;
use crate::entities::energy_packet::{EnergyPacket, PacketStatus};
use crate::entities::plan::SiteFlexibilityEnvelope;
use crate::simulator::SimState;

/// Compute the site-level flexibility envelope from current asset state and reservations.
///
/// Algorithm (§9):
///   For each asset:
///     phys_cap  = config.capability(&entry.state)
///     avail_cap = reservation_layer.available_cap(id, phys_cap, now)
///     up_kw    += (entry.last_power_kw − avail_cap.max_export_kw).max(0.0)
///     down_kw  += (avail_cap.max_import_kw − entry.last_power_kw).max(0.0)
///
/// Uncontrollable assets (PV, BaseLoad) have a point-range capability where
/// max_export_kw == max_import_kw == actual_power_kw, so they contribute 0
/// to both up_kw and down_kw. Correct by design.
///
/// Duration is estimated from available storage energy:
///   up_duration_s   = available_discharge_kwh / up_kw × 3600
///   down_duration_s = available_charge_kwh    / down_kw × 3600
/// Both are None when no storage assets are present or the corresponding kw is 0.
pub fn compute_envelope(
    sim: &SimState,
    reservation_layer: &ReservationLayer,
    packets: &[EnergyPacket],
    now: DateTime<Utc>,
) -> SiteFlexibilityEnvelope {
    let mut up_kw = 0.0_f64;
    let mut down_kw = 0.0_f64;
    let mut available_discharge_kwh = 0.0_f64;
    let mut available_charge_kwh = 0.0_f64;

    for (entry, config) in sim.iter_assets() {
        let phys_cap = config.capability(&entry.state);
        let avail_cap = reservation_layer.available_cap(&entry.id, phys_cap, now);

        // up: how much this asset can reduce its consumption from current level
        up_kw   += (entry.last_power_kw - avail_cap.max_export_kw).max(0.0);
        // down: how much this asset can increase its consumption from current level
        down_kw += (avail_cap.max_import_kw - entry.last_power_kw).max(0.0);

        // Duration estimate from storage assets
        match config {
            AssetConfig::Battery(b) => {
                let soc = match &entry.state {
                    AssetState::Battery(s) => s.soc,
                    _ => continue,
                };
                available_discharge_kwh += (soc - b.min_soc).max(0.0) * b.capacity_kwh;
                available_charge_kwh    += (1.0_f64 - soc).max(0.0) * b.capacity_kwh;
            }
            AssetConfig::Ev(e) => {
                let (soc, plugged) = match &entry.state {
                    AssetState::Ev(s) => (s.soc, s.plugged),
                    _ => continue,
                };
                if plugged {
                    available_discharge_kwh += (soc - e.min_soc).max(0.0) * e.battery_kwh;
                    available_charge_kwh    += (1.0_f64 - soc).max(0.0) * e.battery_kwh;
                }
            }
            _ => {}
        }
    }

    // Interruptible scheduled/active packets donate their running power as up headroom (§8.4).
    for p in packets {
        if p.interruptible
            && !p.is_terminal()
            && matches!(p.status, PacketStatus::Active | PacketStatus::Scheduled)
        {
            up_kw += p.desired_power_kw.max(0.0);
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

/// Build a fresh `ReservationLayer` from the current event list
/// (SIMPLE FIRM events only) and compute the site envelope.
///
/// This is the entry point for `GET /flexibility` and the dispatcher tick.
/// It does NOT modify any state — it is a pure read + compute.
pub fn compute_envelope_from_events(
    sim: &SimState,
    events: &[Value],
    packets: &[EnergyPacket],
    now: DateTime<Utc>,
) -> SiteFlexibilityEnvelope {
    let reservations = parse_firm_reservations(events, now);
    let mut layer = ReservationLayer::new();
    for r in reservations {
        layer.insert(r);
    }
    compute_envelope(sim, &layer, packets, now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    use crate::assets::{
        AssetConfig, AssetHistoryBuffer, AssetState, Battery, BatteryState, EvCharger, EvState,
        Grid, PvInverter, PvState,
    };
    use crate::controller::reservation::{FlexDirection, Reservation, ReservationSource};
    use crate::simulator::{energy::EnergyCounter, AssetEntry, GridMeter, SimState};

    // ── helpers ──────────────────────────────────────────────────────────────

    fn make_battery_entry(id: &str, soc: f64, last_power_kw: f64) -> AssetEntry {
        AssetEntry {
            id: id.to_string(),
            state: AssetState::Battery(BatteryState { soc, actual_power_kw: last_power_kw }),
            setpoint_kw: last_power_kw,
            last_power_kw,
            energy: EnergyCounter::new(),
            history: AssetHistoryBuffer::new(3600),
        }
    }

    fn make_battery_config(
        capacity_kwh: f64,
        max_kw: f64,
        min_soc: f64,
    ) -> AssetConfig {
        AssetConfig::Battery(Battery {
            capacity_kwh,
            max_charge_kw: max_kw,
            max_discharge_kw: max_kw,
            round_trip_efficiency: 1.0,
            min_soc,
        })
    }

    fn make_ev_entry(id: &str, soc: f64, plugged: bool, last_power_kw: f64) -> AssetEntry {
        AssetEntry {
            id: id.to_string(),
            state: AssetState::Ev(EvState { soc, plugged, actual_power_kw: last_power_kw }),
            setpoint_kw: last_power_kw,
            last_power_kw,
            energy: EnergyCounter::new(),
            history: AssetHistoryBuffer::new(3600),
        }
    }

    fn make_ev_config(max_charge_kw: f64, battery_kwh: f64) -> AssetConfig {
        AssetConfig::Ev(EvCharger {
            max_charge_kw,
            max_discharge_kw: 0.0,
            battery_kwh,
            soc_target: 0.8,
            soc_target_profile: 0.8,
            default_charge_kw: max_charge_kw,
            min_soc: 0.0,
        })
    }

    fn make_pv_entry(id: &str, actual_power_kw: f64) -> AssetEntry {
        AssetEntry {
            id: id.to_string(),
            state: AssetState::Pv(PvState { actual_power_kw }),
            setpoint_kw: 0.0,
            last_power_kw: actual_power_kw,
            energy: EnergyCounter::new(),
            history: AssetHistoryBuffer::new(3600),
        }
    }

    fn make_pv_config(rated_kw: f64) -> AssetConfig {
        AssetConfig::Pv(PvInverter {
            rated_kw,
            export_limit_kw: None,
            irradiance: 0.5,
            irradiance_offset: 0.0,
            pv_alpha: 0.1,
        })
    }

    fn make_sim(asset_configs: Vec<AssetConfig>, assets: Vec<AssetEntry>) -> SimState {
        SimState {
            asset_configs,
            assets,
            grid: GridMeter::default(),
            grid_asset: Grid::new(),
            pv_smoothing: crate::simulator::PvSmoothingState::default(),
            base_load_smoothing: Default::default(),
            last_tick: Utc::now(),
        }
    }

    fn up_reservation(asset_id: &str, kw: f64, now: DateTime<Utc>) -> Reservation {
        Reservation {
            id: Uuid::new_v4(),
            window: (now - chrono::Duration::seconds(1), now + chrono::Duration::hours(1)),
            asset_id: Some(asset_id.to_string()),
            kw,
            direction: FlexDirection::Up,
            source: ReservationSource::PolicyDefault,
            priority: 2,
        }
    }

    // ── tests ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_compute_envelope_no_assets_returns_zero() {
        let sim = make_sim(vec![], vec![]);
        let env = compute_envelope(&sim, &ReservationLayer::new(), &[], Utc::now());
        assert_eq!(env.up_kw, 0.0);
        assert_eq!(env.down_kw, 0.0);
        assert!(env.up_duration_s.is_none());
        assert!(env.down_duration_s.is_none());
    }

    #[test]
    fn test_compute_envelope_ev_charging_contributes_up() {
        // EV charging at max rate — can reduce (up) but cannot increase (down).
        let sim = make_sim(
            vec![make_ev_config(7.0, 40.0)],
            vec![make_ev_entry("ev", 0.5, true, 7.0)],
        );
        let env = compute_envelope(&sim, &ReservationLayer::new(), &[], Utc::now());
        assert!((env.up_kw - 7.0).abs() < 1e-6, "up_kw should be 7.0, got {}", env.up_kw);
        assert!((env.down_kw).abs() < 1e-6, "down_kw should be 0.0, got {}", env.down_kw);
    }

    #[test]
    fn test_compute_envelope_battery_idle_contributes_both() {
        // Battery at rest, mid-SoC: can both reduce and increase consumption.
        let sim = make_sim(
            vec![make_battery_config(10.0, 5.0, 0.1)],
            vec![make_battery_entry("bat", 0.5, 0.0)],
        );
        let env = compute_envelope(&sim, &ReservationLayer::new(), &[], Utc::now());
        assert!(env.up_kw > 0.0, "expected up_kw > 0, got {}", env.up_kw);
        assert!(env.down_kw > 0.0, "expected down_kw > 0, got {}", env.down_kw);
        assert!((env.up_kw - 5.0).abs() < 1e-6, "up_kw should be 5.0, got {}", env.up_kw);
        assert!((env.down_kw - 5.0).abs() < 1e-6, "down_kw should be 5.0, got {}", env.down_kw);
    }

    #[test]
    fn test_compute_envelope_reservation_reduces_down() {
        // UP reservation of 3 kW shrinks max_import_kw → reduces down_kw.
        let now = Utc::now();
        let sim = make_sim(
            vec![make_battery_config(10.0, 5.0, 0.1)],
            vec![make_battery_entry("bat", 0.5, 0.0)],
        );
        let mut layer = ReservationLayer::new();
        layer.insert(up_reservation("bat", 3.0, now));
        let env = compute_envelope(&sim, &layer, &[], now);
        // up_kw unaffected (UP reservation does not change max_export_kw)
        assert!((env.up_kw - 5.0).abs() < 1e-6, "up_kw should be 5.0, got {}", env.up_kw);
        // down_kw reduced from 5.0 to 2.0 (5.0 - 3.0)
        assert!((env.down_kw - 2.0).abs() < 1e-6, "down_kw should be 2.0, got {}", env.down_kw);
    }

    #[test]
    fn test_compute_envelope_pv_contributes_nothing() {
        // PV has a point-range capability — no controllable headroom.
        let sim = make_sim(
            vec![make_pv_config(5.0)],
            vec![make_pv_entry("pv", -2.0)],
        );
        let env = compute_envelope(&sim, &ReservationLayer::new(), &[], Utc::now());
        assert!((env.up_kw).abs() < 1e-6, "PV up_kw should be 0, got {}", env.up_kw);
        assert!((env.down_kw).abs() < 1e-6, "PV down_kw should be 0, got {}", env.down_kw);
    }

    #[test]
    fn test_compute_envelope_duration_from_battery_soc() {
        // Battery: capacity=10 kWh, min_soc=0.1, soc=0.5, max_kw=5.
        // available_discharge_kwh = (0.5 - 0.1) * 10 = 4.0
        // available_charge_kwh    = (1.0 - 0.5) * 10 = 5.0
        // up_kw = 5.0, down_kw = 5.0
        // up_duration_s   = 4.0 / 5.0 * 3600 = 2880
        // down_duration_s = 5.0 / 5.0 * 3600 = 3600
        let sim = make_sim(
            vec![make_battery_config(10.0, 5.0, 0.1)],
            vec![make_battery_entry("bat", 0.5, 0.0)],
        );
        let env = compute_envelope(&sim, &ReservationLayer::new(), &[], Utc::now());
        assert_eq!(env.up_duration_s, Some(2880), "up_duration_s mismatch: {:?}", env.up_duration_s);
        assert_eq!(env.down_duration_s, Some(3600), "down_duration_s mismatch: {:?}", env.down_duration_s);
    }

    // ── NEAR_ZERO_KW / NEAR_ZERO_KWH — duration suppression boundary ─────────

    /// Battery with max_kw below NEAR_ZERO_KW → up_kw < NEAR_ZERO_KW →
    /// duration guard fires → up_duration_s and down_duration_s are both None.
    #[test]
    fn duration_suppressed_when_max_kw_below_near_zero_kw() {
        // 0.5 × NEAR_ZERO_KW = 0.0005 kW — below the 1 W threshold.
        // up_kw  = last_power_kw(0.0) - max_export_kw(-0.0005) = 0.0005 < NEAR_ZERO_KW
        // down_kw = max_import_kw(0.0005) - last_power_kw(0.0) = 0.0005 < NEAR_ZERO_KW
        let sub_threshold = NEAR_ZERO_KW * 0.5;
        let sim = make_sim(
            vec![make_battery_config(10.0, sub_threshold, 0.1)],
            vec![make_battery_entry("bat", 0.5, 0.0)],
        );
        let env = compute_envelope(&sim, &ReservationLayer::new(), &[], Utc::now());
        assert!(env.up_duration_s.is_none(),
            "up_duration_s must be None when up_kw < NEAR_ZERO_KW, got {:?}", env.up_duration_s);
        assert!(env.down_duration_s.is_none(),
            "down_duration_s must be None when down_kw < NEAR_ZERO_KW, got {:?}", env.down_duration_s);
    }

    /// Battery with max_kw above NEAR_ZERO_KW and non-trivial SoC headroom →
    /// both duration fields are populated.
    #[test]
    fn duration_present_when_max_kw_above_near_zero_kw() {
        // 2.0 × NEAR_ZERO_KW = 0.002 kW — just above the 1 W threshold.
        let above_threshold = NEAR_ZERO_KW * 2.0;
        let sim = make_sim(
            vec![make_battery_config(10.0, above_threshold, 0.1)],
            vec![make_battery_entry("bat", 0.5, 0.0)],
        );
        let env = compute_envelope(&sim, &ReservationLayer::new(), &[], Utc::now());
        assert!(env.up_duration_s.is_some(),
            "up_duration_s must be Some when up_kw > NEAR_ZERO_KW, got {:?}", env.up_duration_s);
        assert!(env.down_duration_s.is_some(),
            "down_duration_s must be Some when down_kw > NEAR_ZERO_KW, got {:?}", env.down_duration_s);
    }
}
