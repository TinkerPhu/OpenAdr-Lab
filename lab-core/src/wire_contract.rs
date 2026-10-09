//! What a number on the wire *means*: the machine-readable half of
//! `docs/reference/WIRE_PROFILE.md`, for both directions.
//!
//! `wire-contracts`: every message we emit declares what its values mean, and every message we
//! accept is interpreted from what it says, never from an assumption in the reader. Emitting was
//! done first (`VEN/src/controller/report_payload.rs` builds report payloads from the table here).
//! This module adds the accepting half (GB-50): [`PayloadReader`] is the one place an incoming
//! event value becomes a number the VEN or the BFF uses.
//!
//! Shared because both read the same events: the VEN to act on them, the BFF to draw them. Two
//! readers is how one of them ends up assuming a unit the other checks.
//!
//! No conversion between units happens here. OpenADR's `Unit` enum has no watt and no megawatt,
//! so "a unit we could convert" does not occur in practice: a payload declares the unit the
//! profile names, declares nothing (the profile default is assumed, and said to be), or declares
//! something else and is refused.

use std::collections::{BTreeMap, HashMap};

use openleadr_wire::event::{EventPayloadDescriptor, EventValuesMap};

use crate::event_timing::{numeric_value, wire_name, OadrEvent};

/// The quantity a payload type carries, in the unit this lab uses for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quantity {
    /// Energy accumulated over the interval. `units: "KWH"`.
    EnergyKwh,
    /// Instantaneous or interval-average power. `units: "KW"`.
    PowerKw,
    /// A percentage, 0-100. `units: "PERCENT"`.
    Percent,
    /// A price per kWh in EUR. `units: "KWH"` (the denominator) plus `currency: "EUR"`.
    PricePerKwhEur,
    /// Emission intensity in grams CO2e per kWh. `units: "GHG"`.
    EmissionGPerKwh,
    /// A label, not a measurement: no unit applies.
    State,
    /// A number with no unit, e.g. `SIMPLE`'s 0-3 shed level.
    Dimensionless,
}

impl Quantity {
    /// The `units` string this quantity declares on the wire, if any.
    pub fn units(self) -> Option<&'static str> {
        match self {
            Quantity::EnergyKwh | Quantity::PricePerKwhEur => Some("KWH"),
            Quantity::PowerKw => Some("KW"),
            Quantity::Percent => Some("PERCENT"),
            Quantity::EmissionGPerKwh => Some("GHG"),
            Quantity::State | Quantity::Dimensionless => None,
        }
    }

    /// The `currency` this quantity declares on the wire, if it is a price.
    pub fn currency(self) -> Option<&'static str> {
        matches!(self, Quantity::PricePerKwhEur).then_some("EUR")
    }
}

/// What each report payload type this lab emits means.
///
/// `None` is not "dimensionless": it is "this lab has not decided", which is why an unknown type
/// is reported rather than quietly given a unit.
pub fn report_quantity_of(payload_type: &str) -> Option<Quantity> {
    Some(match payload_type {
        // Spec: "Energy usage over an interval."
        "USAGE" | "USAGE_FORECAST" | "BASELINE" | "DELTA_USAGE" => Quantity::EnergyKwh,
        // Spec: "Power usage for an interval, i.e. Real Power." The fleet series: instantaneous
        // site power, signed, not accumulated energy.
        "DEMAND" => Quantity::PowerKw,
        // Spec: additional import/export capacity requested, a power.
        "IMPORT_RESERVATION_CAPACITY" | "EXPORT_RESERVATION_CAPACITY" => Quantity::PowerKw,
        // A battery's charge/discharge limit is a power, not a consumption.
        "STORAGE_MAX_CHARGE_POWER" | "STORAGE_MAX_DISCHARGE_POWER" => Quantity::PowerKw,
        "STORAGE_CHARGE_LEVEL" => Quantity::Percent,
        "OPERATING_STATE" | "DATA_QUALITY" => Quantity::State,
        "SIMPLE" => Quantity::Dimensionless,
        _ => return None,
    })
}

