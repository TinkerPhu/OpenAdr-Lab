//! What a value in a report payload *means* — the machine-readable half of
//! `docs/reference/WIRE_PROFILE.md` for the report-out direction.
//!
//! GB-50 was four parts of this project disagreeing about the same numbers.
//! Two of those disagreements lived here: the VEN emitted **watts** under
//! `USAGE`, a payload type OpenADR 3.1 defines as *"Energy usage over an
//! interval"* (Definition, payload type table), and it emitted no
//! `payloadDescriptors` at all, so nothing on the wire said which it was.
//! `experiments/kpi.py` then carried a `x duration` compensation for the
//! mismatch — a unit conversion living in the reader rather than the message.
//!
//! Two rules make that unrepeatable:
//!
//! * A payload is built by naming its quantity — [`OadrReportPayload::energy_kwh`],
//!   [`power_kw`](OadrReportPayload::power_kw), [`percent`](OadrReportPayload::percent),
//!   [`state`](OadrReportPayload::state). There is no constructor that takes a
//!   bare number, so a value whose quantity nobody decided cannot be sent.
//! * [`descriptors_for`] derives the descriptors from the payloads actually
//!   present, so an undeclared payload is not something that can be forgotten.
//!
//! Power is kW and energy is kWh because OpenADR's `Unit` enum has no watt: a
//! value in watts is a value that *cannot be declared*, and under
//! `wire-contracts` anything we cannot declare we do not send.

use tracing::warn;

use crate::controller::vtn_port::{OadrReportInterval, OadrReportPayload};

/// The quantity a report payload type carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quantity {
    /// Energy accumulated over the interval. `units: "KWH"`.
    EnergyKwh,
    /// Instantaneous or interval-average power. `units: "KW"`.
    PowerKw,
    /// A percentage, 0–100. `units: "PERCENT"`.
    Percent,
    /// A label, not a measurement — no unit applies.
    State,
    /// A number with no unit, e.g. `SIMPLE`'s 0–3 shed level.
    Dimensionless,
}

impl Quantity {
    /// The `units` string this quantity declares on the wire, if any.
    fn units(self) -> Option<&'static str> {
        match self {
            Quantity::EnergyKwh => Some("KWH"),
            Quantity::PowerKw => Some("KW"),
            Quantity::Percent => Some("PERCENT"),
            Quantity::State | Quantity::Dimensionless => None,
        }
    }
}

/// What each report payload type this VEN emits means.
///
/// `None` is not "dimensionless" — it is "this lab has not decided", which is
/// why an unknown type is reported rather than quietly given a unit.
pub fn quantity_of(payload_type: &str) -> Option<Quantity> {
    Some(match payload_type {
        // Spec: "Energy usage over an interval."
        "USAGE" | "USAGE_FORECAST" | "BASELINE" | "DELTA_USAGE" => Quantity::EnergyKwh,
        // Spec: additional import/export capacity requested — a power.
        "IMPORT_RESERVATION_CAPACITY" | "EXPORT_RESERVATION_CAPACITY" => Quantity::PowerKw,
        // A battery's charge/discharge limit is a power, not a consumption.
        "STORAGE_MAX_CHARGE_POWER" | "STORAGE_MAX_DISCHARGE_POWER" => Quantity::PowerKw,
        "STORAGE_CHARGE_LEVEL" => Quantity::Percent,
        "OPERATING_STATE" | "DATA_QUALITY" => Quantity::State,
        "SIMPLE" => Quantity::Dimensionless,
        _ => return None,
    })
}

impl OadrReportPayload {
    /// Energy over the interval, in kWh.
    pub fn energy_kwh(payload_type: &str, kwh: f64) -> Self {
        Self::checked(
            payload_type,
            Quantity::EnergyKwh,
            serde_json::Value::from(kwh),
        )
    }

    /// Energy over an interval, from the average power across it.
    ///
    /// The one place kW becomes kWh. Four call sites each did their own
    /// `x 1000.0` into watts instead, which is what `kpi.py` then undid.
    pub fn energy_from_power_kw(payload_type: &str, kw: f64, interval_s: u64) -> Self {
        Self::energy_kwh(payload_type, kw * (interval_s as f64 / 3600.0))
    }

    /// Power, in kW.
    pub fn power_kw(payload_type: &str, kw: f64) -> Self {
        Self::checked(payload_type, Quantity::PowerKw, serde_json::Value::from(kw))
    }

    /// A percentage, 0–100.
    pub fn percent(payload_type: &str, pct: f64) -> Self {
        Self::checked(
            payload_type,
            Quantity::Percent,
            serde_json::Value::from(pct),
        )
    }

    /// A number the spec gives no unit, e.g. `SIMPLE`'s 0–3 shed level.
    pub fn level(payload_type: &str, level: f64) -> Self {
        Self::checked(
            payload_type,
            Quantity::Dimensionless,
            serde_json::Value::from(level),
        )
    }

