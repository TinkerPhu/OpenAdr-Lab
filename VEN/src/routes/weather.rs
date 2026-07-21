//! `GET /weather` — read-only visibility into the weather-forecast plugin:
//! the most recently received forecast plus, when a `weather_pv` profile
//! section is configured, its derived PV forecast. See
//! docs/plans/weather-forecast-plugin.md and the weather-forecast-visibility
//! OpenSpec change. Computes its own view independently of the planner's PV
//! input (`tasks::planning::spawn_planning`) — both resolve through
//! `entities::solar::resolve_weather_pv_kw`/`weather_pv_forecast_series`
//! (R-50), so the two can't silently diverge, but this route never blocks
//! on or feeds the solve path itself.

use axum::{extract::State, Json};
use chrono::Duration;
use serde::Serialize;

use crate::entities::asset_params::PvForecastParams;
use crate::entities::solar::{weather_pv_forecast_series, WeatherPvForecastSlot};
use crate::entities::weather::WeatherForecast;
use crate::AppCtx;

/// Cached forecasts older than this are still shown (never hidden) but
/// flagged `status: "stale"` — mirrors `WeatherForecast::is_fresh`'s own
/// starting default (see docs/plans/weather-forecast-plugin.md's staleness
/// policy discussion).
const STALENESS_THRESHOLD: Duration = Duration::hours(2);

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WeatherStatus {
    Ok,
    Stale,
    NoForecast,
}

#[derive(Serialize)]
pub struct WeatherResponse {
    status: WeatherStatus,
    is_fresh: bool,
    raw: Option<WeatherForecast>,
    derived: Option<Vec<WeatherPvForecastSlot>>,
}

/// Pure response builder — testable without `AppCtx`, same shape as
/// `system::build_health_response`/`build_vtn_status_response`.
fn build_weather_response(
    forecast: Option<WeatherForecast>,
    pv_params: Option<&PvForecastParams>,
    now: chrono::DateTime<chrono::Utc>,
) -> WeatherResponse {
    let is_fresh = forecast
        .as_ref()
        .is_some_and(|f| f.is_fresh(now, STALENESS_THRESHOLD));
    let status = match &forecast {
        None => WeatherStatus::NoForecast,
        Some(_) if is_fresh => WeatherStatus::Ok,
        Some(_) => WeatherStatus::Stale,
    };
    let derived = match (&forecast, pv_params) {
        (Some(f), Some(params)) => Some(weather_pv_forecast_series(params, f)),
        _ => None,
    };
    WeatherResponse {
        status,
        is_fresh,
        raw: forecast,
        derived,
    }
}

pub async fn get_weather(State(ctx): State<AppCtx>) -> Json<WeatherResponse> {
    let forecast = ctx.weather.latest().await;
    Json(build_weather_response(
        forecast,
        ctx.weather_pv_params.as_ref(),
        chrono::Utc::now(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::asset_params::{PvArrayGeometry, PvSnowParams};
    use crate::entities::weather::{GeoPosition, WeatherForecastSample};
    use chrono::{TimeZone, Utc};

    fn sample_forecast(fetched_at: chrono::DateTime<chrono::Utc>) -> WeatherForecast {
        WeatherForecast {
            source_id: "srf_meteo".into(),
            location: GeoPosition {
                latitude_deg: 47.4491,
                longitude_deg: 7.8081,
            },
            fetched_at,
            samples: vec![WeatherForecastSample {
                valid_at: fetched_at + Duration::hours(1),
                age_h: 1,
                temperature_c: 16.0,
                ghi_w_m2: 300.0,
                wind_speed_kmh: None,
                rain_prob_pct: None,
                new_snowfall_cm: None,
                sky_condition: None,
                irradiance_variability: None,
            }],
        }
    }

    fn sample_params() -> PvForecastParams {
        PvForecastParams {
            rated_kwp: 10.0,
            geometry: PvArrayGeometry {
                location: GeoPosition {
                    latitude_deg: 47.4491,
                    longitude_deg: 7.8081,
                },
                tilt_deg: 30.0,
                azimuth_deg: 180.0,
            },
            performance_ratio: 0.87,
            temp_coeff_pct_per_c: -0.35,
            noct_c: 45.0,
            ac_limit_kw: None,
            snow: PvSnowParams::default(),
        }
    }

    #[test]
    fn fresh_forecast_with_config_returns_ok_with_both_raw_and_derived() {
        let now = Utc.with_ymd_and_hms(2026, 7, 19, 6, 5, 0).unwrap();
        let fetched_at = Utc.with_ymd_and_hms(2026, 7, 19, 5, 54, 48).unwrap();
        let resp = build_weather_response(
            Some(sample_forecast(fetched_at)),
            Some(&sample_params()),
            now,
        );
        assert_eq!(resp.status, WeatherStatus::Ok);
        assert!(resp.is_fresh);
        assert!(resp.raw.is_some());
        assert_eq!(resp.derived.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn stale_forecast_still_returns_raw_but_flags_stale() {
        let now = Utc.with_ymd_and_hms(2026, 7, 19, 6, 0, 0).unwrap();
        let fetched_at = Utc.with_ymd_and_hms(2026, 7, 19, 3, 0, 0).unwrap(); // 3h old
        let resp = build_weather_response(
            Some(sample_forecast(fetched_at)),
            Some(&sample_params()),
            now,
        );
        assert_eq!(resp.status, WeatherStatus::Stale);
        assert!(!resp.is_fresh);
        assert!(
            resp.raw.is_some(),
            "stale forecast must still be shown, not hidden"
        );
    }

    #[test]
    fn no_forecast_returns_null_raw_and_no_forecast_status() {
        let now = Utc.with_ymd_and_hms(2026, 7, 19, 6, 0, 0).unwrap();
        let resp = build_weather_response(None, Some(&sample_params()), now);
        assert_eq!(resp.status, WeatherStatus::NoForecast);
        assert!(resp.raw.is_none());
        assert!(resp.derived.is_none());
    }

    #[test]
    fn forecast_present_without_config_returns_null_derived() {
        let now = Utc.with_ymd_and_hms(2026, 7, 19, 6, 5, 0).unwrap();
        let fetched_at = Utc.with_ymd_and_hms(2026, 7, 19, 5, 54, 48).unwrap();
        let resp = build_weather_response(Some(sample_forecast(fetched_at)), None, now);
        assert_eq!(resp.status, WeatherStatus::Ok);
        assert!(resp.raw.is_some());
        assert!(resp.derived.is_none());
    }
}
