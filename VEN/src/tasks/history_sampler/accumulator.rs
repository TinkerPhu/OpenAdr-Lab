//! Pure 1-minute downsampling accumulator (WP1.2), split out of `mod.rs` to
//! keep the `tasks/` file-size cap. Clock-injected (`now` passed in per call)
//! so minute-boundary logic is testable without sleeps.
use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::controller::simulator_port::SimSnapshot;
use crate::entities::history::{GridSample, TickSample};
use crate::entities::tariff_snapshot::TariffSnapshot;

#[derive(Default, Clone)]
struct AssetAcc {
    power_kw_sum: f64,
    soc_pct_sum: f64,
    soc_pct_n: u32,
    temperature_c_sum: f64,
    temperature_c_n: u32,
    n: u32,
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
            if let Some(soc) = snap.val("soc") {
                acc.soc_pct_sum += soc * 100.0;
                acc.soc_pct_n += 1;
            }
            if let Some(temp) = snap.val("temp_c") {
                acc.temperature_c_sum += temp;
                acc.temperature_c_n += 1;
            }
        }

        let net_kw = sim.grid.net_power_w / 1000.0;
        self.grid.import_kw_sum += net_kw.max(0.0);
        self.grid.export_kw_sum += (-net_kw).max(0.0);
        self.grid.n += 1;
        let applicable = tariffs
            .iter()
            .find(|r| r.interval_start <= now && now < r.interval_end);
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
        };
        Some((ticks, grid_sample))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::simulator_port::{AssetSnapshot, GridSnapshot};
    use chrono::TimeZone;

    fn ts(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(secs, 0).unwrap()
    }

    fn snap(now: DateTime<Utc>, power_kw: f64, soc: Option<f64>) -> SimSnapshot {
        let mut values = HashMap::new();
        if let Some(s) = soc {
            values.insert("soc".to_string(), s);
        }
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
                default_setpoint_kw: power_kw,
                setpoint_kw: power_kw,
                values,
            },
        );
        SimSnapshot {
            ts: now,
            grid: GridSnapshot {
                net_power_w: power_kw * 1000.0,
                voltage_v: 230.0,
                import_kwh: 0.0,
                export_kwh: 0.0,
            },
            assets,
        }
    }

    #[test]
    fn test_record_same_minute_does_not_flush() {
        let mut sampler = HistorySampler::new();
        assert!(sampler
            .record(ts(0), &snap(ts(0), 1.0, Some(0.5)), &[])
            .is_none());
        assert!(sampler
            .record(ts(30), &snap(ts(30), 2.0, Some(0.5)), &[])
            .is_none());
    }

    #[test]
    fn test_record_crossing_minute_boundary_flushes_previous_window_mean() {
        let mut sampler = HistorySampler::new();
        sampler.record(ts(0), &snap(ts(0), 1.0, Some(0.5)), &[]);
        sampler.record(ts(30), &snap(ts(30), 3.0, Some(0.5)), &[]);
        let (ticks, grid) = sampler
            .record(ts(60), &snap(ts(60), 5.0, Some(0.5)), &[])
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
        sampler.record(ts(0), &snap(ts(0), 4.0, None), &[]);
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
        sampler.record(ts(0), &snap(ts(0), -3.0, None), &[]);
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
        sampler.record(ts(0), &snap(ts(0), 1.0, None), &tariffs);
        let (_, grid) = sampler.flush().unwrap();
        assert_eq!(grid.import_tariff_eur_kwh, Some(0.25));
        assert_eq!(grid.export_tariff_eur_kwh, Some(0.05));
        assert_eq!(grid.co2_g_kwh, Some(300.0));
    }
}
