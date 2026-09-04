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
    Asset, AssetHistoryBuffer, AssetState, BaseLoad, Battery, EvCharger, Grid, Heater, PvInverter,
    TickOverrides,
};
use crate::controller::simulator_port::{SimSnapshot, SimulatorPort, SnapshotError};
use crate::entities::asset_params::AssetParams;
use energy::EnergyCounter;
pub use pv_smoothing::PvSmoothingState;
pub use snapshot::{SensorInput, SensorSnapshot};

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

impl BaseLoadSmoothingState {
    /// Alpha decay factor shared by all Behaviour B controls. Converts per-plan-step
    /// alpha (one step = 300 s) to a per-tick factor so the offset reaches
    /// (1−alpha) × original after exactly one plan step, matching the forecast
    /// formula exp(−t/tau_s).
    const PLAN_STEP_S: f64 = 300.0;

    /// Apply this tick's forced value (if any) or decay the offset, returning the
    /// resulting baseline (kW, never negative). Mirrors `PvSmoothingState::update`.
    pub fn update(
        &mut self,
        forced_kw: Option<f64>,
        natural_base_kw: f64,
        dt_s: f64,
        base_load_alpha: f64,
    ) -> f64 {
        self.load_offset_kw =
            self.next_offset_kw(forced_kw, natural_base_kw, dt_s, base_load_alpha);
        Self::baseline_kw(natural_base_kw, self.load_offset_kw)
    }

    /// Combine the natural load with a resolved offset. Trivial, but shared with
    /// `SimState::peek_base_load_kw` on purpose: a missing clamp in one of two
    /// hand-copied implementations is exactly the bug this change was fixing on
    /// the PV side (see `PvInverter::resolve_power_kw`).
    pub fn baseline_kw(natural_base_kw: f64, offset_kw: f64) -> f64 {
        (natural_base_kw + offset_kw).max(0.0)
    }

    /// Pure counterpart of `update` — see `PvSmoothingState::next_offset` for why
    /// the pure/mutating split exists (`peek_base_load_kw` previews a tick without
    /// advancing state).
    pub fn next_offset_kw(
        &self,
        forced_kw: Option<f64>,
        natural_base_kw: f64,
        dt_s: f64,
        base_load_alpha: f64,
    ) -> f64 {
        if let Some(forced_kw) = forced_kw {
            return forced_kw - natural_base_kw;
        }
        let per_tick_factor = (1.0 - base_load_alpha).powf(dt_s / Self::PLAN_STEP_S);
        let decayed = self.load_offset_kw * per_tick_factor;
        if decayed.abs() < 0.005 {
            0.0
        } else {
            decayed
        }
    }
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
///
/// No longer `#[derive(Debug)]`: `Box<dyn Asset>` doesn't implement `Debug`
/// (no supertrait bound requiring it — `AssetHandle` doesn't derive `Debug`
/// today, and adding the bound would force it to). Confirmed nothing
/// actually formats a whole `SimState` with `{:?}` before removing this.
#[derive(Clone, Serialize, Deserialize)]
pub struct SimState {
    /// Physics config — parallel to `assets` by index, loaded from profile.
    ///
    /// `#[serde(skip)]`: `Box<dyn Asset>` can't derive `Deserialize` (no way
    /// to know which concrete type a JSON blob names without extra tagging
    /// machinery). This is safe and, in fact, already how this field
    /// behaved before Spec A: `persist::load_with_params` unconditionally
    /// overwrites the deserialized value with `SimState::from_params(...)`'s
    /// fresh one immediately after every load
    /// (`loaded.asset_configs = fresh.asset_configs`) — the persisted value
    /// was already discarded, never actually used, kept only because
    /// `AssetConfig` happened to derive `Serialize`/`Deserialize`. Old
    /// persisted files with this field present still load fine — serde
    /// ignores unknown fields by default (no `deny_unknown_fields` here).
    #[serde(skip, default)]
    pub asset_configs: Vec<Box<dyn Asset>>,
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
    pub last_tick: DateTime<Utc>,
    #[serde(skip, default = "StdRng::from_entropy")]
    pub rng: StdRng, // R-24: seeds power_model::random_voltage; reseeded fresh on load
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
    pub fn find_asset(&self, id: &str) -> Option<(&AssetEntry, &dyn Asset)> {
        self.assets
            .iter()
            .zip(self.asset_configs.iter())
            .find(|(e, _)| e.id == id)
            .map(|(e, c)| (e, c.as_ref()))
    }

    /// Look up entry + config by id (mutable). Uses index to satisfy borrow checker.
    pub fn find_asset_mut(&mut self, id: &str) -> Option<(&mut AssetEntry, &mut dyn Asset)> {
        let idx = self.assets.iter().position(|a| a.id == id)?;
        Some((&mut self.assets[idx], self.asset_configs[idx].as_mut()))
    }

    /// Iterator over (entry, config) pairs — parallel by index.
    pub fn iter_assets(&self) -> impl Iterator<Item = (&AssetEntry, &dyn Asset)> {
        self.assets
            .iter()
            .zip(self.asset_configs.iter().map(|c| c.as_ref()))
    }

