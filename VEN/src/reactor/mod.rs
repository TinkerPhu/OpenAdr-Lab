pub mod arbitration;
pub mod fsm;
pub mod interval;
pub mod trace;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::profile::Profile;
use crate::simulator::SimState;
use arbitration::{arbitrate, ControlIntent, ReactorMode};
use fsm::ReactorFsm;
use interval::find_active_intervals;
use trace::DecisionTrace;

/// Setpoints computed by the reactor for the simulator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Setpoints {
    pub ev_charge_kw: f64,
    pub heater_kw: f64,
    pub pv_curtailment: f64, // 0.0 = no curtailment, 1.0 = full curtailment
    pub mode: String,
}

impl Setpoints {
    /// Default setpoints: all devices at their normal operating point.
    pub fn defaults(profile: &Profile) -> Self {
        Self {
            ev_charge_kw: profile
                .devices
                .ev
                .as_ref()
                .map(|e| e.max_charge_kw)
                .unwrap_or(0.0),
            heater_kw: profile
                .devices
                .heater
                .as_ref()
                .map(|h| h.max_kw * 0.5) // default: half power
                .unwrap_or(0.0),
            pv_curtailment: 0.0,
            mode: "IDLE".to_string(),
        }
    }
}

/// The reactor: evaluates events and computes setpoints for the simulator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reactor {
    pub fsm: ReactorFsm,
    pub trace: DecisionTrace,
    #[serde(skip)]
    last_mode: Option<ReactorMode>,
}

impl Reactor {
    pub fn new() -> Self {
        Self {
            fsm: ReactorFsm::new(),
            trace: DecisionTrace::new(),
            last_mode: None,
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
    ) -> Setpoints {
        let defaults = Setpoints::defaults(profile);

        // Find currently active intervals
        let active = find_active_intervals(events, now);
        let active_event_names: Vec<String> =
            active.iter().map(|a| a.event_name.clone()).collect();

        // Arbitrate: select winning control intent
        let intent = arbitrate(&active);

        let (event_active, mode, winning_desc) = match &intent {
            Some(ci) => (true, ci.mode.clone(), Some(ci.description.clone())),
            None => (false, ReactorMode::Idle, None),
        };

        // FSM transition
        let factor = self.fsm.transition(
            event_active,
            dt_s,
            profile.reactor.delay_s,
            profile.reactor.ramp_duration_s,
            &profile.reactor.strategy,
        );

        // Compute setpoints from intent
        let mut setpoints = if let Some(ref ci) = intent {
            self.compute_setpoints(ci, factor, profile, sim)
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
            constraints.push("PV curtailment 0-100%".to_string());
        }

        // Build reason
        let reason = match (&mode, &self.fsm.state) {
            (ReactorMode::Idle, _) => "No active events".to_string(),
            (_, fsm::FsmState::Delaying { .. }) => {
                format!("Delaying before response (strategy: {})", profile.reactor.strategy)
            }
            (_, fsm::FsmState::Ramping { .. }) => {
                format!("Ramping to target (factor: {:.0}%)", factor * 100.0)
            }
            (_, fsm::FsmState::Holding) => {
                format!("Holding setpoints for {}", winning_desc.as_deref().unwrap_or("event"))
            }
            (_, fsm::FsmState::RampingBack { .. }) => "Ramping back to defaults".to_string(),
            (_, fsm::FsmState::Idle) => "Idle".to_string(),
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
    ) -> Setpoints {
        let defaults = Setpoints::defaults(profile);

        let target = match intent.mode {
            ReactorMode::ExportCapLimit => {
                // Reduce export: increase consumption (charge EV, heat more), curtail PV
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
                    ev_charge_kw: ev_max,        // charge at max
                    heater_kw: heater_max,        // heat at max
                    pv_curtailment: 0.5,          // curtail 50% as fallback
                    mode: "EXPORT_CAP".to_string(),
                }
            }
            ReactorMode::ImportCapLimit => {
                // Reduce import: decrease consumption, maximize PV export
                Setpoints {
                    ev_charge_kw: 0.0,   // stop charging
                    heater_kw: 0.0,      // stop heating (thermostat override may kick in)
                    pv_curtailment: 0.0,  // full PV output
                    mode: "IMPORT_CAP".to_string(),
                }
            }
            ReactorMode::Price => {
                let price = intent.value;
                if price >= profile.reactor.price_high {
                    // High price: reduce flexible loads
                    Setpoints {
                        ev_charge_kw: 0.0,
                        heater_kw: 0.0,
                        pv_curtailment: 0.0,
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
                        pv_curtailment: 0.0,
                        mode: "PRICE".to_string(),
                    }
                } else {
                    // Mid-range price: use defaults
                    defaults.clone()
                }
            }
            ReactorMode::Idle => defaults.clone(),
        };

        // Interpolate between defaults and target based on factor
        self.interpolate(&defaults, &target, factor)
    }

    /// Linear interpolation between two setpoint sets.
    fn interpolate(&self, from: &Setpoints, to: &Setpoints, factor: f64) -> Setpoints {
        let f = factor.clamp(0.0, 1.0);
        Setpoints {
            ev_charge_kw: from.ev_charge_kw + (to.ev_charge_kw - from.ev_charge_kw) * f,
            heater_kw: from.heater_kw + (to.heater_kw - from.heater_kw) * f,
            pv_curtailment: from.pv_curtailment + (to.pv_curtailment - from.pv_curtailment) * f,
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
