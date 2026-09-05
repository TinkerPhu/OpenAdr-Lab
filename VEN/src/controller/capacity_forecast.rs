//! Closed-form sustained-commitment capacity curve — "if the site committed
//! now to sustained max import (or export), how does the achievable power
//! step down over elapsed time, and how much energy is behind it."
//!
//! Deliberately independent of `envelope_forecast.rs`: that module's
//! per-slot `up_kw`/`down_kw` are independent point-in-time counterfactuals
//! computed by re-simulating each asset forward under the ACTIVE PLAN's own
//! schedule (`simulator::forecast::build_forecast_frames`) — exactly the
//! wrong scenario for this module, which asks "what if the plan's schedule
//! were abandoned in favour of a sustained extreme commitment." Battery/EV/
//! heater/base-load contributions here are therefore computed directly from
//! the CURRENT snapshot (`SimSnapshot`), not from forecast frames. PV is the
//! one exception: its ceiling is driven by the weather forecast, not by
//! anything the plan decided, so its forecast-frame data (`AssetForecastFrame`,
//! resolved per slot via `entities::solar::pv_ceiling_kw`) is safe and correct
//! to reuse here too.
//!
//! Every per-asset term is an ABSOLUTE achievable-power value (not a delta
//! from current dispatch) — battery/EV/heater commit to their full rated
//! power, PV's export term is its full forecast ceiling. Base load and
//! heater's *current* draw are therefore included as constant net-grid-power
//! terms (additive on import, subtractive on export) even though they
//! contribute no additional *flexibility* — omitting them would make the
//! curve represent flexible-asset dispatch instead of real net grid power
//! (design.md decision 7, corrected from an earlier "netting pass" framing).

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};

use crate::controller::simulator_port::{AssetForecastFrame, AssetSnapshot, SimSnapshot};
use crate::entities::capacity_curve::{CapacityCurve, CapacityCurveStep, CommitmentDirection};
use crate::ids::ASSET_PV;

/// One (elapsed_s, delta_kw) breakpoint. Multiple events at the same
/// `elapsed_s` are summed by `merge_events`.
type Event = (i64, f64);

/// `start` anchors the curve's elapsed-time axis (used by PV/shiftable-load
/// placement) but does NOT forecast `snapshot`'s battery/EV/heater SoC or
/// temperature forward to that instant — those formulas read `snapshot`'s
/// CURRENT state as-is. For `start == now` (the common case) this is exact;
/// for a future `start` (advance-notice queries), it's a first-order
/// approximation that assumes today's SoC/temperature still holds at that
/// future instant. Producing a genuinely forecasted starting state would
/// require re-simulating forward from now to `start` — exactly the
/// plan-driven trajectory this module deliberately avoids (see module doc).
pub fn compute_capacity_curve(
    direction: CommitmentDirection,
    start: DateTime<Utc>,
    snapshot: &SimSnapshot,
    pv_frames: &[AssetForecastFrame],
) -> CapacityCurve {
    let mut events: Vec<Event> = Vec::new();

    for asset in snapshot.assets.values() {
        match asset.asset_type.as_str() {
            "battery" => events.extend(battery_events(direction, asset)),
            "ev" => events.extend(ev_events(direction, asset)),
            "heater" => events.extend(heater_events(direction, asset)),
            "base_load" => events.extend(base_load_events(direction, asset.power_kw)),
            _ => {}
        }
    }
    events.extend(pv_events(direction, start, pv_frames));
    if direction == CommitmentDirection::Import {
        events.extend(shiftable_events(start, snapshot));
    }

    let cap_kw = match direction {
        CommitmentDirection::Import => snapshot.grid.import_limit_kw,
        CommitmentDirection::Export => -snapshot.grid.export_limit_kw,
    }
    .max(0.0);

    CapacityCurve {
        direction,
        start,
        steps: merge_events(events, cap_kw),
    }
}

