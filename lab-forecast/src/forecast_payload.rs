//! The payload every forecast topic carries, and the topics themselves
//! (`openspec/changes/lab-forecaster`, spec `forecast-publication`).
//!
//! Owns: the JSON object of one forecast series (contract declared on the wire: quantity, unit, sign,
//! reading), the status object, the topic names, and the one validator both producer and consumer use.
//! Reads: bytes or typed values handed in. Must never: read a clock (callers pass `now`), open a socket,
//! or let a payload exist without its declaration: the enums below have no "unknown" member and the
//! constructors validate, so an undeclared payload cannot be built.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Major version 1 is what this crate reads and writes.
pub const SCHEMA_VERSION: &str = "1.0.0";
/// Default freshness limit of a series: `valid_until - generated_at` (a setting of the producer).
pub const DEFAULT_FRESHNESS_MINUTES: i64 = 90;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quantity {
    Power,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Unit {
    #[serde(rename = "kW")]
    Kw,
}

/// Which way a positive value points: PV generation, or base-load consumption.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sign {
    GenerationPositive,
    ConsumptionPositive,
}

/// What one value means over its step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reading {
    AverageOverStep,
}

/// PV only: the series is the natural (uncurtailed) generation, the exogenous half.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Curtailment {
    Natural,
}

/// Why a payload was refused or could not be built. One line, no remediation text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PayloadError {
    /// Not JSON, a field missing (the message names it), or a value of the wrong type.
    Parse(String),
    /// A `schema_version` whose major is not the one this crate speaks.
    UnsupportedSchema(String),
    /// Well-formed but breaks a rule of the contract.
    Invalid(String),
}

impl std::fmt::Display for PayloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PayloadError::Parse(m) => write!(f, "parse error: {m}"),
            PayloadError::UnsupportedSchema(v) => write!(f, "unsupported schema_version: {v}"),
            PayloadError::Invalid(m) => write!(f, "invalid forecast: {m}"),
        }
    }
}

impl std::error::Error for PayloadError {}

/// Where a series starts and how long one value lasts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeriesGrid {
    /// Start of the first step.
    pub start: DateTime<Utc>,
    pub step_s: u32,
}

/// What a series is, which fixes what it declares on the wire.
#[derive(Debug, Clone, Copy)]
enum SeriesKind {
    PvNatural,
    BaseLoad,
}

impl SeriesKind {
    /// The sign and the curtailment a series of this kind declares.
    fn declaration(self) -> (Sign, Option<Curtailment>) {
        match self {
            SeriesKind::PvNatural => (Sign::GenerationPositive, Some(Curtailment::Natural)),
            SeriesKind::BaseLoad => (Sign::ConsumptionPositive, None),
        }
    }
}

/// What every forecast carries besides its values.
#[derive(Debug, Clone, PartialEq)]
pub struct PayloadMeta {
    pub generated_at: DateTime<Utc>,
    pub valid_until: DateTime<Utc>,
    pub model_version: String,
    pub producer_id: String,
    pub confidence: f64,
}

/// One forecast series on the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForecastPayload {
    pub schema_version: String,
    pub generated_at: DateTime<Utc>,
    pub valid_until: DateTime<Utc>,
    pub model_version: String,
    pub producer_id: String,
    /// Start of the first step.
    pub start: DateTime<Utc>,
    pub step_s: u32,
    pub values: Vec<f64>,
    pub confidence: f64,
    /// The horizon actually delivered, in hours: `values.len() * step_s / 3600`.
    pub coverage_h: f64,
    pub quantity: Quantity,
    pub unit: Unit,
    pub sign: Sign,
    pub reading: Reading,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curtailment: Option<Curtailment>,
}

impl ForecastPayload {
    /// A PV series: natural generation, positive for generation. `values_kw` is average kW per step.
    pub fn pv_natural(
        meta: PayloadMeta,
        grid: SeriesGrid,
        values_kw: Vec<f64>,
    ) -> Result<Self, PayloadError> {
        Self::build(SeriesKind::PvNatural, meta, grid, values_kw)
    }

