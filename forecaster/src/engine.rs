//! The forecaster's decision: which messages to publish now, and in which state the producer is.
//!
//! Owns: the per-array PV series from the latest weather forecast (one call of
//! `lab_forecast::solar::fresh_weather_pv_series`, the function the VEN's fallback uses), the snow-cover
//! state per array (observed from the PV measurement with `lab_forecast::pv_snow_observation`), the site
//! total, and the status. Reads: what was handed in (weather, measurement, `now`). Must never: read a
//! clock, open a socket, or compute a PV value itself.

use std::collections::BTreeMap;

use crate::base_load::BaseLoadModel;
use chrono::{DateTime, Duration, Utc};
use lab_forecast::base_load_learner::{LearnConfig, LoadSample};
use lab_forecast::confidence::mean_slot_confidence;
use lab_forecast::forecast_payload::{
    ForecastPayload, ForecastState, ForecastStatus, ForecastTopics, PayloadError, PayloadMeta,
    SeriesGrid, SnowState, SCHEMA_VERSION,
};
use lab_forecast::measurement_wire::{
    resolve_measured_kw, MeasurementReading, MEASUREMENT_STALENESS_THRESHOLD,
};
use lab_forecast::pv_profile::PvArraySettings;
use lab_forecast::pv_snow::PvSnowState;
use lab_forecast::pv_snow_observation::{observe_snow, SnowObservationInput};
use lab_forecast::solar::weather_pv_forecast_series;
use lab_forecast::weather::WeatherForecast;

/// What the engine is configured with.
#[derive(Debug, Clone)]
pub struct EngineSettings {
    pub topics: ForecastTopics,
    pub arrays: Vec<PvArraySettings>,
    pub producer_id: String,
    /// Names the model that produced a series (`pv-weather-1`).
    pub model_version: String,
    /// `valid_until - generated_at` of every series, and how old a weather forecast may be.
    pub freshness: Duration,
    /// Whether the profile switches the base-load measurement on: then `base_load/power_kw` is published.
    pub base_load_enabled: bool,
}

/// One retained message to publish.
#[derive(Debug, Clone, PartialEq)]
pub struct Publication {
    pub topic: String,
    pub payload: Vec<u8>,
}

pub struct Engine {
    settings: EngineSettings,
    weather: Option<WeatherForecast>,
    pv_measurement: Option<MeasurementReading>,
    snow: BTreeMap<String, PvSnowState>,
    state: ForecastState,
    last_problem: Option<String>,
    base_load: Option<BaseLoadModel>,
}

impl Engine {
    pub fn new(settings: EngineSettings) -> Self {
        let base_load = settings
            .base_load_enabled
            .then(|| BaseLoadModel::new(LearnConfig::default()));
        Self {
            base_load,
            settings,
            weather: None,
            pv_measurement: None,
            snow: BTreeMap::new(),
            state: ForecastState::ColdStart,
            last_problem: None,
        }
    }

    /// The latest weather forecast replaces the previous one.
    pub fn on_weather(&mut self, forecast: WeatherForecast) {
        self.weather = Some(forecast);
    }

    /// The latest PV measurement replaces the previous one.
    pub fn on_pv_measurement(&mut self, reading: MeasurementReading) {
        self.pv_measurement = Some(reading);
    }

    /// The latest base-load measurement joins the learner's history.
    pub fn on_base_load_measurement(&mut self, reading: MeasurementReading) {
        if let Some(model) = self.base_load.as_mut() {
            model.record(reading);
        }
    }

    /// The learner's history as samples, for the cache file.
    pub fn base_load_history(&self) -> Vec<LoadSample> {
        self.base_load
            .as_ref()
            .map(BaseLoadModel::history)
            .unwrap_or_default()
    }

    /// Put cached samples back after a restart.
    pub fn restore_base_load_history(&mut self, samples: &[LoadSample]) {
        if let Some(model) = self.base_load.as_mut() {
            model.restore(samples);
        }
    }

