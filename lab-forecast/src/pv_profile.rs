//! The PV settings a site declares once in the VEN profile YAML, read by the VEN and by the forecaster.
//!
//! Owns: `WeatherPvConfig` (the `weather:` block of a `type: pv` asset) and the one reader that lists a
//! profile's PV arrays with their forecast parameters and measurement switch. Reads: YAML text handed in.
//! Must never: open a file, or interpret anything of the profile besides its PV assets.

use serde::Deserialize;

use crate::pv_params::{PvArrayGeometry, PvForecastParams, PvSnowParams};
use crate::weather::GeoPosition;

fn default_performance_ratio() -> f64 {
    0.87
}
fn default_temp_coeff_pct_per_c() -> f64 {
    -0.35
}
fn default_noct_c() -> f64 {
    45.0
}
fn default_snowfall_trigger_cm() -> f64 {
    0.2
}
fn default_clear_threshold_c() -> f64 {
    1.5
}

/// YAML config for the weather-sourced PV forecast (`solar::forecast_ac_kw` / `weather_pv_forecast_series`).
/// Geometry/rating fields are required (no sensible universal default for a specific site); tuning fields
/// default to typical residential values.
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
    #[serde(default = "default_performance_ratio")]
    pub performance_ratio: f64,
    #[serde(default = "default_temp_coeff_pct_per_c")]
    pub temp_coeff_pct_per_c: f64,
    #[serde(default = "default_noct_c")]
    pub noct_c: f64,
    #[serde(default)]
    pub ac_limit_kw: Option<f64>,
    #[serde(default = "default_snowfall_trigger_cm")]
    pub snowfall_trigger_cm: f64,
    #[serde(default = "default_clear_threshold_c")]
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

/// One PV array of the profile, as the forecaster needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct PvArraySettings {
    /// The PV asset id; the `<array_id>` of its forecast topic.
    pub id: String,
    /// The asset's own `rated_kw`, the scale the snow observation compares against.
    pub rated_kw: f64,
    pub params: PvForecastParams,
    /// Whether the profile switches the real PV measurement on for this array.
    pub measured: bool,
}

/// The PV arrays of a profile that declare a `weather:` block, in profile order. An array without one
/// cannot be forecast from weather and is not listed. A profile that is not YAML, or whose PV `weather:`
/// block is incomplete, is an error naming the cause.
pub fn pv_arrays_from_yaml(yaml: &str) -> Result<Vec<PvArraySettings>, String> {
    let doc: ProfileDoc =
        serde_yaml::from_str(yaml).map_err(|e| format!("profile is not readable: {e}"))?;
    let mut arrays = Vec::new();
    for asset in doc.assets.into_iter().filter(|a| a.kind == "pv") {
        let Some(weather) = asset.weather else {
            continue;
        };
        let cfg: WeatherPvConfig = serde_yaml::from_value(weather)
            .map_err(|e| format!("pv asset '{}': weather block: {e}", asset.id))?;
        arrays.push(PvArraySettings {
            id: asset.id,
            rated_kw: asset.rated_kw,
            params: cfg.to_params(),
            measured: asset.measurement.enabled,
        });
    }
    Ok(arrays)
}

/// Whether the profile switches the real base-load measurement on (a `type: base_load` asset with
/// `measurement: {enabled: true}`); the forecaster then listens for it and publishes `base_load/power_kw`.
pub fn base_load_measured_from_yaml(yaml: &str) -> Result<bool, String> {
    let doc: ProfileDoc =
        serde_yaml::from_str(yaml).map_err(|e| format!("profile is not readable: {e}"))?;
    Ok(doc
        .assets
        .iter()
        .any(|a| a.kind == "base_load" && a.measurement.enabled))
}

/// Only the parts of the profile this reader interprets; every other key is ignored.
#[derive(Deserialize)]
struct ProfileDoc {
    #[serde(default)]
    assets: Vec<AssetEntry>,
}

#[derive(Deserialize)]
struct AssetEntry {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    rated_kw: f64,
    weather: Option<serde_yaml::Value>,
    #[serde(default)]
    measurement: MeasurementSwitch,
}

