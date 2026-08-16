mod base_load_preview;
pub mod energy;
pub mod forecast;
mod grid_meter;
pub mod persist;
pub mod plan_context;
pub mod power_model;
mod pv_preview;
mod pv_smoothing;
mod snapshot;

use chrono::{DateTime, Utc};
use rand::{rngs::StdRng, SeedableRng};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::assets::{
    AssetConfig, AssetHistoryBuffer, AssetState, BaseLoad, Battery, EvCharger, Grid, Heater,
    PvInverter,
};
use crate::controller::simulator_port::{SimSnapshot, SimulatorPort, SnapshotError};
use crate::entities::asset_params::AssetParams;
use energy::EnergyCounter;
pub use pv_smoothing::PvSmoothingState;

/// Tracks the user-induced base load perturbation between ticks.
///
/// While the user drags the base load slider, the offset is set to
/// `slider_value − baseline_kw_profile`. After release the offset decays
/// exponentially (EMA with factor `base_load_alpha`) until it reaches zero.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BaseLoadSmoothingState {
    /// Perturbation above (or below) the profile baseline (kW). Zero = no override.
    pub load_offset_kw: f64,
}

/// One entry in the generic asset list.
/// Config is NOT stored here — it lives in `SimState.asset_configs` (parallel by index).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetEntry {
    pub id: String,
    /// Mutable physics state. Written by the dispatcher every tick.
    pub state: AssetState,
    /// Last commanded setpoint (kW, signed).
    pub setpoint_kw: f64,
    /// Actual power from the last tick (kW). Positive = import, negative = export.
    pub last_power_kw: f64,
    /// Cumulative energy for this asset since startup.
    pub energy: EnergyCounter,
    /// Per-asset history ring buffer. Initialized empty in CP1; wired in CP2.
    /// Ephemeral — not persisted to disk.
    #[serde(skip, default = "default_history_buffer")]
    pub history: AssetHistoryBuffer,
}

/// Grid-level totals derived by summing all asset powers.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GridMeter {
    pub net_power_w: f64,
    pub import_w: f64,
    pub export_w: f64,
    pub voltage_v: f64,
    pub import_kwh: f64,
    pub export_kwh: f64,
}

/// Full simulator state — persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimState {
    /// Physics config — parallel to `assets` by index, loaded from profile.
    pub asset_configs: Vec<AssetConfig>,
    /// Mutable state + history for each asset.
    pub assets: Vec<AssetEntry>,
    pub grid: GridMeter,
    /// Grid virtual asset — implements the full `Asset` trait (id, current_state,
    /// history, capability). Updated each tick with net power + VTN limits.
    /// Not part of `asset_configs` / `assets` (Grid is read-only, not dispatched).
    #[serde(skip, default)]
    pub grid_asset: Grid,
    /// PV irradiance EMA state for Behaviour B smoothing. Ephemeral — resets on restart.
    #[serde(skip, default)]
    pub pv_smoothing: PvSmoothingState,
    /// Base load EMA state for Behaviour B smoothing. Ephemeral — resets on restart.
    #[serde(skip, default)]
    pub base_load_smoothing: BaseLoadSmoothingState,
    /// Peak of the deterministic diurnal unmodelled load added to the derived
    /// grid meter but to no asset (kW); 0.0 disables. Gives `site-residual` a
    /// real, learnable signal — otherwise the meter is the exact sum of the
    /// modelled assets and the residual is structurally 0. Set from the
    /// profile at startup (`persist::load_with_params`), not persisted state.
    #[serde(skip, default)]
    pub unmodelled_load_kw: f64,
    pub last_tick: DateTime<Utc>,
    #[serde(skip, default = "StdRng::from_entropy")]
    pub rng: StdRng, // R-24: seeds power_model::random_voltage; reseeded fresh on load
}

/// Deterministic diurnal unmodelled-load curve: 0 at 06:00, `peak_kw` at
/// 18:00, smooth cosine in between. Pure function of the injected clock so
/// simulation stays reproducible (no RNG).
pub fn unmodelled_load_at(now: DateTime<Utc>, peak_kw: f64) -> f64 {
    if peak_kw == 0.0 {
        return 0.0;
    }
    let secs = now.timestamp().rem_euclid(86_400) as f64;
    let hour = secs / 3600.0;
    peak_kw * 0.5 * (1.0 - (std::f64::consts::PI * (hour - 6.0) / 12.0).cos())
}

