use crate::controller::trace::ControllerTrace;
use crate::controller::vtn_port::{OadrEvent, OadrProgram, OadrReport};
use crate::controller::SimSnapshot;
use crate::entities::asset_ledger::AssetLedgerEntry;
use crate::entities::capacity::{
    AlertWindow, CapacitySnapshot, DispatchWindow, OadrCapacityState, OadrReportObligation,
    SimpleWindow,
};
use crate::entities::capacity_curve::CapacityCurve;
use crate::entities::design_vocabulary::{AssetForecast, AssetHeuristics};
use crate::entities::device_session::{
    BaselineOverride, EvSessionQueue, HeaterTarget, ShiftableLoad,
};
use crate::entities::plan::{Plan, SiteHeadroom, SiteHeadroomSample, SolveStatus};
use crate::entities::sensor::SensorSnapshot;
use crate::entities::user_request::UserRequest;
use crate::entities::{sim_inject::SimInjectState, tariff_snapshot::TariffSnapshot};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};
use tokio::sync::RwLock;

mod arbiter;
mod bounded_log;
mod capacity_curves;
mod connection;
mod diagnostics;
mod ev_sessions;
mod event_log;
mod flexibility_history;
mod grid_signals;
mod heuristics;
mod obligations;
mod openadr_objects;
mod persistence;
mod pv_snow;
mod report_submissions;
mod report_windows;
mod site_headroom_forecast;
mod task_status;
mod user_requests;
mod wire_health;

pub use arbiter::ArbiterDiagnostics;
pub use connection::VtnConnectionStatus;
pub use diagnostics::DiagnosticsState;
pub use event_log::EventLogEntry;
pub use task_status::TaskStatus;

/// R-39: a domain value, so it lives in `entities::ev_settings`. Re-exported here
/// because `HemsState` holds one and the state layer is where callers already look
/// for it; the definition is not duplicated.
pub use crate::entities::ev_settings::EvSettings;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PollingState {
    pub programs: Vec<OadrProgram>,
    pub events: Vec<OadrEvent>,
    /// Full-fidelity typed reports from the VTN (wire shape preserved via
    /// serde flatten) — pass-through for GET /reports and BDD assertions.
    pub reports: Vec<OadrReport>,
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
            sensor: SensorSnapshot::never_sampled(),
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
    /// The Dynamic Operating Envelope schedule (per-interval import/export capacity
    /// limits) — the timeline `capacity_state` collapses into a current-value scalar.
    pub planned_capacity_limits: Vec<CapacitySnapshot>,
    /// WP3.1 (BL-04): active grid-alert windows parsed from ALERT_* events.
    pub alert_windows: Vec<AlertWindow>,
    /// WP3.2: active SIMPLE load-shed windows (levels 1–3).
    pub simple_windows: Vec<SimpleWindow>,
    /// WP3.4: active DISPATCH_SETPOINT windows (direct site-setpoint control).
    pub dispatch_windows: Vec<DispatchWindow>,
    /// WP3.6 (BL-15): per-asset forecasts from the latest plan cycle.
    pub asset_forecasts: Vec<AssetForecast>,
    /// WP5.2 (BL-14): learned per-asset behavioral heuristics, keyed by asset_id.
    pub asset_heuristics: HashMap<String, AssetHeuristics>,
    pub report_obligations: Vec<OadrReportObligation>,
    pub asset_ledger: HashMap<String, AssetLedgerEntry>,
    pub user_requests: Vec<UserRequest>,
    pub site_headroom: Option<SiteHeadroom>,
    pub site_headroom_forecast: Vec<crate::entities::plan::SiteHeadroomForecastSlot>, // state/site_headroom_forecast.rs
    pub capacity_curves: Option<(CapacityCurve, CapacityCurve)>, // (import, export) — state/capacity_curves.rs
    /// Each EV's charging sessions by asset id (W4): ordered, non-overlapping, possibly
    /// several. `EvSessionQueue` owns that invariant per EV - see its doc comment for why the
    /// three producers cannot be trusted with a bare `Vec`.
    pub ev_sessions: BTreeMap<String, EvSessionQueue>,
    /// Each heater's target by asset id (W4); a heater has at most one.
    pub heater_targets: BTreeMap<String, HeaterTarget>,
    pub shiftable_loads: Vec<ShiftableLoad>,
    pub baseline_override: Option<BaselineOverride>,
    /// Each EV's settings by asset id (W4); an EV without a record reads `EvSettings::default`.
    pub ev_settings: BTreeMap<String, EvSettings>,
    /// Deviation-arbiter rollout gate, default `false` (see `state::arbiter`).
    pub deviation_arbiter_enabled: bool,
    /// Arbiter limit-enforcement pass (GB-47), default `true` (see `state::arbiter`).
    pub limit_enforcement_enabled: bool,
    /// The limit pass's previous-tick lever — drives its release hysteresis.
    pub limit_active_lever: Option<String>,
    /// Per-asset absorbed-deviation accumulator, keyed by asset id (§5.5).
    pub arbiter_residual: HashMap<String, crate::entities::arbiter_residual::AssetResidual>,
    /// Cooldown guard for `PlanTrigger::ResidualThreshold` (§5.5/§4).
    pub last_residual_trigger_at: Option<DateTime<Utc>>,
    /// Previous tick's active lever (§4a.1); last tick's arbiter reasoning.
    pub arbiter_active_lever: Option<String>,
    /// The PV asset's own snow-cover conclusion, published each tick (R-55): the starting state
    /// of every weather-sourced PV forecast. See `state/pv_snow.rs`.
    pub pv_snow_state: crate::entities::pv_snow::PvSnowState,
    pub arbiter_diagnostics: arbiter::ArbiterDiagnostics,
}