/// An asset's real-measurement switch (`measurement:` in its profile entry); off by default.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct MeasurementSwitch {
    #[serde(default)]
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEATHER: &str = "    weather:
      latitude_deg: 47.4491
      longitude_deg: 7.8081
      tilt_deg: 22.0
      azimuth_deg: 267.0
      rated_kwp: 14.4
";

    #[test]
    fn pv_arrays_from_yaml_lists_the_arrays_with_a_weather_block() {
        let yaml = format!(
            "assets:\n  - type: battery\n    id: battery\n  - type: pv\n    id: pv\n    rated_kw: 14.4\n{WEATHER}    measurement: {{enabled: true}}\n  - type: pv\n    id: roof-2\n    rated_kw: 5.0\n{WEATHER}"
        );
        let arrays = pv_arrays_from_yaml(&yaml).unwrap();
        assert_eq!(arrays.len(), 2);
        assert_eq!(arrays[0].id, "pv");
        assert_eq!(arrays[0].rated_kw, 14.4);
        assert!(arrays[0].measured);
        assert_eq!(arrays[0].params.rated_kwp, 14.4);
        assert_eq!(arrays[1].id, "roof-2");
        assert!(
            !arrays[1].measured,
            "measurement is off unless the profile switches it on"
        );
    }

    #[test]
    fn pv_arrays_from_yaml_skips_an_array_without_a_weather_block() {
        let yaml = "assets:\n  - type: pv\n    id: pv\n    rated_kw: 10.0\n";
        assert!(pv_arrays_from_yaml(yaml).unwrap().is_empty());
    }

    #[test]
    fn pv_arrays_from_yaml_ignores_every_other_asset_and_unknown_fields() {
        let yaml = "site: x\nassets:\n  - type: ev\n    id: ev\n    capacity_kwh: 50\n    some_future_key: 1\n  - type: heater\n    id: h\n";
        assert!(pv_arrays_from_yaml(yaml).unwrap().is_empty());
    }

    #[test]
    fn pv_arrays_from_yaml_names_an_incomplete_weather_block() {
        let yaml = "assets:\n  - type: pv\n    id: pv\n    weather:\n      latitude_deg: 47.0\n";
        let err = pv_arrays_from_yaml(yaml).unwrap_err();
        assert!(err.contains("longitude_deg"), "{err}");
    }

    #[test]
    fn pv_arrays_from_yaml_rejects_text_that_is_not_yaml() {
        assert!(pv_arrays_from_yaml("assets: [unclosed").is_err());
    }

    #[test]
    fn base_load_measured_from_yaml_follows_the_measurement_switch_of_the_base_load_asset() {
        let on = "assets:
  - type: base_load
    id: base_load
    measurement: {enabled: true}
";
        let off = "assets:
  - type: base_load
    id: base_load
";
        let pv_only = "assets:
  - type: pv
    id: pv
    measurement: {enabled: true}
";
        assert_eq!(base_load_measured_from_yaml(on), Ok(true));
        assert_eq!(base_load_measured_from_yaml(off), Ok(false));
        assert_eq!(
            base_load_measured_from_yaml(pv_only),
            Ok(false),
            "a PV switch is not a base-load one"
        );
        assert!(base_load_measured_from_yaml("assets: [unclosed").is_err());
    }

    #[test]
    fn weather_pv_config_applies_defaults_for_the_tuning_fields() {
        let cfg: WeatherPvConfig = serde_yaml::from_str(
            "latitude_deg: 47.4491\nlongitude_deg: 7.8081\ntilt_deg: 30.0\nazimuth_deg: 180.0\nrated_kwp: 10.0\n",
        )
        .unwrap();
        let p = cfg.to_params();
        assert_eq!(p.performance_ratio, 0.87);
        assert_eq!(p.temp_coeff_pct_per_c, -0.35);
        assert_eq!(p.noct_c, 45.0);
        assert_eq!(p.ac_limit_kw, None);
        assert_eq!(p.snow.snowfall_trigger_cm, 0.2);
        assert_eq!(p.snow.clear_threshold_c, 1.5);
        assert_eq!(p.snow.covered_output_fraction, 0.0);
    }
}
