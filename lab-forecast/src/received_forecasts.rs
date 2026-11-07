//! The forecasts this VEN has received from the lab-forecaster, and why a message was refused.
//!
//! Owns: the latest valid payload per series (one per PV array, one for the base load) and the reason the
//! newest message of a series was refused. Written by the subscriber task (`forecast_feed`), read per tick
//! by the feed resolution and by the routes that show the source. Never interprets a series: the PV and
//! base-load assets do, from what is handed to them.

use std::collections::BTreeMap;

use crate::forecast_payload::{ForecastPayload, ForecastTopics, PayloadError, Sign};
use crate::received_pv::{usable_base_load, ReceivedSeries};
use chrono::{DateTime, Utc};
use serde::Serialize;
use ts_rs::TS;

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

/// Which source a consumer plans on for one series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ForecastSourceKind {
    /// The forecaster's retained message, while it is before its `valid_until`.
    Forecaster,
    /// The consumer's own computation: nothing received, or what was received is past `valid_until`.
    InProcess,
}

/// What the UI shows for one series: the source in use, how old the newest forecaster message is (also
/// when it is too old to use), and why the newest message was refused, if it was.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct ForecastSourceView {
    pub source: ForecastSourceKind,
    /// `model_version` of the newest message received, if any.
    pub model_version: Option<String>,
    /// Seconds since that message's `generated_at`.
    pub age_s: Option<u32>,
    pub valid_until: Option<DateTime<Utc>>,
    pub rejected_reason: Option<String>,
    pub rejected_at: Option<DateTime<Utc>>,
}

