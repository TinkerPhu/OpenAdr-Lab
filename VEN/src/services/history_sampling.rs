//! Pure 1-minute downsampling of the simulator's 1 s snapshots into the
//! `TickSample`/`GridSample` rows the history store keeps (WP1.2).
//! Clock-injected (`now` passed in per call), so minute-boundary logic is
//! testable without sleeps.
//!
//! Lived in `tasks/history_sampler/`, where it was "split out of mod.rs to
//! keep the tasks/ file-size cap" — but there is no scheduling in it, which
//! is all `tasks/` is for. It aggregates port snapshots into domain values,
//! so it belongs in the application ring; the task module keeps the loop,
//! the `spawn_blocking` writes and the retention/rollover boundaries.
use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::controller::simulator_port::SimSnapshot;
use crate::entities::asset_params::PvCurtailmentSource;
use crate::entities::capacity::{tightest_capacity_limit, CapacitySnapshot};
use crate::entities::capacity_curve::CommitmentDirection::{Export, Import};
use crate::entities::history::{GridSample, TickSample};
use crate::entities::plan::SiteFlexibilityEnvelope;
use crate::entities::tariff_snapshot::TariffSnapshot;

#[derive(Default, Clone)]
struct AssetAcc {
    power_kw_sum: f64,
    soc_pct_sum: f64,
    soc_pct_n: u32,
    temperature_c_sum: f64,
    temperature_c_n: u32,
    /// Mean-shaped like `soc_pct`: each sample is 1.0 or 0.0, so the window mean is the
    /// fraction of the window the asset was plugged in.
    plugged_sum: f64,
    plugged_n: u32,
    n: u32,
    /// PV curtailment: not a mean (categorical + intermittent). Tracks the highest-ranked
    /// source seen this window (`PvCurtailmentSource::outranks`; persisted as
    /// `PvCurtailmentSource::as_f64()`) and the tightest limit value observed for that source,
    /// so a brief capacity-sourced event is never masked by a plan-sourced or unlimited
    /// majority within the same window. See `docs/reference/KEY_LEARNINGS.md` (PV Curtailment History).
    curtailment_source: PvCurtailmentSource,
    curtailment_limit_kw: Option<f64>,
}

#[derive(Default, Clone)]
struct GridAcc {
    import_kw_sum: f64,
    export_kw_sum: f64,
    import_tariff_sum: f64,
    import_tariff_n: u32,
    export_tariff_sum: f64,
    export_tariff_n: u32,
    co2_sum: f64,
    co2_n: u32,
    n: u32,
    /// Dynamic Operating Envelope capacity limit: not a mean (categorical + intermittent, usually
    /// absent). Tracks the tightest (lowest) value observed anywhere in the window; `None` if no
    /// capacity event was ever applicable. Simpler than PV curtailment's priority-tier tracking —
    /// `parse_capacity_schedule` already resolves multi-event conflicts before this ever sees the
    /// data, so there's only one source to track, not several to rank. See
    /// `openspec/changes/history-envelope-persistence/`.
    import_limit_kw: Option<f64>,
    export_limit_kw: Option<f64>,
    /// Mean instant site-flexibility headroom this window — see
    /// `entities::plan::SiteFlexibilityEnvelope`. A mean like `import_kw`/`export_kw`
    /// above, not a tightest-value tracker like the DOE limit fields.
    up_kw_sum: f64,
    down_kw_sum: f64,
    headroom_n: u32,
}

/// Feed samples via `record`; a flush (previous window's means) is returned
/// exactly when a sample belongs to a new minute. Call `flush` directly to
/// force-emit a partial window (shutdown).
#[derive(Default)]
pub struct HistorySampler {
    window_minute: Option<i64>,
    window_start: Option<DateTime<Utc>>,
    assets: HashMap<String, AssetAcc>,
    grid: GridAcc,
}