/// What each event payload type this lab reads means (`WIRE_PROFILE.md`, v1 and v2).
///
/// `None` = the profile does not define the type: its value is passed through as written and
/// is neither an assumption nor a refusal, because nothing here acts on it.
pub fn event_quantity_of(payload_type: &str) -> Option<Quantity> {
    Some(match payload_type {
        "SIMPLE" => Quantity::Dimensionless,
        "IMPORT_CAPACITY_LIMIT" | "EXPORT_CAPACITY_LIMIT" => Quantity::PowerKw,
        "CHARGE_STATE_SETPOINT" => Quantity::Percent,
        "PRICE" | "EXPORT_PRICE" => Quantity::PricePerKwhEur,
        "GHG" => Quantity::EmissionGPerKwh,
        // Profile v2: the types the VEN read without the profile ever deciding their unit.
        "DISPATCH_SETPOINT" => Quantity::PowerKw,
        "IMPORT_CAPACITY_SUBSCRIPTION"
        | "EXPORT_CAPACITY_SUBSCRIPTION"
        | "IMPORT_CAPACITY_RESERVATION"
        | "EXPORT_CAPACITY_RESERVATION" => Quantity::PowerKw,
        _ => return None,
    })
}

/// Who said what unit a value is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclaredBy {
    /// The event's own `payloadDescriptors`.
    Event,
    /// The program's `payloadDescriptors`.
    Program,
    /// Nobody: the profile default was applied. Legal for a conformant peer, and surfaced.
    ProfileDefault,
    /// The quantity has no unit to declare (`SIMPLE`), or the profile does not define the type.
    NothingToDeclare,
}

/// An incoming value in the lab's unit for its quantity, with who declared that unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    pub value: f64,
    pub declared_by: DeclaredBy,
}

/// A payload whose declared unit or currency this lab cannot honour. The payload is not used;
/// its event and the rest of the poll are unaffected.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Refusal {
    pub event_id: String,
    pub payload_type: String,
    /// What the peer declared, as written on the wire (e.g. `units VOLTS`, `currency USD`).
    pub declared: String,
    /// What the profile reads this type as (e.g. `units KW`).
    pub expected: String,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "event {} {} declares {}, this profile reads {}",
            self.event_id, self.payload_type, self.declared, self.expected
        )
    }
}

/// What one poll's events declared, assumed and had refused: the input of the health surface.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WireAudit {
    /// Payload type -> how many payloads were read from the profile default.
    pub assumed: BTreeMap<String, usize>,
    /// One entry per event and payload type whose declaration could not be honoured.
    pub refusals: Vec<Refusal>,
}

/// Reads incoming event values through their declarations: the event's descriptors, else the
/// program's, else the profile default. `Default` has no program descriptors, so it resolves
/// from the event or the default (what a test fixture without programs wants).
#[derive(Debug, Clone, Default)]
pub struct PayloadReader {
    program_descriptors: HashMap<String, Vec<EventPayloadDescriptor>>,
}

/// The event payload descriptors in a program's `payloadDescriptors` list.
///
/// The list mixes event and report descriptors, told apart by `objectType`, which is optional on
/// the wire while the wire crate's enum requires it. The event ones are kept; a report
/// descriptor, or an entry that is not a valid event descriptor, is skipped rather than failing
/// the whole program: a peer is not refused for a descriptor this reader has no use for.
pub fn program_event_descriptors(entries: Vec<serde_json::Value>) -> Vec<EventPayloadDescriptor> {
    use openleadr_wire::program::PayloadDescriptor;
    entries
        .into_iter()
        .filter_map(|entry| match entry.get("objectType").is_some() {
            true => match serde_json::from_value(entry).ok()? {
                PayloadDescriptor::EventPayloadDescriptor(d) => Some(d),
                PayloadDescriptor::ReportPayloadDescriptor(_) => None,
            },
            false => serde_json::from_value(entry).ok(),
        })
        .collect()
}