impl SimState {
    /// Look up an asset entry by id (immutable).
    pub fn asset(&self, id: &str) -> Option<&AssetEntry> {
        self.assets.iter().find(|a| a.id == id)
    }

    /// Look up an asset entry by id (mutable).
    pub fn asset_mut(&mut self, id: &str) -> Option<&mut AssetEntry> {
        self.assets.iter_mut().find(|a| a.id == id)
    }

    /// Look up entry + config by id (immutable).
    pub fn find_asset(&self, id: &str) -> Option<(&AssetEntry, &AssetConfig)> {
        self.assets
            .iter()
            .zip(self.asset_configs.iter())
            .find(|(e, _)| e.id == id)
    }

    /// Look up entry + config by id (mutable). Uses index to satisfy borrow checker.
    pub fn find_asset_mut(&mut self, id: &str) -> Option<(&mut AssetEntry, &mut AssetConfig)> {
        let idx = self.assets.iter().position(|a| a.id == id)?;
        Some((&mut self.assets[idx], &mut self.asset_configs[idx]))
    }

    /// Iterator over (entry, config) pairs — parallel by index.
    pub fn iter_assets(&self) -> impl Iterator<Item = (&AssetEntry, &AssetConfig)> {
        self.assets.iter().zip(self.asset_configs.iter())
    }

    /// Initialize from domain asset parameters.
    pub fn from_params(params: &[AssetParams], now: DateTime<Utc>) -> Self {
        Self::from_params_seeded(params, now, StdRng::from_entropy())
    }
    /// Like `from_params`, but with an explicit RNG for deterministic tests (R-24).
    pub fn from_params_seeded(params: &[AssetParams], now: DateTime<Utc>, rng: StdRng) -> Self {
        let mut configs: Vec<AssetConfig> = Vec::new();
        let mut entries: Vec<AssetEntry> = Vec::new();

        for ap in params {
            let (id, cfg, state) = asset_config_and_state_from_params(ap);
            let setpoint_kw = cfg.default_setpoint(&state);
            entries.push(AssetEntry {
                id,
                state,
                setpoint_kw,
                last_power_kw: 0.0,
                energy: EnergyCounter::new(),
                history: AssetHistoryBuffer::new(3600),
            });
            configs.push(cfg);
        }

        Self {
            asset_configs: configs,
            assets: entries,
            grid: GridMeter::default(),
            grid_asset: Grid::new(),
            pv_smoothing: PvSmoothingState::default(),
            base_load_smoothing: BaseLoadSmoothingState::default(),
            unmodelled_load_kw: 0.0,
            rng,
            last_tick: now,
        }
    }