/// Reservoir-bound contribution (battery/EV/heater-import): constant
/// `power_kw` from elapsed 0 until the energy budget runs out, then 0.
/// `efficiency` scales how much of `power_kw` actually lands in the
/// reservoir (round-trip efficiency on battery charge; 1.0 elsewhere).
fn reservoir_events(energy_kwh: f64, power_kw: f64, efficiency: f64) -> Vec<Event> {
    if power_kw <= 0.0 || energy_kwh <= 0.0 || efficiency <= 0.0 {
        return Vec::new();
    }
    let duration_s = (energy_kwh / (power_kw * efficiency) * 3600.0).round() as i64;
    if duration_s <= 0 {
        return Vec::new();
    }
    vec![(0, power_kw), (duration_s, -power_kw)]
}

fn battery_events(direction: CommitmentDirection, asset: &AssetSnapshot) -> Vec<Event> {
    let soc = asset.val("soc").unwrap_or(0.0);
    let capacity_kwh = asset.val("capacity_kwh").unwrap_or(0.0);
    let min_soc = asset.val("min_soc").unwrap_or(0.0);
    match direction {
        CommitmentDirection::Export => {
            let energy_kwh = (soc - min_soc).max(0.0) * capacity_kwh;
            let power_kw = asset.val("max_discharge_kw").unwrap_or(0.0);
            reservoir_events(energy_kwh, power_kw, 1.0)
        }
        CommitmentDirection::Import => {
            let energy_kwh = (1.0 - soc).max(0.0) * capacity_kwh;
            let power_kw = asset.val("max_charge_kw").unwrap_or(0.0);
            let efficiency = asset.val("round_trip_efficiency").unwrap_or(1.0);
            reservoir_events(energy_kwh, power_kw, efficiency)
        }
    }
}

/// EV charge headroom is bounded by `soc_target` (BMS ceiling), NOT 1.0 —
/// `AssetConfig::available_storage_kwh` computes it to 1.0, which would
/// overstate EV import capacity for anything past `soc_target`; this
/// function is deliberately independent of that helper (design.md decision
/// 2). An unplugged EV contributes nothing to either direction.
fn ev_events(direction: CommitmentDirection, asset: &AssetSnapshot) -> Vec<Event> {
    if asset.val("plugged").unwrap_or(0.0) < 0.5 {
        return Vec::new();
    }
    let soc = asset.val("soc").unwrap_or(0.0);
    let battery_kwh = asset.val("battery_kwh").unwrap_or(0.0);
    let min_soc = asset.val("min_soc").unwrap_or(0.0);
    match direction {
        CommitmentDirection::Export => {
            let energy_kwh = (soc - min_soc).max(0.0) * battery_kwh;
            let power_kw = asset.val("max_discharge_kw").unwrap_or(0.0);
            reservoir_events(energy_kwh, power_kw, 1.0)
        }
        CommitmentDirection::Import => {
            let soc_target = asset.val("soc_target").unwrap_or(1.0);
            let energy_kwh = (soc_target - soc).max(0.0) * battery_kwh;
            let power_kw = asset.val("max_charge_kw").unwrap_or(0.0);
            reservoir_events(energy_kwh, power_kw, 1.0)
        }
    }
}

/// Heater: import-direction is a genuine thermal reservoir (steps down to 0
/// once `temp_max_c` is reached); export-direction is the heater's current
/// draw treated as a constant reducible baseline (like base load, but
/// flexible down to 0) — turning it down doesn't consume a stored energy
/// budget, so it holds for the whole horizon rather than stepping down.
/// Ignores ongoing thermal loss/draw and the forced-on floor re-engaging at
/// `temp_min_c` on the export side (documented simplifications).
fn heater_events(direction: CommitmentDirection, asset: &AssetSnapshot) -> Vec<Event> {
    match direction {
        CommitmentDirection::Import => {
            let temp_c = asset.val("temp_c").unwrap_or(0.0);
            let temp_max_c = asset.val("temp_max_c").unwrap_or(0.0);
            let thermal_mass = asset.val("thermal_mass_kwh_per_c").unwrap_or(0.0);
            let energy_kwh = (temp_max_c - temp_c).max(0.0) * thermal_mass;
            let power_kw = asset.val("max_kw").unwrap_or(0.0);
            reservoir_events(energy_kwh, power_kw, 1.0)
        }
        CommitmentDirection::Export => {
            if asset.power_kw <= 0.0 {
                Vec::new()
            } else {
                vec![(0, asset.power_kw)]
            }
        }
    }
}