impl HistorySampler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one 1-second sample. Returns the previous window's means the
    /// instant `now` crosses into a new minute; `None` otherwise (still
    /// accumulating the current window).
    pub fn record(
        &mut self,
        now: DateTime<Utc>,
        sim: &SimSnapshot,
        tariffs: &[TariffSnapshot],
        capacity_limits: &[CapacitySnapshot],
        envelope: Option<&SiteFlexibilityEnvelope>,
    ) -> Option<(Vec<TickSample>, GridSample)> {
        let minute = now.timestamp() / 60;
        let flushed = if self.window_minute.is_some_and(|m| m != minute) {
            self.flush()
        } else {
            None
        };
        if self.window_minute.is_none() || self.window_minute != Some(minute) {
            self.window_minute = Some(minute);
            self.window_start = Some(now);
        }

        for (asset_id, snap) in &sim.assets {
            let acc = self.assets.entry(asset_id.clone()).or_default();
            acc.power_kw_sum += snap.power_kw;
            acc.n += 1;
            let view = &snap.history;
            if let Some(soc_frac) = view.soc_frac {
                acc.soc_pct_sum += soc_frac * 100.0;
                acc.soc_pct_n += 1;
            }
            if let Some(temperature_c) = view.temperature_c {
                acc.temperature_c_sum += temperature_c;
                acc.temperature_c_n += 1;
            }
            if let Some(plugged) = view.plugged {
                acc.plugged_sum += if plugged { 1.0 } else { 0.0 };
                acc.plugged_n += 1;
            }
            if let Some(limit_kw) = view.generation_limit_kw {
                let source = view.curtailment_source.unwrap_or_default();
                if source.outranks(acc.curtailment_source) {
                    acc.curtailment_source = source;
                    acc.curtailment_limit_kw = Some(limit_kw);
                } else if source == acc.curtailment_source {
                    acc.curtailment_limit_kw =
                        Some(acc.curtailment_limit_kw.unwrap_or(limit_kw).max(limit_kw));
                }
            }
        }

        let net_kw = sim.grid.net_power_w / 1000.0;
        self.grid.import_kw_sum += net_kw.max(0.0);
        self.grid.export_kw_sum += (-net_kw).max(0.0);
        self.grid.n += 1;
        let applicable = crate::entities::tariff_snapshot::tariff_at(tariffs, now);
        if let Some(t) = applicable.and_then(|r| r.import_tariff_eur_kwh) {
            self.grid.import_tariff_sum += t;
            self.grid.import_tariff_n += 1;
        }
        if let Some(t) = applicable.and_then(|r| r.export_tariff_eur_kwh) {
            self.grid.export_tariff_sum += t;
            self.grid.export_tariff_n += 1;
        }
        if let Some(c) = applicable.and_then(|r| r.co2_g_kwh) {
            self.grid.co2_sum += c;
            self.grid.co2_n += 1;
        }

        let limit_now =
            |d| tightest_capacity_limit(capacity_limits, d, now, now).map(|l| l.limit_kw);
        if let Some(v) = limit_now(Import) {
            self.grid.import_limit_kw = Some(self.grid.import_limit_kw.map_or(v, |cur| cur.min(v)));
        }
        if let Some(v) = limit_now(Export) {
            self.grid.export_limit_kw = Some(self.grid.export_limit_kw.map_or(v, |cur| cur.min(v)));
        }

        if let Some(env) = envelope {
            self.grid.up_kw_sum += env.up_kw;
            self.grid.down_kw_sum += env.down_kw;
            self.grid.headroom_n += 1;
        }

        flushed
    }

    /// Emit the current window's means (whether full or partial) and reset.
    /// Emitted rows are timestamped at the window's start.
    pub fn flush(&mut self) -> Option<(Vec<TickSample>, GridSample)> {
        let window_start = self.window_start.take()?;
        self.window_minute = None;
        let assets = std::mem::take(&mut self.assets);
        let grid = std::mem::take(&mut self.grid);

        let ticks = assets
            .into_iter()
            .filter(|(_, acc)| acc.n > 0)
            .map(|(asset_id, acc)| TickSample {
                ts: window_start,
                asset_id,
                power_kw: acc.power_kw_sum / acc.n as f64,
                soc_pct: (acc.soc_pct_n > 0).then(|| acc.soc_pct_sum / acc.soc_pct_n as f64),
                temperature_c: (acc.temperature_c_n > 0)
                    .then(|| acc.temperature_c_sum / acc.temperature_c_n as f64),
                generation_limit_kw: acc.curtailment_limit_kw,
                curtailment_source: (acc.curtailment_source != PvCurtailmentSource::None)
                    .then(|| acc.curtailment_source.as_f64()),
                plugged: (acc.plugged_n > 0).then(|| acc.plugged_sum / acc.plugged_n as f64),
            })
            .collect();

        let grid_sample = GridSample {
            ts: window_start,
            import_kw: if grid.n > 0 {
                grid.import_kw_sum / grid.n as f64
            } else {
                0.0
            },
            export_kw: if grid.n > 0 {
                grid.export_kw_sum / grid.n as f64
            } else {
                0.0
            },
            import_tariff_eur_kwh: (grid.import_tariff_n > 0)
                .then(|| grid.import_tariff_sum / grid.import_tariff_n as f64),
            export_tariff_eur_kwh: (grid.export_tariff_n > 0)
                .then(|| grid.export_tariff_sum / grid.export_tariff_n as f64),
            co2_g_kwh: (grid.co2_n > 0).then(|| grid.co2_sum / grid.co2_n as f64),
            import_limit_kw: grid.import_limit_kw,
            export_limit_kw: grid.export_limit_kw,
            up_kw: (grid.headroom_n > 0).then(|| grid.up_kw_sum / grid.headroom_n as f64),
            down_kw: (grid.headroom_n > 0).then(|| grid.down_kw_sum / grid.headroom_n as f64),
        };
        Some((ticks, grid_sample))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::simulator_port::{AssetSnapshot, GridSnapshot};
    use crate::entities::asset::{AssetHistoryView, SetpointResponse};
    use chrono::TimeZone;

    fn ts(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(secs, 0).unwrap()
    }

    fn snap(now: DateTime<Utc>, power_kw: f64, soc: Option<f64>) -> SimSnapshot {
        let history = AssetHistoryView {
            soc_frac: soc,
            ..Default::default()
        };
        let mut assets = HashMap::new();
        assets.insert(
            "ev".to_string(),
            AssetSnapshot {
                power_kw,
                asset_type: "ev".into(),
                cap_max_import_kw: 7.4,
                cap_max_export_kw: 0.0,
                available_discharge_kwh: None,
                available_charge_kwh: None,
                forced_power_kw: None,
                response: SetpointResponse::continuous(),
                default_setpoint_kw: power_kw,
                setpoint_kw: power_kw,
                values: HashMap::new(),
                history,
                emergency_what_ifs: None,
                ac_ceiling_kw: None,
            },
        );
        SimSnapshot {
            ts: now,
            grid: GridSnapshot {
                net_power_w: power_kw * 1000.0,
                voltage_v: 230.0,
                import_kwh: 0.0,
                export_kwh: 0.0,
                import_limit_kw: f64::MAX,
                export_limit_kw: -f64::MAX,
            },
            assets,
        }
    }

    #[test]
    fn test_record_same_minute_does_not_flush() {
        let mut sampler = HistorySampler::new();
        assert!(sampler
            .record(ts(0), &snap(ts(0), 1.0, Some(0.5)), &[], &[], None)
            .is_none());
        assert!(sampler
            .record(ts(30), &snap(ts(30), 2.0, Some(0.5)), &[], &[], None)
            .is_none());
    }

    #[test]
    fn test_record_crossing_minute_boundary_flushes_previous_window_mean() {
        let mut sampler = HistorySampler::new();
        sampler.record(ts(0), &snap(ts(0), 1.0, Some(0.5)), &[], &[], None);
        sampler.record(ts(30), &snap(ts(30), 3.0, Some(0.5)), &[], &[], None);
        let (ticks, grid) = sampler
            .record(ts(60), &snap(ts(60), 5.0, Some(0.5)), &[], &[], None)
            .expect("crossing into minute 1 must flush minute 0");

        assert_eq!(ticks.len(), 1);
        assert_eq!(ticks[0].asset_id, "ev");
        assert!(
            (ticks[0].power_kw - 2.0).abs() < 1e-9,
            "mean of 1.0 and 3.0 is 2.0"
        );
        assert_eq!(ticks[0].ts, ts(0), "row timestamp is the window start");
        assert!((ticks[0].soc_pct.unwrap() - 50.0).abs() < 1e-9);
        assert!((grid.import_kw - 2.0).abs() < 1e-9);
    }

    #[test]
    fn test_flush_emits_partial_window_on_shutdown() {
        let mut sampler = HistorySampler::new();
        sampler.record(ts(0), &snap(ts(0), 4.0, None), &[], &[], None);
        let (ticks, grid) = sampler
            .flush()
            .expect("a single-sample partial window must still flush");
        assert_eq!(ticks[0].power_kw, 4.0);
        assert!(ticks[0].soc_pct.is_none(), "no soc sample this window");
        assert_eq!(grid.import_kw, 4.0);
    }

    #[test]
    fn test_flush_with_no_samples_returns_none() {
        let mut sampler = HistorySampler::new();
        assert!(sampler.flush().is_none());
    }

    #[test]
    fn test_record_grid_export_when_net_power_negative() {
        let mut sampler = HistorySampler::new();
        sampler.record(ts(0), &snap(ts(0), -3.0, None), &[], &[], None);
        let (_, grid) = sampler.flush().unwrap();
        assert_eq!(grid.import_kw, 0.0);
        assert!((grid.export_kw - 3.0).abs() < 1e-9);
    }

    #[test]
    fn test_record_applies_matching_tariff() {
        let mut sampler = HistorySampler::new();
        let tariffs = vec![TariffSnapshot {
            interval_start: ts(0),
            interval_end: ts(3600),
            import_tariff_eur_kwh: Some(0.25),
            export_tariff_eur_kwh: Some(0.05),
            co2_g_kwh: Some(300.0),
        }];
        sampler.record(ts(0), &snap(ts(0), 1.0, None), &tariffs, &[], None);
        let (_, grid) = sampler.flush().unwrap();
        assert_eq!(grid.import_tariff_eur_kwh, Some(0.25));
        assert_eq!(grid.export_tariff_eur_kwh, Some(0.05));
        assert_eq!(grid.co2_g_kwh, Some(300.0));
    }

    // ── Capacity-limit envelope aggregation (history-envelope-persistence) ──

    fn capacity_snap(
        interval_start: DateTime<Utc>,
        interval_end: DateTime<Utc>,
        import_limit_kw: Option<f64>,
        export_limit_kw: Option<f64>,
    ) -> CapacitySnapshot {
        CapacitySnapshot {
            interval_start,
            interval_end,
            import_limit_kw,
            export_limit_kw,
            import_limit_event_id: None,
            export_limit_event_id: None,
        }
    }

    #[test]
    fn test_record_applies_matching_capacity_limit() {
        let mut sampler = HistorySampler::new();
        let limits = vec![capacity_snap(ts(0), ts(3600), Some(5.0), Some(3.0))];
        sampler.record(ts(0), &snap(ts(0), 1.0, None), &[], &limits, None);
        let (_, grid) = sampler.flush().unwrap();
        assert_eq!(grid.import_limit_kw, Some(5.0));
        assert_eq!(grid.export_limit_kw, Some(3.0));
    }

    fn envelope(up_kw: f64, down_kw: f64) -> SiteFlexibilityEnvelope {
        SiteFlexibilityEnvelope {
            up_kw,
            down_kw,
            ..Default::default()
        }
    }

    #[test]
    fn test_record_no_envelope_ever_seen_persists_none_not_zero() {
        let mut sampler = HistorySampler::new();
        sampler.record(ts(0), &snap(ts(0), 1.0, None), &[], &[], None);
        let (_, grid) = sampler.flush().unwrap();
        assert_eq!(grid.up_kw, None);
        assert_eq!(grid.down_kw, None);
    }

    #[test]
    fn test_record_up_down_kw_is_the_mean_across_the_window() {
        let mut sampler = HistorySampler::new();
        sampler.record(
            ts(0),
            &snap(ts(0), 1.0, None),
            &[],
            &[],
            Some(&envelope(2.0, 4.0)),
        );
        sampler.record(
            ts(30),
            &snap(ts(30), 1.0, None),
            &[],
            &[],
            Some(&envelope(6.0, 8.0)),
        );
        let (_, grid) = sampler.flush().unwrap();
        assert_eq!(grid.up_kw, Some(4.0), "mean of 2.0 and 6.0 is 4.0");
        assert_eq!(grid.down_kw, Some(6.0), "mean of 4.0 and 8.0 is 6.0");
    }

    #[test]
    fn test_record_no_applicable_capacity_limit_persists_none_not_zero() {
        let mut sampler = HistorySampler::new();
        sampler.record(ts(0), &snap(ts(0), 1.0, None), &[], &[], None);
        let (_, grid) = sampler.flush().unwrap();
        assert_eq!(grid.import_limit_kw, None);
        assert_eq!(grid.export_limit_kw, None);
    }

    #[test]
    fn test_record_mid_window_tighter_limit_is_not_diluted_by_unconstrained_portion() {
        let mut sampler = HistorySampler::new();
        // No applicable limit for the first half of the window...
        sampler.record(ts(0), &snap(ts(0), 1.0, None), &[], &[], None);
        // ...then a limit becomes applicable partway through.
        let limits = vec![capacity_snap(ts(30), ts(3600), Some(2.0), None)];
        sampler.record(ts(30), &snap(ts(30), 1.0, None), &[], &limits, None);
        let (_, grid) = sampler.flush().unwrap();
        assert_eq!(
            grid.import_limit_kw,
            Some(2.0),
            "the window's persisted limit must reflect the constrained portion, not be averaged \
             away by the unconstrained portion"
        );
    }

    #[test]
    fn test_record_keeps_tightest_of_multiple_distinct_limits_order_independent() {
        // Looser-then-tighter within the window.
        let mut looser_then_tighter = HistorySampler::new();
        looser_then_tighter.record(
            ts(0),
            &snap(ts(0), 1.0, None),
            &[],
            &[capacity_snap(ts(0), ts(30), Some(5.0), None)],
            None,
        );
        looser_then_tighter.record(
            ts(30),
            &snap(ts(30), 1.0, None),
            &[],
            &[capacity_snap(ts(30), ts(3600), Some(2.0), None)],
            None,
        );
        let (_, grid_a) = looser_then_tighter.flush().unwrap();

        // Tighter-then-looser within the window.
        let mut tighter_then_looser = HistorySampler::new();
        tighter_then_looser.record(
            ts(0),
            &snap(ts(0), 1.0, None),
            &[],
            &[capacity_snap(ts(0), ts(30), Some(2.0), None)],
            None,
        );
        tighter_then_looser.record(
            ts(30),
            &snap(ts(30), 1.0, None),
            &[],
            &[capacity_snap(ts(30), ts(3600), Some(5.0), None)],
            None,
        );
        let (_, grid_b) = tighter_then_looser.flush().unwrap();

        assert_eq!(grid_a.import_limit_kw, Some(2.0));
        assert_eq!(
            grid_b.import_limit_kw,
            Some(2.0),
            "order of observation within the window must not change which value survives"
        );
    }

    // ── PV curtailment aggregation (pv-curtailment-history) ─────────────────

    /// A SimSnapshot with a single "pv" asset carrying the given curtailment fields
    /// (mirrors PvInverter::state_values()'s flattened map). `None` omits the
    /// generation_limit_kw key entirely, matching "no limit active this tick".
    fn pv_snap(
        now: DateTime<Utc>,
        power_kw: f64,
        limit_and_source: Option<(f64, f64)>,
    ) -> SimSnapshot {
        let source_of = |code: f64| {
            use PvCurtailmentSource::*;
            [None, Plan, Capacity, Arbiter, Manual, CommsLoss]
                .into_iter()
                .find(|s| s.as_f64() == code)
                .expect("a PvCurtailmentSource code")
        };
        let history = AssetHistoryView {
            generation_limit_kw: limit_and_source.map(|(limit_kw, _)| limit_kw),
            curtailment_source: Some(
                limit_and_source.map_or(PvCurtailmentSource::None, |(_, c)| source_of(c)),
            ),
            ..Default::default()
        };
        let mut assets = HashMap::new();
        assets.insert(
            "pv".to_string(),
            AssetSnapshot {
                power_kw,
                asset_type: "pv".into(),
                cap_max_import_kw: 0.0,
                cap_max_export_kw: -power_kw,
                available_discharge_kwh: None,
                available_charge_kwh: None,
                forced_power_kw: None,
                response: SetpointResponse::continuous(),
                default_setpoint_kw: power_kw,
                setpoint_kw: power_kw,
                values: HashMap::new(),
                history,
                emergency_what_ifs: None,
                ac_ceiling_kw: None,
            },
        );
        SimSnapshot {
            ts: now,
            grid: GridSnapshot {
                net_power_w: power_kw * 1000.0,
                voltage_v: 230.0,
                import_kwh: 0.0,
                export_kwh: 0.0,
                import_limit_kw: f64::MAX,
                export_limit_kw: -f64::MAX,
            },
            assets,
        }
    }

    #[test]
    fn flush_uncurtailed_window_has_no_limit_or_source() {
        let mut sampler = HistorySampler::new();
        sampler.record(ts(0), &pv_snap(ts(0), -3.0, None), &[], &[], None);
        let (ticks, _) = sampler.flush().unwrap();
        assert_eq!(ticks[0].generation_limit_kw, None);
        assert_eq!(ticks[0].curtailment_source, None);
    }

    #[test]
    fn flush_persists_single_active_limit_and_source() {
        let mut sampler = HistorySampler::new();
        sampler.record(
            ts(0),
            &pv_snap(ts(0), -2.0, Some((-2.0, 1.0))),
            &[],
            &[],
            None,
        );
        let (ticks, _) = sampler.flush().unwrap();
        assert!((ticks[0].generation_limit_kw.unwrap() - (-2.0)).abs() < 1e-9);
        assert_eq!(ticks[0].curtailment_source, Some(1.0), "plan");
    }

    #[test]
    fn flush_prefers_capacity_source_over_plan_within_same_window() {
        // First half of the window is plan-sourced; a brief capacity-sourced event
        // fires for the second half. The window must be tagged capacity, not plan —
        // a brief unplanned event must never be masked by a plan-sourced majority.
        let mut sampler = HistorySampler::new();
        sampler.record(
            ts(0),
            &pv_snap(ts(0), -3.0, Some((-3.0, 1.0))),
            &[],
            &[],
            None,
        );
        sampler.record(
            ts(10),
            &pv_snap(ts(10), -3.0, Some((-3.0, 1.0))),
            &[],
            &[],
            None,
        );
        sampler.record(
            ts(20),
            &pv_snap(ts(20), -2.0, Some((-2.0, 2.0))),
            &[],
            &[],
            None,
        );
        let (ticks, _) = sampler.flush().unwrap();
        assert_eq!(
            ticks[0].curtailment_source,
            Some(2.0),
            "a brief capacity-sourced event must win over a plan-sourced majority"
        );
        assert!(
            (ticks[0].generation_limit_kw.unwrap() - (-2.0)).abs() < 1e-9,
            "the persisted limit must be the capacity-sourced value, not the plan one"
        );
    }

    #[test]
    fn flush_picks_tightest_value_within_same_priority() {
        let mut sampler = HistorySampler::new();
        sampler.record(
            ts(0),
            &pv_snap(ts(0), -3.0, Some((-3.0, 2.0))),
            &[],
            &[],
            None,
        );
        sampler.record(
            ts(10),
            &pv_snap(ts(10), -1.5, Some((-1.5, 2.0))),
            &[],
            &[],
            None,
        );
        let (ticks, _) = sampler.flush().unwrap();
        assert!(
            (ticks[0].generation_limit_kw.unwrap() - (-1.5)).abs() < 1e-9,
            "the tighter (less negative) value within the same priority must win, got {:?}",
            ticks[0].generation_limit_kw
        );
    }

    #[test]
    fn flush_plugged_is_the_window_mean_of_the_plugged_state() {
        let mut sampler = HistorySampler::new();
        for (i, plugged) in [1.0, 1.0, 0.0, 0.0].into_iter().enumerate() {
            let now = ts(i as i64 * 10);
            let mut sim = snap(now, 1.0, Some(0.5));
            sim.assets.get_mut("ev").unwrap().history.plugged = Some(plugged > 0.5);
            sampler.record(now, &sim, &[], &[], None);
        }
        let (ticks, _) = sampler.flush().expect("a partial window still flushes");
        assert_eq!(
            ticks[0].plugged,
            Some(0.5),
            "plugged for half the window's samples"
        );
    }

    #[test]
    fn flush_temperature_is_the_window_mean() {
        let mut sampler = HistorySampler::new();
        for (i, temperature_c) in [20.0, 22.0].into_iter().enumerate() {
            let now = ts(i as i64 * 10);
            let mut sim = snap(now, 1.0, None);
            sim.assets.get_mut("ev").unwrap().history.temperature_c = Some(temperature_c);
            sampler.record(now, &sim, &[], &[], None);
        }
        let (ticks, _) = sampler.flush().unwrap();
        assert_eq!(ticks[0].temperature_c, Some(21.0));
    }

    #[test]
    fn flush_keeps_the_higher_ranked_source_when_a_lower_one_arrives_later() {
        let mut sampler = HistorySampler::new();
        // CommsLoss (5.0) first, then Plan (1.0) with a tighter limit: the source stays
        // CommsLoss and the lower-ranked sample's limit is ignored entirely.
        sampler.record(
            ts(0),
            &pv_snap(ts(0), -1.0, Some((-1.0, 5.0))),
            &[],
            &[],
            None,
        );
        sampler.record(
            ts(10),
            &pv_snap(ts(10), -1.0, Some((-9.0, 1.0))),
            &[],
            &[],
            None,
        );
        let (ticks, _) = sampler.flush().unwrap();
        assert_eq!(ticks[0].curtailment_source, Some(5.0));
        assert_eq!(ticks[0].generation_limit_kw, Some(-1.0));
    }

    #[test]
    fn flush_plugged_is_none_for_an_asset_without_a_plugged_state() {
        let mut sampler = HistorySampler::new();
        sampler.record(ts(0), &snap(ts(0), 1.0, Some(0.5)), &[], &[], None);
        let (ticks, _) = sampler.flush().unwrap();
        assert_eq!(
            ticks[0].plugged, None,
            "no `plugged` value reported: nothing to record, never a 0 that reads as away"
        );
    }

    // R-101: the stored source is `PvCurtailmentSource::as_f64()`, the code the live
    // timeline carries — so arbiter, manual and comms-loss limits keep their source
    // instead of being stored with a limit and no source at all.
    #[test]
    fn flush_stores_every_source_as_its_live_code() {
        for code in [3.0, 4.0, 5.0] {
            let mut sampler = HistorySampler::new();
            sampler.record(
                ts(0),
                &pv_snap(ts(0), -2.0, Some((-2.0, code))),
                &[],
                &[],
                None,
            );
            let (ticks, _) = sampler.flush().unwrap();
            assert_eq!(ticks[0].curtailment_source, Some(code));
            assert!(ticks[0].generation_limit_kw.is_some());
        }
    }
}