impl PayloadReader {
    /// A reader from program objects as the VTN lists them (`id`, `payloadDescriptors`), for a
    /// caller that holds the raw rows. A row without an id contributes nothing.
    pub fn from_program_rows(rows: &[serde_json::Value]) -> Self {
        Self::with_programs(rows.iter().filter_map(|row| {
            let id = row.get("id")?.as_str()?.to_string();
            let entries = row.get("payloadDescriptors")?.as_array()?.clone();
            Some((id, program_event_descriptors(entries)))
        }))
    }

    /// A reader that also knows each program's event payload descriptors, by program id.
    pub fn with_programs(
        programs: impl IntoIterator<Item = (String, Vec<EventPayloadDescriptor>)>,
    ) -> Self {
        Self {
            program_descriptors: programs.into_iter().collect(),
        }
    }

    /// `payload`'s first value in the lab's unit, with who declared it; `Err` when the
    /// declaration cannot be honoured; `None` when the payload carries no number.
    pub fn read(
        &self,
        event: &OadrEvent,
        payload: &EventValuesMap,
    ) -> Option<Result<Reading, Refusal>> {
        let value = payload.values.first().and_then(numeric_value)?;
        let payload_type = wire_name(&payload.value_type);
        let reading = |declared_by| Some(Ok(Reading { value, declared_by }));
        let Some(quantity) = event_quantity_of(&payload_type) else {
            return reading(DeclaredBy::NothingToDeclare);
        };

        let event_desc = find_descriptor(event.content.payload_descriptors.as_deref(), payload);
        let program_desc = self
            .program_descriptors
            .get(event.content.program_id.as_str())
            .and_then(|d| find_descriptor(Some(d), payload));
        for (descriptor, declared_by) in [
            (event_desc, DeclaredBy::Event),
            (program_desc, DeclaredBy::Program),
        ] {
            let Some(descriptor) = descriptor else {
                continue;
            };
            match check(descriptor, quantity) {
                Declaration::Matches => return reading(declared_by),
                // A descriptor that names neither unit nor currency declares nothing.
                Declaration::Silent => continue,
                Declaration::Differs { declared, expected } => {
                    return Some(Err(Refusal {
                        event_id: event.id.to_string(),
                        payload_type,
                        declared,
                        expected,
                    }))
                }
            }
        }
        if quantity.units().is_none() {
            return reading(DeclaredBy::NothingToDeclare);
        }
        reading(DeclaredBy::ProfileDefault)
    }

    /// `payload`'s value when it can be used: a refused payload reads as absent.
    pub fn value(&self, event: &OadrEvent, payload: &EventValuesMap) -> Option<f64> {
        self.read(event, payload)?.ok().map(|r| r.value)
    }

    /// Everything `events` assumed or had refused, each refusal once per event and type.
    pub fn audit(&self, events: &[OadrEvent]) -> WireAudit {
        let mut audit = WireAudit::default();
        for event in events {
            let payloads = event
                .content
                .intervals
                .iter()
                .flatten()
                .flat_map(|i| i.payloads.iter());
            for payload in payloads {
                match self.read(event, payload) {
                    Some(Ok(Reading {
                        declared_by: DeclaredBy::ProfileDefault,
                        ..
                    })) => {
                        *audit
                            .assumed
                            .entry(wire_name(&payload.value_type))
                            .or_default() += 1;
                    }
                    Some(Err(refusal)) => audit.refusals.push(refusal),
                    _ => {}
                }
            }
        }
        audit.refusals.sort();
        audit.refusals.dedup();
        audit
    }
}

fn find_descriptor<'a>(
    descriptors: Option<&'a [EventPayloadDescriptor]>,
    payload: &EventValuesMap,
) -> Option<&'a EventPayloadDescriptor> {
    descriptors?
        .iter()
        .find(|d| d.payload_type == payload.value_type)
}

enum Declaration {
    Matches,
    Silent,
    Differs { declared: String, expected: String },
}

