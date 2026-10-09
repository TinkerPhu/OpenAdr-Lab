//! What each VEN was being told, and when (phase 1 §1/§2).
//!
//! The power curves say what a site did. On their own they cannot say what it
//! was asked to do, so a dip at 09:15 looks the same whether a limit landed or
//! somebody boiled a kettle. This resolves the VTN's events into per-VEN bands
//! — "ven-3 was under a 3 kW import limit from 09:10 to 09:40" — so the two can
//! be drawn on one axis.
//!
//! The interval timing comes from `lab_core::event_timing`, the same rule the
//! VEN plans against. That is the whole point of the shared crate: a band drawn
//! here and the limit actually applied there cannot disagree about when
//! interval *i* runs, which is precisely what GB-48 was.

use chrono::{DateTime, Utc};
use lab_core::event_timing::{timed_intervals, wire_name, OadrEvent};
use lab_core::wire_contract::{DeclaredBy, PayloadReader};
use serde::Serialize;

/// One interval of one event, as it applies to one VEN.
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SignalBand {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    #[serde(rename = "eventID")]
    pub event_id: String,
    pub event_name: Option<String>,
    /// The payload type this band carries, spelled as OpenADR spells it
    /// (`IMPORT_CAPACITY_LIMIT`, `PRICE`, `SIMPLE`…) — one vocabulary across
    /// the wire, the API and the chart legend (`dto`).
    pub payload_type: String,
    /// The first numeric value, when the payload has one. `None` for payload
    /// types that carry no number (a state, say) rather than a zero, and for a
    /// value whose declared unit this profile does not read (see `refused`).
    pub value: Option<f64>,
    /// Who said what unit `value` is in: `EVENT`, `PROGRAM`, or `PROFILE_DEFAULT` when nobody
    /// did and `docs/reference/WIRE_PROFILE.md`'s default was assumed. `None` when there is no
    /// unit to declare. The same reader the VEN acts through (`lab_core::wire_contract`), so a
    /// band and the limit a VEN applied cannot read one value two ways.
    pub units_declared_by: Option<&'static str>,
    /// Why `value` was not read, when the payload declares a unit or currency this profile does
    /// not read. A VEN drops such a payload too.
    pub refused: Option<String>,
}

// Priority is deliberately absent: `openleadr_wire::event::Priority` is a
// newtype with no accessor, and a band nothing draws is not worth reaching
// around a type's own encapsulation for. When overlapping events need ranking
// here, the accessor belongs upstream.

/// Does this VEN match the object's target list?
///
/// An empty list means "every VEN" in OpenADR 3.1 — not "no VEN", which is the
/// reading that silently hides every open program from every site.
pub fn targets_ven(targets: &[String], ven_name: &str) -> bool {
    targets.is_empty() || targets.iter().any(|t| t == ven_name)
}