/// Base load never changes with the commitment, but its forecasted draw is
/// still part of net grid power: additive on import (it's already drawing
/// this), subtractive on export (it keeps consuming, reducing net export).
fn base_load_events(direction: CommitmentDirection, power_kw: f64) -> Vec<Event> {
    if power_kw <= 0.0 {
        return Vec::new();
    }
    match direction {
        CommitmentDirection::Import => vec![(0, power_kw)],
        CommitmentDirection::Export => vec![(0, -power_kw)],
    }
}

/// PV is forecast-bound, not a reservoir — no exhaustion step-down. Import
/// (curtailment) at elapsed `t` is the forecast/planned output at `t`;
/// export is the forecast ceiling itself at `t` (already the FULL
/// achievable export from PV alone — NOT ceiling-minus-current, which would
/// under-count already-flowing export).
fn pv_events(
    direction: CommitmentDirection,
    start: DateTime<Utc>,
    pv_frames: &[AssetForecastFrame],
) -> Vec<Event> {
    let mut events = Vec::new();
    let mut prev_value = 0.0_f64;
    for frame in pv_frames {
        let Some(point) = frame.assets.get(ASSET_PV) else {
            continue;
        };
        let value = match direction {
            CommitmentDirection::Import => (-point.planned_kw).max(0.0),
            CommitmentDirection::Export => (-point.cap_max_export_kw).max(0.0),
        };
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

/// Import-commitment only (design.md decision 5): a not-yet-run,
/// not-currently-running load contributes its full `power_kw` for exactly
/// `duration_min`, placed at the earliest elapsed time its window allows,
/// then is done — never on the export-commitment curve, since starting a
/// load can only ever increase draw.
///
/// Reads shiftable-load asset entries straight off the live `SimSnapshot`
/// (`shiftable-load-as-asset` proposal.md scope) instead of a bolt-on
/// `&[ShiftableLoad]`/`&[ShiftableLoadRuntime]` pair — window bounds are
/// encoded as unix-seconds floats in `ShiftableLoadAsset::state_values()`
/// since `AssetSnapshot.values` is a flat `HashMap<String, f64>`.
fn shiftable_events(start: DateTime<Utc>, snapshot: &SimSnapshot) -> Vec<Event> {
    let mut events = Vec::new();
    for asset in snapshot.assets.values() {
        if asset.asset_type != "shiftable_load" {
            continue;
        }
        let v = &asset.values;
        let started = v.get("started").copied().unwrap_or(0.0) > 0.5;
        if started {
            continue;
        }
        let (Some(&power_kw), Some(&duration_min), Some(&earliest_unix), Some(&latest_unix)) = (
            v.get("power_kw"),
            v.get("duration_min"),
            v.get("earliest_start_unix"),
            v.get("latest_end_unix"),
        ) else {
            continue;
        };
        let Some(earliest_start) = DateTime::<Utc>::from_timestamp(earliest_unix as i64, 0) else {
            continue;
        };
        let Some(latest_end) = DateTime::<Utc>::from_timestamp(latest_unix as i64, 0) else {
            continue;
        };
        let duration = chrono::Duration::minutes(duration_min as i64);
        let candidate_ts = start.max(earliest_start);
        if candidate_ts + duration > latest_end {
            continue;
        }
        let elapsed_start = (candidate_ts - start).num_seconds().max(0);
        let elapsed_end = elapsed_start + duration.num_seconds();
        events.push((elapsed_start, power_kw));
        events.push((elapsed_end, -power_kw));
    }
    events
}

/// Sweep-line merge of piecewise-constant contributions: sum deltas at each
/// distinct elapsed time, accumulate a running total, clip to `[0, cap_kw]`.
/// Always includes an elapsed-0 step so the curve starts at the commitment
/// instant even if every contributor is silent there.
fn merge_events(events: Vec<Event>, cap_kw: f64) -> Vec<CapacityCurveStep> {
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
            CapacityCurveStep {
                elapsed_s,
                power_kw: running.clamp(0.0, cap_kw),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::simulator_port::{AssetForecastPoint, GridSnapshot};
    use chrono::TimeZone;
    use std::collections::HashMap;

    fn t0() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 20, 12, 0, 0).unwrap()
    }

    fn asset(asset_type: &str, power_kw: f64, values: &[(&str, f64)]) -> AssetSnapshot {
        AssetSnapshot {
            power_kw,
            asset_type: asset_type.to_string(),
            cap_max_import_kw: 0.0,
            cap_max_export_kw: 0.0,
            available_discharge_kwh: None,
            available_charge_kwh: None,
            default_setpoint_kw: 0.0,
            setpoint_kw: 0.0,
            values: values.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        }
    }

    fn empty_snapshot() -> SimSnapshot {
        SimSnapshot {
            ts: t0(),
            grid: GridSnapshot {
                net_power_w: 0.0,
                voltage_v: 230.0,
                import_kwh: 0.0,
                export_kwh: 0.0,
                import_limit_kw: f64::MAX,
                export_limit_kw: -f64::MAX,
            },
            assets: HashMap::new(),
        }
    }

    // ── battery ──────────────────────────────────────────────────────────

    #[test]
    fn battery_export_energy_and_duration() {
        // 10 kWh capacity, soc=0.6, min_soc=0.1 → 5.0 kWh available, 5 kW
        // discharge rate → 3600s duration.
        let a = asset(
            "battery",
            0.0,
            &[
                ("soc", 0.6),
                ("capacity_kwh", 10.0),
                ("min_soc", 0.1),
                ("max_discharge_kw", 5.0),
            ],
        );
        let events = battery_events(CommitmentDirection::Export, &a);
        assert_eq!(events, vec![(0, 5.0), (3600, -5.0)]);
    }

    #[test]
    fn battery_import_applies_round_trip_efficiency() {
        // 5.0 kWh headroom, 5 kW charge rate, 0.5 efficiency → only 2.5
        // kWh/h actually lands in storage → 2h = 7200s duration.
        let a = asset(
            "battery",
            0.0,
            &[
                ("soc", 0.5),
                ("capacity_kwh", 10.0),
                ("max_charge_kw", 5.0),
                ("round_trip_efficiency", 0.5),
            ],
        );
        let events = battery_events(CommitmentDirection::Import, &a);
        assert_eq!(events, vec![(0, 5.0), (7200, -5.0)]);
    }

    #[test]
    fn battery_at_min_soc_contributes_zero_export() {
        let a = asset(
            "battery",
            0.0,
            &[
                ("soc", 0.1),
                ("capacity_kwh", 10.0),
                ("min_soc", 0.1),
                ("max_discharge_kw", 5.0),
            ],
        );
        assert!(battery_events(CommitmentDirection::Export, &a).is_empty());
    }

    // ── EV ───────────────────────────────────────────────────────────────

    #[test]
    fn ev_import_headroom_stops_at_soc_target_not_full() {
        // soc=0.5, soc_target=0.8, battery_kwh=10 → 3.0 kWh headroom (NOT
        // 5.0 kWh, which is what (1.0 - soc)*battery_kwh would give).
        let a = asset(
            "ev",
            0.0,
            &[
                ("plugged", 1.0),
                ("soc", 0.5),
                ("soc_target", 0.8),
                ("battery_kwh", 10.0),
                ("max_charge_kw", 3.0),
            ],
        );
        let events = ev_events(CommitmentDirection::Import, &a);
        // 3.0 kWh / 3.0 kW = 1h = 3600s.
        assert_eq!(events, vec![(0, 3.0), (3600, -3.0)]);
    }

    #[test]
    fn unplugged_ev_contributes_nothing_either_direction() {
        let a = asset(
            "ev",
            0.0,
            &[
                ("plugged", 0.0),
                ("soc", 0.2),
                ("soc_target", 0.8),
                ("battery_kwh", 10.0),
                ("max_charge_kw", 3.0),
                ("max_discharge_kw", 3.0),
            ],
        );
        assert!(ev_events(CommitmentDirection::Import, &a).is_empty());
        assert!(ev_events(CommitmentDirection::Export, &a).is_empty());
    }

    // ── heater ───────────────────────────────────────────────────────────

    #[test]
    fn heater_import_reservoir_steps_down_on_exhaustion() {
        // thermal_mass=2.0 kWh/C, temp_max=23, temp=21 → 4.0 kWh headroom,
        // 2.5 kW tier → 1.6h = 5760s.
        let a = asset(
            "heater",
            0.0,
            &[
                ("temp_c", 21.0),
                ("temp_max_c", 23.0),
                ("thermal_mass_kwh_per_c", 2.0),
                ("max_kw", 2.5),
            ],
        );
        let events = heater_events(CommitmentDirection::Import, &a);
        assert_eq!(events, vec![(0, 2.5), (5760, -2.5)]);
    }

    #[test]
    fn heater_at_temp_max_contributes_zero_import() {
        let a = asset(
            "heater",
            0.0,
            &[
                ("temp_c", 23.0),
                ("temp_max_c", 23.0),
                ("thermal_mass_kwh_per_c", 2.0),
                ("max_kw", 2.5),
            ],
        );
        assert!(heater_events(CommitmentDirection::Import, &a).is_empty());
    }

    #[test]
    fn heater_export_contributes_current_draw_as_constant() {
        let a = asset("heater", 1.25, &[]);
        let events = heater_events(CommitmentDirection::Export, &a);
        assert_eq!(events, vec![(0, 1.25)]);
    }

    #[test]
    fn heater_idle_contributes_zero_export() {
        let a = asset("heater", 0.0, &[]);
        assert!(heater_events(CommitmentDirection::Export, &a).is_empty());
    }

    // ── base load ────────────────────────────────────────────────────────

    #[test]
    fn base_load_is_additive_on_import_subtractive_on_export() {
        assert_eq!(
            base_load_events(CommitmentDirection::Import, 0.5),
            vec![(0, 0.5)]
        );
        assert_eq!(
            base_load_events(CommitmentDirection::Export, 0.5),
            vec![(0, -0.5)]
        );
    }

    // ── PV ───────────────────────────────────────────────────────────────

    #[test]
    fn pv_export_uses_ceiling_not_ceiling_minus_current() {
        let start = t0();
        let frames = vec![AssetForecastFrame {
            ts: start + chrono::Duration::seconds(300),
            assets: HashMap::from([(
                ASSET_PV.to_string(),
                AssetForecastPoint {
                    planned_kw: -2.0,
                    cap_max_import_kw: 0.0,
                    cap_max_export_kw: -5.0,
                },
            )]),
        }];
        let events = pv_events(CommitmentDirection::Export, start, &frames);
        // Ceiling magnitude 5.0, not (5.0 - 2.0) = 3.0.
        assert_eq!(events, vec![(300, 5.0)]);
    }

    #[test]
    fn pv_import_tracks_forecast_shape_not_exhaustion() {
        let start = t0();
        let frames = vec![
            AssetForecastFrame {
                ts: start + chrono::Duration::seconds(300),
                assets: HashMap::from([(
                    ASSET_PV.to_string(),
                    AssetForecastPoint {
                        planned_kw: -4.0,
                        cap_max_import_kw: 0.0,
                        cap_max_export_kw: -4.0,
                    },
                )]),
            },
            AssetForecastFrame {
                ts: start + chrono::Duration::seconds(600),
                assets: HashMap::from([(
                    ASSET_PV.to_string(),
                    AssetForecastPoint {
                        planned_kw: -1.0, // irradiance dropped — forecast shape, not exhaustion
                        cap_max_import_kw: 0.0,
                        cap_max_export_kw: -1.0,
                    },
                )]),
            },
        ];
        let events = pv_events(CommitmentDirection::Import, start, &frames);
        assert_eq!(events, vec![(300, 4.0), (600, -3.0)]);
    }

    // ── shiftable loads ──────────────────────────────────────────────────

    fn shiftable_snapshot(
        power_kw: f64,
        duration_min: u32,
        earliest_h: i64,
        latest_h: i64,
        started: bool,
    ) -> SimSnapshot {
        let mut snap = empty_snapshot();
        snap.assets.insert(
            "wm".to_string(),
            asset(
                "shiftable_load",
                0.0,
                &[
                    ("power_kw", power_kw),
                    ("duration_min", duration_min as f64),
                    (
                        "earliest_start_unix",
                        (t0() + chrono::Duration::hours(earliest_h)).timestamp() as f64,
                    ),
                    (
                        "latest_end_unix",
                        (t0() + chrono::Duration::hours(latest_h)).timestamp() as f64,
                    ),
                    ("started", if started { 1.0 } else { 0.0 }),
                ],
            ),
        );
        snap
    }

    #[test]
    fn shiftable_load_placed_once_bounded_duration() {
        let snap = shiftable_snapshot(2.0, 60, 0, 4, false);
        let events = shiftable_events(t0(), &snap);
        assert_eq!(events, vec![(0, 2.0), (3600, -2.0)]);
    }

    #[test]
    fn shiftable_load_already_run_contributes_nothing() {
        let snap = shiftable_snapshot(2.0, 60, 0, 4, true);
        assert!(shiftable_events(t0(), &snap).is_empty());
    }

    #[test]
    fn shiftable_load_no_valid_start_contributes_nothing() {
        // Window too short for a 60-min run.
        let snap = shiftable_snapshot(2.0, 60, 0, 0, false); // latest_end == earliest_start
        assert!(shiftable_events(t0(), &snap).is_empty());
    }

    // ── merge ────────────────────────────────────────────────────────────

    #[test]
    fn merge_clips_combined_total_to_cap() {
        let events = vec![(0, 6.0), (0, 6.0)]; // two 6 kW contributors = 12 kW combined
        let steps = merge_events(events, 10.0);
        assert_eq!(
            steps,
            vec![CapacityCurveStep {
                elapsed_s: 0,
                power_kw: 10.0
            }]
        );
    }

    #[test]
    fn merge_never_goes_negative() {
        let events = vec![(0, 2.0), (0, -5.0)]; // base load exceeds a lone contributor
        let steps = merge_events(events, 100.0);
        assert_eq!(
            steps,
            vec![CapacityCurveStep {
                elapsed_s: 0,
                power_kw: 0.0
            }]
        );
    }

    #[test]
    fn merge_always_includes_an_elapsed_zero_step() {
        let steps = merge_events(vec![], 100.0);
        assert_eq!(
            steps,
            vec![CapacityCurveStep {
                elapsed_s: 0,
                power_kw: 0.0
            }]
        );
    }

    // ── integration: compute_capacity_curve ────────────────────────────────

    #[test]
    fn compute_capacity_curve_merges_battery_and_base_load_on_export() {
        let mut snapshot = empty_snapshot();
        snapshot.assets.insert(
            "battery".to_string(),
            asset(
                "battery",
                0.0,
                &[
                    ("soc", 0.6),
                    ("capacity_kwh", 10.0),
                    ("min_soc", 0.1),
                    ("max_discharge_kw", 5.0),
                ],
            ),
        );
        snapshot
            .assets
            .insert("base_load".to_string(), asset("base_load", 0.5, &[]));

        let curve = compute_capacity_curve(CommitmentDirection::Export, t0(), &snapshot, &[]);
        // 5.0 kW discharge - 0.5 kW base load = 4.5 kW at t=0, drops to
        // -0.5 clipped to 0.0 once the battery exhausts at 3600s.
        assert_eq!(
            curve.steps,
            vec![
                CapacityCurveStep {
                    elapsed_s: 0,
                    power_kw: 4.5
                },
                CapacityCurveStep {
                    elapsed_s: 3600,
                    power_kw: 0.0
                },
            ]
        );
    }

    #[test]
    fn compute_capacity_curve_clips_to_grid_import_limit() {
        let mut snapshot = empty_snapshot();
        snapshot.grid.import_limit_kw = 3.0;
        snapshot.assets.insert(
            "battery".to_string(),
            asset(
                "battery",
                0.0,
                &[
                    ("soc", 0.5),
                    ("capacity_kwh", 10.0),
                    ("max_charge_kw", 5.0),
                    ("round_trip_efficiency", 1.0),
                ],
            ),
        );
        let curve = compute_capacity_curve(CommitmentDirection::Import, t0(), &snapshot, &[]);
        assert_eq!(curve.steps[0].power_kw, 3.0);
    }
}