    /// Recompute at `now`: the messages to publish (every PV array and the total, and the base load when
    /// the profile measures it; republished even when unchanged so `generated_at` is a heartbeat). The PV
    /// series need a fresh weather forecast and are skipped without one; the base load does not. Sets the
    /// state that `status` reports.
    pub fn recompute(&mut self, now: DateTime<Utc>) -> Vec<Publication> {
        let mut publications = Vec::new();
        let pv_state = match self.weather.clone() {
            None => ForecastState::ColdStart,
            Some(forecast) if !forecast.is_fresh(now, self.settings.freshness) => {
                ForecastState::WeatherStale
            }
            Some(forecast) => match self.publish_pv(&forecast, now) {
                Ok(pv) => {
                    publications.extend(pv);
                    ForecastState::Ok
                }
                Err(reason) => {
                    self.last_problem = Some(reason);
                    ForecastState::Degraded
                }
            },
        };
        let base_load_cold = match self.publish_base_load(now) {
            Ok(Some((publication, cold))) => {
                publications.push(publication);
                cold
            }
            Ok(None) => false,
            Err(reason) => {
                self.last_problem = Some(reason);
                false
            }
        };
        self.state = if pv_state == ForecastState::Ok && base_load_cold {
            ForecastState::ColdStart
        } else {
            pv_state
        };
        publications
    }

    /// The base-load series and whether it is a cold-start one; `None` when the profile has no base load.
    fn publish_base_load(
        &mut self,
        now: DateTime<Utc>,
    ) -> Result<Option<(Publication, bool)>, String> {
        let Some(model) = self.base_load.as_mut() else {
            return Ok(None);
        };
        let series = model.series(now);
        let meta = PayloadMeta {
            generated_at: now,
            valid_until: now + self.settings.freshness,
            model_version: series.model_version.to_string(),
            producer_id: self.settings.producer_id.clone(),
            confidence: series.confidence,
        };
        let grid = SeriesGrid {
            start: series.start,
            step_s: 3600,
        };
        let publication = build_publication(
            self.settings.topics.base_load(),
            ForecastPayload::base_load(meta, grid, series.values_kw),
        )?;
        Ok(Some((publication, series.cold_start)))
    }

    /// Why the last `recompute` ended `Degraded`, for the log.
    pub fn last_problem(&self) -> Option<&str> {
        self.last_problem.as_deref()
    }

    /// The retained status at `now`.
    pub fn status(&self, now: DateTime<Utc>) -> Publication {
        let age_s = |at: DateTime<Utc>| u64::try_from((now - at).num_seconds()).ok();
        let mut input_age_s = BTreeMap::new();
        input_age_s.insert(
            "weather".to_string(),
            self.weather.as_ref().and_then(|w| age_s(w.fetched_at)),
        );
        if self.settings.arrays.iter().any(|a| a.measured) {
            input_age_s.insert(
                "pv_measurement".to_string(),
                self.pv_measurement.and_then(|(_, at)| age_s(at)),
            );
        }
        if let Some(model) = &self.base_load {
            input_age_s.insert(
                "base_load_measurement".to_string(),
                model.last_reading_at().and_then(age_s),
            );
        }
        let snow_state = self
            .settings
            .arrays
            .iter()
            .map(|a| {
                (
                    a.id.clone(),
                    if a.measured {
                        SnowState::Observed
                    } else {
                        SnowState::Unobserved
                    },
                )
            })
            .collect();
        let status = ForecastStatus {
            schema_version: SCHEMA_VERSION.to_string(),
            producer_id: self.settings.producer_id.clone(),
            generated_at: now,
            state: self.state,
            input_age_s,
            snow_state,
        };
        Publication {
            topic: self.settings.topics.status(),
            payload: serde_json::to_vec(&status).expect("a status always serialises"),
        }
    }

    /// The state the last `recompute` ended in (`ColdStart` before the first).
    pub fn state(&self) -> ForecastState {
        self.state
    }

