use crate::controller::trace::ControllerTrace;
use crate::entities::capacity::{OadrCapacityState, OadrReportObligation};
use crate::entities::device_session::{BaselineOverride, EvSession, HeaterTarget, ShiftableLoad, ShiftableLoadRuntime};
use crate::entities::plan::{Plan, SiteFlexibilityEnvelope};
use crate::entities::tariff_snapshot::TariffSnapshot;
use crate::entities::user_request::{SessionType, UserRequest, UserRequestStatus};
use crate::models::SensorSnapshot;
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
    pub started_at: Option<DateTime<Utc>>,
}

impl AssetLedgerEntry {
    pub fn new(asset_id: &str) -> Self {
        Self {
            asset_id: asset_id.to_string(),
            started_at: Some(Utc::now()),
            ..Default::default()
        }
    }
}

/// Simulation injection state — set via POST /sim/inject.
/// Three injection behaviours:
/// - A (one-shot): applied once to physics state, then cleared automatically.
/// - B (frozen + EMA return): held while active; EMA-blended back to natural model on release.
/// - C (frozen + snap): held while active; snaps to profile default on release.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SimInjectState {
    // Behaviour A — one-shot (cleared after application to physics state)
    pub battery_soc: Option<f64>,
    pub ev_soc: Option<f64>,
    pub heater_temp_c: Option<f64>,
    // Behaviour B — frozen + EMA return on release
    pub pv_irradiance: Option<f64>,
    pub pv_irradiance_alpha: f64,  // default 0.1
    pub base_load_kw: Option<f64>, // one-shot: offset stored in smoothing state, then cleared
    pub base_load_alpha: f64,      // default 0.1
    // Behaviour C — frozen while active, snap to profile default on release
    pub ev_plugged: Option<bool>,
    pub ev_soc_target: Option<f64>,
    pub heater_setpoint_c: Option<f64>,
    pub heater_temp_min_c: Option<f64>,
    pub heater_temp_max_c: Option<f64>,
    pub ambient_temp_c: Option<f64>,
    pub grid_import_limit_kw: Option<f64>,
    pub grid_export_limit_kw: Option<f64>,
}

impl Default for SimInjectState {
    fn default() -> Self {
        Self {
            battery_soc: None,
            ev_soc: None,
            heater_temp_c: None,
            pv_irradiance: None,
            pv_irradiance_alpha: 0.1,
            base_load_kw: None,
            base_load_alpha: 0.1,
            ev_plugged: None,
            ev_soc_target: None,
            heater_setpoint_c: None,
            heater_temp_min_c: None,
            heater_temp_max_c: None,
            ambient_temp_c: None,
            grid_import_limit_kw: None,
            grid_export_limit_kw: None,
        }
    }
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
    pub controller_trace: ControllerTrace,
    #[serde(skip)]
    pub inject_state: SimInjectState,

    // HEMS state (not persisted in simple state.json — managed by controller loops)
    #[serde(skip)]
    pub active_plan: Option<Plan>,
    #[serde(skip)]
    pub planned_tariffs: Vec<TariffSnapshot>,
    #[serde(skip)]
    pub capacity_state: OadrCapacityState,
    #[serde(skip)]
    pub report_obligations: Vec<OadrReportObligation>,
    #[serde(skip)]
    pub asset_ledger: HashMap<String, AssetLedgerEntry>,
    #[serde(skip)]
    pub active_requests: Vec<UserRequest>,
    #[serde(skip)]
    pub site_envelope: Option<SiteFlexibilityEnvelope>,
    #[serde(skip)]
    pub ev_session: Option<EvSession>,
    #[serde(skip)]
    pub heater_target: Option<HeaterTarget>,
    #[serde(skip)]
    pub shiftable_loads: Vec<ShiftableLoad>,
    #[serde(skip)]
    pub shiftable_runtimes: Vec<ShiftableLoadRuntime>,
    #[serde(skip)]
    pub baseline_override: Option<BaselineOverride>,
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
                controller_trace: ControllerTrace::new(),
                inject_state: SimInjectState::default(),
                active_plan: None,
                planned_tariffs: vec![],
                capacity_state: OadrCapacityState::default(),
                report_obligations: vec![],
                asset_ledger: HashMap::new(),
                active_requests: vec![],
                site_envelope: None,
                ev_session: None,
                heater_target: None,
                shiftable_loads: vec![],
                shiftable_runtimes: vec![],
                baseline_override: None,
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

