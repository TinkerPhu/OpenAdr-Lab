//! YAML config for the weather-sourced PV forecast (weather-forecast-visibility)
//! — split out of `schema.rs` to keep that file under the file-size cap.

use crate::entities::asset_params::{PvArrayGeometry, PvForecastParams, PvSnowParams};
use crate::entities::weather::GeoPosition;
use serde::Deserialize;

/// YAML config for the weather-sourced PV forecast
/// (`entities::solar::forecast_ac_kw` / `weather_pv_forecast_series`).
/// Geometry/rating fields are required (no sensible universal default for a
/// specific site); tuning fields default to typical residential values.
#[derive(Debug, Clone, Deserialize)]
pub struct WeatherPvConfig {
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    /// 0° = horizontal, 90° = vertical wall.
    pub tilt_deg: f64,
    /// Compass bearing the panel faces: 0°=N, 90°=E, 180°=S, 270°=W.
    pub azimuth_deg: f64,
    /// DC nameplate rating at STC (1000 W/m², 25°C cell temp).
    pub rated_kwp: f64,
    #[serde(default = "super::defaults::default_weather_pv_performance_ratio")]
    pub performance_ratio: f64,
    #[serde(default = "super::defaults::default_weather_pv_temp_coeff_pct_per_c")]
    pub temp_coeff_pct_per_c: f64,
    #[serde(default = "super::defaults::default_weather_pv_noct_c")]
    pub noct_c: f64,
    #[serde(default)]
    pub ac_limit_kw: Option<f64>,
    #[serde(default = "super::defaults::default_weather_pv_snowfall_trigger_cm")]
    pub snowfall_trigger_cm: f64,
    #[serde(default = "super::defaults::default_weather_pv_clear_threshold_c")]
    pub clear_threshold_c: f64,
    #[serde(default)]
    pub covered_output_fraction: f64,
}

impl WeatherPvConfig {
    pub fn to_params(&self) -> PvForecastParams {
        PvForecastParams {
            rated_kwp: self.rated_kwp,
            geometry: PvArrayGeometry {
                location: GeoPosition {
                    latitude_deg: self.latitude_deg,
                    longitude_deg: self.longitude_deg,
                },
                tilt_deg: self.tilt_deg,
                azimuth_deg: self.azimuth_deg,
            },
            performance_ratio: self.performance_ratio,
            temp_coeff_pct_per_c: self.temp_coeff_pct_per_c,
            noct_c: self.noct_c,
            ac_limit_kw: self.ac_limit_kw,
            snow: PvSnowParams {
                snowfall_trigger_cm: self.snowfall_trigger_cm,
                clear_threshold_c: self.clear_threshold_c,
                covered_output_fraction: self.covered_output_fraction,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::profile::schema::Profile;

    /// One PV asset `pv` with `weather` as its `weather:` block.
    fn profile_with_weather(weather: &str) -> Profile {
        let yaml = format!(
            "assets:\n  - type: pv\n    id: pv\n    rated_kw: 10.0\n    weather:\n{weather}"
        );
        serde_yaml::from_str(&yaml).unwrap()
    }

    #[test]
    fn profile_without_weather_has_no_weather_params() {
        let p: Profile = serde_yaml::from_str("assets:\n  - type: pv\n    id: pv\n").unwrap();
        assert!(p.pv_weather_params().is_empty());
        assert_eq!(p.measured_pv_asset(), None);
        assert_eq!(p.measured_base_load_asset(), None);
    }

    #[test]
    fn a_pv_weather_block_parses_required_fields_and_defaults() {
        let p = profile_with_weather(
            "      latitude_deg: 47.4491\n      longitude_deg: 7.8081\n      tilt_deg: 30.0\n      azimuth_deg: 180.0\n      rated_kwp: 10.0\n",
        );
        let params = &p.pv_weather_params()["pv"];
        assert_eq!(params.rated_kwp, 10.0);
        assert_eq!(params.geometry.tilt_deg, 30.0);
        assert_eq!(params.geometry.azimuth_deg, 180.0);
        assert_eq!(params.geometry.location.latitude_deg, 47.4491);
        assert_eq!(params.geometry.location.longitude_deg, 7.8081);
        // Defaults applied for omitted tuning fields.
        assert_eq!(params.performance_ratio, 0.87);
        assert_eq!(params.temp_coeff_pct_per_c, -0.35);
        assert_eq!(params.noct_c, 45.0);
        assert_eq!(params.ac_limit_kw, None);
        assert_eq!(params.snow.snowfall_trigger_cm, 0.2);
        assert_eq!(params.snow.clear_threshold_c, 1.5);
        assert_eq!(params.snow.covered_output_fraction, 0.0);
    }

    #[test]
    fn a_pv_weather_block_overrides_tuning_fields() {
        let p = profile_with_weather(
            "      latitude_deg: 47.4491\n      longitude_deg: 7.8081\n      tilt_deg: 30.0\n      azimuth_deg: 180.0\n      rated_kwp: 10.0\n      performance_ratio: 0.80\n      ac_limit_kw: 8.0\n",
        );
        let params = &p.pv_weather_params()["pv"];
        assert_eq!(params.performance_ratio, 0.80);
        assert_eq!(params.ac_limit_kw, Some(8.0));
    }
}