    /// Every array's series from `forecast`, then the site total; a reason when any cannot be built.
    fn publish_pv(
        &mut self,
        forecast: &WeatherForecast,
        now: DateTime<Utc>,
    ) -> Result<Vec<Publication>, String> {
        if self.settings.arrays.is_empty() {
            return Err("the profile declares no PV array with a weather block".to_string());
        }
        let step_s = hourly_step_s(forecast)?;
        let meta = PayloadMeta {
            generated_at: now,
            valid_until: now + self.settings.freshness,
            model_version: self.settings.model_version.clone(),
            producer_id: self.settings.producer_id.clone(),
            confidence: mean_slot_confidence(&forecast.samples),
        };
        let grid = SeriesGrid {
            start: forecast.samples[0].valid_at,
            step_s,
        };
        let mut publications = Vec::new();
        let mut total = vec![0.0; forecast.samples.len()];
        for array in self.settings.arrays.clone() {
            let values = self.array_values(&array, forecast, now)?;
            for (sum, v) in total.iter_mut().zip(&values) {
                *sum += v;
            }
            let topic = self
                .settings
                .topics
                .pv_array(&array.id)
                .map_err(|e| e.to_string())?;
            publications.push(build_publication(
                topic,
                ForecastPayload::pv_natural(meta.clone(), grid, values),
            )?);
        }
        publications.push(build_publication(
            self.settings.topics.pv_total(),
            ForecastPayload::pv_natural(meta, grid, total),
        )?);
        Ok(publications)
    }

    /// One array's series at `now`, after its snow state has looked at the live measurement.
    fn array_values(
        &mut self,
        array: &PvArraySettings,
        forecast: &WeatherForecast,
        now: DateTime<Utc>,
    ) -> Result<Vec<f64>, String> {
        let prior = self.snow.get(&array.id).copied().unwrap_or_default();
        let measured_kw = if array.measured {
            resolve_measured_kw(self.pv_measurement, now, MEASUREMENT_STALENESS_THRESHOLD)
        } else {
            None
        };
        // The snow-free reference of a slot does not depend on the snow state, so the series under the
        // prior state is the right thing to look at; the published one is computed under the new state.
        let series = weather_pv_forecast_series(&array.params, forecast, prior);
        let state = observe_snow(
            prior,
            SnowObservationInput {
                series: &series,
                now,
                measured_kw,
                generation_limit_kw: None,
                rated_kw: array.rated_kw,
            },
        );
        self.snow.insert(array.id.clone(), state);
        let series = weather_pv_forecast_series(&array.params, forecast, state);
        Ok(series.iter().map(|s| s.forecast_ac_kw).collect())
    }
}

/// A payload (built by one of `ForecastPayload`'s constructors, so already validated) as a publication.
fn build_publication(
    topic: String,
    payload: Result<ForecastPayload, PayloadError>,
) -> Result<Publication, String> {
    let payload = payload.map_err(|e| format!("{topic}: {e}"))?;
    Ok(Publication {
        topic,
        payload: serde_json::to_vec(&payload).expect("a payload always serialises"),
    })
}