    pub async fn update_sim(&self, sim: SimSnapshot) {
        self.inner.write().await.sim = Some(sim);
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

    pub async fn controller_trace(&self) -> ControllerTrace {
        self.inner.read().await.controller_trace.clone()
    }

    pub async fn push_controller_event(&self, event: crate::controller::trace::ControllerEvent) {
        self.inner.write().await.controller_trace.push_event(event);
    }

    pub async fn inject_state(&self) -> SimInjectState {
        self.inner.read().await.inject_state.clone()
    }

    pub async fn set_inject_state(&self, s: SimInjectState) {
        self.inner.write().await.inject_state = s;
    }

    /// Clear a single Behaviour A (one-shot) field after it has been applied.
    pub async fn clear_inject_field(&self, field: &str) {
        let mut inner = self.inner.write().await;
        match field {
            "battery_soc" => inner.inject_state.battery_soc = None,
            "ev_soc" => inner.inject_state.ev_soc = None,
            "heater_temp_c" => inner.inject_state.heater_temp_c = None,
            "pv_irradiance" => inner.inject_state.pv_irradiance = None,
            "base_load_kw" => inner.inject_state.base_load_kw = None,
            _ => {}
        }
    }

    // --- HEMS accessors ---

    pub async fn active_plan(&self) -> Option<Plan> {
        self.inner.read().await.active_plan.clone()
    }

    pub async fn set_active_plan(&self, plan: Option<Plan>) {
        self.inner.write().await.active_plan = plan;
    }

    pub async fn planned_tariffs(&self) -> Vec<TariffSnapshot> {
        self.inner.read().await.planned_tariffs.clone()
    }

    pub async fn set_planned_tariffs(&self, tariffs: Vec<TariffSnapshot>) {
        self.inner.write().await.planned_tariffs = tariffs;
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

    pub async fn active_requests(&self) -> Vec<UserRequest> {
        self.inner.read().await.active_requests.clone()
    }

    pub async fn set_active_requests(&self, requests: Vec<UserRequest>) {
        self.inner.write().await.active_requests = requests;
    }

    /// Add a user request; replace if same id already exists.
    pub async fn upsert_request(&self, req: UserRequest) {
        let mut inner = self.inner.write().await;
        if let Some(pos) = inner.active_requests.iter().position(|r| r.id == req.id) {
            inner.active_requests[pos] = req;
        } else {
            inner.active_requests.push(req);
        }
    }

    /// Cancel a user request by id: marks it Cancelled and clears any linked device session.
    pub async fn cancel_request(&self, id: uuid::Uuid) -> bool {
        let mut inner = self.inner.write().await;
        if let Some(req) = inner.active_requests.iter_mut().find(|r| r.id == id) {
            req.status = UserRequestStatus::Cancelled;
            let session_type = req.session_type.clone();
            let session_id = req.session_id;
            match session_type {
                Some(SessionType::Ev) => {
                    inner.ev_session = None;
                }
                Some(SessionType::Heater) => {
                    inner.heater_target = None;
                }
                Some(SessionType::ShiftableLoad) => {
                    if let Some(sid) = session_id {
                        inner.shiftable_loads.retain(|l| l.id != sid);
                        inner.shiftable_runtimes.retain(|r| r.load_id != sid);
                    }
                }
                None => {
                    // Legacy path: match session_id against ev/heater for requests
                    // created before Plan C added session_type.
                    if let Some(sid) = session_id {
                        if inner.ev_session.as_ref().map(|s| s.id) == Some(sid) {
                            inner.ev_session = None;
                        } else if inner.heater_target.as_ref().map(|t| t.id) == Some(sid) {
                            inner.heater_target = None;
                        }
                    }
                }
            }
            true
        } else {
            false
        }
    }

    pub async fn asset_ledger(&self) -> HashMap<String, AssetLedgerEntry> {
        self.inner.read().await.asset_ledger.clone()
    }

    pub async fn set_asset_ledger(&self, ledger: HashMap<String, AssetLedgerEntry>) {
        self.inner.write().await.asset_ledger = ledger;
    }

    pub async fn site_envelope(&self) -> Option<SiteFlexibilityEnvelope> {
        self.inner.read().await.site_envelope.clone()
    }

    pub async fn set_site_envelope(&self, env: SiteFlexibilityEnvelope) {
        self.inner.write().await.site_envelope = Some(env);
    }

    pub async fn ev_session(&self) -> Option<EvSession> {
        self.inner.read().await.ev_session.clone()
    }

    pub async fn set_ev_session(&self, session: Option<EvSession>) {
        self.inner.write().await.ev_session = session;
    }

    pub async fn heater_target(&self) -> Option<HeaterTarget> {
        self.inner.read().await.heater_target.clone()
    }

    pub async fn set_heater_target(&self, target: Option<HeaterTarget>) {
        self.inner.write().await.heater_target = target;
    }

    pub async fn shiftable_loads(&self) -> Vec<ShiftableLoad> {
        self.inner.read().await.shiftable_loads.clone()
    }

    pub async fn add_shiftable_load(&self, load: ShiftableLoad) -> Result<(), &'static str> {
        let mut w = self.inner.write().await;
        if w.shiftable_loads.iter().any(|l| l.asset_id == load.asset_id) {
            return Err("duplicate asset_id");
        }
        w.shiftable_loads.push(load);
        Ok(())
    }

    pub async fn remove_shiftable_load(&self, id: uuid::Uuid) -> bool {
        let mut w = self.inner.write().await;
        let before = w.shiftable_loads.len();
        w.shiftable_loads.retain(|l| l.id != id);
        w.shiftable_runtimes.retain(|r| r.load_id != id);
        w.shiftable_loads.len() < before
    }

    pub async fn shiftable_runtimes(&self) -> Vec<ShiftableLoadRuntime> {
        self.inner.read().await.shiftable_runtimes.clone()
    }

    pub async fn start_shiftable(&self, runtime: ShiftableLoadRuntime) {
        self.inner.write().await.shiftable_runtimes.push(runtime);
    }

    pub async fn complete_shiftable(&self, load_id: uuid::Uuid) {
        let mut w = self.inner.write().await;
        w.shiftable_runtimes.retain(|r| r.load_id != load_id);
        w.shiftable_loads.retain(|l| l.id != load_id);
        // Also mark linked UserRequest as Completed.
        if let Some(req) = w
            .active_requests
            .iter_mut()
            .find(|r| r.session_id == Some(load_id) && r.status == UserRequestStatus::Active)
        {
            req.status = UserRequestStatus::Completed;
            req.updated_at = Utc::now();
        }
    }

    pub async fn baseline_override(&self) -> Option<BaselineOverride> {
        self.inner.read().await.baseline_override.clone()
    }

    pub async fn set_baseline_override(&self, ovr: Option<BaselineOverride>) {
        self.inner.write().await.baseline_override = ovr;
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
            controller_trace: self.controller_trace.clone(),
            inject_state: self.inject_state.clone(),
            active_plan: self.active_plan.clone(),
            planned_tariffs: self.planned_tariffs.clone(),
            capacity_state: self.capacity_state.clone(),
            report_obligations: self.report_obligations.clone(),
            asset_ledger: self.asset_ledger.clone(),
            active_requests: self.active_requests.clone(),
            site_envelope: self.site_envelope.clone(),
            ev_session: self.ev_session.clone(),
            heater_target: self.heater_target.clone(),
            shiftable_loads: self.shiftable_loads.clone(),
            shiftable_runtimes: self.shiftable_runtimes.clone(),
            baseline_override: self.baseline_override.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::device_session::{ShiftableLoad, ShiftableLoadRuntime};
    use chrono::{Duration, Utc};
    use uuid::Uuid;

    fn make_load(asset_id: &str) -> ShiftableLoad {
        let now = Utc::now();
        ShiftableLoad {
            id: Uuid::new_v4(),
            asset_id: asset_id.to_string(),
            power_kw: 2.0,
            duration_min: 60,
            earliest_start: now,
            latest_end: now + Duration::hours(6),
            created_at: now,
            updated_at: now,
        }
    }

    fn make_runtime(load: &ShiftableLoad) -> ShiftableLoadRuntime {
        let now = Utc::now();
        ShiftableLoadRuntime {
            load_id: load.id,
            asset_id: load.asset_id.clone(),
            power_kw: load.power_kw,
            started_at: now,
            ends_at: now + Duration::minutes(load.duration_min as i64),
        }
    }

    #[test]
    fn shiftable_runtime_is_running() {
        let now = Utc::now();
        let rt = ShiftableLoadRuntime {
            load_id: Uuid::new_v4(),
            asset_id: "wm-1".to_string(),
            power_kw: 2.0,
            started_at: now,
            ends_at: now + Duration::minutes(60),
        };
        assert!(rt.is_running(now), "should be running at start");
        assert!(rt.is_running(now + Duration::minutes(30)), "should be running mid-way");
        assert!(!rt.is_running(now + Duration::minutes(60)), "half-open: not running at ends_at");
        assert!(!rt.is_running(now - Duration::seconds(1)), "not running before start");
    }

    #[tokio::test]
    async fn add_shiftable_load_rejects_duplicate() {
        let state = AppState::new();
        let load1 = make_load("wm-1");
        let mut load2 = make_load("wm-1");
        load2.id = Uuid::new_v4(); // different id, same asset_id

        assert!(state.add_shiftable_load(load1).await.is_ok());
        assert!(state.add_shiftable_load(load2).await.is_err(), "duplicate asset_id rejected");
        assert_eq!(state.shiftable_loads().await.len(), 1);
    }

    #[tokio::test]
    async fn start_and_read_runtime() {
        let state = AppState::new();
        let load = make_load("wm-1");
        state.add_shiftable_load(load.clone()).await.unwrap();

        let rt = make_runtime(&load);
        state.start_shiftable(rt.clone()).await;

        let runtimes = state.shiftable_runtimes().await;
        assert_eq!(runtimes.len(), 1);
        assert_eq!(runtimes[0].load_id, load.id);
        assert_eq!(runtimes[0].asset_id, "wm-1");
    }

    #[tokio::test]
    async fn complete_removes_both_collections() {
        let state = AppState::new();
        let load = make_load("wm-1");
        let load_id = load.id;
        state.add_shiftable_load(load.clone()).await.unwrap();
        state.start_shiftable(make_runtime(&load)).await;

        state.complete_shiftable(load_id).await;

        assert!(state.shiftable_loads().await.is_empty(), "load removed");
        assert!(state.shiftable_runtimes().await.is_empty(), "runtime removed");
    }

    #[tokio::test]
    async fn remove_load_removes_runtime() {
        let state = AppState::new();
        let load = make_load("wm-1");
        let load_id = load.id;
        state.add_shiftable_load(load.clone()).await.unwrap();
        state.start_shiftable(make_runtime(&load)).await;

        let removed = state.remove_shiftable_load(load_id).await;

        assert!(removed, "remove should return true");
        assert!(state.shiftable_loads().await.is_empty(), "load removed");
        assert!(state.shiftable_runtimes().await.is_empty(), "runtime also removed");
    }
}
