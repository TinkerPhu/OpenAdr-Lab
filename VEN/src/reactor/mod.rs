pub mod arbitration;
pub mod fsm;
pub mod interval;
pub mod trace;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::profile::Profile;
use crate::simulator::SimState;
use crate::state::UserOverrides;
use arbitration::{arbitrate, ControlIntent, ReactorMode};
use fsm::ReactorFsm;
use interval::find_active_intervals;
use trace::DecisionTrace;

/// Setpoints computed by the reactor for the simulator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Setpoints {
    pub ev_charge_kw: f64,
    pub heater_kw: f64,
    /// Maximum PV export in kW. None = no limit (full output). Some(x) = clamp output to x kW.
    pub pv_export_limit_kw: Option<f64>,
    pub mode: String,
}

impl Setpoints {
    /// Default setpoints: all devices at their normal operating point.
    /// `overrides.ev_desired_kw` replaces the profile-based idle charge rate when set.
    pub fn defaults(profile: &Profile, overrides: &UserOverrides) -> Self {
        Self {
            ev_charge_kw: overrides.ev_desired_kw.unwrap_or_else(|| {
                profile.devices.ev.as_ref().map(|e| e.max_charge_kw).unwrap_or(0.0)
            }),
            heater_kw: profile
                .devices
                .heater
                .as_ref()
                .map(|h| h.max_kw * 0.5) // default: half power
                .unwrap_or(0.0),
            pv_export_limit_kw: None,
            mode: "IDLE".to_string(),
        }
    }
}

/// Compute a key representing the effective target from the current intent.
/// When this key changes between ticks, the FSM resets to react to the new instruction.
fn target_key(intent: &Option<ControlIntent>, profile: &Profile) -> String {
    match intent {
        None => "IDLE".to_string(),
        Some(ci) => match ci.mode {
            ReactorMode::Price => {
                if ci.value >= profile.reactor.price_high {
                    format!("PRICE_HIGH_{:.4}", ci.value)
                } else if ci.value <= profile.reactor.price_low {
                    format!("PRICE_LOW_{:.4}", ci.value)
                } else {
                    "PRICE_MID".to_string()
                }
            }
            _ => format!("{}_{:.2}", ci.mode, ci.value),
        },
    }
}

/// Whether the current intent requires an active response (setpoints differ from defaults).
fn is_effectively_active(intent: &Option<ControlIntent>, profile: &Profile) -> bool {
    match intent {
        None => false,
        Some(ci) => match ci.mode {
            ReactorMode::Price => {
                ci.value >= profile.reactor.price_high || ci.value <= profile.reactor.price_low
            }
            ReactorMode::Idle => false,
            _ => true, // capacity limits, SIMPLE always require action
        },
    }
}

/// The reactor: evaluates events and computes setpoints for the simulator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reactor {
    pub fsm: ReactorFsm,
    pub trace: DecisionTrace,
    #[serde(skip)]
    last_mode: Option<ReactorMode>,
    #[serde(skip)]
    last_target_key: Option<String>,
}

impl Reactor {
    pub fn new() -> Self {
        Self {
            fsm: ReactorFsm::new(),
            trace: DecisionTrace::new(),
            last_mode: None,
            last_target_key: None,
        }
    }