    /// Run one simulation tick.
    ///
    /// Inject parameters implement Behaviour B (pv_irradiance + EMA smoothing) and
    /// Behaviour C (frozen env/state while active, snap-back on release):
    /// - `pv_irradiance_override`: if Some, freeze PV irradiance; if None and was active,
    ///   EMA-blend back to natural model at rate `pv_alpha` (0.0–1.0; default 0.1).
    /// - `ambient_temp_c_override`: if Some, override heater ambient temp; else use 10.0°C.
    /// - `base_load_kw_override`: if Some, one-shot: captures offset then cleared by sim loop.
    /// - `base_load_alpha`: EMA factor for base load blend-back (0.0–1.0; default 0.1).
    /// - `ev_plugged_override`: if Some, hold EV plugged state; else let physics drive it.
    /// - `weather_pv_kw`: weather-sourced actual PV power (kW, generation-positive), via
    ///   `entities::solar::resolve_weather_pv_kw` (R-50). A forced `pv_irradiance_override`
    ///   (the tick it's posted) wins outright, weather ignored; otherwise weather (if
    ///   configured/fresh) is the base with any decaying override offset blended
    ///   additively on top; sin model only when no weather is configured at all.
    /// - `heater_emergency_curtail/absorb_override`: Behaviour C, see `Heater::apply_tick_overrides`.
    /// - `pv_measured_kw`/`base_load_measured_kw`: real-measurement MQTT feeds;
    ///   PV outranks `weather_pv_kw`, BaseLoad replaces the natural profile+noise base.
    /// - `base_load_heuristic_kw`: BL-40's 3rd fallback tier — the site's learned
    ///   base-load heuristic (`AssetHeuristics::sample_kw`), used for `natural_base_kw`
    ///   only when `base_load_measured_kw` is absent; the synthetic spike model
    ///   remains the true last resort when neither is available.
    ///
    /// See `peek_pv_kw` (`pv_preview.rs`) for a read-only preview of this tick's PV term.
    #[allow(clippy::too_many_arguments)]
    pub fn tick(
        &mut self,
        dt_s: f64,
        setpoints: HashMap<String, f64>,
        now: DateTime<Utc>,
        pv_irradiance_override: Option<f64>,
        pv_alpha: f64,
        ambient_temp_c_override: Option<f64>,
        heater_temp_min_override: Option<f64>,
        heater_temp_max_override: Option<f64>,
        base_load_kw_override: Option<f64>,
        base_load_alpha: f64,
        ev_plugged_override: Option<bool>,
        ev_soc_target_override: Option<f64>,
        weather_pv_kw: Option<f64>,
        heater_emergency_curtail_override: Option<bool>,
        heater_emergency_absorb_override: Option<bool>,
        pv_generation_limit_override: Option<f64>,
        pv_curtailment_source: crate::entities::asset_params::PvCurtailmentSource,
        pv_measured_kw: Option<f64>,
        base_load_measured_kw: Option<f64>,
        base_load_heuristic_kw: Option<f64>,
    ) {
        let hour = now.format("%H").to_string().parse::<f64>().unwrap_or(12.0)
            + now.format("%M").to_string().parse::<f64>().unwrap_or(0.0) / 60.0;

        let natural_irradiance = if (6.0..=18.0).contains(&hour) {
            let angle = std::f64::consts::PI * (hour - 6.0) / 12.0;
            angle.sin()
        } else {
            0.0
        };

        // Alpha decay factor shared by all Behaviour B controls.
        // Converts per-plan-step alpha (one step = 300 s) to a per-tick factor so the
        // offset reaches (1−alpha) × original after exactly one plan step, matching
        // the forecast formula exp(−t/tau_s).
        const PLAN_STEP_S: f64 = 300.0;

        // Behaviour B — PV perturbation overlay.
        let irradiance =
            self.pv_smoothing
                .update(pv_irradiance_override, natural_irradiance, dt_s, pv_alpha);

        let dt = chrono::Duration::milliseconds((dt_s * 1000.0) as i64);
        let mut total_kw = 0.0;

        for (cfg, entry) in self.asset_configs.iter_mut().zip(self.assets.iter_mut()) {
            // ── Apply environment and Behaviour C state injections ────────
            match cfg {
                AssetConfig::Pv(pv) => {
                    pv.irradiance = irradiance;
                    pv.irradiance_offset = self.pv_smoothing.irradiance_offset;
                    pv.pv_alpha = pv_alpha;
                    pv.generation_limit_kw = pv_generation_limit_override;
                    pv.curtailment_source = pv_curtailment_source;
                    // Weather is never nulled by a manual override anymore — a
                    // recently-released override's decaying irradiance_offset
                    // blends additively on top of it instead (see
                    // PvInverter::step_inner). Only a forced override (this
                    // exact tick) takes exclusive control.
                    pv.weather_power_kw = weather_pv_kw;
                    pv.measured_power_kw = pv_measured_kw;
                    pv.irradiance_forced = pv_irradiance_override.is_some();
                }
                AssetConfig::Heater(h) => h.apply_tick_overrides(
                    ambient_temp_c_override,
                    heater_temp_min_override,
                    heater_temp_max_override,
                    heater_emergency_curtail_override,
                    heater_emergency_absorb_override,
                ),
                AssetConfig::BaseLoad(bl) => {
                    // Behaviour B: base load — one-shot sets offset; EMA decays it back.
                    // `natural_base_kw` (profile + simulated appliance noise) plays the
                    // same role here that `natural_irradiance` plays for PV above: the
                    // override folds into the offset relative to it, so a forced value
                    // lands exactly on `forced_kw` — not `forced_kw` plus a hidden bump.
                    bl.measured_load_kw = base_load_measured_kw;
                    let natural_base_kw = bl
                        .measured_load_kw
                        .or(base_load_heuristic_kw)
                        .unwrap_or_else(|| bl.baseline_kw_profile + bl.appliance_noise_kw(now));
                    if let Some(forced_kw) = base_load_kw_override {
                        self.base_load_smoothing.load_offset_kw = forced_kw - natural_base_kw;
                    } else {
                        let per_tick_factor = (1.0 - base_load_alpha).powf(dt_s / PLAN_STEP_S);
                        self.base_load_smoothing.load_offset_kw *= per_tick_factor;
                        if self.base_load_smoothing.load_offset_kw.abs() < 0.005 {
                            self.base_load_smoothing.load_offset_kw = 0.0;
                        }
                    }
                    bl.baseline_kw =
                        (natural_base_kw + self.base_load_smoothing.load_offset_kw).max(0.0);
                }
                AssetConfig::Ev(ev) => {
                    // Behaviour C: ev_plugged — hold override or snap back to profile default
                    // (plugged=true) when released. Without snap-back, releasing the inject
                    // leaves the EV permanently unplugged because there is no physics to
                    // re-plug it.
                    if let AssetState::Ev(s) = &mut entry.state {
                        s.plugged = ev_plugged_override.unwrap_or(true);
                    }
                    // Behaviour C: ev_soc_target — override BMS charge ceiling.
                    ev.soc_target = ev_soc_target_override.unwrap_or(ev.soc_target_profile);
                }
                _ => {}
            }

            // ── Dispatch physics ──────────────────────────────────────────
            let sp = setpoints
                .get(&entry.id)
                .copied()
                .unwrap_or_else(|| cfg.default_setpoint(&entry.state));
            let (new_state, actual_kw) = cfg.step(&entry.state, sp, dt);
            entry.state = new_state;
            entry.last_power_kw = actual_kw;
            entry.setpoint_kw = sp;
            entry.energy.integrate(actual_kw * 1000.0, dt_s);
            total_kw += actual_kw;
        }

        self.derive_grid_meter(total_kw, now, dt_s);
    }
}

