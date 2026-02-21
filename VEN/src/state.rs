use crate::models::SensorSnapshot;
use crate::reactor::trace::TraceEntry;
use crate::simulator::SimSnapshot;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// User-adjustable simulation parameters, sent via POST /sim/override.
/// All fields are optional; None means "use profile default".
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct UserOverrides {
    // Environment inputs
    pub pv_irradiance: Option<f64>,      // 0.0–1.0; None = auto (time-based sin model)
    pub ambient_temp_c: Option<f64>,     // None = fixed 10.0°C

    // EV preference (overridden by active events)
    pub ev_desired_kw: Option<f64>,      // idle charge rate; None = ev.max_charge_kw
    pub ev_plugged: Option<bool>,        // None = always true

    // Owner force-overrides: beat VTN events; None = let reactor/events control
    pub ev_force_kw: Option<f64>,
    pub heater_force_kw: Option<f64>,
    pub pv_force_export_limit_kw: Option<f64>,

    // Device spec overrides (shadow profile values)
    pub ev_max_charge_kw: Option<f64>,
    pub ev_soc_target: Option<f64>,
    pub heater_max_kw: Option<f64>,
    pub heater_temp_min_c: Option<f64>,
    pub heater_temp_max_c: Option<f64>,
    pub pv_rated_kw: Option<f64>,
    pub base_load_w: Option<f64>,
}

#[derive(Clone)]
pub struct AppState {
    inner: Arc<RwLock<InnerState>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InnerState {
    pub programs: Vec<serde_json::Value>,
    pub events: Vec<serde_json::Value>,
    pub reports: Vec<serde_json::Value>,
    pub sensor: SensorSnapshot,
    #[serde(skip)]
    pub sim: Option<SimSnapshot>,
    #[serde(skip)]
    pub trace: Vec<TraceEntry>,
    #[serde(default)]
    pub overrides: UserOverrides,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(InnerState {
                programs: vec![],
                events: vec![],
                reports: vec![],
                sensor: SensorSnapshot::empty_now(),
                sim: None,
                trace: vec![],
                overrides: UserOverrides::default(),
            })),
        }
    }

    pub async fn set_programs(&self, programs: Vec<serde_json::Value>) {
        self.inner.write().await.programs = programs;
    }

    pub async fn set_events(&self, mut events: Vec<serde_json::Value>, max_keep: usize) {
        events.truncate(max_keep);
        self.inner.write().await.events = events;
    }

    pub async fn set_reports(&self, reports: Vec<serde_json::Value>) {
        self.inner.write().await.reports = reports;
    }

    pub async fn update_sensor(&self, sensor: SensorSnapshot) {
        self.inner.write().await.sensor = sensor;
    }

    pub async fn update_sim(&self, sim: SimSnapshot, trace: Vec<TraceEntry>) {
        let mut inner = self.inner.write().await;
        inner.sim = Some(sim);
        inner.trace = trace;
    }

    pub async fn snapshot(&self) -> InnerState {
        self.inner.read().await.clone()
    }

    pub async fn programs(&self) -> Vec<serde_json::Value> {
        self.inner.read().await.programs.clone()
    }

    pub async fn events(&self) -> Vec<serde_json::Value> {
        self.inner.read().await.events.clone()
    }

    pub async fn reports(&self) -> Vec<serde_json::Value> {
        self.inner.read().await.reports.clone()
    }

    pub async fn sensor(&self) -> SensorSnapshot {
        self.inner.read().await.sensor.clone()
    }

    pub async fn sim(&self) -> Option<SimSnapshot> {
        self.inner.read().await.sim.clone()
    }

    pub async fn trace(&self) -> Vec<TraceEntry> {
        self.inner.read().await.trace.clone()
    }

    pub async fn overrides(&self) -> UserOverrides {
        self.inner.read().await.overrides.clone()
    }

    pub async fn set_overrides(&self, o: UserOverrides) {
        self.inner.write().await.overrides = o;
    }

    pub async fn load_from_json(&self, json: &str) -> anyhow::Result<()> {
        let parsed: InnerState = serde_json::from_str(json)?;
        *self.inner.write().await = parsed;
        Ok(())
    }

    pub async fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string_pretty(&*self.inner.read().await)?)
    }
}

impl Clone for InnerState {
    fn clone(&self) -> Self {
        Self {
            programs: self.programs.clone(),
            events: self.events.clone(),
            reports: self.reports.clone(),
            sensor: self.sensor.clone(),
            sim: self.sim.clone(),
            trace: self.trace.clone(),
            overrides: self.overrides.clone(),
        }
    }
}