    /// A base-load series: positive for consumption. `values_kw` is average kW per step.
    pub fn base_load(
        meta: PayloadMeta,
        grid: SeriesGrid,
        values_kw: Vec<f64>,
    ) -> Result<Self, PayloadError> {
        Self::build(SeriesKind::BaseLoad, meta, grid, values_kw)
    }

    /// The one place a payload is assembled; it validates, so an invalid or undeclared one cannot exist.
    fn build(
        kind: SeriesKind,
        meta: PayloadMeta,
        grid: SeriesGrid,
        values: Vec<f64>,
    ) -> Result<Self, PayloadError> {
        let (sign, curtailment) = kind.declaration();
        let payload = Self {
            schema_version: SCHEMA_VERSION.to_string(),
            generated_at: meta.generated_at,
            valid_until: meta.valid_until,
            model_version: meta.model_version,
            producer_id: meta.producer_id,
            start: grid.start,
            step_s: grid.step_s,
            coverage_h: horizon_h(values.len(), grid.step_s),
            values,
            confidence: meta.confidence,
            quantity: Quantity::Power,
            unit: Unit::Kw,
            sign,
            reading: Reading::AverageOverStep,
            curtailment,
        };
        payload.validate()?;
        Ok(payload)
    }

    /// Refuse a payload that breaks the contract. Producer and consumer both call this.
    pub fn validate(&self) -> Result<(), PayloadError> {
        check_major(&self.schema_version)?;
        let invalid = |m: &str| Err(PayloadError::Invalid(m.to_string()));
        if self.step_s == 0 {
            return invalid("step_s must be positive");
        }
        if self.values.is_empty() {
            return invalid("values is empty");
        }
        if self.values.iter().any(|v| !v.is_finite()) {
            return invalid("values holds a non-finite number");
        }
        if self.valid_until <= self.generated_at {
            return invalid("valid_until must be after generated_at");
        }
        if !(0.0..=1.0).contains(&self.confidence) {
            return invalid("confidence must be within 0..1");
        }
        if (self.coverage_h - horizon_h(self.values.len(), self.step_s)).abs() > 1e-6 {
            return invalid("coverage_h does not match values and step_s");
        }
        match (self.sign, self.curtailment) {
            (Sign::GenerationPositive, Some(Curtailment::Natural)) => {
                if self.values.iter().any(|v| *v < 0.0) {
                    return invalid("a generation_positive series holds a negative value");
                }
                Ok(())
            }
            (Sign::GenerationPositive, None) => {
                invalid("a generation_positive series must declare curtailment natural")
            }
            (Sign::ConsumptionPositive, None) => Ok(()),
            (Sign::ConsumptionPositive, Some(_)) => {
                invalid("a consumption_positive series has no curtailment")
            }
        }
    }

    /// Parse and validate bytes from a topic. Unknown fields are ignored (additive evolution).
    pub fn parse(payload: &[u8]) -> Result<Self, PayloadError> {
        let value = parse_json_checking_major(payload)?;
        let parsed: Self =
            serde_json::from_value(value).map_err(|e| PayloadError::Parse(e.to_string()))?;
        parsed.validate()?;
        Ok(parsed)
    }

    /// Fresh until `valid_until`; the caller supplies `now`.
    pub fn is_fresh(&self, now: DateTime<Utc>) -> bool {
        now <= self.valid_until
    }

    /// The series' freshness limit as the consumer's catalogue states it: `valid_until - generated_at`.
    pub fn freshness_limit(&self) -> Duration {
        self.valid_until - self.generated_at
    }
}

/// `values.len() * step_s` in hours.
fn horizon_h(len: usize, step_s: u32) -> f64 {
    len as f64 * f64::from(step_s) / 3600.0
}

