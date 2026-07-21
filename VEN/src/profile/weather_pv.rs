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

    #[test]
    fn profile_without_weather_pv_parses_with_none() {
        let yaml = "assets: []\n";
        let p: Profile = serde_yaml::from_str(yaml).unwrap();
        assert!(p.weather_pv.is_none());
        assert!(p.weather_pv_params().is_none());
    }

    #[test]
    fn profile_with_weather_pv_parses_required_fields_and_defaults() {
        let yaml = r#"
assets: []
weather_pv:
  latitude_deg: 47.4491
  longitude_deg: 7.8081
  tilt_deg: 30.0
  azimuth_deg: 180.0
  rated_kwp: 10.0
"#;
        let p: Profile = serde_yaml::from_str(yaml).unwrap();
        let cfg = p.weather_pv.as_ref().expect("weather_pv should parse");
        assert_eq!(cfg.latitude_deg, 47.4491);
        assert_eq!(cfg.rated_kwp, 10.0);
        // Defaults applied for omitted tuning fields.
        assert_eq!(cfg.performance_ratio, 0.87);
        assert_eq!(cfg.temp_coeff_pct_per_c, -0.35);
        assert_eq!(cfg.noct_c, 45.0);
        assert_eq!(cfg.ac_limit_kw, None);
        assert_eq!(cfg.snowfall_trigger_cm, 0.2);
        assert_eq!(cfg.clear_threshold_c, 1.5);
        assert_eq!(cfg.covered_output_fraction, 0.0);

        let params = p.weather_pv_params().expect("params should be Some");
        assert_eq!(params.rated_kwp, 10.0);
        assert_eq!(params.geometry.tilt_deg, 30.0);
        assert_eq!(params.geometry.azimuth_deg, 180.0);
        assert_eq!(params.geometry.location.latitude_deg, 47.4491);
        assert_eq!(params.geometry.location.longitude_deg, 7.8081);
    }

    #[test]
    fn profile_with_weather_pv_overrides_tuning_fields() {
        let yaml = r#"
assets: []
weather_pv:
  latitude_deg: 47.4491
  longitude_deg: 7.8081
  tilt_deg: 30.0
  azimuth_deg: 180.0
  rated_kwp: 10.0
  performance_ratio: 0.80
  ac_limit_kw: 8.0
"#;
        let p: Profile = serde_yaml::from_str(yaml).unwrap();
        let params = p.weather_pv_params().unwrap();
        assert_eq!(params.performance_ratio, 0.80);
        assert_eq!(params.ac_limit_kw, Some(8.0));
    }
}