/// The views of every series, by key (`pv/<array_id>` for each given array, and `base_load`).
pub fn source_views(
    received: &ReceivedForecasts,
    pv_array_ids: &[String],
    now: DateTime<Utc>,
) -> BTreeMap<String, ForecastSourceView> {
    let view = |key: String, payload: Option<&ForecastPayload>| {
        let usable = payload.is_some_and(|p| p.is_fresh(now));
        let rejection = received.rejection(&key);
        (
            key,
            ForecastSourceView {
                source: if usable {
                    ForecastSourceKind::Forecaster
                } else {
                    ForecastSourceKind::InProcess
                },
                model_version: payload.map(|p| p.model_version.clone()),
                age_s: payload.map(|p| {
                    u32::try_from((now - p.generated_at).num_seconds().max(0)).unwrap_or(u32::MAX)
                }),
                valid_until: payload.map(|p| p.valid_until),
                rejected_reason: rejection.map(|r| r.reason.clone()),
                rejected_at: rejection.map(|r| r.at),
            },
        )
    };
    pv_array_ids
        .iter()
        .map(|id| view(pv_key(id), received.pv.get(id)))
        .chain(std::iter::once(view(
            BASE_LOAD_KEY.to_string(),
            received.base_load.as_ref(),
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
    /// The base-load asset the received series belongs to (the one whose measurement the profile switches
    /// on, the series the forecaster learns from); set when the subscriber is wired.
    pub base_load_asset: Option<String>,
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

    /// The received base-load series and its payload for `asset_id`, while usable at `now`: only for the
    /// asset the series belongs to.
    pub fn base_load_for(
        &self,
        asset_id: &str,
        now: DateTime<Utc>,
    ) -> Option<(ReceivedSeries, &ForecastPayload)> {
        if self.base_load_asset.as_deref() != Some(asset_id) {
            return None;
        }
        let payload = self.base_load.as_ref()?;
        usable_base_load(Some(payload), now).map(|series| (series, payload))
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

    fn views(r: &ReceivedForecasts, now: DateTime<Utc>) -> BTreeMap<String, ForecastSourceView> {
        source_views(r, &["pv".to_string()], now)
    }

    #[test]
    fn source_views_say_forecaster_with_model_age_and_validity_while_usable() {
        let mut r = ReceivedForecasts::default();
        r.apply(&pv_key("pv"), &pv_bytes(at(10, 0)), at(10, 1));
        let v = &views(&r, at(10, 20))["pv/pv"];
        assert_eq!(v.source, ForecastSourceKind::Forecaster);
        assert_eq!(v.model_version.as_deref(), Some("pv-weather-1"));
        assert_eq!(v.age_s, Some(20 * 60));
        assert_eq!(v.valid_until, Some(at(11, 30)));
        assert_eq!(v.rejected_reason, None);
    }

    #[test]
    fn source_views_say_in_process_but_still_give_the_age_once_the_message_is_past_valid_until() {
        let mut r = ReceivedForecasts::default();
        r.apply(&pv_key("pv"), &pv_bytes(at(10, 0)), at(10, 1));
        let v = &views(&r, at(12, 0))["pv/pv"];
        assert_eq!(v.source, ForecastSourceKind::InProcess);
        assert_eq!(
            v.age_s,
            Some(2 * 3600),
            "how old the forecaster's last word is"
        );
    }

    #[test]
    fn source_views_say_in_process_with_no_age_when_nothing_was_received() {
        let v = &views(&ReceivedForecasts::default(), at(10, 0))["pv/pv"];
        assert_eq!(v.source, ForecastSourceKind::InProcess);
        assert_eq!(
            (v.age_s, v.model_version.clone(), v.valid_until),
            (None, None, None)
        );
    }

    #[test]
    fn source_views_carry_the_rejection_reason_and_time() {
        let mut r = ReceivedForecasts::default();
        r.apply(&pv_key("pv"), b"{}", at(10, 5));
        let v = &views(&r, at(10, 6))["pv/pv"];
        assert!(v
            .rejected_reason
            .as_deref()
            .unwrap()
            .contains("parse error"));
        assert_eq!(v.rejected_at, Some(at(10, 5)));
        assert_eq!(v.source, ForecastSourceKind::InProcess);
    }

    #[test]
    fn source_views_cover_the_base_load_and_every_given_array() {
        let r = ReceivedForecasts::default();
        let v = source_views(&r, &["a".to_string(), "b".to_string()], at(10, 0));
        let keys: Vec<&str> = v.keys().map(String::as_str).collect();
        assert_eq!(keys, ["base_load", "pv/a", "pv/b"]);
    }

    fn base_load_bytes(generated: DateTime<Utc>) -> Vec<u8> {
        let p = ForecastPayload::base_load(
            PayloadMeta {
                generated_at: generated,
                valid_until: generated + chrono::Duration::minutes(90),
                model_version: "base-load-profile-1".into(),
                producer_id: "fc".into(),
                confidence: 0.5,
            },
            SeriesGrid {
                start: generated,
                step_s: 3600,
            },
            vec![0.4, 0.6],
        )
        .unwrap();
        serde_json::to_vec(&p).unwrap()
    }

    #[test]
    fn base_load_for_gives_the_series_and_payload_only_to_its_own_asset_while_usable() {
        let mut r = ReceivedForecasts {
            base_load_asset: Some("house".into()),
            ..Default::default()
        };
        r.apply(BASE_LOAD_KEY, &base_load_bytes(at(10, 0)), at(10, 1));
        let (series, payload) = r.base_load_for("house", at(10, 30)).unwrap();
        assert_eq!(series.at(at(10, 30)), Some(0.4));
        assert_eq!(payload.model_version, "base-load-profile-1");
        assert!(
            r.base_load_for("other", at(10, 30)).is_none(),
            "not another asset's"
        );
        assert!(
            r.base_load_for("house", at(12, 0)).is_none(),
            "not past valid_until"
        );
    }

    #[test]
    fn base_load_for_is_none_when_no_asset_is_named() {
        let mut r = ReceivedForecasts::default();
        r.apply(BASE_LOAD_KEY, &base_load_bytes(at(10, 0)), at(10, 1));
        assert!(r.base_load_for("house", at(10, 30)).is_none());
    }

    #[test]
    fn subscriptions_pair_each_pv_array_topic_and_the_base_load_with_its_key() {
        let topics = ForecastTopics::new("openadr-lab", "hometown").unwrap();
        let subs = subscriptions(&topics, &["pv".to_string(), "roof-2".to_string()]);
        assert_eq!(
            subs,
            vec![
                (
                    "openadr-lab/forecast/hometown/pv/pv/power_kw".to_string(),
                    "pv/pv".to_string()
                ),
                (
                    "openadr-lab/forecast/hometown/pv/roof-2/power_kw".to_string(),
                    "pv/roof-2".to_string()
                ),
                (
                    "openadr-lab/forecast/hometown/base_load/power_kw".to_string(),
                    BASE_LOAD_KEY.to_string()
                ),
            ]
        );
    }

    #[test]
    fn subscriptions_leave_out_an_array_id_the_topic_layout_refuses() {
        let topics = ForecastTopics::new("openadr-lab", "hometown").unwrap();
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