    /// Evaluate events and produce setpoints.
    pub fn evaluate(
        &mut self,
        events: &[serde_json::Value],
        sim: &SimState,
        profile: &Profile,
        now: DateTime<Utc>,
        dt_s: f64,
        overrides: &UserOverrides,
    ) -> Setpoints {
        let defaults = Setpoints::defaults(profile, overrides);

        // Find currently active intervals
        let active = find_active_intervals(events, now);
        let active_event_names: Vec<String> =
            active.iter().map(|a| a.event_name.clone()).collect();

        // Arbitrate: select winning control intent
        let intent = arbitrate(&active);

        // Detect instruction changes between intervals and reset FSM
        let current_key = target_key(&intent, profile);
        let key_changed = self.last_target_key.as_ref() != Some(&current_key);
        if key_changed {
            if let Some(ref _prev) = self.last_target_key {
                // Target changed — reset FSM so it ramps fresh toward new target
                self.fsm = ReactorFsm::new();
            }
            self.last_target_key = Some(current_key);
        }

        // Mid-range price = target is defaults = no action needed
        let effective_active = is_effectively_active(&intent, profile);

        let (mode, winning_desc) = match &intent {
            Some(ci) => (ci.mode.clone(), Some(ci.description.clone())),
            None => (ReactorMode::Idle, None),
        };

        // FSM transition
        let factor = self.fsm.transition(
            effective_active,
            dt_s,
            profile.reactor.delay_s,
            profile.reactor.ramp_duration_s,
            &profile.reactor.strategy,
        );

        // Compute setpoints from intent
        let mut setpoints = if let Some(ref ci) = intent {
            self.compute_setpoints(ci, factor, profile, sim, overrides)
        } else {
            defaults.clone()
        };
        setpoints.mode = mode.to_string();

        // Apply compliance factor for "partial" strategy
        if profile.reactor.strategy == "partial" && factor > 0.0 {
            let c = profile.reactor.compliance;
            setpoints = self.apply_compliance(&setpoints, &defaults, c);
        }

        // Build constraints list for trace
        let mut constraints = Vec::new();
        if let Some(ref ev) = profile.devices.ev {
            constraints.push(format!("EV max {:.1}kW", ev.max_charge_kw));
        }
        if let Some(ref h) = profile.devices.heater {
            constraints.push(format!(
                "Heater max {:.1}kW, range {:.0}-{:.0}°C",
                h.max_kw, h.temp_min_c, h.temp_max_c
            ));
        }
        if profile.devices.pv.is_some() {
            constraints.push("PV export limit (kW)".to_string());
        }

        // Build reason
        let reason = match (&mode, &self.fsm.state, effective_active) {
            (ReactorMode::Idle, _, _) => "No active events".to_string(),
            (ReactorMode::Price, fsm::FsmState::Idle, false) => {
                format!(
                    "Price ${:.2} in mid-range (low: ${:.2}, high: ${:.2}) — no action",
                    intent.as_ref().map(|ci| ci.value).unwrap_or(0.0),
                    profile.reactor.price_low,
                    profile.reactor.price_high
                )
            }
            (_, fsm::FsmState::Delaying { .. }, _) => {
                format!("Delaying before response (strategy: {})", profile.reactor.strategy)
            }
            (_, fsm::FsmState::Ramping { .. }, _) => {
                format!("Ramping to target (factor: {:.0}%)", factor * 100.0)
            }
            (_, fsm::FsmState::Holding, _) => {
                format!("Holding setpoints for {}", winning_desc.as_deref().unwrap_or("event"))
            }
            (_, fsm::FsmState::RampingBack { .. }, _) => "Ramping back to defaults".to_string(),
            (_, fsm::FsmState::Idle, _) => "Idle".to_string(),
        };

        // Record trace entry
        self.trace.record(
            now,
            &mode,
            &self.fsm.state,
            active_event_names,
            winning_desc,
            &setpoints,
            constraints,
            reason,
        );

        self.last_mode = Some(mode);
        setpoints
    }

