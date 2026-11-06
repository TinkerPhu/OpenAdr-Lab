//! The forecasts this VEN has received from the lab-forecaster, and why a message was refused.
//!
//! Owns: the latest valid payload per series (one per PV array, one for the base load) and the reason the
//! newest message of a series was refused. Written by the subscriber task (`forecast_feed`), read per tick
//! by the feed resolution and by the routes that show the source. Never interprets a series: the PV and
//! base-load assets do, from what is handed to them.

use std::collections::BTreeMap;

use crate::forecast_payload::{ForecastPayload, ForecastTopics, PayloadError, Sign};
use chrono::{DateTime, Utc};

/// The key of the base-load series in `rejected`; PV arrays are `pv/<array_id>`.
pub const BASE_LOAD_KEY: &str = "base_load";

pub fn pv_key(array_id: &str) -> String {
    format!("pv/{array_id}")
}

/// The topics a consumer subscribes to and the series key each one feeds: one per PV array it has, and the
/// base load. An array id the topic layout refuses is left out.
pub fn subscriptions(topics: &ForecastTopics, pv_array_ids: &[String]) -> Vec<(String, String)> {
    pv_array_ids
        .iter()
        .filter_map(|id| Some((topics.pv_array(id).ok()?, pv_key(id))))
        .chain(std::iter::once((
            topics.base_load(),
            BASE_LOAD_KEY.to_string(),
        )))
        .collect()
}

/// A refused message: when, and why (one line).
#[derive(Debug, Clone, PartialEq)]
pub struct Rejection {
    pub at: DateTime<Utc>,
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct ReceivedForecasts {
    /// Latest valid PV payload by array id.
    pub pv: BTreeMap<String, ForecastPayload>,
    pub base_load: Option<ForecastPayload>,
    /// The newest refused message by series key; cleared when a valid one for the key arrives.
    pub rejected: BTreeMap<String, Rejection>,
}

impl ReceivedForecasts {
    /// Take the raw bytes of a message for series `key`. A valid payload replaces the previous one and clears
    /// the rejection; a refused one leaves the previous payload (it stays usable until its own `valid_until`)
    /// and records the reason, so the UI can show it.
    pub fn apply(&mut self, key: &str, payload: &[u8], now: DateTime<Utc>) {
        match ForecastPayload::parse(payload).and_then(|p| Self::check_kind(key, p)) {
            Ok(parsed) => {
                self.rejected.remove(key);
                match key.strip_prefix("pv/") {
                    Some(array_id) => {
                        self.pv.insert(array_id.to_string(), parsed);
                    }
                    None => self.base_load = Some(parsed),
                }
            }
            Err(error) => {
                self.rejected.insert(
                    key.to_string(),
                    Rejection {
                        at: now,
                        reason: rejection_reason(&error),
                    },
                );
            }
        }
    }

    /// A PV key takes only a series declared as generation, the base-load key only one declared as consumption:
    /// the sign is what tells a reader which way a positive value points.
    fn check_kind(key: &str, payload: ForecastPayload) -> Result<ForecastPayload, PayloadError> {
        let (wanted, name) = if key.starts_with("pv/") {
            (Sign::GenerationPositive, "generation")
        } else {
            (Sign::ConsumptionPositive, "consumption")
        };
        if payload.sign == wanted {
            Ok(payload)
        } else {
            Err(PayloadError::Invalid(format!(
                "{key} takes a {name}_positive series"
            )))
        }
    }

    /// The reason for `key`, if its newest message was refused.
    pub fn rejection(&self, key: &str) -> Option<&Rejection> {
        self.rejected.get(key)
    }
}

/// Why a payload was refused, as shown in the UI: the validator's own one-line text.
pub fn rejection_reason(error: &PayloadError) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forecast_payload::{PayloadMeta, SeriesGrid};
    use chrono::TimeZone;