#[derive(Clone)]
pub struct AppState {
    pub polling: Arc<RwLock<PollingState>>,
    pub ctrl_sim: Arc<RwLock<ControllerSimState>>,
    pub hems: Arc<RwLock<HemsState>>,
    /// WP4.3 (BL-20): bounded log of user-facing notifications.
    pub notifications: bounded_log::BoundedLog<crate::entities::notification::UserNotification>,
    /// WP4.2 (BL-19): per-asset user comfort-curve overrides (hot map;
    /// persisted through SettingsPort, re-seeded at startup).
    pub comfort_overrides:
        Arc<RwLock<std::collections::HashMap<String, Vec<crate::entities::asset::ComfortRate>>>>,
    /// What the VEN knows about its own health: VTN reachability, refused wire
    /// objects, the last persist outcome, per-task restart status. Grouped so the
    /// next observability feed joins a family instead of becoming a fifth flat
    /// field here (R-47) — see `state/diagnostics.rs`.
    pub diagnostics: DiagnosticsState,
    /// WP-T4: VEN-operational event log — deliberately separate from
    /// `notifications` (see `state/event_log.rs`).
    pub event_log: bounded_log::BoundedLog<EventLogEntry>,
    pub event_log_tx: tokio::sync::broadcast::Sender<EventLogEntry>,
    /// D-3: what each report has already told the VTN, so a submission adds to
    /// the series rather than replacing it (`state/report_windows.rs`).
    pub report_windows: Arc<RwLock<report_windows::ReportWindows>>,
    /// Live controller-trace decisions, for anything watching rather than
    /// polling `/trace/events`. The fleet trace publisher is the first such
    /// subscriber (phase 0 §6.2); the ring above remains the record.
    ///
    /// A broadcast, not a direct call into the publisher: a decision the
    /// controller makes is a fact about the controller, and it must not become
    /// slower or fallible because something is listening to it.
    pub controller_trace_tx:
        tokio::sync::broadcast::Sender<crate::controller::trace::ControllerEvent>,
    /// WP-T5 (G-5): bounded ring of report submission outcomes, newest last.
    pub report_submissions:
        bounded_log::BoundedLog<crate::entities::report_submission::ReportSubmissionRecord>,
    /// BL-43: bounded log of `SiteHeadroom` snapshots (`site_headroom`
    /// holds only the latest).
    pub flexibility_history: bounded_log::BoundedLog<SiteHeadroomSample>,
}

