//! The text of the base-load cache file: what survives a restart so the learner does not start from nothing.
//!
//! Owns: the encoding of the learner's bucket samples as JSON lines and the tolerant reading of it back.
//! Reads: text handed in. Must never: touch a file (`runtime` does), and never fail a start because a line
//! is damaged: the cache is rebuilt from the live measurement, losing only learned history.

use chrono::{DateTime, Utc};
use lab_forecast::base_load_learner::LoadSample;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Line {
    ts: DateTime<Utc>,
    power_kw: f64,
}

/// One JSON object per line, oldest first.
pub fn encode(samples: &[LoadSample]) -> String {
    samples
        .iter()
        .map(|s| {
            let line = Line {
                ts: s.ts,
                power_kw: s.power_kw,
            };
            serde_json::to_string(&line).expect("a sample always serialises") + "\n"
        })
        .collect()
}

/// The samples of `text`; a line that is not a finite sample is skipped and counted in the second value.
pub fn decode(text: &str) -> (Vec<LoadSample>, usize) {
    let mut skipped = 0;
    let samples = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| match serde_json::from_str::<Line>(l) {
            Ok(line) if line.power_kw.is_finite() => Some(LoadSample {
                ts: line.ts,
                power_kw: line.power_kw,
            }),
            _ => {
                skipped += 1;
                None
            }
        })
        .collect();
    (samples, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn sample(m: u32, kw: f64) -> LoadSample {
        LoadSample {
            ts: Utc.with_ymd_and_hms(2026, 7, 14, 10, m, 0).unwrap(),
            power_kw: kw,
        }
    }

    #[test]
    fn encode_then_decode_returns_the_same_samples_in_order() {
        let samples = vec![
            sample(0, 0.31),
            sample(10, 0.4),
            sample(20, 1.2345678901234567),
        ];
        let (back, skipped) = decode(&encode(&samples));
        assert_eq!(back, samples);
        assert_eq!(skipped, 0);
    }

    #[test]
    fn decode_skips_and_counts_damaged_lines_and_keeps_the_rest() {
        let text = format!(
            "{}not json\n\n{{\"ts\":\"2026-07-14T10:10:00Z\"}}\n{}",
            encode(&[sample(0, 0.3)]),
            encode(&[sample(20, 0.5)])
        );
        let (back, skipped) = decode(&text);
        assert_eq!(back, vec![sample(0, 0.3), sample(20, 0.5)]);
        assert_eq!(skipped, 2);
    }

    #[test]
    fn decode_of_nothing_is_nothing() {
        assert_eq!(decode(""), (vec![], 0));
    }
}
