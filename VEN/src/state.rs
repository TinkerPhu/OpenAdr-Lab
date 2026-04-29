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

/// User-controllable settings for the opportunistic EV charging overlay.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvSettings {
    /// When true (default), the dispatcher routes live PV surplus to the EV when
    /// no EvSession is active. Automatically paused while an EvSession exists.
    #[serde(default = "bool_true")]
    pub opportunistic_charging_enabled: bool,
    /// Derived: true while any EvSession is active. Set by tick loop, not user-settable.
    #[serde(default)]
    pub paused_by_active_session: bool,
}

fn bool_true() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PollingState {
    pub programs: Vec<serde_json::Value>,
    pub events: Vec<serde_json::Value>,
    pub reports: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControllerSimState {
    pub sensor: SensorSnapshot,
    #[serde(skip)]
    pub sim: Option<SimSnapshot>,
    #[serde(skip)]
    pub inject_state: SimInjectState,
    #[serde(skip)]
    pub controller_trace: ControllerTrace,
}

impl Default for ControllerSimState {
    fn default() -> Self {
        Self {
            sensor: SensorSnapshot::empty_now(),
            sim: None,
            inject_state: SimInjectState::default(),
            controller_trace: ControllerTrace::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct HemsState {
    pub active_plan: Option<Plan>,
    pub planned_tariffs: Vec<TariffSnapshot>,
    pub capacity_state: OadrCapacityState,
    pub report_obligations: Vec<OadrReportObligation>,
    pub asset_ledger: HashMap<String, AssetLedgerEntry>,
    pub active_requests: Vec<UserRequest>,
    pub site_envelope: Option<SiteFlexibilityEnvelope>,
    pub ev_session: Option<EvSession>,
    pub heater_target: Option<HeaterTarget>,
    pub shiftable_loads: Vec<ShiftableLoad>,
    pub shiftable_runtimes: Vec<ShiftableLoadRuntime>,
    pub baseline_override: Option<BaselineOverride>,
    pub ev_settings: EvSettings,
}

#[derive(Clone)]
pub struct AppState {
    pub polling: Arc<RwLock<PollingState>>,
    pub ctrl_sim: Arc<RwLock<ControllerSimState>>,
    pub hems: Arc<RwLock<HemsState>>,
}

#[derive(Serialize, Deserialize)]
struct PersistedVenState {
    programs: Vec<serde_json::Value>,
    events: Vec<serde_json::Value>,
    reports: Vec<serde_json::Value>,
    sensor: SensorSnapshot,
}

impl AppState {
    // INVARIANT: No function may acquire more than one lock simultaneously. Always
    // snapshot-and-release: acquire → clone needed fields → drop guard → work on snapshot.
    // No guard may cross an .await point or a second lock acquisition.

    pub fn new() -> Self {
        Self {
            polling: Arc::new(RwLock::new(PollingState::default())),
            ctrl_sim: Arc::new(RwLock::new(ControllerSimState::default())),
            hems: Arc::new(RwLock::new(HemsState {
                ev_settings: EvSettings {
                    opportunistic_charging_enabled: true,
                    paused_by_active_session: false,
                },
                ..HemsState::default()
            })),
        }
    }

    pub async fn set_programs(&self, programs: Vec<serde_json::Value>) {
        self.polling.write().await.programs = programs;
    }

    pub async fn set_events(&self, mut events: Vec<serde_json::Value>, max_keep: usize) {
        events.truncate(max_keep);
        self.polling.write().await.events = events;
    }

    pub async fn set_reports(&self, reports: Vec<serde_json::Value>) {
        self.polling.write().await.reports = reports;
    }

    pub async fn update_sensor(&self, sensor: SensorSnapshot) {
        self.ctrl_sim.write().await.sensor = sensor;
    }

    pub async fn update_sim(&self, sim: SimSnapshot) {
        self.ctrl_sim.write().await.sim = Some(sim);
    }

    pub async fn programs(&self) -> Vec<serde_json::Value> {
        self.polling.read().await.programs.clone()
    }

    pub async fn events(&self) -> Vec<serde_json::Value> {
        self.polling.read().await.events.clone()
    }

    pub async fn reports(&self) -> Vec<serde_json::Value> {
        self.polling.read().await.reports.clone()
    }

    pub async fn sensor(&self) -> SensorSnapshot {
        self.ctrl_sim.read().await.sensor.clone()
    }

    pub async fn sim(&self) -> Option<SimSnapshot> {
        self.ctrl_sim.read().await.sim.clone()
    }

    pub async fn controller_trace(&self) -> ControllerTrace {
        self.ctrl_sim.read().await.controller_trace.clone()
    }

    pub async fn push_controller_event(&self, event: crate::controller::trace::ControllerEvent) {
        self.ctrl_sim.write().await.controller_trace.push_event(event);
    }

    pub async fn inject_state(&self) -> SimInjectState {
        self.ctrl_sim.read().await.inject_state.clone()
    }

    pub async fn set_inject_state(&self, s: SimInjectState) {
        self.ctrl_sim.write().await.inject_state = s;
    }

    /// Clear a single Behaviour A (one-shot) field after it has been applied.
    pub async fn clear_inject_field(&self, field: &str) {
        let mut cs = self.ctrl_sim.write().await;
        match field {
            "battery_soc" => cs.inject_state.battery_soc = None,
            "ev_soc" => cs.inject_state.ev_soc = None,
            "heater_temp_c" => cs.inject_state.heater_temp_c = None,
            "pv_irradiance" => cs.inject_state.pv_irradiance = None,
            "base_load_kw" => cs.inject_state.base_load_kw = None,
            _ => {}
        }
    }

    // --- HEMS accessors ---

    pub async fn active_plan(&self) -> Option<Plan> {
        self.hems.read().await.active_plan.clone()
    }

    pub async fn set_active_plan(&self, plan: Option<Plan>) {
        self.hems.write().await.active_plan = plan;
    }

    pub async fn planned_tariffs(&self) -> Vec<TariffSnapshot> {
        self.hems.read().await.planned_tariffs.clone()
    }

    pub async fn set_planned_tariffs(&self, tariffs: Vec<TariffSnapshot>) {
        self.hems.write().await.planned_tariffs = tariffs;
    }

    pub async fn capacity_state(&self) -> OadrCapacityState {
        self.hems.read().await.capacity_state.clone()
    }

    pub async fn set_capacity_state(&self, state: OadrCapacityState) {
        self.hems.write().await.capacity_state = state;
    }

    pub async fn report_obligations(&self) -> Vec<OadrReportObligation> {
        self.hems.read().await.report_obligations.clone()
    }

    pub async fn set_report_obligations(&self, obligations: Vec<OadrReportObligation>) {
        self.hems.write().await.report_obligations = obligations;
    }

    /// Append new obligations without duplicating existing ones (keyed by id).
    pub async fn add_obligations(&self, new_obs: Vec<OadrReportObligation>) {
        if new_obs.is_empty() {
            return;
        }
        let mut hems = self.hems.write().await;
        for ob in new_obs {
            if !hems.report_obligations.iter().any(|e| e.id == ob.id) {
                hems.report_obligations.push(ob);
            }
        }
    }

    /// Mark an obligation as fulfilled by its UUID.
    pub async fn mark_obligation_fulfilled(&self, id: uuid::Uuid) {
        let mut hems = self.hems.write().await;
        if let Some(ob) = hems.report_obligations.iter_mut().find(|o| o.id == id) {
            ob.fulfilled = true;
        }
    }

    pub async fn active_requests(&self) -> Vec<UserRequest> {
        self.hems.read().await.active_requests.clone()
    }

    pub async fn set_active_requests(&self, requests: Vec<UserRequest>) {
        self.hems.write().await.active_requests = requests;
    }

    /// Add a user request; replace if same id already exists.
    pub async fn upsert_request(&self, req: UserRequest) {
        let mut hems = self.hems.write().await;
        if let Some(pos) = hems.active_requests.iter().position(|r| r.id == req.id) {
            hems.active_requests[pos] = req;
        } else {
            hems.active_requests.push(req);
        }
    }

    /// Cancel a user request by id: marks it Cancelled and clears any linked device session.
    pub async fn cancel_request(&self, id: uuid::Uuid) -> bool {
        let mut hems = self.hems.write().await;
        if let Some(req) = hems.active_requests.iter_mut().find(|r| r.id == id) {
            req.status = UserRequestStatus::Cancelled;
            let session_type = req.session_type.clone();
            let session_id = req.session_id;
            match session_type {
                Some(SessionType::Ev) => {
                    hems.ev_session = None;
                }
                Some(SessionType::Heater) => {
                    hems.heater_target = None;
                }
                Some(SessionType::ShiftableLoad) => {
                    if let Some(sid) = session_id {
                        hems.shiftable_loads.retain(|l| l.id != sid);
                        hems.shiftable_runtimes.retain(|r| r.load_id != sid);
                    }
                }
                None => {
                    tracing::warn!(request_id = %id, "cancel_request: unexpected session_type: None for request {}", id);
                }
            }
            true
        } else {
            false
        }
    }

    pub async fn asset_ledger(&self) -> HashMap<String, AssetLedgerEntry> {
        self.hems.read().await.asset_ledger.clone()
    }

    pub async fn set_asset_ledger(&self, ledger: HashMap<String, AssetLedgerEntry>) {
        self.hems.write().await.asset_ledger = ledger;
    }

    pub async fn site_envelope(&self) -> Option<SiteFlexibilityEnvelope> {
        self.hems.read().await.site_envelope.clone()
    }

    pub async fn set_site_envelope(&self, env: SiteFlexibilityEnvelope) {
        self.hems.write().await.site_envelope = Some(env);
    }

    pub async fn ev_session(&self) -> Option<EvSession> {
        self.hems.read().await.ev_session.clone()
    }

    pub async fn set_ev_session(&self, session: Option<EvSession>) {
        self.hems.write().await.ev_session = session;
    }

    pub async fn heater_target(&self) -> Option<HeaterTarget> {
        self.hems.read().await.heater_target.clone()
    }

    pub async fn set_heater_target(&self, target: Option<HeaterTarget>) {
        self.hems.write().await.heater_target = target;
    }

    pub async fn shiftable_loads(&self) -> Vec<ShiftableLoad> {
        self.hems.read().await.shiftable_loads.clone()
    }

    pub async fn add_shiftable_load(&self, load: ShiftableLoad) -> Result<(), &'static str> {
        let mut w = self.hems.write().await;
        if w.shiftable_loads.iter().any(|l| l.asset_id == load.asset_id) {
            return Err("duplicate asset_id");
        }
        w.shiftable_loads.push(load);
        Ok(())
    }

    pub async fn remove_shiftable_load(&self, id: uuid::Uuid) -> bool {
        let mut w = self.hems.write().await;
        let before = w.shiftable_loads.len();
        w.shiftable_loads.retain(|l| l.id != id);
        w.shiftable_runtimes.retain(|r| r.load_id != id);
        w.shiftable_loads.len() < before
    }

    pub async fn shiftable_runtimes(&self) -> Vec<ShiftableLoadRuntime> {
        self.hems.read().await.shiftable_runtimes.clone()
    }

    pub async fn start_shiftable(&self, runtime: ShiftableLoadRuntime) {
        self.hems.write().await.shiftable_runtimes.push(runtime);
    }

    pub async fn complete_shiftable(&self, load_id: uuid::Uuid) {
        let mut w = self.hems.write().await;
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
        self.hems.read().await.baseline_override.clone()
    }

    pub async fn set_baseline_override(&self, ovr: Option<BaselineOverride>) {
        self.hems.write().await.baseline_override = ovr;
    }

    pub async fn ev_settings(&self) -> EvSettings {
        self.hems.read().await.ev_settings.clone()
    }

    pub async fn set_ev_settings(&self, s: EvSettings) {
        self.hems.write().await.ev_settings = s;
    }

    /// Return all unfulfilled obligations whose due_at <= now.
    pub async fn due_obligations(&self, now: DateTime<Utc>) -> Vec<OadrReportObligation> {
        self.hems
            .read()
            .await
            .report_obligations
            .iter()
            .filter(|o| o.is_due(now))
            .cloned()
            .collect()
    }

    pub async fn load_from_json(&self, json: &str) -> anyhow::Result<()> {
        let parsed: PersistedVenState = serde_json::from_str(json)?;
        {
            let mut p = self.polling.write().await;
            p.programs = parsed.programs;
            p.events = parsed.events;
            p.reports = parsed.reports;
        }
        {
            let mut cs = self.ctrl_sim.write().await;
            cs.sensor = parsed.sensor;
        }
        Ok(())
    }

    pub async fn to_json(&self) -> anyhow::Result<String> {
        // Acquire each lock separately (INVARIANT: no guard held across a second lock acquisition).
        let (programs, events, reports) = {
            let p = self.polling.read().await;
            (p.programs.clone(), p.events.clone(), p.reports.clone())
        };
        let sensor = self.ctrl_sim.read().await.sensor.clone();
        let state = PersistedVenState { programs, events, reports, sensor };
        Ok(serde_json::to_string_pretty(&state)?)
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

    fn make_request(session_type: Option<SessionType>, session_id: Option<uuid::Uuid>) -> UserRequest {
        let now = Utc::now();
        UserRequest {
            id: Uuid::new_v4(),
            asset_id: "test".to_string(),
            target_soc: None,
            target_energy_kwh: 0.0,
            desired_power_kw: 0.0,
            deadlines: vec![],
            completion_policy: "best_effort".to_string(),
            max_total_cost_eur: None,
            tier_count: 0,
            session_id,
            session_type,
            status: UserRequestStatus::Active,
            estimated_cost_eur: 0.0,
            estimated_co2_g: 0.0,
            interruptible: false,
            tolerance_min: None,
            budget_eur: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn cancel_request_ev_clears_session() {
        use crate::entities::device_session::EvSession;
        let state = AppState::new();
        let session_id = Uuid::new_v4();
        let req = make_request(Some(SessionType::Ev), Some(session_id));
        let req_id = req.id;
        state.upsert_request(req).await;
        state.set_ev_session(Some(EvSession {
            id: session_id,
            target_soc: 0.8,
            departure_time: Utc::now() + chrono::Duration::hours(2),
            soft_deadline: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })).await;

        let found = state.cancel_request(req_id).await;
        assert!(found, "cancel should return true");
        assert!(state.ev_session().await.is_none(), "ev_session cleared");
        let requests = state.active_requests().await;
        assert_eq!(requests[0].status, UserRequestStatus::Cancelled);
    }

    #[tokio::test]
    async fn cancel_request_heater_clears_target() {
        use crate::entities::device_session::HeaterTarget;
        let state = AppState::new();
        let session_id = Uuid::new_v4();
        let req = make_request(Some(SessionType::Heater), Some(session_id));
        let req_id = req.id;
        state.upsert_request(req).await;
        state.set_heater_target(Some(HeaterTarget {
            id: session_id,
            target_temp_c: 21.0,
            ready_by: Utc::now() + chrono::Duration::hours(1),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })).await;

        let found = state.cancel_request(req_id).await;
        assert!(found, "cancel should return true");
        assert!(state.heater_target().await.is_none(), "heater_target cleared");
        let requests = state.active_requests().await;
        assert_eq!(requests[0].status, UserRequestStatus::Cancelled);
    }

    #[tokio::test]
    async fn cancel_request_shiftable_removes_load_and_runtime() {
        let state = AppState::new();
        let load = make_load("wm-1");
        let load_id = load.id;
        state.add_shiftable_load(load.clone()).await.unwrap();
        state.start_shiftable(make_runtime(&load)).await;

        let req = make_request(Some(SessionType::ShiftableLoad), Some(load_id));
        let req_id = req.id;
        state.upsert_request(req).await;

        let found = state.cancel_request(req_id).await;
        assert!(found, "cancel should return true");
        assert!(state.shiftable_loads().await.is_empty(), "load removed");
        assert!(state.shiftable_runtimes().await.is_empty(), "runtime removed");
        let requests = state.active_requests().await;
        assert_eq!(requests[0].status, UserRequestStatus::Cancelled);
    }

    #[tokio::test]
    async fn test_state_persistence_roundtrip() {
        use serde_json::json;
        let state = AppState::new();
        // Set programs via polling lock
        state.set_programs(vec![json!({"id": "p1"})]).await;
        // The sensor default has power_kw = 0.0; we'll just verify programs survive roundtrip
        let json_str = state.to_json().await.unwrap();
        let state2 = AppState::new();
        state2.load_from_json(&json_str).await.unwrap();
        let programs = state2.programs().await;
        assert_eq!(programs.len(), 1);
        assert_eq!(programs[0]["id"], "p1");
    }
}