// `to_sensor_snapshot`/`to_sim_snapshot`/`to_timeline_snapshot` live in
// `snapshot.rs` to keep this file under the file-size cap.

/// Convert domain asset parameters into (asset_id, AssetConfig, initial AssetState).
fn asset_config_and_state_from_params(ap: &AssetParams) -> (String, AssetConfig, AssetState) {
    match ap {
        AssetParams::Battery(c) => (
            c.id.clone(),
            AssetConfig::Battery(Battery::from_params(c)),
            AssetState::Battery(Battery::initial_state(c)),
        ),
        AssetParams::Ev(c) => (
            c.id.clone(),
            AssetConfig::Ev(EvCharger::from_params(c)),
            AssetState::Ev(EvCharger::initial_state(c)),
        ),
        AssetParams::Heater(c) => (
            c.id.clone(),
            AssetConfig::Heater(Heater::from_params(c)),
            AssetState::Heater(Heater::initial_state(c)),
        ),
        AssetParams::Pv(c) => (
            c.id.clone(),
            AssetConfig::Pv(PvInverter::from_params(c)),
            AssetState::Pv(PvInverter::initial_state(c)),
        ),
        AssetParams::BaseLoad(c) => (
            c.id.clone(),
            AssetConfig::BaseLoad(BaseLoad::from_params(c)),
            AssetState::BaseLoad(BaseLoad::initial_state(c)),
        ),
    }
}

/// Build the sim control schema from domain asset params — no mutex required.
///
/// The schema is static: it depends only on startup configuration, not on runtime
/// simulator state. This allows `GET /sim/schema` to respond without blocking
/// on the sim mutex during MILP solving.
pub fn schema_from_params(
    params: &[AssetParams],
) -> HashMap<String, Vec<crate::assets::ControlDescriptor>> {
    let mut out = HashMap::new();
    for ap in params {
        let (id, cfg, _) = asset_config_and_state_from_params(ap);
        out.insert(id, cfg.control_schema());
    }
    out
}

fn default_history_buffer() -> AssetHistoryBuffer {
    AssetHistoryBuffer::new(3600)
}

impl SimulatorPort for SimState {
    fn snapshot(&self) -> Result<SimSnapshot, SnapshotError> {
        Ok(self.to_sim_snapshot())
    }
}

#[cfg(test)]
mod tests;
