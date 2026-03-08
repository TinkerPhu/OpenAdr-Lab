use crate::entities::capacity::{OadrCapacityState, OadrReportObligation};
use crate::entities::energy_packet::EnergyPacket;
use crate::entities::plan::Plan;
use crate::entities::rate_snapshot::RateSnapshot;
use crate::models::SensorSnapshot;
use crate::reactor::trace::TraceEntry;
use crate::simulator::SimSnapshot;
use chrono::DateTime;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Per-asset cumulative energy/cost/CO₂ since VEN startup.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AssetLedgerEntry {
    pub asset_id: String,
    pub energy_kwh: f64,
    pub cost_eur: f64,
    pub co2_g: f64,
    pub updated_at: Option<DateTime<Utc>>,
}

impl AssetLedgerEntry {
    pub fn new(asset_id: &str) -> Self {
        Self {
            asset_id: asset_id.to_string(),
            ..Default::default()
        }
    }
}

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

    // HEMS state (not persisted in simple state.json — managed by controller loops)
    #[serde(skip)]
    pub active_packets: Vec<EnergyPacket>,
    #[serde(skip)]
    pub active_plan: Option<Plan>,
    #[serde(skip)]
    pub planned_rates: Vec<RateSnapshot>,
    #[serde(skip)]
    pub capacity_state: OadrCapacityState,
    #[serde(skip)]
    pub report_obligations: Vec<OadrReportObligation>,
    #[serde(skip)]
    pub asset_ledger: HashMap<String, AssetLedgerEntry>,
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
                active_packets: vec![],
                active_plan: None,
                planned_rates: vec![],
                capacity_state: OadrCapacityState::default(),
                report_obligations: vec![],
                asset_ledger: HashMap::new(),
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

    // --- HEMS accessors ---

    pub async fn active_packets(&self) -> Vec<EnergyPacket> {
        self.inner.read().await.active_packets.clone()
    }

    pub async fn set_active_packets(&self, packets: Vec<EnergyPacket>) {
        self.inner.write().await.active_packets = packets;
    }

    pub async fn active_plan(&self) -> Option<Plan> {
        self.inner.read().await.active_plan.clone()
    }

    pub async fn set_active_plan(&self, plan: Option<Plan>) {
        self.inner.write().await.active_plan = plan;
    }

    pub async fn planned_rates(&self) -> Vec<RateSnapshot> {
        self.inner.read().await.planned_rates.clone()
    }

    pub async fn set_planned_rates(&self, rates: Vec<RateSnapshot>) {
        self.inner.write().await.planned_rates = rates;
    }

    pub async fn capacity_state(&self) -> OadrCapacityState {
        self.inner.read().await.capacity_state.clone()
    }

    pub async fn set_capacity_state(&self, state: OadrCapacityState) {
        self.inner.write().await.capacity_state = state;
    }

    pub async fn report_obligations(&self) -> Vec<OadrReportObligation> {
        self.inner.read().await.report_obligations.clone()
    }

    pub async fn set_report_obligations(&self, obligations: Vec<OadrReportObligation>) {
        self.inner.write().await.report_obligations = obligations;
    }

    /// Alias used by the controller (Stage 2).
    pub async fn obligations(&self) -> Vec<OadrReportObligation> {
        self.inner.read().await.report_obligations.clone()
    }

    /// Append new obligations without duplicating existing ones (keyed by id).
    pub async fn add_obligations(&self, new_obs: Vec<OadrReportObligation>) {
        if new_obs.is_empty() {
            return;
        }
        let mut inner = self.inner.write().await;
        for ob in new_obs {
            if !inner.report_obligations.iter().any(|e| e.id == ob.id) {
                inner.report_obligations.push(ob);
            }
        }
    }

    /// Mark an obligation as fulfilled by its UUID.
    pub async fn mark_obligation_fulfilled(&self, id: uuid::Uuid) {
        let mut inner = self.inner.write().await;
        if let Some(ob) = inner.report_obligations.iter_mut().find(|o| o.id == id) {
            ob.fulfilled = true;
        }
    }

    pub async fn asset_ledger(&self) -> HashMap<String, AssetLedgerEntry> {
        self.inner.read().await.asset_ledger.clone()
    }

    pub async fn set_asset_ledger(&self, ledger: HashMap<String, AssetLedgerEntry>) {
        self.inner.write().await.asset_ledger = ledger;
    }

    /// Return all unfulfilled obligations whose due_at <= now.
    pub async fn due_obligations(&self, now: DateTime<Utc>) -> Vec<OadrReportObligation> {
        self.inner
            .read()
            .await
            .report_obligations
            .iter()
            .filter(|o| o.is_due(now))
            .cloned()
            .collect()
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
            active_packets: self.active_packets.clone(),
            active_plan: self.active_plan.clone(),
            planned_rates: self.planned_rates.clone(),
            capacity_state: self.capacity_state.clone(),
            report_obligations: self.report_obligations.clone(),
            asset_ledger: self.asset_ledger.clone(),
        }
    }
}
