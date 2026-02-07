use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SensorSnapshot {
    pub id: Uuid,
    pub ts: DateTime<Utc>,
    pub temperature_c: Option<f64>,
    pub power_w: Option<f64>,
    pub voltage_v: Option<f64>,
    pub raw: serde_json::Value,
}

impl SensorSnapshot {
    pub fn empty_now() -> Self {
        Self {
            id: Uuid::new_v4(),
            ts: Utc::now(),
            temperature_c: None,
            power_w: None,
            voltage_v: None,
            raw: serde_json::json!({}),
        }
    }
}
