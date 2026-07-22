//! Solar position + PV transposition physics — see
//! `docs/architecture/weather_forecast.md` ("The transposition problem")
//! for the full derivation. Pure math, no I/O, no `crate::profile` import.
//!
//! `resolve_weather_pv_kw` (below) is the single entry point both consumers
//! share: `GET /weather` (`routes::weather`, read-only diagnostic) and the
//! planner's own PV input (`tasks::planning::spawn_planning` →
//! `SolveRequest.weather_pv_kw` → `controller::milp_planner::inputs::build_milp_inputs`,
//! R-50) both resolve through the same staleness-gated function, so the two
//! can never silently diverge on what a `WeatherForecast` implies for PV
//! output.

use chrono::{DateTime, Datelike, Timelike, Utc};

use crate::entities::asset_params::{PvArrayGeometry, PvForecastParams};
use crate::entities::pv_snow::{snow_coverage_trajectory, PvSnowState};
use crate::entities::weather::{GeoPosition, WeatherForecast, WeatherForecastSample};

/// Sun position at a given instant and location.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolarPosition {
    pub elevation_deg: f64,
    pub azimuth_deg: f64,
}

fn julian_date(t: DateTime<Utc>) -> f64 {
    // Standard low-precision solar-position formula (accurate to a fraction
    // of a degree — plenty for hourly PV forecasting).
    let year = t.year() as f64;
    let month = t.month() as f64;
    let day = t.day() as f64
        + t.hour() as f64 / 24.0
        + t.minute() as f64 / 1440.0
        + t.second() as f64 / 86400.0;
    let (y, m) = if month <= 2.0 {
        (year - 1.0, month + 12.0)
    } else {
        (year, month)
    };
    let a = (y / 100.0).floor();
    let b = 2.0 - a + (a / 4.0).floor();
    (365.25 * (y + 4716.0)).floor() + (30.6001 * (m + 1.0)).floor() + day + b - 1524.5
}

/// Solar elevation/azimuth at time `t` for a given geo position.
pub fn solar_position(pos: &GeoPosition, t: DateTime<Utc>) -> SolarPosition {
    let n = julian_date(t) - 2451545.0;
    let l = 280.460 + 0.9856474 * n;
    let g = (357.528 + 0.9856003 * n).to_radians();
    let lambda_sun = l + 1.915 * g.sin() + 0.020 * (2.0 * g).sin();
    let epsilon = (23.439 - 0.0000004 * n).to_radians();
    let lambda_rad = lambda_sun.to_radians();
    let lat_rad = pos.latitude_deg.to_radians();

    let alpha = (epsilon.cos() * lambda_rad.sin()).atan2(lambda_rad.cos());
    let delta = (epsilon.sin() * lambda_rad.sin()).asin();
    let theta = ((280.46061837 + 360.98564736629 * n + pos.longitude_deg) % 360.0).to_radians();
    let h_angle = theta - alpha;

    let elevation = (lat_rad.sin() * delta.sin() + lat_rad.cos() * delta.cos() * h_angle.cos())
        .asin()
        .to_degrees();
    let azimuth_raw =
        (-h_angle.sin()).atan2(delta.tan() * lat_rad.cos() - lat_rad.sin() * h_angle.cos());
    let azimuth = azimuth_raw.to_degrees();
    let azimuth = if azimuth < 0.0 {
        azimuth + 360.0
    } else {
        azimuth
    };

    SolarPosition {
        elevation_deg: elevation,
        azimuth_deg: azimuth,
    }
}

fn unit_vector(elevation_deg: f64, azimuth_deg: f64) -> (f64, f64, f64) {
    let e = elevation_deg.to_radians();
    let a = azimuth_deg.to_radians();
    (e.cos() * a.cos(), e.cos() * a.sin(), e.sin())
}

/// Kasten-Young air mass from solar elevation (dimensionless; large near
/// the horizon, ~1.0 at zenith). Returns `f64::INFINITY`-like large value
/// when the sun is below the horizon — callers must gate on elevation > 0.
fn air_mass(elevation_deg: f64) -> f64 {
    1.0 / (elevation_deg.to_radians().sin() + 0.50572 * (6.07995 + elevation_deg).powf(-1.6364))
}