/// Resolve one event into the bands it puts on one VEN inside `[from, to)`.
///
/// Returns nothing when the event does not target the VEN, and clips the bands
/// to the window so a month-long tariff does not arrive as a month-long band.
pub fn bands_for_ven(
    event: &OadrEvent,
    reader: &PayloadReader,
    ven_name: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Vec<SignalBand> {
    let targets: Vec<String> = event
        .content
        .targets
        .iter()
        .map(|t| t.to_string())
        .collect();
    if !targets_ven(&targets, ven_name) {
        return Vec::new();
    }

    let event_id = event.id.to_string();
    let event_name = event.content.event_name.clone();

    timed_intervals(event)
        .into_iter()
        .filter(|iv| iv.start < to && iv.end > from)
        .flat_map(|iv| {
            // Clipped to the asked-for window: the caller drew an axis, and a
            // band that runs off it is a band that misleads about its extent.
            let band_from = iv.start.max(from);
            let band_to = iv.end.min(to);
            let (id, name) = (event_id.clone(), event_name.clone());
            iv.interval.payloads.iter().map(move |p| {
                let read = reader.read(event, p);
                let reading = read.as_ref().and_then(|r| r.as_ref().ok());
                SignalBand {
                    from: band_from,
                    to: band_to,
                    event_id: id.clone(),
                    event_name: name.clone(),
                    payload_type: wire_name(&p.value_type),
                    value: reading.map(|r| r.value),
                    units_declared_by: reading.and_then(|r| match r.declared_by {
                        DeclaredBy::Event => Some("EVENT"),
                        DeclaredBy::Program => Some("PROGRAM"),
                        DeclaredBy::ProfileDefault => Some("PROFILE_DEFAULT"),
                        DeclaredBy::NothingToDeclare => None,
                    }),
                    refused: read
                        .and_then(|r| r.err())
                        .map(|refusal| refusal.to_string()),
                }
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn event(json: serde_json::Value) -> OadrEvent {
        lab_core::test_fixtures::events_from_json(json).remove(0)
    }

    fn limit_event(targets: serde_json::Value) -> OadrEvent {
        event(serde_json::json!({
            "id": "ev-1",
            "programID": "p",
            "eventName": "peak",
            "targets": targets,
            "intervalPeriod": {"start": "2026-09-22T09:00:00Z", "duration": "PT1H"},
            "intervals": [{
                "id": 0,
                "payloads": [{"type": "IMPORT_CAPACITY_LIMIT", "values": [3.0]}]
            }]
        }))
    }

    #[test]
    fn a_targeted_ven_gets_the_band_with_its_value_and_type() {
        let bands = bands_for_ven(
            &limit_event(serde_json::json!(["ven-1"])),
            &PayloadReader::default(),
            "ven-1",
            t("2026-09-22T08:00:00Z"),
            t("2026-09-22T11:00:00Z"),
        );
        assert_eq!(bands.len(), 1);
        assert_eq!(bands[0].payload_type, "IMPORT_CAPACITY_LIMIT");
        assert_eq!(bands[0].value, Some(3.0));
        assert_eq!(bands[0].from, t("2026-09-22T09:00:00Z"));
        assert_eq!(bands[0].to, t("2026-09-22T10:00:00Z"));
    }

    #[test]
    fn a_ven_the_event_does_not_target_gets_nothing() {
        let bands = bands_for_ven(
            &limit_event(serde_json::json!(["ven-1"])),
            &PayloadReader::default(),
            "ven-7",
            t("2026-09-22T08:00:00Z"),
            t("2026-09-22T11:00:00Z"),
        );
        assert!(bands.is_empty());
    }

    /// An empty target list is "every VEN" in 3.1. Reading it as "no VEN"
    /// hides every open program from every site — quietly, since an empty
    /// result looks like a quiet grid.
    #[test]
    fn an_empty_target_list_reaches_every_ven() {
        let bands = bands_for_ven(
            &limit_event(serde_json::json!([])),
            &PayloadReader::default(),
            "ven-19",
            t("2026-09-22T08:00:00Z"),
            t("2026-09-22T11:00:00Z"),
        );
        assert_eq!(bands.len(), 1);
    }

    /// The caller drew an axis; a band running off it misleads about its own
    /// extent.
    #[test]
    fn bands_are_clipped_to_the_asked_for_window() {
        let bands = bands_for_ven(
            &limit_event(serde_json::json!([])),
            &PayloadReader::default(),
            "ven-1",
            t("2026-09-22T09:15:00Z"),
            t("2026-09-22T09:45:00Z"),
        );
        assert_eq!(bands[0].from, t("2026-09-22T09:15:00Z"));
        assert_eq!(bands[0].to, t("2026-09-22T09:45:00Z"));
    }

    #[test]
    fn an_event_entirely_outside_the_window_contributes_nothing() {
        let bands = bands_for_ven(
            &limit_event(serde_json::json!([])),
            &PayloadReader::default(),
            "ven-1",
            t("2026-09-22T12:00:00Z"),
            t("2026-09-22T13:00:00Z"),
        );
        assert!(bands.is_empty());
    }

    /// SIMPLE's level is an `Integer` on the wire. A `Number`-only match drops
    /// every SIMPLE band, and an empty overlay reads as "no signal was sent".
    #[test]
    fn a_simple_level_is_read_as_a_number() {
        let ev = event(serde_json::json!({
            "id": "ev-2", "programID": "p",
            "intervalPeriod": {"start": "2026-09-22T09:00:00Z", "duration": "PT1H"},
            "intervals": [{"id": 0, "payloads": [{"type": "SIMPLE", "values": [2]}]}]
        }));
        let bands = bands_for_ven(
            &ev,
            &PayloadReader::default(),
            "ven-1",
            t("2026-09-22T08:00:00Z"),
            t("2026-09-22T11:00:00Z"),
        );
        assert_eq!(bands[0].value, Some(2.0));
    }

    // ── GB-50: a band says who declared its unit, or why it was not read ─────────────

    fn limit_event_declaring(units: Option<&str>) -> OadrEvent {
        let mut ev = serde_json::to_value(limit_event(serde_json::json!([]))).unwrap();
        if let Some(units) = units {
            ev["payloadDescriptors"] =
                serde_json::json!([{ "payloadType": "IMPORT_CAPACITY_LIMIT", "units": units }]);
        }
        serde_json::from_value(ev).unwrap()
    }

    fn one_band(ev: &OadrEvent, reader: &PayloadReader) -> SignalBand {
        bands_for_ven(
            ev,
            reader,
            "ven-1",
            t("2026-09-22T08:00:00Z"),
            t("2026-09-22T11:00:00Z"),
        )
        .remove(0)
    }

    #[test]
    fn a_band_says_who_declared_its_unit() {
        let declared = one_band(
            &limit_event_declaring(Some("KW")),
            &PayloadReader::default(),
        );
        assert_eq!(declared.units_declared_by, Some("EVENT"));

        let assumed = one_band(&limit_event_declaring(None), &PayloadReader::default());
        assert_eq!(
            (assumed.value, assumed.units_declared_by),
            (Some(3.0), Some("PROFILE_DEFAULT"))
        );

        let by_program = PayloadReader::from_program_rows(&[serde_json::json!({
            "id": "p", "payloadDescriptors": [{ "payloadType": "IMPORT_CAPACITY_LIMIT", "units": "KW" }],
        })]);
        assert_eq!(
            one_band(&limit_event_declaring(None), &by_program).units_declared_by,
            Some("PROGRAM")
        );
    }

    /// A VEN drops a payload whose declared unit it cannot read; a band showing its number
    /// anyway would draw a limit no site applied.
    #[test]
    fn a_band_with_a_unit_this_profile_does_not_read_has_no_value_and_says_why() {
        let band = one_band(
            &limit_event_declaring(Some("VOLTS")),
            &PayloadReader::default(),
        );
        assert_eq!(band.value, None);
        let why = band.refused.expect("the refusal is stated");
        assert!(
            why.contains("units VOLTS") && why.contains("units KW"),
            "{why}"
        );
    }
}