    /// Initialize from domain asset parameters.
    pub fn from_params(params: &[AssetParams], now: DateTime<Utc>) -> Self {
        Self::from_params_seeded(params, now, StdRng::from_entropy())
    }
    /// Like `from_params`, but with an explicit RNG for deterministic tests (R-24).
    pub fn from_params_seeded(params: &[AssetParams], now: DateTime<Utc>, rng: StdRng) -> Self {
        let mut configs: Vec<Box<dyn Asset>> = Vec::new();
        let mut entries: Vec<AssetEntry> = Vec::new();

        for ap in params {
            let (id, cfg, state) = asset_config_and_state_from_params(ap);
            let setpoint_kw = cfg.default_setpoint();
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
        let natural_irradiance = crate::entities::solar::natural_irradiance_at(now);

        // Behaviour B — PV perturbation overlay.
        let irradiance =
            self.pv_smoothing
                .update(pv_irradiance_override, natural_irradiance, dt_s, pv_alpha);

        // Behaviour B — base load perturbation overlay. Hoisted out of the
        // per-asset loop below (Spec A Phase 2b prerequisite for
        // `TickOverridable`, mirroring how PV's `irradiance` is already
        // resolved above, before any asset-specific match): needs the
        // BaseLoad config's own fields (`baseline_kw_profile`,
        // `appliance_noise_kw`) to compute `natural_base_kw`, so it's found
        // by a read-only pre-pass rather than known ahead of time like PV's
        // natural-irradiance curve is. `None` if no BaseLoad asset is
        // configured, matching today's behavior of the match arm simply
        // never firing in that case.
        let base_load_baseline_kw = self
            .asset_configs
            .iter()
            .find_map(|cfg| cfg.as_any().downcast_ref::<BaseLoad>())
            .map(|bl| bl.natural_base_kw(base_load_measured_kw, base_load_heuristic_kw, now))
            .map(|natural_base_kw| {
                self.base_load_smoothing.update(
                    base_load_kw_override,
                    natural_base_kw,
                    dt_s,
                    base_load_alpha,
                )
            });

        // Bundles this tick's override inputs for `TickOverridable`
        // (design.md Decision D5) — pre-resolved where resolution needs
        // cross-asset state (`pv_irradiance`/`pv_irradiance_offset`,
        // `base_load_baseline_kw`, both resolved above), so each asset's
        // `apply_tick_overrides` only assigns fields, never recomputes
        // smoothing. Built once, shared read-only by every asset in the loop
        // below (Battery declines — no arm in the old match for it either).
        let tick_overrides = TickOverrides {
            pv_irradiance: irradiance,
            pv_irradiance_offset: self.pv_smoothing.irradiance_offset,
            pv_alpha,
            pv_generation_limit_kw: pv_generation_limit_override,
            pv_curtailment_source,
            // Weather is never nulled by a manual override anymore — a
            // recently-released override's decaying irradiance_offset blends
            // additively on top of it instead (see `PvInverter::step_inner`).
            // Only a forced override (this exact tick) takes exclusive control.
            pv_weather_power_kw: weather_pv_kw,
            pv_measured_power_kw: pv_measured_kw,
            pv_irradiance_forced: pv_irradiance_override.is_some(),
            heater_ambient_temp_c_override: ambient_temp_c_override,
            heater_temp_min_override,
            heater_temp_max_override,
            heater_emergency_curtail_override,
            heater_emergency_absorb_override,
            base_load_measured_kw,
            base_load_baseline_kw,
            ev_plugged_override,
            ev_soc_target_override,
        };

        let dt = chrono::Duration::milliseconds((dt_s * 1000.0) as i64);
        let mut total_kw = 0.0;

        for (cfg, entry) in self.asset_configs.iter_mut().zip(self.assets.iter_mut()) {
            // ── Apply environment and Behaviour C state injections ────────
            if let Some(overridable) = cfg.as_tick_overridable() {
                overridable.apply_tick_overrides(&mut entry.state, &tick_overrides);
            }

            // ── Dispatch physics ──────────────────────────────────────────
            let sp = setpoints
                .get(&entry.id)
                .copied()
                .unwrap_or_else(|| cfg.default_setpoint());
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

/// Convert domain asset parameters into (asset_id, boxed Asset, initial AssetState).
fn asset_config_and_state_from_params(ap: &AssetParams) -> (String, Box<dyn Asset>, AssetState) {
    match ap {
        AssetParams::Battery(c) => (
            c.id.clone(),
            Box::new(Battery::from_params(c)),
            AssetState::Battery(Battery::initial_state(c)),
        ),
        AssetParams::Ev(c) => (
            c.id.clone(),
            Box::new(EvCharger::from_params(c)),
            AssetState::Ev(EvCharger::initial_state(c)),
        ),
        AssetParams::Heater(c) => (
            c.id.clone(),
            Box::new(Heater::from_params(c)),
            AssetState::Heater(Heater::initial_state(c)),
        ),
        AssetParams::Pv(c) => (
            c.id.clone(),
            Box::new(PvInverter::from_params(c)),
            AssetState::Pv(PvInverter::initial_state(c)),
        ),
        AssetParams::BaseLoad(c) => (
            c.id.clone(),
            Box::new(BaseLoad::from_params(c)),
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