/// Empirical diffuse-fraction curve (0..~0.27), from zenith angle.
fn diffuse_fraction(elevation_deg: f64) -> f64 {
    let zenith_deg = 90.0 - elevation_deg;
    0.271 - 0.294 * (-0.036 * zenith_deg).exp()
}

/// Clear-sky irradiance model, run once per plane (horizontal or panel):
/// direct-beam transmittance projected through the incidence-angle cosine,
/// plus an isotropic-on-zenith diffuse term. Returns 0.0 when the sun is at
/// or below the horizon, or when the incidence angle exceeds 90° for the
/// direct term (diffuse still applies).
fn clear_sky_irradiance_w_m2(
    sun: &SolarPosition,
    plane_elevation_deg: f64,
    plane_azimuth_deg: f64,
) -> f64 {
    if sun.elevation_deg <= 0.0 {
        return 0.0;
    }
    const I0_W_M2: f64 = 1361.0; // extraterrestrial solar constant
    const K: f64 = 0.21; // atmospheric attenuation coefficient

    let sun_vec = unit_vector(sun.elevation_deg, sun.azimuth_deg);
    let plane_vec = unit_vector(plane_elevation_deg, plane_azimuth_deg);
    let incidence_cos = sun_vec.0 * plane_vec.0 + sun_vec.1 * plane_vec.1 + sun_vec.2 * plane_vec.2;

    let am = air_mass(sun.elevation_deg);
    let kn = (-K * am).exp();
    let kd = diffuse_fraction(sun.elevation_deg);

    let direct = if incidence_cos > 0.0 {
        I0_W_M2 * kn * incidence_cos
    } else {
        0.0
    };
    let diffuse = I0_W_M2 * kd * sun.elevation_deg.to_radians().sin();
    direct + diffuse
}

/// Panel-normal orientation as (elevation, azimuth) of its normal vector,
/// matching the convention used throughout the design doc: a flat/horizontal
/// panel has a normal pointing straight up (elevation 90°); tilt reduces it.
fn panel_normal(panel: &PvArrayGeometry) -> (f64, f64) {
    (90.0 - panel.tilt_deg, panel.azimuth_deg)
}

/// Transpose a supplier's Global Horizontal Irradiance forecast onto the
/// panel's actual tilted plane via the clear-sky-index method: run the
/// clear-sky model for both horizontal and panel planes at the same instant,
/// take the forecast-to-clear-sky ratio on the horizontal plane, and apply
/// that ratio to the panel-plane clear-sky value.
pub fn poa_irradiance_w_m2(ghi_w_m2: f64, sun: &SolarPosition, panel: &PvArrayGeometry) -> f64 {
    if sun.elevation_deg <= 0.0 {
        return 0.0;
    }
    let ghi_clearsky = clear_sky_irradiance_w_m2(sun, 90.0, 0.0); // horizontal: normal points straight up
    if ghi_clearsky <= 0.0 {
        return 0.0;
    }
    let (panel_elev, panel_az) = panel_normal(panel);
    let poa_clearsky = clear_sky_irradiance_w_m2(sun, panel_elev, panel_az);
    let clear_sky_index = (ghi_w_m2 / ghi_clearsky).clamp(0.0, 1.5);
    (clear_sky_index * poa_clearsky).max(0.0)
}

/// NOCT (Nominal Operating Cell Temperature) model: estimates PV cell
/// temperature from ambient air temperature and plane-of-array irradiance.
pub fn cell_temperature_c(air_temp_c: f64, poa_w_m2: f64, noct_c: f64) -> f64 {
    air_temp_c + (noct_c - 20.0) / 800.0 * poa_w_m2
}