    /// A label — an operating state, a data-quality flag.
    pub fn state(payload_type: &str, value: impl Into<String>) -> Self {
        Self::checked(
            payload_type,
            Quantity::State,
            serde_json::Value::from(value.into()),
        )
    }

    /// Build the payload, complaining if the caller's quantity disagrees with
    /// the contract. A mismatch is a bug in this crate, not a peer's doing, so
    /// it fails the test suite rather than being silently corrected.
    fn checked(payload_type: &str, q: Quantity, value: serde_json::Value) -> Self {
        // Only a type the contract has an opinion about can be got wrong. For
        // one it has not decided on there is nothing to check against, and
        // `descriptors_for` reports it as undeclared rather than guessing.
        debug_assert!(
            quantity_of(payload_type).is_none_or(|known| known == q),
            "payload type {payload_type} is not a {q:?} — see controller::report_payload"
        );
        Self {
            r#type: payload_type.to_string(),
            values: vec![value],
        }
    }
}

/// Declares every payload type present in `intervals`.
///
/// Derived from the payloads rather than maintained alongside them: a payload
/// that has not been declared is not a thing that can be forgotten, only a
/// thing this lab has not yet decided the meaning of — which is reported.
pub fn descriptors_for(intervals: &[OadrReportInterval]) -> Vec<OadrReportPayloadDescriptor> {
    let mut seen: Vec<&str> = Vec::new();
    let mut out = Vec::new();
    for interval in intervals {
        for payload in &interval.payloads {
            if seen.contains(&payload.r#type.as_str()) {
                continue;
            }
            seen.push(&payload.r#type);
            match quantity_of(&payload.r#type) {
                Some(q) => out.push(OadrReportPayloadDescriptor {
                    payloadType: payload.r#type.clone(),
                    readingType: None,
                    units: q.units().map(str::to_string),
                }),
                None => warn!(
                    payload_type = %payload.r#type,
                    "no declared quantity for this report payload type; \
                     sending it undeclared (see docs/reference/WIRE_PROFILE.md)"
                ),
            }
        }
    }
    out
}

/// Declares the meaning of one payload type carried by a report.
#[allow(non_snake_case)] // wire field names, per the `dto` rule
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OadrReportPayloadDescriptor {
    pub payloadType: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readingType: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interval(payloads: Vec<OadrReportPayload>) -> OadrReportInterval {
        OadrReportInterval {
            id: 0,
            intervalPeriod: None,
            payloads,
        }
    }

    /// The GB-50 rule: every value we send says what it is.
    #[test]
    fn descriptors_are_derived_from_the_payloads_present() {
        let ivs = vec![interval(vec![
            OadrReportPayload::energy_kwh("USAGE", 1.25),
            OadrReportPayload::state("OPERATING_STATE", "normal"),
        ])];
        let d = descriptors_for(&ivs);
        assert_eq!(d.len(), 2);
        let usage = d.iter().find(|x| x.payloadType == "USAGE").unwrap();
        assert_eq!(usage.units.as_deref(), Some("KWH"));
        // A state has no unit; declaring one would be a lie, not a courtesy.
        let op = d
            .iter()
            .find(|x| x.payloadType == "OPERATING_STATE")
            .unwrap();
        assert_eq!(op.units, None);
    }

    #[test]
    fn descriptors_declare_each_payload_type_once() {
        let ivs = vec![
            interval(vec![OadrReportPayload::energy_kwh("USAGE", 1.0)]),
            interval(vec![OadrReportPayload::energy_kwh("USAGE", 2.0)]),
        ];
        assert_eq!(descriptors_for(&ivs).len(), 1);
    }

    #[test]
    fn reservation_capacity_is_power_not_energy() {
        assert_eq!(
            quantity_of("IMPORT_RESERVATION_CAPACITY"),
            Some(Quantity::PowerKw)
        );
        let p = OadrReportPayload::power_kw("IMPORT_RESERVATION_CAPACITY", 4.2);
        assert_eq!(p.values[0].as_f64(), Some(4.2));
    }

    /// The spec defines USAGE as energy over an interval, which is why the VEN
    /// may not send instantaneous watts under it.
    #[test]
    fn usage_is_energy() {
        assert_eq!(quantity_of("USAGE"), Some(Quantity::EnergyKwh));
        assert_eq!(Quantity::EnergyKwh.units(), Some("KWH"));
    }

    #[test]
    fn an_undecided_payload_type_yields_no_descriptor() {
        let ivs = vec![interval(vec![OadrReportPayload {
            r#type: "SOMETHING_NEW".to_string(),
            values: vec![serde_json::Value::from(1.0)],
        }])];
        assert!(descriptors_for(&ivs).is_empty());
    }
}
