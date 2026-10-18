//! What the signal workbench needs from the BFF: the wire facts of each signal type, and a draft
//! event resolved by the same interval rule the VEN plans against.
//!
//! It decides nothing about timing or units. A draft's repetition is unfolded by
//! `lab_core::event_timing::timed_intervals`, a stored event's declared pattern is timed by
//! `base_intervals`, and a signal's unit comes from `lab_core::wire_contract::EVENT_QUANTITIES`.
//! The UI therefore draws what a receiver will compute and declares what a receiver will read,
//! without a TypeScript copy of either rule (`one-concept-one-function`, GB-48).
//!
//! It never publishes: a draft only becomes an event when the UI posts it to `/api/events`.

use chrono::{DateTime, Utc};
use lab_core::event_timing::{
    base_intervals, timed_intervals, wire_name, OadrEvent, MAX_LOOPED_INTERVALS,
};
use lab_core::wire_contract::{PayloadReader, EVENT_QUANTITIES};
use serde::Serialize;
use serde_json::Value;

use crate::fleet_signals::{bands_for_ven, SignalBand};

/// What a number of one payload type means on the wire.
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WireSignalFacts {
    pub payload_type: &'static str,
    pub units: Option<&'static str>,
    pub currency: Option<&'static str>,
}

/// Every payload type the lab's wire profile defines, with its unit and currency.
pub fn registry() -> Vec<WireSignalFacts> {
    EVENT_QUANTITIES
        .iter()
        .map(|(payload_type, quantity)| WireSignalFacts {
            payload_type,
            units: quantity.units(),
            currency: quantity.currency(),
        })
        .collect()
}

/// One declared interval of an event, timed but not repeated.
#[derive(Debug, Serialize, PartialEq)]
pub struct PatternInterval {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub payloads: Vec<PatternPayload>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct PatternPayload {
    #[serde(rename = "type")]
    pub payload_type: String,
    /// In the lab's unit for the type; null when the payload carries no readable number.
    pub value: Option<f64>,
}

/// The draft as the VTN would hold it, without being sent there.
///
/// A draft has no id, no timestamps and perhaps no program yet; they are filled with placeholders
/// that nothing here reads (priority tie-breaks on `createdDateTime` need an event to be
/// compared with, and a single draft is not). Targets are dropped: the preview shows what the
/// series says, and which VENs it reaches is the page's separate statement.
pub fn draft_event(body: &Value) -> Result<OadrEvent, String> {
    let epoch = "0001-01-01T00:00:00Z";
    let mut event = body
        .as_object()
        .cloned()
        .ok_or("the draft event must be a JSON object")?;
    event.entry("id").or_insert_with(|| "draft".into());
    event.entry("programID").or_insert_with(|| "draft".into());
    event
        .entry("createdDateTime")
        .or_insert_with(|| epoch.into());
    event
        .entry("modificationDateTime")
        .or_insert_with(|| epoch.into());
    event.remove("targets");
    if let Some(Value::Array(intervals)) = event.get_mut("intervals") {
        for (n, interval) in intervals.iter_mut().enumerate() {
            if let Some(interval) = interval.as_object_mut() {
                interval.entry("id").or_insert_with(|| n.into());
            }
        }
    }
    serde_json::from_value(Value::Object(event)).map_err(|e| format!("not a valid event: {e}"))
}

/// The intervals the event declares, each timed by the shared rule and not repeated.
pub fn pattern(event: &OadrEvent, reader: &PayloadReader) -> Result<Vec<PatternInterval>, String> {
    base_intervals(event)
        .into_iter()
        .enumerate()
        .map(|(n, t)| {
            if !t.is_bounded() {
                return Err(format!(
                    "interval {n} has no start or no end the event lets us derive"
                ));
            }
            Ok(PatternInterval {
                start: t.start,
                end: t.end,
                payloads: t
                    .interval
                    .payloads
                    .iter()
                    .map(|p| PatternPayload {
                        payload_type: wire_name(&p.value_type),
                        value: reader
                            .read(event, p)
                            .and_then(|r| r.ok())
                            .map(|reading| reading.value),
                    })
                    .collect(),
            })
        })
        .collect()
}

/// A draft expanded the way receivers expand it, over `[from, to)`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub bands: Vec<SignalBand>,
    /// Receivers stop expanding at `MAX_LOOPED_INTERVALS`; true when that stop falls inside
    /// the asked-for window, so the bands end before the window does.
    pub truncated: bool,
    pub max_looped_intervals: usize,
}