/// Only major version 1 is spoken; an absent version is a parse problem, not a version problem.
fn check_major(version: &str) -> Result<(), PayloadError> {
    if version.is_empty() {
        return Err(PayloadError::Parse(
            "missing field `schema_version`".to_string(),
        ));
    }
    match version.split('.').next() {
        Some("1") => Ok(()),
        _ => Err(PayloadError::UnsupportedSchema(version.to_string())),
    }
}

/// JSON first, then the version, before any field is read: a future major may have a shape this crate
/// cannot read, and it must be reported as a version problem.
fn parse_json_checking_major(payload: &[u8]) -> Result<serde_json::Value, PayloadError> {
    let value: serde_json::Value =
        serde_json::from_slice(payload).map_err(|e| PayloadError::Parse(e.to_string()))?;
    check_major(
        value
            .get("schema_version")
            .and_then(|v| v.as_str())
            .unwrap_or_default(),
    )?;
    Ok(value)
}

/// The topic names of one site under one root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastTopics {
    root: String,
    site: String,
}

impl ForecastTopics {
    pub fn new(root: &str, site: &str) -> Result<Self, PayloadError> {
        check_topic_part(root, true)?;
        check_topic_part(site, false)?;
        Ok(Self {
            root: root.to_string(),
            site: site.to_string(),
        })
    }

    /// The topic of one PV array; `total` is reserved for the site sum.
    pub fn pv_array(&self, array_id: &str) -> Result<String, PayloadError> {
        check_topic_part(array_id, false)?;
        if array_id == PV_TOTAL_ID {
            return Err(PayloadError::Invalid(format!(
                "'{PV_TOTAL_ID}' is the site sum, not an array id"
            )));
        }
        Ok(format!(
            "{}/forecast/{}/pv/{array_id}/power_kw",
            self.root, self.site
        ))
    }

    pub fn pv_total(&self) -> String {
        format!(
            "{}/forecast/{}/pv/{PV_TOTAL_ID}/power_kw",
            self.root, self.site
        )
    }

    pub fn base_load(&self) -> String {
        format!("{}/forecast/{}/base_load/power_kw", self.root, self.site)
    }

    pub fn status(&self) -> String {
        format!("{}/forecast/{}/status", self.root, self.site)
    }
}

const PV_TOTAL_ID: &str = "total";