/// Does `descriptor` say what the profile reads `quantity` as?
fn check(descriptor: &EventPayloadDescriptor, quantity: Quantity) -> Declaration {
    let units = descriptor.units.as_ref().map(wire_name);
    let currency = descriptor.currency.as_ref().map(wire_name);
    if units.is_none() && currency.is_none() {
        return Declaration::Silent;
    }
    let differs = |declared: &Option<String>, expected: Option<&str>| {
        declared.as_deref().is_some_and(|d| Some(d) != expected)
    };
    if differs(&units, quantity.units()) || differs(&currency, quantity.currency()) {
        let side = |unit: Option<&str>, currency: Option<&str>| {
            [unit.map(|u| format!("units {u}")), currency.map(|c| format!("currency {c}"))]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ")
        };
        let expected = match side(quantity.units(), quantity.currency()) {
            s if s.is_empty() => "no unit".to_string(),
            s => s,
        };
        return Declaration::Differs {
            declared: side(units.as_deref(), currency.as_deref()),
            expected,
        };
    }
    Declaration::Matches
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::events_from_json;
    use serde_json::json;

    /// One event with one payload of `payload_type` = `value`, and the given event descriptors.
    fn event(payload_type: &str, value: serde_json::Value, descriptors: serde_json::Value) -> OadrEvent {
        let mut ev = json!({
            "id": "evt-a",
            "programID": "prog-1",
            "intervals": [{ "payloads": [{ "type": payload_type, "values": [value] }] }],
        });
        if !descriptors.is_null() {
            ev["payloadDescriptors"] = descriptors;
        }
        events_from_json(ev).remove(0)
    }

    fn read(reader: &PayloadReader, ev: &OadrEvent) -> Option<Result<Reading, Refusal>> {
        reader.read(ev, &ev.content.intervals.as_ref().unwrap()[0].payloads[0])
    }

    fn descriptors(value: serde_json::Value) -> Vec<EventPayloadDescriptor> {
        serde_json::from_value(value).unwrap()
    }

    fn program_reader(value: serde_json::Value) -> PayloadReader {
        PayloadReader::with_programs([("prog-1".to_string(), descriptors(value))])
    }

    #[test]
    fn the_events_own_declaration_is_used() {
        let ev = event(
            "IMPORT_CAPACITY_LIMIT",
            json!(4.5),
            json!([{ "payloadType": "IMPORT_CAPACITY_LIMIT", "units": "KW" }]),
        );
        assert_eq!(
            read(&PayloadReader::default(), &ev),
            Some(Ok(Reading { value: 4.5, declared_by: DeclaredBy::Event }))
        );
    }

    #[test]
    fn the_programs_declaration_is_used_when_the_event_is_silent() {
        let ev = event("IMPORT_CAPACITY_LIMIT", json!(4.5), json!(null));
        let reader = program_reader(json!([{ "payloadType": "IMPORT_CAPACITY_LIMIT", "units": "KW" }]));
        assert_eq!(read(&reader, &ev).unwrap().unwrap().declared_by, DeclaredBy::Program);
    }

    #[test]
    fn the_events_declaration_wins_over_the_programs() {
        let ev = event(
            "IMPORT_CAPACITY_LIMIT",
            json!(4.5),
            json!([{ "payloadType": "IMPORT_CAPACITY_LIMIT", "units": "VOLTS" }]),
        );
        let reader = program_reader(json!([{ "payloadType": "IMPORT_CAPACITY_LIMIT", "units": "KW" }]));
        assert!(read(&reader, &ev).unwrap().is_err(), "the event said VOLTS; the program cannot rescue it");
    }

    #[test]
    fn an_undeclared_value_is_read_from_the_profile_default_and_says_so() {
        let ev = event("PRICE", json!(0.21), json!(null));
        assert_eq!(
            read(&PayloadReader::default(), &ev),
            Some(Ok(Reading { value: 0.21, declared_by: DeclaredBy::ProfileDefault }))
        );
    }

    #[test]
    fn a_descriptor_for_another_type_or_without_units_declares_nothing() {
        let other = event("PRICE", json!(0.21), json!([{ "payloadType": "GHG", "units": "GHG" }]));
        assert_eq!(read(&PayloadReader::default(), &other).unwrap().unwrap().declared_by, DeclaredBy::ProfileDefault);
        let bare = event("PRICE", json!(0.21), json!([{ "payloadType": "PRICE" }]));
        assert_eq!(read(&PayloadReader::default(), &bare).unwrap().unwrap().declared_by, DeclaredBy::ProfileDefault);
    }

    #[test]
    fn a_unit_the_profile_does_not_read_is_refused_and_named() {
        let ev = event(
            "IMPORT_CAPACITY_LIMIT",
            json!(230.0),
            json!([{ "payloadType": "IMPORT_CAPACITY_LIMIT", "units": "VOLTS" }]),
        );
        let refusal = read(&PayloadReader::default(), &ev).unwrap().unwrap_err();
        assert_eq!(
            refusal,
            Refusal {
                event_id: "evt-a".into(),
                payload_type: "IMPORT_CAPACITY_LIMIT".into(),
                declared: "units VOLTS".into(),
                expected: "units KW".into(),
            }
        );
        assert_eq!(PayloadReader::default().value(&ev, &ev.content.intervals.as_ref().unwrap()[0].payloads[0]), None);
    }

    #[test]
    fn a_price_in_another_currency_is_refused() {
        let ev = event(
            "PRICE",
            json!(0.21),
            json!([{ "payloadType": "PRICE", "units": "KWH", "currency": "USD" }]),
        );
        let refusal = read(&PayloadReader::default(), &ev).unwrap().unwrap_err();
        assert_eq!(refusal.declared, "units KWH currency USD");
        assert_eq!(refusal.expected, "units KWH currency EUR");
    }

    #[test]
    fn a_price_declaring_its_unit_and_currency_is_read() {
        let ev = event(
            "PRICE",
            json!(0.21),
            json!([{ "payloadType": "PRICE", "units": "KWH", "currency": "EUR" }]),
        );
        assert_eq!(read(&PayloadReader::default(), &ev).unwrap().unwrap().declared_by, DeclaredBy::Event);
    }

    #[test]
    fn a_private_unit_is_refused() {
        let ev = event(
            "IMPORT_CAPACITY_LIMIT",
            json!(4500.0),
            json!([{ "payloadType": "IMPORT_CAPACITY_LIMIT", "units": "W" }]),
        );
        assert_eq!(read(&PayloadReader::default(), &ev).unwrap().unwrap_err().declared, "units W");
    }

    /// No magnitude guessing: 0.8 declared PERCENT is 0.8 percent.
    #[test]
    fn a_value_is_never_rescaled_by_its_size() {
        let ev = event(
            "CHARGE_STATE_SETPOINT",
            json!(0.8),
            json!([{ "payloadType": "CHARGE_STATE_SETPOINT", "units": "PERCENT" }]),
        );
        assert_eq!(read(&PayloadReader::default(), &ev).unwrap().unwrap().value, 0.8);
    }

    #[test]
    fn simple_has_nothing_to_declare_and_an_integer_level_is_a_number() {
        let ev = event("SIMPLE", json!(2), json!(null));
        assert_eq!(
            read(&PayloadReader::default(), &ev),
            Some(Ok(Reading { value: 2.0, declared_by: DeclaredBy::NothingToDeclare }))
        );
        let with_unit = event("SIMPLE", json!(2), json!([{ "payloadType": "SIMPLE", "units": "KW" }]));
        assert_eq!(read(&PayloadReader::default(), &with_unit).unwrap().unwrap_err().expected, "no unit");
    }

    #[test]
    fn a_type_the_profile_does_not_define_passes_through_and_text_is_not_a_number() {
        let private = event("MY_PRIVATE_SIGNAL", json!(7.0), json!(null));
        assert_eq!(read(&PayloadReader::default(), &private).unwrap().unwrap().declared_by, DeclaredBy::NothingToDeclare);
        let text = event("ALERT_GRID_EMERGENCY", json!("grid emergency"), json!(null));
        assert_eq!(read(&PayloadReader::default(), &text), None);
    }

    #[test]
    fn audit_counts_assumptions_per_type_and_lists_each_refusal_once() {
        let events = events_from_json(json!([
            {
                "id": "evt-undeclared",
                "intervals": [
                    { "payloads": [{ "type": "PRICE", "values": [0.2] }, { "type": "GHG", "values": [300.0] }] },
                    { "payloads": [{ "type": "PRICE", "values": [0.3] }] },
                ],
            },
            {
                "id": "evt-volts",
                "payloadDescriptors": [{ "payloadType": "IMPORT_CAPACITY_LIMIT", "units": "VOLTS" }],
                "intervals": [
                    { "payloads": [{ "type": "IMPORT_CAPACITY_LIMIT", "values": [230.0] }] },
                    { "payloads": [{ "type": "IMPORT_CAPACITY_LIMIT", "values": [231.0] }] },
                ],
            },
            {
                "id": "evt-declared",
                "payloadDescriptors": [{ "payloadType": "IMPORT_CAPACITY_LIMIT", "units": "KW" }],
                "intervals": [{ "payloads": [{ "type": "IMPORT_CAPACITY_LIMIT", "values": [5.0] }, { "type": "SIMPLE", "values": [1] }] }],
            },
        ]));
        let audit = PayloadReader::default().audit(&events);
        assert_eq!(audit.assumed, BTreeMap::from([("GHG".to_string(), 1), ("PRICE".to_string(), 2)]));
        assert_eq!(audit.refusals.len(), 1, "two intervals of one event and type are one refusal");
        assert_eq!(audit.refusals[0].event_id, "evt-volts");
    }

    #[test]
    fn a_reader_built_from_program_rows_reads_their_event_descriptors() {
        let reader = PayloadReader::from_program_rows(&[
            json!({ "id": "prog-1", "payloadDescriptors": [
                { "objectType": "REPORT_PAYLOAD_DESCRIPTOR", "payloadType": "USAGE", "units": "KWH" },
                { "objectType": "EVENT_PAYLOAD_DESCRIPTOR", "payloadType": "IMPORT_CAPACITY_LIMIT", "units": "KW" },
                { "units": "KW" },
            ]}),
            json!({ "programName": "no id" }),
            json!({ "id": "prog-2" }),
        ]);
        let ev = event("IMPORT_CAPACITY_LIMIT", json!(4.5), json!(null));
        assert_eq!(read(&reader, &ev).unwrap().unwrap().declared_by, DeclaredBy::Program);
    }

    /// The Rust table is the same table `docs/reference/WIRE_PROFILE.md` publishes.
    #[test]
    fn the_table_matches_the_published_profile() {
        let doc = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../docs/reference/WIRE_PROFILE.md"
        ))
        .expect("WIRE_PROFILE.md is readable from lab-core");
        let mut seen = 0;
        for line in doc.lines().filter(|l| l.starts_with("| `")) {
            let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
            let units = cells[2].split('`').nth(1);
            let currency = cells[2].split("currency: ").nth(1).map(|c| c.trim_end_matches('`'));
            for payload_type in cells[0].split('`').filter(|s| s.chars().all(|c| c.is_ascii_uppercase() || c == '_') && !s.is_empty()) {
                let quantity = event_quantity_of(payload_type)
                    .or_else(|| report_quantity_of(payload_type))
                    .unwrap_or_else(|| panic!("{payload_type} is in WIRE_PROFILE.md but not in the table"));
                assert_eq!(quantity.units(), units, "{payload_type} units");
                if quantity.currency().is_some() {
                    assert_eq!(quantity.currency(), currency, "{payload_type} currency");
                }
                seen += 1;
            }
        }
        assert!(seen >= 20, "expected the event and report tables, parsed {seen} types");
    }
}
