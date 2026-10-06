//! Summary of an asset's recorded power over a trailing window: what the Controller's
//! "Flexibility & Forecast" panel shows for base load. The window's samples are acquired
//! outside the asset (the history store) and injected; turning them into "what does this
//! load typically draw" is this module.

/// Length of the trailing window, in days: the "14 d" in the base-load labels.
pub const LOAD_WINDOW_DAYS: i64 = 14;

/// Mean and peak of the recorded power samples inside the window. Precision is
/// deliberately modest: each sample is already a 1-minute mean, and the window only
/// moves once an hour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoadWindowStats {
    pub avg_kw: f64,
    pub max_kw: f64,
}

impl LoadWindowStats {
    /// `None` when there are no samples: the caller shows "-", never 0.
    pub fn from_power_kw(samples_kw: impl IntoIterator<Item = f64>) -> Option<Self> {
        let (sum_kw, n, max_kw) = samples_kw
            .into_iter()
            .fold((0.0_f64, 0usize, f64::NEG_INFINITY), |(s, n, m), v| {
                (s + v, n + 1, m.max(v))
            });
        (n > 0).then(|| Self {
            avg_kw: sum_kw / n as f64,
            max_kw,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_power_kw_is_none_without_samples() {
        assert_eq!(LoadWindowStats::from_power_kw([]), None);
    }

    #[test]
    fn from_power_kw_averages_whatever_samples_exist() {
        let s = LoadWindowStats::from_power_kw([0.2, 0.4, 1.8]).unwrap();
        assert!((s.avg_kw - 0.8).abs() < 1e-9);
        assert_eq!(s.max_kw, 1.8);
    }
}
