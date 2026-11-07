//! YAML config for the weather-sourced PV forecast (weather-forecast-visibility)
//! — split out of `schema.rs` to keep that file under the file-size cap.

pub use lab_forecast::pv_profile::WeatherPvConfig;

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
            "      latitude_deg: 46.9480\n      longitude_deg: 7.4474\n      tilt_deg: 30.0\n      azimuth_deg: 180.0\n      rated_kwp: 10.0\n",
        );
        let params = &p.pv_weather_params()["pv"];
        assert_eq!(params.rated_kwp, 10.0);
        assert_eq!(params.geometry.tilt_deg, 30.0);
        assert_eq!(params.geometry.azimuth_deg, 180.0);
        assert_eq!(params.geometry.location.latitude_deg, 46.9480);
        assert_eq!(params.geometry.location.longitude_deg, 7.4474);
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
            "      latitude_deg: 46.9480\n      longitude_deg: 7.4474\n      tilt_deg: 30.0\n      azimuth_deg: 180.0\n      rated_kwp: 10.0\n      performance_ratio: 0.80\n      ac_limit_kw: 8.0\n",
        );
        let params = &p.pv_weather_params()["pv"];
        assert_eq!(params.performance_ratio, 0.80);
        assert_eq!(params.ac_limit_kw, Some(8.0));
    }
}