/// Compose the full weather-sourced PV forecast for one sample: transposition
/// → DC power → cell-temperature derate → performance ratio → AC clamp →
/// snow-cover override (applied last, per the design doc).
pub fn forecast_ac_kw(
    params: &PvForecastParams,
    sample: &WeatherForecastSample,
    t: DateTime<Utc>,
    snow_state: PvSnowState,
) -> f64 {
    let sun = solar_position(&params.geometry.location, t);
    let poa = poa_irradiance_w_m2(sample.ghi_w_m2, &sun, &params.geometry);

    let dc_kw = (poa / 1000.0) * params.rated_kwp;

    let cell_temp = cell_temperature_c(sample.temperature_c, poa, params.noct_c);
    let temp_derate = 1.0 + params.temp_coeff_pct_per_c / 100.0 * (cell_temp - 25.0);
    let derated_kw = (dc_kw * temp_derate.max(0.0)).max(0.0);

    let ac_kw = derated_kw * params.performance_ratio;
    let clamped_kw = params
        .ac_limit_kw
        .map(|limit| ac_kw.min(limit))
        .unwrap_or(ac_kw);

    if snow_state.covered {
        clamped_kw * params.snow.covered_output_fraction
    } else {
        clamped_kw
    }
}

/// One hour of the weather-sourced PV forecast — `forecast_ac_kw`'s output
/// plus the snow-cover flag that produced it, tied to the forecast sample's
/// own timestamp.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct WeatherPvForecastSlot {
    pub valid_at: DateTime<Utc>,
    pub forecast_ac_kw: f64,
    pub snow_covered: bool,
}

/// Compute the full weather-sourced PV forecast series over a
/// `WeatherForecast`'s own horizon: one `WeatherPvForecastSlot` per sample.
/// Snow-cover state uses the forecast-only fallback (`PvSnowState::default()`,
/// folded forward through `forecast.samples` starting at whatever `age_h` the
/// first sample happens to be — normally `age_h=0`, the "fact" hour) since no
/// live telemetry cross-check is wired up (R-55).
///
/// This is the single source of truth for "how a `WeatherForecast` becomes a
/// PV forecast" — both `GET /weather` (read-only diagnostic) and the
/// planner's own PV input (via `resolve_weather_pv_kw`, R-50) call this same
/// function rather than re-deriving the transposition/snow-override loop.
pub fn weather_pv_forecast_series(
    params: &PvForecastParams,
    forecast: &WeatherForecast,
) -> Vec<WeatherPvForecastSlot> {
    let snow_states =
        snow_coverage_trajectory(PvSnowState::default(), &params.snow, &forecast.samples);
    forecast
        .samples
        .iter()
        .zip(snow_states)
        .map(|(sample, snow_state)| WeatherPvForecastSlot {
            valid_at: sample.valid_at,
            forecast_ac_kw: forecast_ac_kw(params, sample, sample.valid_at, snow_state),
            snow_covered: snow_state.covered,
        })
        .collect()
}

/// The full "should the planner trust this weather feed right now" decision,
/// R-50's staleness gate — resolves `None` (fall back to the pre-existing
/// sin-model/live-snapshot behavior) unless a `PvForecastParams` config
/// exists, a forecast has actually been received, AND that forecast is
/// still fresh; otherwise runs `weather_pv_forecast_series` +
/// `weather_pv_kw_for_slots` and returns the aligned series. Pure — the
/// only I/O (fetching `latest()` from `WeatherForecastPort`) happens in the
/// caller, so this three-way decision is unit-testable without a live port.
pub fn resolve_weather_pv_kw(
    params: Option<&PvForecastParams>,
    forecast: Option<&WeatherForecast>,
    now: DateTime<Utc>,
    staleness_threshold: chrono::Duration,
    slot_starts: &[DateTime<Utc>],
) -> Option<Vec<f64>> {
    let params = params?;
    let forecast = forecast?;
    if !forecast.is_fresh(now, staleness_threshold) {
        return None;
    }
    let series = weather_pv_forecast_series(params, forecast);
    Some(weather_pv_kw_for_slots(&series, slot_starts))
}

