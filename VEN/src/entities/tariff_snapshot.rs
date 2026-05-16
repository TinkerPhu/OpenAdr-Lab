use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::common::{Interpolation, TimeSeries};

/// A single tariff data point for a time interval (tariff = price per kWh).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TariffSnapshot {
    pub interval_start: DateTime<Utc>,
    #[serde(skip_serializing)]
    pub interval_end: DateTime<Utc>,
    pub import_tariff_eur_kwh: Option<f64>,
    pub export_tariff_eur_kwh: Option<f64>,
    pub co2_g_kwh: Option<f64>,
}

/// Three independent TimeSeries (import, export, CO2) with Step interpolation,
/// constructed from TariffSnapshot lists at the OpenADR interface boundary.
#[derive(Debug, Clone)]
pub struct TariffTimeSeries {
    pub import_eur_kwh: TimeSeries,
    pub export_eur_kwh: TimeSeries,
    pub co2_g_kwh: TimeSeries,
}

impl TariffTimeSeries {
    /// Convert a slice of TariffSnapshots into three Step-interpolated TimeSeries.
    ///
    /// For each snapshot, only `Some` values are emitted into the corresponding series.
    /// Snapshots are sorted by `interval_start`; duplicate timestamps use last-write-wins.
    pub fn from_snapshots(snapshots: &[TariffSnapshot]) -> Self {
        let mut sorted: Vec<&TariffSnapshot> = snapshots.iter().collect();
        sorted.sort_by_key(|s| s.interval_start);

        let mut import_samples: Vec<(DateTime<Utc>, f64)> = Vec::new();
        let mut export_samples: Vec<(DateTime<Utc>, f64)> = Vec::new();
        let mut co2_samples: Vec<(DateTime<Utc>, f64)> = Vec::new();

        for snap in &sorted {
            let ts = snap.interval_start;
            if let Some(v) = snap.import_tariff_eur_kwh {
                // Last-write-wins: replace if duplicate timestamp
                if let Some(last) = import_samples.last_mut() {
                    if last.0 == ts {
                        last.1 = v;
                    } else {
                        import_samples.push((ts, v));
                    }
                } else {
                    import_samples.push((ts, v));
                }
            }
            if let Some(v) = snap.export_tariff_eur_kwh {
                if let Some(last) = export_samples.last_mut() {
                    if last.0 == ts {
                        last.1 = v;
                    } else {
                        export_samples.push((ts, v));
                    }
                } else {
                    export_samples.push((ts, v));
                }
            }
            if let Some(v) = snap.co2_g_kwh {
                if let Some(last) = co2_samples.last_mut() {
                    if last.0 == ts {
                        last.1 = v;
                    } else {
                        co2_samples.push((ts, v));
                    }
                } else {
                    co2_samples.push((ts, v));
                }
            }
        }

        Self {
            import_eur_kwh: TimeSeries {
                samples: import_samples,
                interpolation: Interpolation::Step,
            },
            export_eur_kwh: TimeSeries {
                samples: export_samples,
                interpolation: Interpolation::Step,
            },
            co2_g_kwh: TimeSeries {
                samples: co2_samples,
                interpolation: Interpolation::Step,
            },
        }
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.import_eur_kwh.samples.is_empty()
            && self.export_eur_kwh.samples.is_empty()
            && self.co2_g_kwh.samples.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn ts(hour: u32, min: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 3, 21, hour, min, 0).unwrap()
    }

    fn snap(
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        imp: Option<f64>,
        exp: Option<f64>,
        co2: Option<f64>,
    ) -> TariffSnapshot {
        TariffSnapshot {
            interval_start: start,
            interval_end: end,
            import_tariff_eur_kwh: imp,
            export_tariff_eur_kwh: exp,
            co2_g_kwh: co2,
        }
    }

    #[test]
    fn from_snapshots_normal_case() {
        let snaps = vec![
            snap(ts(10, 0), ts(11, 0), Some(0.20), Some(0.05), Some(300.0)),
            snap(ts(11, 0), ts(12, 0), Some(0.15), Some(0.04), Some(250.0)),
        ];
        let tts = TariffTimeSeries::from_snapshots(&snaps);
        assert_eq!(tts.import_eur_kwh.samples.len(), 2);
        assert_eq!(tts.export_eur_kwh.samples.len(), 2);
        assert_eq!(tts.co2_g_kwh.samples.len(), 2);
        assert!((tts.import_eur_kwh.samples[0].1 - 0.20).abs() < 1e-9);
        assert!((tts.import_eur_kwh.samples[1].1 - 0.15).abs() < 1e-9);
        assert!(!tts.is_empty());
    }

    #[test]
    fn from_snapshots_none_gaps() {
        // CO2 missing on second snapshot
        let snaps = vec![
            snap(ts(10, 0), ts(11, 0), Some(0.20), Some(0.05), Some(300.0)),
            snap(ts(11, 0), ts(12, 0), Some(0.15), Some(0.04), None),
        ];
        let tts = TariffTimeSeries::from_snapshots(&snaps);
        assert_eq!(tts.import_eur_kwh.samples.len(), 2);
        assert_eq!(tts.export_eur_kwh.samples.len(), 2);
        assert_eq!(tts.co2_g_kwh.samples.len(), 1); // only first snap
    }

    #[test]
    fn from_snapshots_empty_input() {
        let tts = TariffTimeSeries::from_snapshots(&[]);
        assert!(tts.import_eur_kwh.samples.is_empty());
        assert!(tts.export_eur_kwh.samples.is_empty());
        assert!(tts.co2_g_kwh.samples.is_empty());
        assert!(tts.is_empty());
    }

    #[test]
    fn from_snapshots_unsorted_input() {
        let snaps = vec![
            snap(ts(12, 0), ts(13, 0), Some(0.10), None, None),
            snap(ts(10, 0), ts(11, 0), Some(0.20), None, None),
            snap(ts(11, 0), ts(12, 0), Some(0.15), None, None),
        ];
        let tts = TariffTimeSeries::from_snapshots(&snaps);
        assert_eq!(tts.import_eur_kwh.samples.len(), 3);
        // Must be sorted ascending
        assert!(tts.import_eur_kwh.samples[0].0 < tts.import_eur_kwh.samples[1].0);
        assert!(tts.import_eur_kwh.samples[1].0 < tts.import_eur_kwh.samples[2].0);
        assert!((tts.import_eur_kwh.samples[0].1 - 0.20).abs() < 1e-9);
        assert!((tts.import_eur_kwh.samples[1].1 - 0.15).abs() < 1e-9);
        assert!((tts.import_eur_kwh.samples[2].1 - 0.10).abs() < 1e-9);
    }

    #[test]
    fn from_snapshots_duplicate_timestamps_last_wins() {
        let snaps = vec![
            snap(ts(10, 0), ts(11, 0), Some(0.20), None, None),
            snap(ts(10, 0), ts(11, 0), Some(0.30), None, None),
        ];
        let tts = TariffTimeSeries::from_snapshots(&snaps);
        assert_eq!(tts.import_eur_kwh.samples.len(), 1);
        assert!((tts.import_eur_kwh.samples[0].1 - 0.30).abs() < 1e-9); // last wins
    }
}