pub fn preview(
    event: &OadrEvent,
    reader: &PayloadReader,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Preview {
    // The cap is always reached by a forever series (2000 hourly intervals is 83 days), so
    // reaching it is not the warning; the warning is that the window runs past where it stopped.
    let expanded = timed_intervals(event);
    let truncated =
        expanded.len() >= MAX_LOOPED_INTERVALS && expanded.last().is_some_and(|t| t.end < to);
    Preview {
        bands: bands_for_ven(event, reader, "draft", from, to),
        truncated,
        max_looped_intervals: MAX_LOOPED_INTERVALS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn t(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    /// 24 hourly prices from 2026-10-18T00:00Z with explicit starts and durations, as the
    /// workbench publishes them.
    fn daily_tariff(duration: Option<&str>) -> Value {
        let intervals: Vec<Value> = (0..24)
            .map(|h| {
                json!({
                    "id": h,
                    "intervalPeriod": {
                        "start": format!("2026-10-18T{h:02}:00:00Z"),
                        "duration": "PT1H",
                    },
                    "payloads": [{ "type": "PRICE", "values": [0.1 + f64::from(h) / 100.0] }],
                })
            })
            .collect();
        let mut event = json!({
            "eventName": "tou",
            "intervalPeriod": { "start": "2026-10-18T00:00:00Z" },
            "intervals": intervals,
        });
        if let Some(d) = duration {
            event["duration"] = json!(d);
        }
        event
    }

    fn reader() -> PayloadReader {
        PayloadReader::default()
    }

    #[test]
    fn registry_declares_price_per_kwh_in_eur() {
        let price = registry()
            .into_iter()
            .find(|f| f.payload_type == "PRICE")
            .expect("PRICE is registered");
        assert_eq!(price.units, Some("KWH"));
        assert_eq!(price.currency, Some("EUR"));
    }

    #[test]
    fn registry_lists_every_type_the_wire_profile_resolves() {
        for (name, _) in EVENT_QUANTITIES {
            assert!(registry().iter().any(|f| f.payload_type == *name));
        }
    }

    #[test]
    fn preview_of_a_forever_series_is_two_days_of_bands_in_a_48h_window() {
        let event = draft_event(&daily_tariff(Some("P9999Y"))).unwrap();
        let p = preview(
            &event,
            &reader(),
            t("2026-10-18T00:00:00Z"),
            t("2026-10-20T00:00:00Z"),
        );
        assert_eq!(p.bands.len(), 48);
        assert!(!p.truncated);
        // Day two repeats day one's prices.
        assert_eq!(p.bands[24].value, p.bands[0].value);
        assert_eq!(p.bands[24].from, t("2026-10-19T00:00:00Z"));
    }

    #[test]
    fn preview_equals_the_bands_the_fleet_view_draws_for_the_same_event() {
        let body = daily_tariff(Some("P9999Y"));
        let draft = draft_event(&body).unwrap();
        let stored = lab_core::test_fixtures::events_from_json(body).remove(0);
        let (from, to) = (t("2026-10-18T00:00:00Z"), t("2026-10-21T00:00:00Z"));
        let via_preview = preview(&draft, &reader(), from, to).bands;
        let via_fleet = bands_for_ven(&stored, &reader(), "ven-1", from, to);
        // The draft carries a placeholder id; everything else must be identical.
        assert_eq!(via_preview.len(), via_fleet.len());
        for (a, b) in via_preview.iter().zip(&via_fleet) {
            assert_eq!((a.from, a.to, a.value), (b.from, b.to, b.value));
        }
    }

    #[test]
    fn preview_without_duration_does_not_repeat() {
        let event = draft_event(&daily_tariff(None)).unwrap();
        let p = preview(
            &event,
            &reader(),
            t("2026-10-18T00:00:00Z"),
            t("2026-10-20T00:00:00Z"),
        );
        assert_eq!(p.bands.len(), 24);
    }

    #[test]
    fn preview_reports_when_the_expansion_cap_cut_the_series_short() {
        // 96 quarter-hour intervals repeated for 30 days is 2880 intervals; receivers stop at 2000.
        let intervals: Vec<Value> = (0..96)
            .map(|q| {
                json!({
                    "intervalPeriod": {
                        "start": format!("2026-10-18T{:02}:{:02}:00Z", q / 4, (q % 4) * 15),
                        "duration": "PT15M",
                    },
                    "payloads": [{ "type": "PRICE", "values": [0.2] }],
                })
            })
            .collect();
        let event = draft_event(&json!({
            "eventName": "quarter-hourly",
            "duration": "P9999Y",
            "intervalPeriod": { "start": "2026-10-18T00:00:00Z" },
            "intervals": intervals,
        }))
        .unwrap();
        let p = preview(
            &event,
            &reader(),
            t("2026-10-18T00:00:00Z"),
            t("2026-11-17T00:00:00Z"),
        );
        assert!(p.truncated);
        assert!(p.bands.len() <= MAX_LOOPED_INTERVALS);
        assert!(p.bands.len() < 2880);
    }

    #[test]
    fn pattern_follows_the_previous_interval_and_does_not_repeat() {
        // No interval declares a start: the first starts at the event's, the rest follow.
        let event = draft_event(&json!({
            "duration": "P9999Y",
            "intervalPeriod": { "start": "2026-10-18T06:00:00Z", "duration": "PT2H" },
            "intervals": [
                { "payloads": [{ "type": "PRICE", "values": [0.1] }] },
                { "payloads": [{ "type": "PRICE", "values": [0.2] }] },
                { "payloads": [{ "type": "PRICE", "values": [0.3] }] },
            ],
        }))
        .unwrap();
        let got = pattern(&event, &reader()).unwrap();
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].start, t("2026-10-18T06:00:00Z"));
        assert_eq!(got[1].start, t("2026-10-18T08:00:00Z"));
        assert_eq!(got[2].end, t("2026-10-18T12:00:00Z"));
        assert_eq!(got[2].payloads[0].value, Some(0.3));
    }

    #[test]
    fn pattern_refuses_an_event_whose_start_cannot_be_derived() {
        let event = draft_event(&json!({
            "intervals": [{ "payloads": [{ "type": "PRICE", "values": [0.1] }] }],
        }))
        .unwrap();
        assert!(pattern(&event, &reader()).is_err());
    }

    #[test]
    fn draft_event_ignores_targets_and_rejects_non_objects() {
        let mut body = daily_tariff(None);
        body["targets"] = json!(["ven-9"]);
        assert!(draft_event(&body).is_ok());
        assert!(draft_event(&json!("nope")).is_err());
    }
}