/// The step of a weather forecast: one hour when it has a single sample, else the spacing, which must be
/// the same everywhere (a missing hour would shift every later value).
fn hourly_step_s(forecast: &WeatherForecast) -> Result<u32, String> {
    let samples = &forecast.samples;
    if samples.is_empty() {
        return Err("the weather forecast has no samples".to_string());
    }
    if samples.len() == 1 {
        return Ok(3600);
    }
    let step = (samples[1].valid_at - samples[0].valid_at).num_seconds();
    let even = samples
        .windows(2)
        .all(|w| (w[1].valid_at - w[0].valid_at).num_seconds() == step);
    match u32::try_from(step) {
        Ok(step_s) if step_s > 0 && even => Ok(step_s),
        _ => Err("the weather samples are not evenly spaced".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use lab_forecast::forecast_payload::{ForecastPayload, ForecastStatus, SnowState};
    use lab_forecast::pv_profile::pv_arrays_from_yaml;
    use lab_forecast::weather::{GeoPosition, WeatherForecastSample};

    fn at(y: i32, mo: u32, d: u32, h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, m, 0).unwrap()
    }

    /// PV arrays as the VEN profile declares them: `(id, measurement switched on)`.
    fn arrays(spec: &[(&str, bool)]) -> Vec<PvArraySettings> {
        let mut yaml = String::from("assets:\n");
        for (id, measured) in spec {
            yaml.push_str(&format!(
                "  - type: pv\n    id: {id}\n    rated_kw: 10.0\n    weather:\n      latitude_deg: 47.4491\n      longitude_deg: 7.8081\n      tilt_deg: 30.0\n      azimuth_deg: 180.0\n      rated_kwp: 10.0\n    measurement: {{enabled: {measured}}}\n"
            ));
        }
        pv_arrays_from_yaml(&yaml).unwrap()
    }

    fn settings(spec: &[(&str, bool)]) -> EngineSettings {
        EngineSettings {
            topics: ForecastTopics::new("openadr-lab", "zunzgen").unwrap(),
            arrays: arrays(spec),
            producer_id: "forecaster-a".into(),
            model_version: "pv-weather-1".into(),
            freshness: Duration::minutes(90),
            base_load_enabled: false,
        }
    }

    fn settings_with_base_load(spec: &[(&str, bool)]) -> EngineSettings {
        EngineSettings {
            base_load_enabled: true,
            ..settings(spec)
        }
    }

    /// One reading a minute for `days` before `end`, constant `kw`.
    fn feed_base_load(e: &mut Engine, end: DateTime<Utc>, days: i64, kw: f64) {
        let mut ts = end - Duration::days(days);
        while ts < end {
            e.on_base_load_measurement((kw, ts));
            ts += Duration::minutes(1);
        }
    }

    const BASE_LOAD_TOPIC: &str = "openadr-lab/forecast/zunzgen/base_load/power_kw";

    /// `hours` hourly samples from `first`, all with the same temperature and irradiance.
    fn weather(
        first: DateTime<Utc>,
        hours: u32,
        temperature_c: f64,
        ghi_w_m2: f64,
        fetched_at: DateTime<Utc>,
    ) -> WeatherForecast {
        WeatherForecast {
            source_id: "test".into(),
            location: GeoPosition {
                latitude_deg: 47.4491,
                longitude_deg: 7.8081,
            },
            fetched_at,
            samples: (0..hours)
                .map(|i| WeatherForecastSample {
                    valid_at: first + Duration::hours(i as i64),
                    age_h: i,
                    temperature_c,
                    ghi_w_m2,
                    wind_speed_kmh: None,
                    rain_prob_pct: None,
                    new_snowfall_cm: None,
                    sky_condition: None,
                    irradiance_variability: Some(0.0),
                })
                .collect(),
        }
    }

    fn summer(fetched_at: DateTime<Utc>) -> WeatherForecast {
        weather(at(2026, 6, 21, 0, 0), 48, 20.0, 600.0, fetched_at)
    }

    fn payload_of(pubs: &[Publication], topic: &str) -> ForecastPayload {
        let p = pubs
            .iter()
            .find(|p| p.topic == topic)
            .unwrap_or_else(|| panic!("no publication on {topic}"));
        ForecastPayload::parse(&p.payload).unwrap()
    }

    fn engine_with_summer(spec: &[(&str, bool)], now: DateTime<Utc>) -> Engine {
        let mut e = Engine::new(settings(spec));
        e.on_weather(summer(now - Duration::minutes(10)));
        e
    }

    #[test]
    fn recompute_without_weather_publishes_nothing_and_the_state_is_cold_start() {
        let mut e = Engine::new(settings(&[("pv", false)]));
        assert!(e.recompute(at(2026, 6, 21, 12, 0)).is_empty());
        assert_eq!(e.state(), ForecastState::ColdStart);
    }

    #[test]
    fn recompute_publishes_every_array_and_the_total() {
        let now = at(2026, 6, 21, 12, 0);
        let mut e = engine_with_summer(&[("pv-1", false), ("pv-2", false)], now);
        let pubs = e.recompute(now);
        let topics: Vec<&str> = pubs.iter().map(|p| p.topic.as_str()).collect();
        assert_eq!(
            topics,
            [
                "openadr-lab/forecast/zunzgen/pv/pv-1/power_kw",
                "openadr-lab/forecast/zunzgen/pv/pv-2/power_kw",
                "openadr-lab/forecast/zunzgen/pv/total/power_kw",
            ]
        );
        assert_eq!(e.state(), ForecastState::Ok);
    }

    #[test]
    fn the_published_series_is_what_the_shared_function_gives() {
        let now = at(2026, 6, 21, 12, 0);
        let mut e = engine_with_summer(&[("pv", false)], now);
        let pubs = e.recompute(now);
        let arr = &settings(&[("pv", false)]).arrays[0];
        let expected: Vec<f64> =
            weather_pv_forecast_series(&arr.params, &summer(now), PvSnowState::default())
                .iter()
                .map(|s| s.forecast_ac_kw)
                .collect();
        let got = payload_of(&pubs, "openadr-lab/forecast/zunzgen/pv/pv/power_kw");
        assert_eq!(got.values, expected);
        assert!(
            got.values.iter().any(|v| *v > 1.0),
            "a June noon must produce"
        );
    }

    #[test]
    fn the_payload_states_start_step_coverage_and_freshness() {
        let now = at(2026, 6, 21, 12, 0);
        let mut e = engine_with_summer(&[("pv", false)], now);
        let got = payload_of(
            &e.recompute(now),
            "openadr-lab/forecast/zunzgen/pv/pv/power_kw",
        );
        assert_eq!(got.start, at(2026, 6, 21, 0, 0));
        assert_eq!(got.step_s, 3600);
        assert!((got.coverage_h - 48.0).abs() < 1e-9);
        assert_eq!(got.generated_at, now);
        assert_eq!(got.valid_until, now + Duration::minutes(90));
        assert_eq!(got.model_version, "pv-weather-1");
        assert_eq!(got.producer_id, "forecaster-a");
    }

    #[test]
    fn a_short_weather_horizon_is_published_as_it_is_without_padding() {
        let now = at(2026, 6, 21, 12, 0);
        let mut e = Engine::new(settings(&[("pv", false)]));
        e.on_weather(weather(
            at(2026, 6, 21, 0, 0),
            30,
            20.0,
            600.0,
            now - Duration::minutes(10),
        ));
        let got = payload_of(
            &e.recompute(now),
            "openadr-lab/forecast/zunzgen/pv/pv/power_kw",
        );
        assert_eq!(got.values.len(), 30);
        assert!((got.coverage_h - 30.0).abs() < 1e-9);
    }

    #[test]
    fn the_total_is_the_sum_of_the_arrays_slot_by_slot() {
        let now = at(2026, 6, 21, 12, 0);
        let mut e = engine_with_summer(&[("pv-1", false), ("pv-2", false)], now);
        let pubs = e.recompute(now);
        let a = payload_of(&pubs, "openadr-lab/forecast/zunzgen/pv/pv-1/power_kw");
        let b = payload_of(&pubs, "openadr-lab/forecast/zunzgen/pv/pv-2/power_kw");
        let total = payload_of(&pubs, "openadr-lab/forecast/zunzgen/pv/total/power_kw");
        let sum: Vec<f64> = a.values.iter().zip(&b.values).map(|(x, y)| x + y).collect();
        assert_eq!(total.values, sum);
    }

    #[test]
    fn an_unchanged_series_is_republished_with_a_newer_generated_at() {
        let now = at(2026, 6, 21, 12, 0);
        let mut e = engine_with_summer(&[("pv", false)], now);
        let first = payload_of(
            &e.recompute(now),
            "openadr-lab/forecast/zunzgen/pv/pv/power_kw",
        );
        let later = now + Duration::minutes(15);
        let second = payload_of(
            &e.recompute(later),
            "openadr-lab/forecast/zunzgen/pv/pv/power_kw",
        );
        assert_eq!(second.values, first.values);
        assert_eq!(second.generated_at, later);
        assert_eq!(second.valid_until, later + Duration::minutes(90));
    }

    #[test]
    fn a_stale_weather_forecast_publishes_nothing_and_the_state_is_weather_stale() {
        let now = at(2026, 6, 21, 12, 0);
        let mut e = Engine::new(settings(&[("pv", false)]));
        e.on_weather(summer(now - Duration::minutes(91)));
        assert!(e.recompute(now).is_empty());
        assert_eq!(e.state(), ForecastState::WeatherStale);
    }

    #[test]
    fn a_weather_forecast_with_uneven_hours_is_degraded_not_published() {
        let now = at(2026, 6, 21, 12, 0);
        let mut w = summer(now - Duration::minutes(10));
        w.samples.remove(5);
        let mut e = Engine::new(settings(&[("pv", false)]));
        e.on_weather(w);
        assert!(e.recompute(now).is_empty());
        assert_eq!(e.state(), ForecastState::Degraded);
    }

    #[test]
    fn a_profile_without_a_weather_pv_array_is_degraded() {
        let now = at(2026, 6, 21, 12, 0);
        let mut e = engine_with_summer(&[], now);
        assert!(e.recompute(now).is_empty());
        assert_eq!(e.state(), ForecastState::Degraded);
    }

    /// January noon, cold and bright: the weather says about 5 kW, the panel must read 0 if covered.
    fn winter_engine(measured_kw: Option<f64>, now: DateTime<Utc>) -> Engine {
        let mut e = Engine::new(settings(&[("pv", true)]));
        e.on_weather(weather(
            at(2026, 1, 15, 8, 0),
            8,
            -3.0,
            500.0,
            now - Duration::minutes(10),
        ));
        if let Some(kw) = measured_kw {
            e.on_pv_measurement((kw, now - Duration::minutes(1)));
        }
        e
    }

    fn noon_value(pubs: &[Publication]) -> f64 {
        // 08:00 + 4 h = 12:00
        payload_of(pubs, "openadr-lab/forecast/zunzgen/pv/pv/power_kw").values[4]
    }

    #[test]
    fn a_panel_measured_far_below_the_weather_is_published_covered() {
        let now = at(2026, 1, 15, 12, 0);
        let free = noon_value(&winter_engine(None, now).recompute(now));
        let covered = noon_value(&winter_engine(Some(0.0), now).recompute(now));
        assert!(
            free > 1.0,
            "the snow-free forecast is the reference: {free}"
        );
        assert_eq!(covered, 0.0);
    }

    #[test]
    fn the_covered_state_is_kept_between_recomputes_while_the_air_stays_cold() {
        let now = at(2026, 1, 15, 12, 0);
        let mut e = winter_engine(Some(0.0), now);
        e.recompute(now);
        let later = now + Duration::minutes(15);
        // The measurement is now too old to count; the state concluded earlier must hold in the cold.
        assert_eq!(noon_value(&e.recompute(later + Duration::minutes(10))), 0.0);
    }

    #[test]
    fn status_reports_snow_state_per_array_observed_only_where_a_measurement_is_configured() {
        let now = at(2026, 6, 21, 12, 0);
        let mut e = engine_with_summer(&[("pv-1", true), ("pv-2", false)], now);
        e.recompute(now);
        let s = ForecastStatus::parse(&e.status(now).payload).unwrap();
        assert_eq!(s.snow_state["pv-1"], SnowState::Observed);
        assert_eq!(s.snow_state["pv-2"], SnowState::Unobserved);
    }

    #[test]
    fn status_reports_producer_state_and_the_age_of_each_input() {
        let now = at(2026, 6, 21, 12, 0);
        let mut e = Engine::new(settings(&[("pv", true)]));
        e.on_weather(summer(now - Duration::minutes(10)));
        e.on_pv_measurement((2.0, now - Duration::seconds(30)));
        e.recompute(now);
        let s = ForecastStatus::parse(&e.status(now).payload).unwrap();
        assert_eq!(e.status(now).topic, "openadr-lab/forecast/zunzgen/status");
        assert_eq!(s.producer_id, "forecaster-a");
        assert_eq!(s.state, ForecastState::Ok);
        assert_eq!(s.generated_at, now);
        assert_eq!(s.input_age_s["weather"], Some(600));
        assert_eq!(s.input_age_s["pv_measurement"], Some(30));
    }

    #[test]
    fn status_names_an_input_that_never_arrived() {
        let now = at(2026, 6, 21, 12, 0);
        let e = Engine::new(settings(&[("pv", true)]));
        let s = ForecastStatus::parse(&e.status(now).payload).unwrap();
        assert_eq!(s.state, ForecastState::ColdStart);
        assert_eq!(s.input_age_s["weather"], None);
        assert_eq!(s.input_age_s["pv_measurement"], None);
    }

    #[test]
    fn base_load_is_published_as_cold_start_without_any_history_even_without_weather() {
        let now = at(2026, 7, 14, 12, 30);
        let mut e = Engine::new(settings_with_base_load(&[("pv", false)]));
        let pubs = e.recompute(now);
        let p = payload_of(&pubs, BASE_LOAD_TOPIC);
        assert_eq!(p.model_version, "base-load-cold-start-1");
        assert_eq!(p.values.len(), 48);
        assert_eq!(p.step_s, 3600);
        assert_eq!(p.start, at(2026, 7, 14, 12, 0));
        assert_eq!(
            p.sign,
            lab_forecast::forecast_payload::Sign::ConsumptionPositive
        );
        assert_eq!(p.curtailment, None);
        assert_eq!(e.state(), ForecastState::ColdStart);
    }

    #[test]
    fn base_load_is_learned_once_the_history_is_enough_and_the_state_is_ok() {
        let now = at(2026, 7, 14, 12, 30);
        let mut e = Engine::new(settings_with_base_load(&[("pv", false)]));
        e.on_weather(summer(now - Duration::minutes(10)));
        feed_base_load(&mut e, now, 14, 0.6);
        let pubs = e.recompute(now);
        let p = payload_of(&pubs, BASE_LOAD_TOPIC);
        assert_eq!(p.model_version, "base-load-profile-1");
        assert!(
            p.values.iter().all(|v| (*v - 0.6).abs() < 0.05),
            "{:?}",
            &p.values[..4]
        );
        assert_eq!(p.generated_at, now);
        assert_eq!(p.valid_until, now + Duration::minutes(90));
        assert_eq!(e.state(), ForecastState::Ok);
    }

    #[test]
    fn base_load_is_still_published_when_the_weather_is_stale() {
        let now = at(2026, 7, 14, 12, 30);
        let mut e = Engine::new(settings_with_base_load(&[("pv", false)]));
        e.on_weather(summer(now - Duration::minutes(200)));
        let pubs = e.recompute(now);
        assert!(pubs.iter().any(|p| p.topic == BASE_LOAD_TOPIC));
        assert!(
            pubs.iter().all(|p| !p.topic.contains("/pv/")),
            "no PV on stale weather"
        );
        assert_eq!(e.state(), ForecastState::WeatherStale);
    }

    #[test]
    fn no_base_load_topic_when_the_profile_has_no_base_load_measurement() {
        let now = at(2026, 7, 14, 12, 30);
        let mut e = engine_with_summer(&[("pv", false)], now);
        assert!(e.recompute(now).iter().all(|p| p.topic != BASE_LOAD_TOPIC));
    }

    #[test]
    fn status_names_the_age_of_the_base_load_measurement() {
        let now = at(2026, 7, 14, 12, 30);
        let mut e = Engine::new(settings_with_base_load(&[("pv", false)]));
        assert_eq!(
            ForecastStatus::parse(&e.status(now).payload)
                .unwrap()
                .input_age_s["base_load_measurement"],
            None
        );
        e.on_base_load_measurement((0.5, now - Duration::seconds(40)));
        assert_eq!(
            ForecastStatus::parse(&e.status(now).payload)
                .unwrap()
                .input_age_s["base_load_measurement"],
            Some(40)
        );
    }

    #[test]
    fn base_load_history_survives_a_restart_through_export_and_import() {
        let now = at(2026, 7, 14, 12, 30);
        let mut a = Engine::new(settings_with_base_load(&[("pv", false)]));
        feed_base_load(&mut a, now, 14, 0.6);
        let saved = a.base_load_history();
        let mut b = Engine::new(settings_with_base_load(&[("pv", false)]));
        b.restore_base_load_history(&saved);
        let p = payload_of(&b.recompute(now), BASE_LOAD_TOPIC);
        assert_eq!(p.model_version, "base-load-profile-1");
    }
}