/// A topic level may not be empty or hold a wildcard; only the root may hold separators.
fn check_topic_part(part: &str, allow_separator: bool) -> Result<(), PayloadError> {
    let bad = part.is_empty()
        || part.contains(['+', '#', '\0'])
        || (!allow_separator && part.contains('/'))
        || part.starts_with('/')
        || part.ends_with('/');
    if bad {
        return Err(PayloadError::Invalid(format!(
            "'{part}' is not usable in a topic name"
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForecastState {
    Ok,
    ColdStart,
    WeatherStale,
    Degraded,
    Offline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnowState {
    Observed,
    Unobserved,
}

/// The retained status of a producer (also its last will, with state `offline`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForecastStatus {
    pub schema_version: String,
    pub producer_id: String,
    pub generated_at: DateTime<Utc>,
    pub state: ForecastState,
    /// Age in seconds of each input; `None` when it never arrived.
    pub input_age_s: BTreeMap<String, Option<u64>>,
    /// Per PV array: whether the snow state is observed from a measurement.
    pub snow_state: BTreeMap<String, SnowState>,
}

impl ForecastStatus {
    /// The last will: this producer is gone.
    pub fn offline(producer_id: &str, now: DateTime<Utc>) -> Self {
        Self {
            schema_version: SCHEMA_VERSION.to_string(),
            producer_id: producer_id.to_string(),
            generated_at: now,
            state: ForecastState::Offline,
            input_age_s: BTreeMap::new(),
            snow_state: BTreeMap::new(),
        }
    }

    pub fn parse(payload: &[u8]) -> Result<Self, PayloadError> {
        let value = parse_json_checking_major(payload)?;
        serde_json::from_value(value).map_err(|e| PayloadError::Parse(e.to_string()))
    }

    /// Whether another producer's status still counts as live: not offline and not older than `limit`.
    pub fn is_live(&self, now: DateTime<Utc>, limit: Duration) -> bool {
        self.state != ForecastState::Offline && now - self.generated_at <= limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 4, h, m, 0).unwrap()
    }

    fn meta() -> PayloadMeta {
        PayloadMeta {
            generated_at: t(10, 0),
            valid_until: t(11, 30),
            model_version: "pv-weather-1".into(),
            producer_id: "forecaster-a".into(),
            confidence: 0.8,
        }
    }

    fn pv() -> ForecastPayload {
        ForecastPayload::pv_natural(
            meta(),
            SeriesGrid {
                start: t(10, 0),
                step_s: 3600,
            },
            vec![0.0, 1.5, 3.0, 2.0],
        )
        .unwrap()
    }

    #[test]
    fn pv_natural_declares_generation_positive_natural_kw() {
        let p = pv();
        assert_eq!(p.quantity, Quantity::Power);
        assert_eq!(p.unit, Unit::Kw);
        assert_eq!(p.sign, Sign::GenerationPositive);
        assert_eq!(p.reading, Reading::AverageOverStep);
        assert_eq!(p.curtailment, Some(Curtailment::Natural));
        assert_eq!(p.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn base_load_declares_consumption_positive_without_curtailment() {
        let p = ForecastPayload::base_load(
            meta(),
            SeriesGrid {
                start: t(10, 0),
                step_s: 900,
            },
            vec![0.4; 8],
        )
        .unwrap();
        assert_eq!(p.sign, Sign::ConsumptionPositive);
        assert_eq!(p.curtailment, None);
    }

    #[test]
    fn coverage_h_is_the_delivered_horizon() {
        let p = pv();
        assert!((p.coverage_h - 4.0).abs() < 1e-9);
        let q = ForecastPayload::base_load(
            meta(),
            SeriesGrid {
                start: t(10, 0),
                step_s: 900,
            },
            vec![0.4; 8],
        )
        .unwrap();
        assert!((q.coverage_h - 2.0).abs() < 1e-9);
    }

    #[test]
    fn serialised_payload_names_every_declaration() {
        let v: serde_json::Value =
            serde_json::from_slice(&serde_json::to_vec(&pv()).unwrap()).unwrap();
        assert_eq!(v["quantity"], "power");
        assert_eq!(v["unit"], "kW");
        assert_eq!(v["sign"], "generation_positive");
        assert_eq!(v["reading"], "average_over_step");
        assert_eq!(v["curtailment"], "natural");
        for k in [
            "schema_version",
            "generated_at",
            "valid_until",
            "model_version",
            "producer_id",
            "start",
            "step_s",
            "values",
            "confidence",
            "coverage_h",
        ] {
            assert!(v.get(k).is_some(), "missing {k}");
        }
    }

    #[test]
    fn base_load_payload_omits_curtailment() {
        let p = ForecastPayload::base_load(
            meta(),
            SeriesGrid {
                start: t(10, 0),
                step_s: 900,
            },
            vec![0.4; 4],
        )
        .unwrap();
        let v: serde_json::Value =
            serde_json::from_slice(&serde_json::to_vec(&p).unwrap()).unwrap();
        assert!(v.get("curtailment").is_none());
    }

    #[test]
    fn parse_round_trips_a_built_payload() {
        let p = pv();
        let back = ForecastPayload::parse(&serde_json::to_vec(&p).unwrap()).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn parse_names_a_missing_declaration() {
        for field in ["unit", "sign", "quantity", "reading"] {
            let mut v: serde_json::Value = serde_json::to_value(pv()).unwrap();
            v.as_object_mut().unwrap().remove(field);
            let err = ForecastPayload::parse(&serde_json::to_vec(&v).unwrap()).unwrap_err();
            assert!(
                matches!(&err, PayloadError::Parse(m) if m.contains(field)),
                "{field}: {err}"
            );
        }
    }

    #[test]
    fn parse_rejects_a_non_numeric_value() {
        let mut v: serde_json::Value = serde_json::to_value(pv()).unwrap();
        v["values"][1] = serde_json::json!("sunny");
        assert!(matches!(
            ForecastPayload::parse(&serde_json::to_vec(&v).unwrap()),
            Err(PayloadError::Parse(_))
        ));
        v["values"][1] = serde_json::Value::Null;
        assert!(matches!(
            ForecastPayload::parse(&serde_json::to_vec(&v).unwrap()),
            Err(PayloadError::Parse(_))
        ));
    }

    #[test]
    fn parse_rejects_an_unsupported_major_version() {
        let mut v: serde_json::Value = serde_json::to_value(pv()).unwrap();
        v["schema_version"] = "2.0.0".into();
        assert_eq!(
            ForecastPayload::parse(&serde_json::to_vec(&v).unwrap()),
            Err(PayloadError::UnsupportedSchema("2.0.0".into()))
        );
    }

    #[test]
    fn parse_accepts_a_newer_minor_version_and_unknown_fields() {
        let mut v: serde_json::Value = serde_json::to_value(pv()).unwrap();
        v["schema_version"] = "1.3.0".into();
        v["some_future_field"] = "x".into();
        assert!(ForecastPayload::parse(&serde_json::to_vec(&v).unwrap()).is_ok());
    }

    #[test]
    fn parse_rejects_a_pv_payload_that_is_not_natural_generation() {
        let mut v: serde_json::Value = serde_json::to_value(pv()).unwrap();
        v.as_object_mut().unwrap().remove("curtailment");
        assert!(matches!(
            ForecastPayload::parse(&serde_json::to_vec(&v).unwrap()),
            Err(PayloadError::Invalid(_))
        ));
    }

    #[test]
    fn parse_rejects_a_base_load_payload_that_claims_curtailment() {
        let mut v: serde_json::Value = serde_json::to_value(
            ForecastPayload::base_load(
                meta(),
                SeriesGrid {
                    start: t(10, 0),
                    step_s: 900,
                },
                vec![0.4; 4],
            )
            .unwrap(),
        )
        .unwrap();
        v["curtailment"] = "natural".into();
        assert!(matches!(
            ForecastPayload::parse(&serde_json::to_vec(&v).unwrap()),
            Err(PayloadError::Invalid(_))
        ));
    }

    #[test]
    fn constructors_refuse_broken_series() {
        let m = meta();
        assert!(
            ForecastPayload::pv_natural(
                m.clone(),
                SeriesGrid {
                    start: t(10, 0),
                    step_s: 0
                },
                vec![1.0]
            )
            .is_err(),
            "zero step"
        );
        assert!(
            ForecastPayload::pv_natural(
                m.clone(),
                SeriesGrid {
                    start: t(10, 0),
                    step_s: 3600
                },
                vec![]
            )
            .is_err(),
            "empty"
        );
        assert!(
            ForecastPayload::pv_natural(
                m.clone(),
                SeriesGrid {
                    start: t(10, 0),
                    step_s: 3600
                },
                vec![f64::NAN]
            )
            .is_err(),
            "nan"
        );
        assert!(
            ForecastPayload::pv_natural(
                m.clone(),
                SeriesGrid {
                    start: t(10, 0),
                    step_s: 3600
                },
                vec![-0.5]
            )
            .is_err(),
            "negative pv"
        );
        let mut bad = m.clone();
        bad.valid_until = bad.generated_at;
        assert!(
            ForecastPayload::pv_natural(
                bad,
                SeriesGrid {
                    start: t(10, 0),
                    step_s: 3600
                },
                vec![1.0]
            )
            .is_err(),
            "valid_until not after generated_at"
        );
        let mut bad = m;
        bad.confidence = 1.5;
        assert!(
            ForecastPayload::pv_natural(
                bad,
                SeriesGrid {
                    start: t(10, 0),
                    step_s: 3600
                },
                vec![1.0]
            )
            .is_err(),
            "confidence above 1"
        );
    }

    #[test]
    fn parse_rejects_a_coverage_that_does_not_match_the_values() {
        let mut v: serde_json::Value = serde_json::to_value(pv()).unwrap();
        v["coverage_h"] = serde_json::json!(48.0);
        assert!(matches!(
            ForecastPayload::parse(&serde_json::to_vec(&v).unwrap()),
            Err(PayloadError::Invalid(_))
        ));
    }

    #[test]
    fn is_fresh_until_valid_until_inclusive() {
        let p = pv();
        assert!(p.is_fresh(t(11, 30)));
        assert!(!p.is_fresh(t(11, 31)));
    }

    #[test]
    fn freshness_limit_is_valid_until_minus_generated_at() {
        assert_eq!(pv().freshness_limit(), Duration::minutes(90));
    }

    #[test]
    fn topics_follow_the_documented_layout() {
        let tp = ForecastTopics::new("openadr-lab", "hometown").unwrap();
        assert_eq!(
            tp.pv_array("pv-1").unwrap(),
            "openadr-lab/forecast/hometown/pv/pv-1/power_kw"
        );
        assert_eq!(
            tp.pv_total(),
            "openadr-lab/forecast/hometown/pv/total/power_kw"
        );
        assert_eq!(
            tp.base_load(),
            "openadr-lab/forecast/hometown/base_load/power_kw"
        );
        assert_eq!(tp.status(), "openadr-lab/forecast/hometown/status");
    }

    #[test]
    fn topics_refuse_wildcards_and_separators_in_names() {
        assert!(ForecastTopics::new("openadr-lab", "a/b").is_err());
        assert!(ForecastTopics::new("openadr-lab", "a+").is_err());
        let tp = ForecastTopics::new("openadr-lab", "hometown").unwrap();
        assert!(tp.pv_array("total/x").is_err());
        assert!(tp.pv_array("#").is_err());
        assert!(
            tp.pv_array("total").is_err(),
            "total is the site sum, not an array id"
        );
        assert!(tp.pv_array("").is_err());
    }

    #[test]
    fn status_offline_is_the_last_will() {
        let s = ForecastStatus::offline("forecaster-a", t(10, 0));
        assert_eq!(s.state, ForecastState::Offline);
        assert_eq!(s.producer_id, "forecaster-a");
        assert!(s.input_age_s.is_empty());
    }

    #[test]
    fn status_round_trips_and_uses_snake_case_states() {
        let mut s = ForecastStatus::offline("forecaster-a", t(10, 0));
        s.state = ForecastState::WeatherStale;
        s.input_age_s.insert("weather".into(), Some(7300));
        s.input_age_s.insert("pv_measurement".into(), None);
        s.snow_state.insert("pv-1".into(), SnowState::Unobserved);
        let bytes = serde_json::to_vec(&s).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["state"], "weather_stale");
        assert_eq!(v["snow_state"]["pv-1"], "unobserved");
        assert_eq!(ForecastStatus::parse(&bytes).unwrap(), s);
    }

    #[test]
    fn status_is_live_when_recent_and_not_offline() {
        let mut s = ForecastStatus::offline("forecaster-a", t(10, 0));
        s.state = ForecastState::Ok;
        assert!(s.is_live(t(10, 5), Duration::minutes(10)));
        assert!(!s.is_live(t(10, 15), Duration::minutes(10)), "too old");
        s.state = ForecastState::Offline;
        assert!(!s.is_live(t(10, 5), Duration::minutes(10)), "offline");
    }
}
