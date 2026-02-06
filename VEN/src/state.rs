use crate::models::{Event, Program, SensorSnapshot};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AppState {
    inner: Arc<RwLock<InnerState>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InnerState {
    pub programs: Vec<Program>,
    pub events: Vec<Event>,
    pub sensor: SensorSnapshot,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(InnerState {
                programs: vec![],
                events: vec![],
                sensor: SensorSnapshot::empty_now(),
            })),
        }
    }

    pub async fn set_programs(&self, programs: Vec<Program>) {
        self.inner.write().await.programs = programs;
    }

    pub async fn set_events(&self, mut events: Vec<Event>, max_keep: usize) {
        // Keep newest first (assume poller provides newest first; enforce anyway)
        events.truncate(max_keep);
        self.inner.write().await.events = events;
    }

    pub async fn update_sensor(&self, sensor: SensorSnapshot) {
        self.inner.write().await.sensor = sensor;
    }

    pub async fn snapshot(&self) -> InnerState {
        self.inner.read().await.clone()
    }

    pub async fn programs(&self) -> Vec<Program> {
        self.inner.read().await.programs.clone()
    }

    pub async fn events(&self) -> Vec<Event> {
        self.inner.read().await.events.clone()
    }

    pub async fn sensor(&self) -> SensorSnapshot {
        self.inner.read().await.sensor.clone()
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

// Required because we clone InnerState in snapshot()
impl Clone for InnerState {
    fn clone(&self) -> Self {
        Self {
            programs: self.programs.clone(),
            events: self.events.clone(),
            sensor: self.sensor.clone(),
        }
    }
}
