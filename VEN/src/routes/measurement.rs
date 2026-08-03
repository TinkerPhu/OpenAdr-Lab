//! `GET /measurement` — read-only visibility into the real-measurement MQTT
//! feeds (real-measurement-mqtt): PV power and baseline-load power. Mirrors
//! `routes/weather.rs`'s shape (raw/derived split, `source_alive` vs
//! content-freshness), but simpler: a measurement has no forecast series to
//! derive, just a single live reading per signal.

use axum::{extract::State, Json};
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::entities::measurement::{resolve_measured_kw, MEASUREMENT_STALENESS_THRESHOLD};
use crate::AppCtx;

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementStatus {
    Ok,
    Stale,
    Disabled,
    NotConfigured,
}

#[derive(Serialize)]
pub struct SignalResponse {
    status: MeasurementStatus,
    is_fresh: bool,
    /// Transport heartbeat: whether the configured source has been heard
    /// from recently. `false` when no MQTT adapter is configured at all.
    source_alive: bool,
    raw_kw: Option<f64>,
    raw_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
pub struct MeasurementResponse {
    pv: SignalResponse,
    base_load: SignalResponse,
}

/// Pure per-signal response builder — testable without `AppCtx`.
fn build_signal_response(
    latest: Option<(f64, DateTime<Utc>)>,
    enabled: bool,
    source_alive: bool,
    now: DateTime<Utc>,
) -> SignalResponse {
    if !enabled {
        return SignalResponse {
            status: MeasurementStatus::Disabled,
            is_fresh: false,
            source_alive,
            raw_kw: None,
            raw_at: None,
        };
    }
    let is_fresh = resolve_measured_kw(latest, now, MEASUREMENT_STALENESS_THRESHOLD).is_some();
    let status = match &latest {
        None => MeasurementStatus::NotConfigured,
        Some(_) if is_fresh => MeasurementStatus::Ok,
        Some(_) => MeasurementStatus::Stale,
    };
    let (raw_kw, raw_at) = match latest {
        Some((kw, at)) => (Some(kw), Some(at)),
        None => (None, None),
    };
    SignalResponse {
        status,
        is_fresh,
        source_alive,
        raw_kw,
        raw_at,
    }
}

pub async fn get_measurement(State(ctx): State<AppCtx>) -> Json<MeasurementResponse> {
    let now = Utc::now();
    let pv_latest = ctx.pv_measurement.latest_kw().await;
    let pv = build_signal_response(
        pv_latest,
        ctx.pv_measurement_enabled,
        ctx.pv_measurement.is_alive(),
        now,
    );
    let base_load_latest = ctx.base_load_measurement.latest_kw().await;
    let base_load = build_signal_response(
        base_load_latest,
        ctx.base_load_measurement_enabled,
        ctx.base_load_measurement.is_alive(),
        now,
    );
    Json(MeasurementResponse { pv, base_load })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 3, h, m, 0).unwrap()
    }

    #[test]
    fn disabled_signal_returns_disabled_status_regardless_of_reading() {
        let resp = build_signal_response(Some((3.5, t(12, 0))), false, true, t(12, 1));
        assert_eq!(resp.status, MeasurementStatus::Disabled);
        assert!(resp.raw_kw.is_none());
    }

    #[test]
    fn enabled_with_no_reading_returns_not_configured() {
        let resp = build_signal_response(None, true, false, t(12, 0));
        assert_eq!(resp.status, MeasurementStatus::NotConfigured);
    }

    #[test]
    fn enabled_fresh_reading_returns_ok_with_raw() {
        let resp = build_signal_response(Some((3.5, t(12, 0))), true, true, t(12, 1));
        assert_eq!(resp.status, MeasurementStatus::Ok);
        assert!(resp.is_fresh);
        assert_eq!(resp.raw_kw, Some(3.5));
    }

    #[test]
    fn enabled_stale_reading_still_shown_but_flagged() {
        let resp = build_signal_response(Some((3.5, t(12, 0))), true, true, t(12, 10));
        assert_eq!(resp.status, MeasurementStatus::Stale);
        assert!(!resp.is_fresh);
        assert!(resp.raw_kw.is_some(), "stale reading must still be shown");
    }

    #[test]
    fn source_alive_reflects_passed_in_flag_independent_of_freshness() {
        let resp = build_signal_response(Some((3.5, t(12, 0))), true, false, t(12, 1));
        assert_eq!(resp.status, MeasurementStatus::Ok);
        assert!(!resp.source_alive);
    }
}