    fn at(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 5, h, m, 0).unwrap()
    }

    fn pv_bytes(generated: DateTime<Utc>) -> Vec<u8> {
        let p = ForecastPayload::pv_natural(
            PayloadMeta {
                generated_at: generated,
                valid_until: generated + chrono::Duration::minutes(90),
                model_version: "pv-weather-1".into(),
                producer_id: "fc".into(),
                confidence: 0.7,
            },
            SeriesGrid {
                start: generated,
                step_s: 3600,
            },
            vec![0.0, 1.0],
        )
        .unwrap();
        serde_json::to_vec(&p).unwrap()
    }

    #[test]
    fn subscriptions_pair_each_pv_array_topic_and_the_base_load_with_its_key() {
        let topics = ForecastTopics::new("openadr-lab", "zunzgen").unwrap();
        let subs = subscriptions(&topics, &["pv".to_string(), "roof-2".to_string()]);
        assert_eq!(
            subs,
            vec![
                (
                    "openadr-lab/forecast/zunzgen/pv/pv/power_kw".to_string(),
                    "pv/pv".to_string()
                ),
                (
                    "openadr-lab/forecast/zunzgen/pv/roof-2/power_kw".to_string(),
                    "pv/roof-2".to_string()
                ),
                (
                    "openadr-lab/forecast/zunzgen/base_load/power_kw".to_string(),
                    BASE_LOAD_KEY.to_string()
                ),
            ]
        );
    }

    #[test]
    fn subscriptions_leave_out_an_array_id_the_topic_layout_refuses() {
        let topics = ForecastTopics::new("openadr-lab", "zunzgen").unwrap();
        let subs = subscriptions(&topics, &["total".to_string(), "a/b".to_string()]);
        assert_eq!(subs.len(), 1, "only the base load remains: {subs:?}");
    }

    #[test]
    fn apply_keeps_a_valid_payload_under_its_key() {
        let mut r = ReceivedForecasts::default();
        r.apply(&pv_key("pv"), &pv_bytes(at(10, 0)), at(10, 1));
        assert_eq!(r.pv["pv"].generated_at, at(10, 0));
        assert!(r.rejected.is_empty());
    }

    #[test]
    fn apply_routes_the_base_load_key_to_the_base_load_slot() {
        let mut r = ReceivedForecasts::default();
        let p = ForecastPayload::base_load(
            PayloadMeta {
                generated_at: at(10, 0),
                valid_until: at(11, 30),
                model_version: "base-load-profile-1".into(),
                producer_id: "fc".into(),
                confidence: 0.5,
            },
            SeriesGrid {
                start: at(10, 0),
                step_s: 3600,
            },
            vec![0.4],
        )
        .unwrap();
        r.apply(BASE_LOAD_KEY, &serde_json::to_vec(&p).unwrap(), at(10, 1));
        assert!(r.base_load.is_some() && r.pv.is_empty());
    }

    #[test]
    fn a_newer_valid_payload_replaces_the_older_and_clears_a_rejection() {
        let mut r = ReceivedForecasts::default();
        r.apply(&pv_key("pv"), b"not json", at(10, 0));
        assert!(r.rejection(&pv_key("pv")).is_some());
        r.apply(&pv_key("pv"), &pv_bytes(at(10, 5)), at(10, 6));
        assert_eq!(r.pv["pv"].generated_at, at(10, 5));
        assert!(r.rejection(&pv_key("pv")).is_none());
    }

    #[test]
    fn a_refused_message_records_the_reason_and_keeps_the_previous_payload() {
        let mut r = ReceivedForecasts::default();
        r.apply(&pv_key("pv"), &pv_bytes(at(10, 0)), at(10, 1));
        let mut v: serde_json::Value = serde_json::from_slice(&pv_bytes(at(10, 5))).unwrap();
        v["schema_version"] = "2.0.0".into();
        r.apply(&pv_key("pv"), &serde_json::to_vec(&v).unwrap(), at(10, 6));
        assert_eq!(
            r.pv["pv"].generated_at,
            at(10, 0),
            "the older valid payload stays"
        );
        let rej = r.rejection(&pv_key("pv")).unwrap();
        assert_eq!(rej.at, at(10, 6));
        assert!(rej.reason.contains("2.0.0"), "{}", rej.reason);
    }

    #[test]
    fn a_message_with_a_missing_declaration_names_the_field() {
        let mut r = ReceivedForecasts::default();
        let mut v: serde_json::Value = serde_json::from_slice(&pv_bytes(at(10, 0))).unwrap();
        v.as_object_mut().unwrap().remove("unit");
        r.apply(&pv_key("pv"), &serde_json::to_vec(&v).unwrap(), at(10, 1));
        assert!(r.rejection(&pv_key("pv")).unwrap().reason.contains("unit"));
        assert!(r.pv.is_empty());
    }

    #[test]
    fn a_pv_series_arriving_on_the_base_load_key_is_refused() {
        let mut r = ReceivedForecasts::default();
        r.apply(BASE_LOAD_KEY, &pv_bytes(at(10, 0)), at(10, 1));
        assert!(r.base_load.is_none());
        assert!(r
            .rejection(BASE_LOAD_KEY)
            .unwrap()
            .reason
            .contains("consumption"));
    }

    #[test]
    fn a_consumption_series_arriving_on_a_pv_key_is_refused() {
        let mut r = ReceivedForecasts::default();
        let p = ForecastPayload::base_load(
            PayloadMeta {
                generated_at: at(10, 0),
                valid_until: at(11, 30),
                model_version: "m".into(),
                producer_id: "fc".into(),
                confidence: 0.5,
            },
            SeriesGrid {
                start: at(10, 0),
                step_s: 3600,
            },
            vec![0.4],
        )
        .unwrap();
        r.apply(&pv_key("pv"), &serde_json::to_vec(&p).unwrap(), at(10, 1));
        assert!(r.pv.is_empty());
        assert!(r
            .rejection(&pv_key("pv"))
            .unwrap()
            .reason
            .contains("generation"));
    }
}
