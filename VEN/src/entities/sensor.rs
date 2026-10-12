//! A sensor reading as `GET`/`POST /sensors` carry it: the simulator writes one per tick
//! (`SimState::to_sensor_snapshot`), a client may post one, and `AppState` holds the latest.
//! Domain data, so it lives here rather than in `simulator/` (R-131).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct SensorSnapshot {
    pub id: Uuid,
    pub ts: DateTime<Utc>,
    pub temperature_c: Option<f64>,
    pub power_kw: Option<f64>,
    pub voltage_v: Option<f64>,
    pub raw: SensorRaw,
}

/// What a sensor reading carries beyond its typed values: where it came from and, from the
/// simulator, the meter's import and export. Any other key a `POST /sensors` client sends is kept
/// as it came (`extra`) and echoed back; the VEN never reads it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct SensorRaw {
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_kw: Option<f64>,
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export_kw: Option<f64>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl SensorSnapshot {
    /// A sensor snapshot nobody has written yet. Stamped at the epoch rather
    /// than "now": the value has no reading time, and a fresh-looking `ts`
    /// would be a made-up one (and a hidden wall-clock read).
    pub fn never_sampled() -> Self {
        Self {
            id: Uuid::new_v4(),
            ts: DateTime::<Utc>::UNIX_EPOCH,
            temperature_c: None,
            power_kw: None,
            voltage_v: None,
            raw: SensorRaw::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct SensorInput {
    pub temperature_c: Option<f64>,
    pub power_kw: Option<f64>,
    pub voltage_v: Option<f64>,
    pub raw: Option<SensorRaw>,
}
