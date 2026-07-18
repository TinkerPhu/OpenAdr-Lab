use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::entities::plan::{Plan, SolveStatus};
use crate::state::VtnConnectionStatus;
use crate::AppCtx;

#[derive(Serialize)]
pub struct HealthComponent {
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

#[derive(Serialize)]
pub struct HealthComponents {
    ven_process: HealthComponent,
    vtn_connection: HealthComponent,
    storage: HealthComponent,
    planner: HealthComponent,
}

#[derive(Serialize)]
pub struct HealthResponse {
    status: &'static str,
    components: HealthComponents,
}

fn component(ok: bool, detail: Option<String>) -> HealthComponent {
    HealthComponent {
        status: if ok { "ok" } else { "degraded" },
        detail: if ok { None } else { detail },
    }
}

/// Pure assembly of the `/health` response from already-read state — kept separate
/// from the `health` handler so it's unit-testable without constructing a full
/// `AppCtx` (no precedent for that in this codebase; `AppCtx` carries heavy adapter
/// fields like the metrics handle and sim state that a handler-level test shouldn't
/// need to care about).
fn build_health_response(
    vtn: &VtnConnectionStatus,
    storage_ok: bool,
    planner_ok: bool,
) -> HealthResponse {
    let vtn_detail = vtn
        .last_error
        .as_ref()
        .map(|e| format!("backoff {:.1}s; last error: {e}", vtn.current_backoff_s));
    let components = HealthComponents {
        ven_process: component(true, None),
        vtn_connection: component(vtn.connected, vtn_detail),
        storage: component(storage_ok, None),
        planner: component(planner_ok, None),
    };
    let status = if components.vtn_connection.status == "ok"
        && components.storage.status == "ok"
        && components.planner.status == "ok"
    {
        "ok"
    } else {
        "degraded"
    };
    HealthResponse { status, components }
}

/// A missing plan (VEN just started, nothing adopted yet) is not degraded — only
/// an actually-infeasible adopted plan is.
fn plan_is_ok(plan: Option<&Plan>) -> bool {
    !matches!(plan, Some(p) if p.solve_status == SolveStatus::Infeasible)
}

/// WP-T1 (`docs/plans/ven-ui-transparency.md`): componentised health, replacing the
/// previous hardcoded `"ok"` string. HTTP status stays 200 regardless of component
/// status — `ven_process` being reachable at all is the only thing a restart could
/// fix; a VTN outage or infeasible plan is not resolved by restarting the container,
/// so Docker/`fleet.sh` healthchecks (which check the HTTP status only) must not
/// treat those as reasons to cycle the VEN.
pub async fn health(State(ctx): State<AppCtx>) -> Json<HealthResponse> {
    let vtn = ctx.state.vtn_connection_status().await;
    let storage_ok = ctx.state.storage_ok().await;
    let plan = ctx.state.active_plan().await;
    Json(build_health_response(
        &vtn,
        storage_ok,
        plan_is_ok(plan.as_ref()),
    ))
}

#[derive(Serialize)]
pub struct VtnStatusResponse {
    connected: bool,
    last_success_ts: Option<DateTime<Utc>>,
    last_error: Option<String>,
    current_backoff_s: f64,
    token_expires_at: Option<DateTime<Utc>>,
}

fn build_vtn_status_response(
    vtn: VtnConnectionStatus,
    token_expires_at: Option<DateTime<Utc>>,
) -> VtnStatusResponse {
    VtnStatusResponse {
        connected: vtn.connected,
        last_success_ts: vtn.last_success_ts,
        last_error: vtn.last_error,
        current_backoff_s: vtn.current_backoff_s,
        token_expires_at,
    }
}

/// WP-T1: VTN-connection detail — the terse `/health` shape has no room for
/// `token_expires_at`; this endpoint answers "what exactly, in detail."
pub async fn vtn_status(State(ctx): State<AppCtx>) -> Json<VtnStatusResponse> {
    let vtn = ctx.state.vtn_connection_status().await;
    let token_expires_at = ctx.vtn.token_expires_at().await;
    Json(build_vtn_status_response(vtn, token_expires_at))
}

pub async fn get_metrics(State(ctx): State<AppCtx>) -> impl IntoResponse {
    ctx.metrics_handle.render()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy_vtn() -> VtnConnectionStatus {
        VtnConnectionStatus {
            connected: true,
            last_success_ts: Some(Utc::now()),
            last_error: None,
            current_backoff_s: 0.0,
        }
    }

    fn degraded_vtn() -> VtnConnectionStatus {
        VtnConnectionStatus {
            connected: false,
            last_success_ts: None,
            last_error: Some("connection refused".to_string()),
            current_backoff_s: 60.0,
        }
    }

    fn make_plan(solve_status: &str) -> Plan {
        serde_json::from_value(serde_json::json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "created_at": Utc::now().to_rfc3339(),
            "trigger": "PERIODIC",
            "horizon": {
                "start_time": "2026-01-01T00:00:00Z",
                "end_time": "2026-01-02T00:00:00Z",
                "step_size_s": 900,
                "num_steps": 96,
                "far_horizon": "2026-01-02T00:00:00Z"
            },
            "slots": [],
            "summary": {
                "total_cost_eur": 0.0,
                "total_co2_g": 0.0,
                "total_import_kwh": 0.0,
                "total_export_kwh": 0.0
            },
            "envelopes": [],
            "warnings": [],
            "solve_status": solve_status
        }))
        .expect("test Plan must deserialize")
    }

    #[test]
    fn health_reports_degraded_vtn_component_after_failure() {
        let resp = build_health_response(&degraded_vtn(), true, true);
        assert_eq!(resp.status, "degraded");
        assert_eq!(resp.components.vtn_connection.status, "degraded");
        assert!(resp.components.vtn_connection.detail.is_some());
        assert_eq!(resp.components.storage.status, "ok");
        assert_eq!(resp.components.planner.status, "ok");
    }

    #[test]
    fn health_all_ok_when_every_component_healthy() {
        let resp = build_health_response(&healthy_vtn(), true, true);
        assert_eq!(resp.status, "ok");
        assert_eq!(resp.components.ven_process.status, "ok");
        assert_eq!(resp.components.vtn_connection.status, "ok");
        assert!(resp.components.vtn_connection.detail.is_none());
    }

    #[test]
    fn health_storage_degraded_when_storage_not_ok() {
        let resp = build_health_response(&healthy_vtn(), false, true);
        assert_eq!(resp.status, "degraded");
        assert_eq!(resp.components.storage.status, "degraded");
    }

    #[test]
    fn health_planner_component_degraded_when_active_plan_infeasible() {
        assert!(!plan_is_ok(Some(&make_plan("INFEASIBLE"))));
        assert!(plan_is_ok(Some(&make_plan("OPTIMAL"))));
    }

    #[test]
    fn health_planner_component_ok_when_no_active_plan_yet() {
        assert!(plan_is_ok(None), "no plan yet is not a degraded state");
    }

    #[test]
    fn vtn_status_reports_connected_and_last_success() {
        let vtn = healthy_vtn();
        let last_success_ts = vtn.last_success_ts;
        let resp = build_vtn_status_response(vtn, None);
        assert!(resp.connected);
        assert_eq!(resp.last_success_ts, last_success_ts);
        assert_eq!(resp.last_error, None);
    }

    #[test]
    fn vtn_status_reports_backoff_and_last_error_after_failure() {
        let resp = build_vtn_status_response(degraded_vtn(), None);
        assert!(!resp.connected);
        assert_eq!(resp.last_error, Some("connection refused".to_string()));
        assert_eq!(resp.current_backoff_s, 60.0);
    }

    #[test]
    fn vtn_status_carries_token_expires_at_through() {
        let expires_at = Utc::now();
        let resp = build_vtn_status_response(healthy_vtn(), Some(expires_at));
        assert_eq!(resp.token_expires_at, Some(expires_at));
    }
}