    /// Compute target setpoints from a control intent and interpolation factor.
    fn compute_setpoints(
        &self,
        intent: &ControlIntent,
        factor: f64,
        profile: &Profile,
        _sim: &SimState,
        overrides: &UserOverrides,
    ) -> Setpoints {
        let defaults = Setpoints::defaults(profile, overrides);

        let target = match intent.mode {
            ReactorMode::ExportCapLimit => {
                // Reduce export: increase consumption (charge EV, heat more), limit PV output
                let ev_max = profile
                    .devices
                    .ev
                    .as_ref()
                    .map(|e| e.max_charge_kw)
                    .unwrap_or(0.0);
                let heater_max = profile
                    .devices
                    .heater
                    .as_ref()
                    .map(|h| h.max_kw)
                    .unwrap_or(0.0);

                Setpoints {
                    ev_charge_kw: ev_max,
                    heater_kw: heater_max,
                    pv_export_limit_kw: Some(intent.value.max(0.0)), // direct from payload (kW)
                    mode: "EXPORT_CAP".to_string(),
                }
            }
            ReactorMode::ImportCapLimit => {
                // Reduce import: decrease consumption, maximize PV export (no limit)
                Setpoints {
                    ev_charge_kw: 0.0,
                    heater_kw: 0.0,
                    pv_export_limit_kw: None,
                    mode: "IMPORT_CAP".to_string(),
                }
            }
            ReactorMode::Simple => {
                if intent.value < 1.0 {
                    // SIMPLE 0 = curtail: reduce all flexible loads to minimum
                    Setpoints {
                        ev_charge_kw: 0.0,
                        heater_kw: 0.0,
                        pv_export_limit_kw: None, // keep PV exporting
                        mode: "SIMPLE".to_string(),
                    }
                } else {
                    // SIMPLE >= 1 = normal operation
                    defaults.clone()
                }
            }
            ReactorMode::Price => {
                let price = intent.value;
                if price >= profile.reactor.price_high {
                    // High price: reduce flexible loads
                    Setpoints {
                        ev_charge_kw: 0.0,
                        heater_kw: 0.0,
                        pv_export_limit_kw: None,
                        mode: "PRICE".to_string(),
                    }
                } else if price <= profile.reactor.price_low {
                    // Low price: increase loads (valley fill)
                    let ev_max = profile
                        .devices
                        .ev
                        .as_ref()
                        .map(|e| e.max_charge_kw)
                        .unwrap_or(0.0);
                    let heater_max = profile
                        .devices
                        .heater
                        .as_ref()
                        .map(|h| h.max_kw)
                        .unwrap_or(0.0);
                    Setpoints {
                        ev_charge_kw: ev_max,
                        heater_kw: heater_max,
                        pv_export_limit_kw: None,
                        mode: "PRICE".to_string(),
                    }
                } else {
                    // Mid-range price: use defaults
                    defaults.clone()
                }
            }
            ReactorMode::ChargeSetpoint => {
                // Direct EV charge setpoint command — may be negative for V2G discharge
                let max_kw = profile.devices.ev.as_ref().map(|e| e.max_charge_kw).unwrap_or(0.0);
                let target_kw = intent.value.clamp(-max_kw, max_kw);
                Setpoints {
                    ev_charge_kw: target_kw,
                    heater_kw: defaults.heater_kw,
                    pv_export_limit_kw: defaults.pv_export_limit_kw,
                    mode: "CHARGE_SETPOINT".to_string(),
                }
            }
            ReactorMode::Idle => defaults.clone(),
        };

        // Interpolate between defaults and target based on factor
        self.interpolate(&defaults, &target, factor)
    }

    /// Linear interpolation between two setpoint sets.
    /// EV and heater ramp gradually. Export limit is a hard constraint applied
    /// immediately when the target has one (no value to interpolate between None and Some).
    fn interpolate(&self, from: &Setpoints, to: &Setpoints, factor: f64) -> Setpoints {
        let f = factor.clamp(0.0, 1.0);
        Setpoints {
            ev_charge_kw: from.ev_charge_kw + (to.ev_charge_kw - from.ev_charge_kw) * f,
            heater_kw: from.heater_kw + (to.heater_kw - from.heater_kw) * f,
            pv_export_limit_kw: if f > 0.0 { to.pv_export_limit_kw } else { from.pv_export_limit_kw },
            mode: to.mode.clone(),
        }
    }

    /// Apply partial compliance: blend target with defaults.
    fn apply_compliance(
        &self,
        setpoints: &Setpoints,
        defaults: &Setpoints,
        compliance: f64,
    ) -> Setpoints {
        self.interpolate(defaults, setpoints, compliance)
    }

    pub fn trace_entries(&self) -> Vec<trace::TraceEntry> {
        self.trace.entries()
    }

    pub fn trace_last_n(&self, n: usize) -> Vec<trace::TraceEntry> {
        self.trace.last_n(n)
    }
}