/// WP4.3: in-memory notification ring capacity (mirrors the /trace/events ring).
pub const NOTIFICATION_RING_CAP: usize = 200;

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
                limit_enforcement_enabled: true,
                ..HemsState::default()
            })),
            notifications: bounded_log::BoundedLog::new(NOTIFICATION_RING_CAP),
            comfort_overrides: Arc::new(RwLock::new(std::collections::HashMap::new())),
            diagnostics: DiagnosticsState::new(),
            event_log: bounded_log::BoundedLog::new(event_log::EVENT_LOG_RING_CAP),
            event_log_tx: tokio::sync::broadcast::channel(64).0,
            report_windows: Arc::new(RwLock::new(Default::default())),
            controller_trace_tx: tokio::sync::broadcast::channel(64).0,
            report_submissions: bounded_log::BoundedLog::new(
                report_submissions::REPORT_SUBMISSION_RING_CAP,
            ),
            flexibility_history: bounded_log::BoundedLog::new(
                flexibility_history::FLEXIBILITY_HISTORY_RING_CAP,
            ),
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
        self.notifications.record(n).await;
    }

    /// 030 (notification-dedup): find the newest ring entry carrying this
    /// dedup key whose `last_seen_at` is within `window` of `now`; bump its
    /// `count`/`last_seen_at` in place and return the updated clone. `None`
    /// when no entry qualifies (caller creates a fresh notification).
    pub async fn bump_notification_seen(
        &self,
        dedup_key: &str,
        now: chrono::DateTime<chrono::Utc>,
        window: chrono::Duration,
    ) -> Option<crate::entities::notification::UserNotification> {
        self.notifications
            .bump_newest_where(
                |n| {
                    n.dedup_key.as_deref() == Some(dedup_key)
                        && now.signed_duration_since(n.last_seen_at) <= window
                },
                |n| {
                    n.count += 1;
                    n.last_seen_at = now;
                },
            )
            .await
    }

    /// WP4.3: notifications strictly newer than `since` (all when `None`), oldest first.
    pub async fn notifications_since(
        &self,
        since: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Vec<crate::entities::notification::UserNotification> {
        self.notifications
            .oldest_first_where(|n| since.is_none_or(|s| n.created_at > s))
            .await
    }

    pub async fn update_sensor(&self, sensor: SensorSnapshot) {
        self.ctrl_sim.write().await.sensor = sensor;
    }

    pub async fn update_sim(&self, sim: SimSnapshot) {
        self.ctrl_sim.write().await.sim = Some(sim);
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
        // Broadcast is fire-and-forget and ignores "no subscribers": the ring
        // is the record, and a listener's absence is not the controller's
        // problem.
        let _ = self.controller_trace_tx.send(event.clone());
        self.ctrl_sim
            .write()
            .await
            .controller_trace
            .push_event(event);
    }

    /// Subscribe to controller decisions as they are made.
    pub fn subscribe_controller_trace(
        &self,
    ) -> tokio::sync::broadcast::Receiver<crate::controller::trace::ControllerEvent> {
        self.controller_trace_tx.subscribe()
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

    /// The adopted plan's solve status alone — for pollers (`/health`) that need one enum
    /// and must not deep-clone the whole `Plan` to read it.
    pub async fn active_plan_solve_status(&self) -> Option<SolveStatus> {
        self.hems
            .read()
            .await
            .active_plan
            .as_ref()
            .map(|p| p.solve_status)
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

    // Tariff/capacity-limit/alert/SIMPLE/dispatch-window accessors moved to
    // `state/grid_signals.rs` (R-40 proactive split, see that file's doc comment).

    pub async fn asset_forecasts(&self) -> Vec<AssetForecast> {
        self.hems.read().await.asset_forecasts.clone()
    }

    pub async fn set_asset_forecasts(&self, forecasts: Vec<AssetForecast>) {
        self.hems.write().await.asset_forecasts = forecasts;
    }

    pub async fn asset_ledger(&self) -> HashMap<String, AssetLedgerEntry> {
        self.hems.read().await.asset_ledger.clone()
    }

    pub async fn set_asset_ledger(&self, ledger: HashMap<String, AssetLedgerEntry>) {
        self.hems.write().await.asset_ledger = ledger;
    }

    pub async fn site_headroom(&self) -> Option<SiteHeadroom> {
        self.hems.read().await.site_headroom.clone()
    }

    pub async fn set_site_headroom(&self, env: SiteHeadroom) {
        self.record_flexibility_sample(&env).await;
        self.hems.write().await.site_headroom = Some(env);
    }

    /// The target of heater `asset_id`, if it has one.
    pub async fn heater_target(&self, asset_id: &str) -> Option<HeaterTarget> {
        self.hems.read().await.heater_targets.get(asset_id).cloned()
    }

    /// Every heater's target, by asset id.
    pub async fn heater_targets(&self) -> BTreeMap<String, HeaterTarget> {
        self.hems.read().await.heater_targets.clone()
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
        w.shiftable_loads.len() < before
    }

    pub async fn baseline_override(&self) -> Option<BaselineOverride> {
        self.hems.read().await.baseline_override.clone()
    }

    pub async fn set_baseline_override(&self, ovr: Option<BaselineOverride>) {
        self.hems.write().await.baseline_override = ovr;
    }

    /// EV `asset_id`'s settings, `EvSettings::default` until they are first set.
    pub async fn ev_settings(&self, asset_id: &str) -> EvSettings {
        self.hems
            .read()
            .await
            .ev_settings
            .get(asset_id)
            .cloned()
            .unwrap_or_default()
    }

    pub async fn set_ev_settings(&self, asset_id: &str, s: EvSettings) {
        self.hems
            .write()
            .await
            .ev_settings
            .insert(asset_id.to_string(), s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::device_session::ShiftableLoad;
    use crate::entities::user_request::{SessionType, UserRequestStatus};
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
    async fn complete_shiftable_removes_the_load_and_completes_its_request() {
        let state = AppState::new();
        let load = make_load("wm-1");
        let load_id = load.id;
        state.add_shiftable_load(load.clone()).await.unwrap();
        let req = make_request(Some(SessionType::ShiftableLoad), Some(load_id));
        let req_id = req.id;
        state.upsert_request(req).await;

        let now = "2026-10-06T12:00:00Z".parse().unwrap();
        let completed = state.complete_shiftable(load_id, now).await;

        assert_eq!(
            completed.map(|r| r.id),
            Some(req_id),
            "returns what it completed"
        );
        assert!(state.shiftable_loads().await.is_empty(), "load removed");
        let requests = state.user_requests().await;
        let done = requests.iter().find(|r| r.id == req_id).unwrap();
        assert_eq!(done.status, UserRequestStatus::Completed);
        assert_eq!(done.updated_at, now, "stamped with the caller's clock");
    }

    #[tokio::test]
    async fn remove_shiftable_load_removes_it() {
        let state = AppState::new();
        let load = make_load("wm-1");
        let load_id = load.id;
        state.add_shiftable_load(load.clone()).await.unwrap();

        let removed = state.remove_shiftable_load(load_id).await;

        assert!(removed, "remove should return true");
        assert!(state.shiftable_loads().await.is_empty(), "load removed");
    }

    fn make_request(
        session_type: Option<SessionType>,
        session_id: Option<uuid::Uuid>,
    ) -> UserRequest {
        UserRequest {
            session_id,
            session_type,
            ..UserRequest::for_test("test", Utc::now())
        }
    }

    /// An EV session for `[from, to)` hours from now, owned by `id`.
    fn ev_sess(id: Uuid, from: i64, to: i64) -> crate::entities::device_session::EvSession {
        let now = Utc::now();
        crate::entities::device_session::EvSession {
            asset_id: "ev".into(),
            mode: Default::default(),
            origin: crate::entities::device_session::EvSessionOrigin::UserRequest,
            id,
            target_soc_frac: 0.8,
            window_start: now + Duration::hours(from),
            expected_trip_distance_km: None,
            expected_return_time: None,
            departure_time: now + Duration::hours(to),
            soft_deadline: false,
            budget_eur: None,
            comfort_rates: vec![],
            created_at: now,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn replace_ev_sessions_cancels_the_requests_it_displaced() {
        // A displaced plan's request must go with it. Leaving it Active would put a
        // commitment in the list pointing at a session that no longer exists, which
        // GET /user-requests renders as a request with no session at all.
        let state = AppState::new();
        let (standing, untouched) = (Uuid::new_v4(), Uuid::new_v4());
        state
            .insert_ev_session(ev_sess(standing, 0, 6))
            .await
            .unwrap();
        state
            .insert_ev_session(ev_sess(untouched, 12, 18))
            .await
            .unwrap();
        let standing_req = make_request(Some(SessionType::Ev), Some(standing));
        let untouched_req = make_request(Some(SessionType::Ev), Some(untouched));
        let (standing_req_id, untouched_req_id) = (standing_req.id, untouched_req.id);
        state.upsert_request(standing_req).await;
        state.upsert_request(untouched_req).await;

        let spontaneous = Uuid::new_v4();
        let removed = state
            .replace_ev_sessions(&[standing], ev_sess(spontaneous, 3, 9), Utc::now())
            .await
            .expect("naming exactly the conflict set is accepted");
        assert_eq!(removed.len(), 1);

        let requests = state.user_requests().await;
        let status = |id: Uuid| {
            requests
                .iter()
                .find(|r| r.id == id)
                .map(|r| r.status.clone())
                .expect("request still listed")
        };
        assert_eq!(status(standing_req_id), UserRequestStatus::Cancelled);
        assert_eq!(
            status(untouched_req_id),
            UserRequestStatus::Active,
            "a request whose session was never displaced is untouched"
        );
    }

    #[tokio::test]
    async fn replace_ev_sessions_refused_leaves_requests_and_queue_alone() {
        let state = AppState::new();
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        state.insert_ev_session(ev_sess(a, 0, 6)).await.unwrap();
        state.insert_ev_session(ev_sess(b, 6, 12)).await.unwrap();
        let req = make_request(Some(SessionType::Ev), Some(a));
        let req_id = req.id;
        state.upsert_request(req).await;

        // Spans both, names only one: a partial instruction.
        let refusal = state
            .replace_ev_sessions(&[a], ev_sess(Uuid::new_v4(), 3, 9), Utc::now())
            .await
            .expect_err("a partial instruction must be refused");
        // The refusal carries the current conflict set so a re-prompt is truthful.
        assert_eq!(refusal.conflicts.len(), 2);

        assert_eq!(state.ev_sessions_of("ev").await.len(), 2, "queue untouched");
        let requests = state.user_requests().await;
        assert_eq!(
            requests.iter().find(|r| r.id == req_id).unwrap().status,
            UserRequestStatus::Active,
            "a refused replace cancels nothing"
        );
    }

    #[tokio::test]
    async fn cancel_request_ev_removes_only_that_requests_session() {
        // The correctness fix the single slot made impossible: three queued
        // sessions, cancel the middle one's request, the other two stay.
        let state = AppState::new();
        let (a, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        for (sid, from, to) in [(a, 0, 2), (b, 3, 5), (c, 6, 8)] {
            state
                .insert_ev_session(ev_sess(sid, from, to))
                .await
                .expect("non-overlapping windows must all queue");
        }
        let req = make_request(Some(SessionType::Ev), Some(b));
        let req_id = req.id;
        state.upsert_request(req).await;

        assert!(
            state.cancel_request(req_id, Utc::now()).await,
            "cancel returns true"
        );

        let left: Vec<_> = state
            .ev_sessions_of("ev")
            .await
            .iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(
            left,
            vec![a, c],
            "only the cancelled request's session goes"
        );
        let requests = state.user_requests().await;
        assert_eq!(requests[0].status, UserRequestStatus::Cancelled);
    }

    #[tokio::test]
    async fn insert_ev_session_refuses_an_overlap_and_keeps_the_queue() {
        let state = AppState::new();
        let first = Uuid::new_v4();
        state.insert_ev_session(ev_sess(first, 0, 4)).await.unwrap();

        let err = state
            .insert_ev_session(ev_sess(Uuid::new_v4(), 2, 6))
            .await
            .expect_err("an overlapping window must be refused");
        // `conflicts` now carries the clashing sessions themselves, not their ids, so
        // a caller can describe the clash without reading the queue a second time.
        // The assertion is unchanged in intent: exactly that one session clashed.
        assert_eq!(
            err.conflicts.iter().map(|s| s.id).collect::<Vec<_>>(),
            vec![first]
        );
        assert_eq!(
            state.ev_sessions_of("ev").await.len(),
            1,
            "nothing was added"
        );
    }

    #[tokio::test]
    async fn current_ev_session_is_window_based_not_queue_emptiness() {
        let state = AppState::new();
        let now = Utc::now();
        let live = Uuid::new_v4();
        state.insert_ev_session(ev_sess(live, -1, 1)).await.unwrap();
        state
            .insert_ev_session(ev_sess(Uuid::new_v4(), 5, 7))
            .await
            .unwrap();

        assert_eq!(
            state.current_ev_session("ev", now).await.map(|s| s.id),
            Some(live)
        );
        // Two sessions queued, yet nothing is current in the gap between them -
        // which is what keeps opportunistic charging from pausing forever.
        assert!(
            state
                .current_ev_session("ev", now + Duration::hours(3))
                .await
                .is_none(),
            "the gap between sessions has no current session"
        );
    }

    #[tokio::test]
    async fn settle_requests_drops_several_passed_ev_sessions() {
        let state = AppState::new();
        let live = Uuid::new_v4();
        for (sid, from, to) in [
            (Uuid::new_v4(), -6, -4),
            (Uuid::new_v4(), -3, -1),
            (live, 2, 4),
        ] {
            state
                .insert_ev_session(ev_sess(sid, from, to))
                .await
                .unwrap();
        }

        assert!(
            state.settle_requests(Utc::now()).await.is_empty(),
            "no request owned them"
        );
        let left: Vec<_> = state
            .ev_sessions_of("ev")
            .await
            .iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(left, vec![live]);
    }

    /// W4: each EV has its own queue, so overlapping sessions of two EVs are no conflict.
    #[tokio::test]
    async fn sessions_of_two_evs_do_not_conflict() {
        let state = AppState::new();
        let for_ev = |asset_id: &str| crate::entities::device_session::EvSession {
            asset_id: asset_id.into(),
            ..ev_sess(Uuid::new_v4(), 0, 4)
        };
        state.insert_ev_session(for_ev("ev-a")).await.unwrap();
        state
            .insert_ev_session(for_ev("ev-b"))
            .await
            .expect("another EV's window is no clash");
        assert_eq!(state.ev_sessions_of("ev-a").await.len(), 1);
        assert_eq!(state.ev_sessions_of("ev-b").await.len(), 1);
        assert!(
            state.insert_ev_session(for_ev("ev-a")).await.is_err(),
            "the same EV still cannot be in two sessions at once"
        );
    }

    /// W4: cancelling a request removes its session from its own EV and no other.
    #[tokio::test]
    async fn cancel_removes_the_session_from_its_own_ev_only() {
        let state = AppState::new();
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let session = |id: Uuid, asset_id: &str| crate::entities::device_session::EvSession {
            asset_id: asset_id.into(),
            ..ev_sess(id, 0, 4)
        };
        state.insert_ev_session(session(a, "ev-a")).await.unwrap();
        state.insert_ev_session(session(b, "ev-b")).await.unwrap();
        let req = make_request(Some(SessionType::Ev), Some(a));
        let req_id = req.id;
        state.upsert_request(req).await;
        assert!(state.cancel_request(req_id, Utc::now()).await);
        assert!(state.ev_sessions_of("ev-a").await.is_empty());
        assert_eq!(state.ev_sessions_of("ev-b").await.len(), 1);
    }

    #[tokio::test]
    async fn remove_ev_session_returns_the_session_it_removed() {
        let state = AppState::new();
        let id = Uuid::new_v4();
        state.insert_ev_session(ev_sess(id, 0, 2)).await.unwrap();

        assert_eq!(state.remove_ev_session(id).await.map(|s| s.id), Some(id));
        assert!(state.ev_sessions_of("ev").await.is_empty());
        assert!(
            state.remove_ev_session(id).await.is_none(),
            "removing twice is not an error"
        );
    }

    fn heater_target(id: Uuid, ready_in_h: i64) -> HeaterTarget {
        HeaterTarget {
            mode: Default::default(),
            id,
            asset_id: "heater".into(),
            target_temp_c: 21.0,
            ready_by: Utc::now() + Duration::hours(ready_in_h),
            comfort_rates: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    /// R-137: an EV request ends when its session's departure passes. It used to stay active,
    /// and overdue, for the life of the VEN.
    #[tokio::test]
    async fn settle_requests_completes_the_request_of_a_passed_ev_session() {
        let state = AppState::new();
        let (passed, live) = (Uuid::new_v4(), Uuid::new_v4());
        state
            .insert_ev_session(ev_sess(passed, -3, -1))
            .await
            .unwrap();
        state.insert_ev_session(ev_sess(live, 2, 4)).await.unwrap();
        let passed_req = make_request(Some(SessionType::Ev), Some(passed));
        let live_req = make_request(Some(SessionType::Ev), Some(live));
        state.upsert_request(passed_req.clone()).await;
        state.upsert_request(live_req.clone()).await;

        let now = Utc::now();
        let completed = state.settle_requests(now).await;

        assert_eq!(
            completed.iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![passed_req.id]
        );
        let requests = state.user_requests().await;
        assert_eq!(requests[0].status, UserRequestStatus::Completed);
        assert_eq!(requests[0].updated_at, now, "stamped when it ended");
        assert_eq!(requests[1].status, UserRequestStatus::Active);
        assert!(
            state.settle_requests(now).await.is_empty(),
            "a request completes once"
        );
    }

    /// R-137: a heater target ends at its ready-by, like an EV session at its departure.
    #[tokio::test]
    async fn settle_requests_clears_a_heater_target_past_its_ready_by_and_completes_its_request() {
        let state = AppState::new();
        let session_id = Uuid::new_v4();
        let req = make_request(Some(SessionType::Heater), Some(session_id));
        state.upsert_request(req).await;
        state
            .replace_heater_target(heater_target(session_id, -1), Utc::now())
            .await;

        let completed = state.settle_requests(Utc::now()).await;

        assert_eq!(completed.len(), 1);
        assert!(state.heater_target("heater").await.is_none());
        assert_eq!(
            state.user_requests().await[0].status,
            UserRequestStatus::Completed
        );
    }

    #[tokio::test]
    async fn settle_requests_keeps_a_heater_target_still_ahead() {
        let state = AppState::new();
        let session_id = Uuid::new_v4();
        state
            .upsert_request(make_request(Some(SessionType::Heater), Some(session_id)))
            .await;
        state
            .replace_heater_target(heater_target(session_id, 1), Utc::now())
            .await;

        assert!(state.settle_requests(Utc::now()).await.is_empty());
        assert!(state.heater_target("heater").await.is_some());
    }

    /// R-137: finished requests do not pile up for the life of the VEN.
    #[tokio::test]
    async fn settle_requests_prunes_finished_requests_past_their_retention() {
        use crate::entities::user_request::FINISHED_REQUESTS_KEPT;
        let state = AppState::new();
        let long_ago = Utc::now() - Duration::days(30);
        for _ in 0..FINISHED_REQUESTS_KEPT + 7 {
            let mut req = make_request(None, None);
            req.finish(UserRequestStatus::Cancelled, long_ago);
            state.upsert_request(req).await;
        }
        let active = make_request(None, None);
        state.upsert_request(active.clone()).await;

        state.settle_requests(Utc::now()).await;

        let requests = state.user_requests().await;
        assert_eq!(requests.len(), FINISHED_REQUESTS_KEPT + 1);
        assert!(requests.iter().any(|r| r.id == active.id));
    }

    /// R-137: the heater has one target, so a second heater request ends the first. The first
    /// used to stay active, pointing at a target that was gone.
    #[tokio::test]
    async fn replace_heater_target_cancels_the_request_that_owned_the_previous_target() {
        let state = AppState::new();
        let (first, second) = (Uuid::new_v4(), Uuid::new_v4());
        let first_req = make_request(Some(SessionType::Heater), Some(first));
        state.upsert_request(first_req.clone()).await;
        assert!(state
            .replace_heater_target(heater_target(first, 1), Utc::now())
            .await
            .is_none());

        let displaced = state
            .replace_heater_target(heater_target(second, 2), Utc::now())
            .await;

        assert_eq!(displaced.map(|r| r.id), Some(first_req.id));
        assert_eq!(
            state.user_requests().await[0].status,
            UserRequestStatus::Cancelled
        );
        assert_eq!(
            state.heater_target("heater").await.map(|t| t.id),
            Some(second)
        );
        // Cancelling the displaced request again must not clear the newer request's target.
        state.cancel_request(first_req.id, Utc::now()).await;
        assert_eq!(
            state.heater_target("heater").await.map(|t| t.id),
            Some(second)
        );
    }

    /// The tick's accounting changes the list in place: a request submitted while the tick
    /// held a copy used to be overwritten when the copy was written back.
    #[tokio::test]
    async fn account_tick_changes_requests_in_place() {
        let state = AppState::new();
        state.upsert_request(make_request(None, None)).await;
        state
            .account_tick(|ledger, requests| {
                requests[0].accumulated_cost_eur = 1.5;
                ledger.insert("ev".into(), Default::default());
            })
            .await;
        assert_eq!(state.user_requests().await[0].accumulated_cost_eur, 1.5);
        assert!(state.asset_ledger().await.contains_key("ev"));
    }

    #[tokio::test]
    async fn cancel_request_heater_clears_target() {
        let state = AppState::new();
        let session_id = Uuid::new_v4();
        let req = make_request(Some(SessionType::Heater), Some(session_id));
        let req_id = req.id;
        state.upsert_request(req).await;
        state
            .replace_heater_target(heater_target(session_id, 1), Utc::now())
            .await;

        let found = state.cancel_request(req_id, Utc::now()).await;
        assert!(found, "cancel should return true");
        assert!(
            state.heater_target("heater").await.is_none(),
            "heater_target cleared"
        );
        let requests = state.user_requests().await;
        assert_eq!(requests[0].status, UserRequestStatus::Cancelled);
    }

    #[tokio::test]
    async fn cancel_request_shiftable_removes_load() {
        let state = AppState::new();
        let load = make_load("wm-1");
        let load_id = load.id;
        state.add_shiftable_load(load.clone()).await.unwrap();

        let req = make_request(Some(SessionType::ShiftableLoad), Some(load_id));
        let req_id = req.id;
        state.upsert_request(req).await;

        let found = state.cancel_request(req_id, Utc::now()).await;
        assert!(found, "cancel should return true");
        assert!(state.shiftable_loads().await.is_empty(), "load removed");
        let requests = state.user_requests().await;
        assert_eq!(requests[0].status, UserRequestStatus::Cancelled);
    }

    #[tokio::test]
    async fn test_state_persistence_roundtrip() {
        use crate::controller::vtn_port::OadrProgram;
        let state = AppState::new();
        let prog = OadrProgram {
            id: "p1".to_string(),
            programName: "TestProgram".to_string(),
            ..Default::default()
        };
        state.set_programs(vec![prog]).await;
        let json_str = state.to_json().await.unwrap();
        let state2 = AppState::new();
        state2.load_from_json(&json_str).await.unwrap();
        let programs = state2.programs().await;
        assert_eq!(programs.len(), 1);
        assert_eq!(programs[0].id, "p1");
    }

    /// A state file written by an older build can carry a section this build
    /// no longer accepts. Losing that section is unavoidable; losing the ones
    /// beside it is not, and used to happen because the whole file was parsed
    /// as one value.
    ///
    /// This is the same failure that `controller::wire_reject` exists for, on
    /// our own data instead of a peer's: one unusable object costing everything
    /// around it.
    #[tokio::test]
    async fn load_from_json_keeps_the_sections_it_can_still_read() {
        let state = AppState::new();
        // `events` here is shaped like a state file from a build whose event
        // type had different requirements -- unreadable now.
        let stale = serde_json::json!({
            "programs": [{"id": "p1", "programName": "TestProgram"}],
            "events": [{"totally": "unexpected", "shape": 42}],
            "reports": [],
            "sensor": {
                "id": "00000000-0000-0000-0000-000000000001",
                "ts": "2026-01-01T00:00:00Z",
                "temperature_c": 21.5,
                "power_kw": null,
                "voltage_v": null,
                "raw": {}
            }
        })
        .to_string();

        state.load_from_json(&stale).await.expect("partial restore");

        let programs = state.programs().await;
        assert_eq!(programs.len(), 1, "programs must survive unreadable events");
        assert_eq!(programs[0].id, "p1");
        assert!(
            state.events().await.is_empty(),
            "the unreadable section is dropped"
        );
        assert_eq!(
            state.sensor().await.temperature_c,
            Some(21.5),
            "the sensor reading must survive too — it sits beside the events, \
             not inside them"
        );
    }

    /// Only a file that is not JSON at all is an error worth failing on.
    #[tokio::test]
    async fn load_from_json_errors_only_when_the_file_is_not_json() {
        let state = AppState::new();
        assert!(state.load_from_json("this is not json").await.is_err());
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
            interval_width_s: 900,
            submit_every_s: 900,
            fulfilled: false,
            created_at: Utc::now(),
            historical: true,
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