/// Align a `weather_pv_forecast_series` (hourly) onto an arbitrary plan slot
/// grid (which may be finer-grained near-term): each `slot_start` takes the
/// `forecast_ac_kw` of whichever series entry is nearest in time. Used by the
/// planner's own PV input (R-50) — the same series that feeds `GET /weather`,
/// just resampled onto the solver's slot boundaries instead of the raw
/// forecast's own hourly ones. Returns an all-zero vec if `series` is empty.
pub fn weather_pv_kw_for_slots(
    series: &[WeatherPvForecastSlot],
    slot_starts: &[DateTime<Utc>],
) -> Vec<f64> {
    slot_starts
        .iter()
        .map(|slot_t| {
            series
                .iter()
                .min_by_key(|s| (s.valid_at - *slot_t).num_seconds().abs())
                .map(|s| s.forecast_ac_kw)
                .unwrap_or(0.0)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::asset_params::PvSnowParams;
    use chrono::TimeZone;

    // Zunzgen, the site used throughout the design doc.
    const ZUNZGEN: GeoPosition = GeoPosition {
        latitude_deg: 47.4491,
        longitude_deg: 7.8081,
    };

    fn south_facing_tilted() -> PvArrayGeometry {
        PvArrayGeometry {
            location: ZUNZGEN,
            tilt_deg: 30.0,
            azimuth_deg: 180.0, // south
        }
    }

    fn horizontal() -> PvArrayGeometry {
        PvArrayGeometry {
            location: ZUNZGEN,
            tilt_deg: 0.0,
            azimuth_deg: 0.0,
        }
    }

    #[test]
    fn solar_position_summer_solstice_noon_elevation_near_expected() {
        // 2026-06-21 ~11:41 UTC ≈ solar noon at this longitude.
        let t = Utc.with_ymd_and_hms(2026, 6, 21, 11, 41, 0).unwrap();
        let pos = solar_position(&ZUNZGEN, t);
        // At 47.45°N on the summer solstice, solar noon elevation ≈ 90 - lat + 23.44 ≈ 66°.
        assert!(
            (pos.elevation_deg - 66.0).abs() < 3.0,
            "expected ~66°, got {}",
            pos.elevation_deg
        );
    }

    #[test]
    fn solar_position_winter_solstice_noon_elevation_near_expected() {
        let t = Utc.with_ymd_and_hms(2026, 12, 21, 11, 41, 0).unwrap();
        let pos = solar_position(&ZUNZGEN, t);
        // ≈ 90 - lat - 23.44 ≈ 19°.
        assert!(
            (pos.elevation_deg - 19.0).abs() < 3.0,
            "expected ~19°, got {}",
            pos.elevation_deg
        );
    }

    #[test]
    fn solar_position_midnight_is_below_horizon() {
        let t = Utc.with_ymd_and_hms(2026, 6, 21, 23, 0, 0).unwrap();
        let pos = solar_position(&ZUNZGEN, t);
        assert!(pos.elevation_deg < 0.0);
    }

    #[test]
    fn tilted_south_panel_exceeds_horizontal_near_solstice_noon() {
        let t = Utc.with_ymd_and_hms(2026, 6, 21, 11, 41, 0).unwrap();
        let sun = solar_position(&ZUNZGEN, t);
        let tilted = poa_irradiance_w_m2(600.0, &sun, &south_facing_tilted());
        let flat = poa_irradiance_w_m2(600.0, &sun, &horizontal());
        assert!(
            tilted > flat,
            "tilted={tilted} should exceed flat={flat} near solstice noon"
        );
    }

    #[test]
    fn panel_facing_away_from_sun_gets_only_diffuse() {
        // Sun roughly south at midday; a panel with normal pointing straight
        // down toward the sun's *anti*-direction (north-facing, steep tilt)
        // should have an incidence angle > 90° and therefore zero direct term
        // — but poa_irradiance_w_m2 (via the clear-sky-index) can't isolate
        // the direct component in isolation, so assert on the lower-level
        // clear_sky_irradiance_w_m2 building block instead.
        let t = Utc.with_ymd_and_hms(2026, 6, 21, 11, 41, 0).unwrap();
        let sun = solar_position(&ZUNZGEN, t);
        let north_facing_steep = PvArrayGeometry {
            location: ZUNZGEN,
            tilt_deg: 90.0,
            azimuth_deg: 0.0, // north
        };
        let (elev, az) = panel_normal(&north_facing_steep);
        let sun_vec = unit_vector(sun.elevation_deg, sun.azimuth_deg);
        let panel_vec = unit_vector(elev, az);
        let incidence_cos =
            sun_vec.0 * panel_vec.0 + sun_vec.1 * panel_vec.1 + sun_vec.2 * panel_vec.2;
        assert!(incidence_cos < 0.0, "expected incidence angle > 90°");
    }

    #[test]
    fn poa_irradiance_zero_at_night() {
        let t = Utc.with_ymd_and_hms(2026, 6, 21, 23, 0, 0).unwrap();
        let sun = solar_position(&ZUNZGEN, t);
        assert_eq!(poa_irradiance_w_m2(0.0, &sun, &south_facing_tilted()), 0.0);
    }

    #[test]
    fn cell_temperature_equals_air_temp_at_zero_irradiance() {
        assert_eq!(cell_temperature_c(10.0, 0.0, 45.0), 10.0);
    }

    #[test]
    fn cell_temperature_equals_noct_at_noct_reference_conditions() {
        // NOCT is defined at 800 W/m² POA, 20°C air temperature.
        let t = cell_temperature_c(20.0, 800.0, 45.0);
        assert!((t - 45.0).abs() < 1e-9);
    }

    fn default_forecast_params() -> crate::entities::asset_params::PvForecastParams {
        crate::entities::asset_params::PvForecastParams {
            rated_kwp: 10.0,
            geometry: south_facing_tilted(),
            performance_ratio: 0.87,
            temp_coeff_pct_per_c: -0.35,
            noct_c: 45.0,
            ac_limit_kw: None,
            snow: PvSnowParams::default(),
        }
    }

    fn sample_at(
        ghi_w_m2: f64,
        temperature_c: f64,
        valid_at: DateTime<Utc>,
    ) -> WeatherForecastSample {
        WeatherForecastSample {
            valid_at,
            age_h: 1,
            temperature_c,
            ghi_w_m2,
            wind_speed_kmh: None,
            rain_prob_pct: None,
            new_snowfall_cm: None,
            sky_condition: None,
            irradiance_variability: None,
        }
    }

    #[test]
    fn forecast_ac_kw_zero_at_night() {
        let t = Utc.with_ymd_and_hms(2026, 6, 21, 23, 0, 0).unwrap();
        let s = sample_at(0.0, 15.0, t);
        let kw = forecast_ac_kw(&default_forecast_params(), &s, t, PvSnowState::default());
        assert_eq!(kw, 0.0);
    }

    #[test]
    fn forecast_ac_kw_non_decreasing_in_ghi() {
        let t = Utc.with_ymd_and_hms(2026, 6, 21, 11, 41, 0).unwrap();
        let params = default_forecast_params();
        let low = forecast_ac_kw(
            &params,
            &sample_at(200.0, 20.0, t),
            t,
            PvSnowState::default(),
        );
        let high = forecast_ac_kw(
            &params,
            &sample_at(800.0, 20.0, t),
            t,
            PvSnowState::default(),
        );
        assert!(high >= low, "high={high} should be >= low={low}");
    }

    #[test]
    fn forecast_ac_kw_clamps_to_ac_limit() {
        let t = Utc.with_ymd_and_hms(2026, 6, 21, 11, 41, 0).unwrap();
        let mut params = default_forecast_params();
        params.ac_limit_kw = Some(1.0);
        let s = sample_at(900.0, 20.0, t);
        let kw = forecast_ac_kw(&params, &s, t, PvSnowState::default());
        assert!(kw <= 1.0 + 1e-9);
    }

    #[test]
    fn forecast_ac_kw_performance_ratio_is_linear_multiplier() {
        let t = Utc.with_ymd_and_hms(2026, 6, 21, 11, 41, 0).unwrap();
        let mut params = default_forecast_params();
        params.performance_ratio = 0.80;
        let s = sample_at(500.0, 20.0, t);
        let full = forecast_ac_kw(&params, &s, t, PvSnowState::default());
        params.performance_ratio = 0.40;
        let half = forecast_ac_kw(&params, &s, t, PvSnowState::default());
        assert!((full - 2.0 * half).abs() < 1e-9, "full={full} half={half}");
    }

    #[test]
    fn forecast_ac_kw_snow_covered_overrides_high_irradiance() {
        let t = Utc.with_ymd_and_hms(2026, 6, 21, 11, 41, 0).unwrap();
        let params = default_forecast_params();
        let s = sample_at(900.0, 20.0, t);
        let covered = PvSnowState { covered: true };
        let kw = forecast_ac_kw(&params, &s, t, covered);
        let uncovered_kw = forecast_ac_kw(&params, &s, t, PvSnowState::default());
        assert!(kw < uncovered_kw);
        assert!((kw - uncovered_kw * params.snow.covered_output_fraction).abs() < 1e-9);
    }

    // ── weather_pv_forecast_series ───────────────────────────────────────────

    fn make_forecast(samples: Vec<WeatherForecastSample>) -> WeatherForecast {
        let fetched_at = samples[0].valid_at;
        WeatherForecast {
            source_id: "test".into(),
            location: ZUNZGEN,
            fetched_at,
            samples,
        }
    }

    #[test]
    fn weather_pv_forecast_series_length_matches_samples() {
        let params = default_forecast_params();
        let t0 = Utc.with_ymd_and_hms(2026, 6, 21, 6, 0, 0).unwrap();
        let samples = vec![
            sample_at(200.0, 15.0, t0),
            sample_at(400.0, 17.0, t0 + chrono::Duration::hours(1)),
            sample_at(600.0, 19.0, t0 + chrono::Duration::hours(2)),
        ];
        let forecast = make_forecast(samples.clone());
        let series = weather_pv_forecast_series(&params, &forecast);
        assert_eq!(series.len(), samples.len());
        for (slot, sample) in series.iter().zip(&samples) {
            assert_eq!(slot.valid_at, sample.valid_at);
        }
    }

    #[test]
    fn weather_pv_forecast_series_snow_covered_matches_direct_trajectory() {
        let params = default_forecast_params();
        let t0 = Utc.with_ymd_and_hms(2026, 1, 1, 6, 0, 0).unwrap();
        let mut snowy = sample_at(300.0, -3.0, t0);
        snowy.new_snowfall_cm = Some(2.0); // above the default trigger
        let cold = sample_at(300.0, -2.0, t0 + chrono::Duration::hours(1));
        let samples = vec![snowy, cold];
        let forecast = make_forecast(samples.clone());

        let series = weather_pv_forecast_series(&params, &forecast);
        let expected_states = crate::entities::pv_snow::snow_coverage_trajectory(
            PvSnowState::default(),
            &params.snow,
            &samples,
        );
        for (slot, expected) in series.iter().zip(&expected_states) {
            assert_eq!(slot.snow_covered, expected.covered);
        }
        assert!(series[0].snow_covered, "fresh snowfall should cover slot 0");
    }

    #[test]
    fn weather_pv_forecast_series_kw_matches_direct_forecast_ac_kw_call() {
        let params = default_forecast_params();
        let t0 = Utc.with_ymd_and_hms(2026, 6, 21, 11, 0, 0).unwrap();
        let samples = vec![sample_at(500.0, 20.0, t0)];
        let forecast = make_forecast(samples.clone());

        let series = weather_pv_forecast_series(&params, &forecast);
        let direct = forecast_ac_kw(&params, &samples[0], t0, PvSnowState::default());
        assert!((series[0].forecast_ac_kw - direct).abs() < 1e-9);
    }

    // ── weather_pv_kw_for_slots ───────────────────────────────────────────────

    #[test]
    fn weather_pv_kw_for_slots_picks_nearest_hourly_sample() {
        let t0 = Utc.with_ymd_and_hms(2026, 6, 21, 6, 0, 0).unwrap();
        let series = vec![
            WeatherPvForecastSlot {
                valid_at: t0,
                forecast_ac_kw: 1.0,
                snow_covered: false,
            },
            WeatherPvForecastSlot {
                valid_at: t0 + chrono::Duration::hours(1),
                forecast_ac_kw: 2.0,
                snow_covered: false,
            },
        ];
        // A finer-grained slot grid within the same two hours: 15-min steps.
        let slot_starts: Vec<_> = (0..8)
            .map(|i| t0 + chrono::Duration::minutes(15 * i))
            .collect();
        let kw = weather_pv_kw_for_slots(&series, &slot_starts);
        assert_eq!(kw.len(), 8);
        // First 3 slots (0, 15, 30 min) are nearer to t0 than t0+1h.
        assert_eq!(kw[0], 1.0);
        assert_eq!(kw[1], 1.0);
        assert_eq!(kw[2], 1.0);
        // Slots from 45 min onward are nearer to t0+1h.
        assert_eq!(kw[3], 2.0);
        assert_eq!(kw[7], 2.0);
    }

    #[test]
    fn weather_pv_kw_for_slots_empty_series_returns_all_zero() {
        let t0 = Utc.with_ymd_and_hms(2026, 6, 21, 6, 0, 0).unwrap();
        let kw = weather_pv_kw_for_slots(&[], &[t0, t0 + chrono::Duration::hours(1)]);
        assert_eq!(kw, vec![0.0, 0.0]);
    }

    // ── resolve_weather_pv_kw — the three R-50 staleness-gate cases ──────────

    #[test]
    fn resolve_weather_pv_kw_fresh_forecast_and_config_is_used() {
        let t0 = Utc.with_ymd_and_hms(2026, 6, 21, 11, 0, 0).unwrap();
        let params = default_forecast_params();
        let forecast = make_forecast(vec![sample_at(500.0, 20.0, t0)]);
        let now = t0; // fetched_at == now → fresh
        let result = resolve_weather_pv_kw(
            Some(&params),
            Some(&forecast),
            now,
            chrono::Duration::hours(2),
            &[t0],
        );
        assert!(result.is_some(), "fresh forecast + config must be used");
        assert!(result.unwrap()[0] > 0.0);
    }

    #[test]
    fn resolve_weather_pv_kw_stale_forecast_falls_back() {
        let t0 = Utc.with_ymd_and_hms(2026, 6, 21, 11, 0, 0).unwrap();
        let params = default_forecast_params();
        let forecast = make_forecast(vec![sample_at(500.0, 20.0, t0)]);
        let now = t0 + chrono::Duration::hours(3); // 3h old, past the 2h threshold
        let result = resolve_weather_pv_kw(
            Some(&params),
            Some(&forecast),
            now,
            chrono::Duration::hours(2),
            &[t0],
        );
        assert!(result.is_none(), "stale forecast must fall back to None");
    }

    #[test]
    fn resolve_weather_pv_kw_no_config_falls_back() {
        let t0 = Utc.with_ymd_and_hms(2026, 6, 21, 11, 0, 0).unwrap();
        let forecast = make_forecast(vec![sample_at(500.0, 20.0, t0)]);
        let result =
            resolve_weather_pv_kw(None, Some(&forecast), t0, chrono::Duration::hours(2), &[t0]);
        assert!(
            result.is_none(),
            "no PvForecastParams config must fall back to None"
        );
    }

    #[test]
    fn resolve_weather_pv_kw_no_forecast_received_falls_back() {
        let t0 = Utc.with_ymd_and_hms(2026, 6, 21, 11, 0, 0).unwrap();
        let params = default_forecast_params();
        let result =
            resolve_weather_pv_kw(Some(&params), None, t0, chrono::Duration::hours(2), &[t0]);
        assert!(
            result.is_none(),
            "no forecast ever received must fall back to None"
        );
    }
}
