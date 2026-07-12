use crate::controller::trace::ControllerTrace;
use crate::controller::vtn_port::{OadrEvent, OadrProgram};
use crate::controller::SimSnapshot;
use crate::entities::capacity::{
    AlertWindow, DispatchWindow, OadrCapacityState, OadrReportObligation, SimpleWindow,
};
use crate::entities::design_vocabulary::AssetForecast;
use crate::entities::device_session::{
    BaselineOverride, EvSession, HeaterTarget, ShiftableLoad, ShiftableLoadRuntime,
};
use crate::entities::plan::{Plan, SiteFlexibilityEnvelope};
use crate::entities::sim_inject::SimInjectState;
use crate::entities::tariff_snapshot::TariffSnapshot;
use crate::entities::user_request::{SessionType, UserRequest, UserRequestStatus};
use crate::models::SensorSnapshot;
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

fn bool_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PollingState {
    pub programs: Vec<OadrProgram>,
    pub events: Vec<OadrEvent>,
    /// Full report JSON from VTN — pass-through for GET /reports and BDD assertions.
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
    pub anchor_until: Option<DateTime<Utc>>,
    pub planned_tariffs: Vec<TariffSnapshot>,
    pub capacity_state: OadrCapacityState,
    /// WP3.1 (BL-04): active grid-alert windows parsed from ALERT_* events.
    pub alert_windows: Vec<AlertWindow>,
    /// WP3.2: active SIMPLE load-shed windows (levels 1–3).
    pub simple_windows: Vec<SimpleWindow>,
    /// WP3.4: active DISPATCH_SETPOINT windows (direct site-setpoint control).
    pub dispatch_windows: Vec<DispatchWindow>,
    /// WP3.6 (BL-15): per-asset forecasts from the latest plan cycle.
    pub asset_forecasts: Vec<AssetForecast>,
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
    /// WP4.3 (BL-20): bounded ring of user-facing notifications, newest last.
    pub notifications:
        Arc<RwLock<std::collections::VecDeque<crate::entities::notification::UserNotification>>>,
    /// WP4.2 (BL-19): per-asset user comfort-curve overrides (hot map;
    /// persisted through SettingsPort, re-seeded at startup).
    pub comfort_overrides:
        Arc<RwLock<std::collections::HashMap<String, Vec<crate::entities::asset::ComfortRate>>>>,
}

/// WP4.3: in-memory notification ring capacity (mirrors the /trace/events ring).
pub const NOTIFICATION_RING_CAP: usize = 200;

#[derive(Serialize, Deserialize)]
struct PersistedVenState {
    programs: Vec<OadrProgram>,
    events: Vec<OadrEvent>,
    reports: Vec<serde_json::Value>,
    sensor: SensorSnapshot,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
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
            notifications: Arc::new(RwLock::new(std::collections::VecDeque::new())),
            comfort_overrides: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// WP4.2: install/replace a comfort-curve override for one asset.
    pub async fn set_comfort_override(
        &self,
        asset_id: String,
        rates: Vec<crate::entities::asset::ComfortRate>,
    ) {
        self.comfort_overrides.write().await.insert(asset_id, rates);
    }

    /// WP4.2: drop an override; returns whether one existed.
    pub async fn remove_comfort_override(&self, asset_id: &str) -> bool {
        self.comfort_overrides
            .write()
            .await
            .remove(asset_id)
            .is_some()
    }

    /// WP4.2: snapshot of all overrides (for effective-curve resolution).
    pub async fn comfort_overrides_map(
        &self,
    ) -> std::collections::HashMap<String, Vec<crate::entities::asset::ComfortRate>> {
        self.comfort_overrides.read().await.clone()
    }

    /// WP4.3: append a notification, evicting the oldest past the ring cap.
    pub async fn push_notification(&self, n: crate::entities::notification::UserNotification) {
        let mut ring = self.notifications.write().await;
        if ring.len() >= NOTIFICATION_RING_CAP {
            ring.pop_front();
        }
        ring.push_back(n);
    }

    /// WP4.3: notifications strictly newer than `since` (all when `None`), oldest first.
    pub async fn notifications_since(
        &self,
        since: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Vec<crate::entities::notification::UserNotification> {
        let ring = self.notifications.read().await;
        ring.iter()
            .filter(|n| since.is_none_or(|s| n.created_at > s))
            .cloned()
            .collect()
    }

    pub async fn set_programs(&self, programs: Vec<OadrProgram>) {
        self.polling.write().await.programs = programs;
    }

    pub async fn set_events(&self, mut events: Vec<OadrEvent>, max_keep: usize) {
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

    pub async fn programs(&self) -> Vec<OadrProgram> {
        self.polling.read().await.programs.clone()
    }

    pub async fn events(&self) -> Vec<OadrEvent> {
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
        self.ctrl_sim
            .write()
            .await
            .controller_trace
            .push_event(event);
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

    pub async fn anchor_until(&self) -> Option<DateTime<Utc>> {
        self.hems.read().await.anchor_until
    }

    pub async fn set_anchor_until(&self, t: Option<DateTime<Utc>>) {
        self.hems.write().await.anchor_until = t;
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

    pub async fn alert_windows(&self) -> Vec<AlertWindow> {
        self.hems.read().await.alert_windows.clone()
    }

    pub async fn set_alert_windows(&self, alerts: Vec<AlertWindow>) {
        self.hems.write().await.alert_windows = alerts;
    }

    pub async fn simple_windows(&self) -> Vec<SimpleWindow> {
        self.hems.read().await.simple_windows.clone()
    }

    pub async fn set_simple_windows(&self, windows: Vec<SimpleWindow>) {
        self.hems.write().await.simple_windows = windows;
    }

    pub async fn dispatch_windows(&self) -> Vec<DispatchWindow> {
        self.hems.read().await.dispatch_windows.clone()
    }

    pub async fn set_dispatch_windows(&self, windows: Vec<DispatchWindow>) {
        self.hems.write().await.dispatch_windows = windows;
    }

    pub async fn asset_forecasts(&self) -> Vec<AssetForecast> {
        self.hems.read().await.asset_forecasts.clone()
    }

    pub async fn set_asset_forecasts(&self, forecasts: Vec<AssetForecast>) {
        self.hems.write().await.asset_forecasts = forecasts;
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

    /// Advance a fulfilled obligation to its next cycle. `fulfilled` stays false —
    /// recurrence is driven entirely by `due_at`; `retire_obligations_not_in` below is
    /// what actually stops an obligation, not this flag.
    pub async fn rearm_obligation(&self, id: uuid::Uuid, next_due_at: DateTime<Utc>) {
        let mut hems = self.hems.write().await;
        if let Some(ob) = hems.report_obligations.iter_mut().find(|o| o.id == id) {
            ob.due_at = next_due_at;
        }
    }

    /// Remove obligations whose parent event is no longer in the active poll set.
    pub async fn retire_obligations_not_in(
        &self,
        active_event_ids: &std::collections::HashSet<String>,
    ) {
        let mut hems = self.hems.write().await;
        hems.report_obligations
            .retain(|o| active_event_ids.contains(&o.event_id));
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
                _ => {
                    tracing::warn!(request_id = %id, "cancel_request: unexpected session_type {:?} for request {}", session_type, id);
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
        if w.shiftable_loads
            .iter()
            .any(|l| l.asset_id == load.asset_id)
        {
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
        let state = PersistedVenState {
            programs,
            events,
            reports,
            sensor,
        };
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
            mode: Default::default(),
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
        assert!(
            rt.is_running(now + Duration::minutes(30)),
            "should be running mid-way"
        );
        assert!(
            !rt.is_running(now + Duration::minutes(60)),
            "half-open: not running at ends_at"
        );
        assert!(
            !rt.is_running(now - Duration::seconds(1)),
            "not running before start"
        );
    }

    #[tokio::test]
    async fn add_shiftable_load_rejects_duplicate() {
        let state = AppState::new();
        let load1 = make_load("wm-1");
        let mut load2 = make_load("wm-1");
        load2.id = Uuid::new_v4(); // different id, same asset_id

        assert!(state.add_shiftable_load(load1).await.is_ok());
        assert!(
            state.add_shiftable_load(load2).await.is_err(),
            "duplicate asset_id rejected"
        );
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
        assert!(
            state.shiftable_runtimes().await.is_empty(),
            "runtime removed"
        );
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
        assert!(
            state.shiftable_runtimes().await.is_empty(),
            "runtime also removed"
        );
    }

    fn make_request(
        session_type: Option<SessionType>,
        session_id: Option<uuid::Uuid>,
    ) -> UserRequest {
        let now = Utc::now();
        UserRequest {
            mode: Default::default(),
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
        state
            .set_ev_session(Some(EvSession {
                mode: Default::default(),
                id: session_id,
                target_soc: 0.8,
                departure_time: Utc::now() + chrono::Duration::hours(2),
                soft_deadline: false,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            }))
            .await;

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
        state
            .set_heater_target(Some(HeaterTarget {
                mode: Default::default(),
                id: session_id,
                target_temp_c: 21.0,
                ready_by: Utc::now() + chrono::Duration::hours(1),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            }))
            .await;

        let found = state.cancel_request(req_id).await;
        assert!(found, "cancel should return true");
        assert!(
            state.heater_target().await.is_none(),
            "heater_target cleared"
        );
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
        assert!(
            state.shiftable_runtimes().await.is_empty(),
            "runtime removed"
        );
        let requests = state.active_requests().await;
        assert_eq!(requests[0].status, UserRequestStatus::Cancelled);
    }

    #[tokio::test]
    async fn test_state_persistence_roundtrip() {
        use crate::controller::vtn_port::OadrProgram;
        let state = AppState::new();
        let prog = OadrProgram {
            id: "p1".to_string(),
            programName: "TestProgram".to_string(),
        };
        state.set_programs(vec![prog]).await;
        let json_str = state.to_json().await.unwrap();
        let state2 = AppState::new();
        state2.load_from_json(&json_str).await.unwrap();
        let programs = state2.programs().await;
        assert_eq!(programs.len(), 1);
        assert_eq!(programs[0].id, "p1");
    }

    fn make_obligation(event_id: &str, due_at: DateTime<Utc>) -> OadrReportObligation {
        OadrReportObligation {
            id: Uuid::new_v4(),
            event_id: event_id.to_string(),
            program_id: Some("prog-1".to_string()),
            payload_type: "USAGE".to_string(),
            reading_type: "DIRECT_READ".to_string(),
            resource_name: None,
            due_at,
            interval_duration_s: 900,
            fulfilled: false,
            created_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn rearm_obligation_advances_due_at_and_stays_unfulfilled() {
        let state = AppState::new();
        let now = Utc::now();
        let ob = make_obligation("evt-1", now);
        let id = ob.id;
        state.add_obligations(vec![ob]).await;

        let next_due = now + Duration::seconds(900);
        state.rearm_obligation(id, next_due).await;

        let obs = state.report_obligations().await;
        assert_eq!(obs.len(), 1, "obligation still present, not removed");
        assert_eq!(obs[0].due_at, next_due);
        assert!(
            !obs[0].fulfilled,
            "fulfilled stays false under the recurring model"
        );
    }

    #[tokio::test]
    async fn retire_obligations_not_in_prunes_expired_event() {
        let state = AppState::new();
        let now = Utc::now();
        let keep = make_obligation("evt-active", now);
        let drop = make_obligation("evt-expired", now);
        state.add_obligations(vec![keep, drop]).await;

        let mut active: std::collections::HashSet<String> = std::collections::HashSet::new();
        active.insert("evt-active".to_string());
        state.retire_obligations_not_in(&active).await;

        let obs = state.report_obligations().await;
        assert_eq!(obs.len(), 1);
        assert_eq!(obs[0].event_id, "evt-active");
    }
}
