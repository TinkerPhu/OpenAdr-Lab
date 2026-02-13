use serde::{Deserialize, Serialize};

/// FSM states for the reactor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FsmState {
    Idle,
    Delaying { elapsed_s: f64, target_s: f64 },
    Ramping { progress: f64, duration_s: f64 },
    Holding,
    RampingBack { progress: f64, duration_s: f64 },
}

impl std::fmt::Display for FsmState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "Idle"),
            Self::Delaying { elapsed_s, target_s } => {
                write!(f, "Delaying ({:.0}/{:.0}s)", elapsed_s, target_s)
            }
            Self::Ramping { progress, .. } => write!(f, "Ramping ({:.0}%)", progress * 100.0),
            Self::Holding => write!(f, "Holding"),
            Self::RampingBack { progress, .. } => {
                write!(f, "RampingBack ({:.0}%)", progress * 100.0)
            }
        }
    }
}

/// FSM for reactor state transitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReactorFsm {
    pub state: FsmState,
    /// The current interpolation factor: 0.0 = defaults, 1.0 = full target.
    pub factor: f64,
}

impl ReactorFsm {
    pub fn new() -> Self {
        Self {
            state: FsmState::Idle,
            factor: 0.0,
        }
    }

    /// Transition the FSM based on whether an event is active.
    /// `strategy`: "instant", "ramp", "delayed", "partial", "ignore"
    /// Returns the interpolation factor (0.0..1.0) for setpoint computation.
    pub fn transition(
        &mut self,
        event_active: bool,
        dt_s: f64,
        delay_s: u64,
        ramp_duration_s: u64,
        strategy: &str,
    ) -> f64 {
        if strategy == "ignore" {
            self.state = FsmState::Idle;
            self.factor = 0.0;
            return 0.0;
        }

        match (&self.state, event_active) {
            // Idle + event starts → begin response
            (FsmState::Idle, true) => {
                if strategy == "instant" {
                    self.state = FsmState::Holding;
                    self.factor = 1.0;
                } else if delay_s > 0 && (strategy == "delayed" || strategy == "ramp" || strategy == "partial") {
                    self.state = FsmState::Delaying {
                        elapsed_s: 0.0,
                        target_s: delay_s as f64,
                    };
                    self.factor = 0.0;
                } else {
                    self.state = FsmState::Ramping {
                        progress: 0.0,
                        duration_s: ramp_duration_s as f64,
                    };
                    self.factor = 0.0;
                }
            }

            // Delaying: wait for delay period
            (FsmState::Delaying { elapsed_s, target_s }, true) => {
                let new_elapsed = elapsed_s + dt_s;
                if new_elapsed >= *target_s {
                    // Delay complete, start ramping
                    self.state = FsmState::Ramping {
                        progress: 0.0,
                        duration_s: ramp_duration_s as f64,
                    };
                    self.factor = 0.0;
                } else {
                    self.state = FsmState::Delaying {
                        elapsed_s: new_elapsed,
                        target_s: *target_s,
                    };
                    self.factor = 0.0;
                }
            }

            // Ramping up to target
            (FsmState::Ramping { progress, duration_s }, true) => {
                let dur = *duration_s;
                if dur <= 0.0 {
                    self.state = FsmState::Holding;
                    self.factor = 1.0;
                } else {
                    let new_progress = (progress + dt_s / dur).min(1.0);
                    if new_progress >= 1.0 {
                        self.state = FsmState::Holding;
                        self.factor = 1.0;
                    } else {
                        self.state = FsmState::Ramping {
                            progress: new_progress,
                            duration_s: dur,
                        };
                        self.factor = new_progress;
                    }
                }
            }

            // Holding steady while event active
            (FsmState::Holding, true) => {
                self.factor = 1.0;
            }

            // Event ends while in any active state → ramp back
            (FsmState::Holding, false)
            | (FsmState::Ramping { .. }, false)
            | (FsmState::Delaying { .. }, false) => {
                if strategy == "instant" {
                    self.state = FsmState::Idle;
                    self.factor = 0.0;
                } else {
                    let current_factor = self.factor;
                    self.state = FsmState::RampingBack {
                        progress: 0.0,
                        duration_s: ramp_duration_s as f64,
                    };
                    self.factor = current_factor;
                }
            }

            // Ramping back to defaults
            (FsmState::RampingBack { progress, duration_s }, false) => {
                let dur = *duration_s;
                if dur <= 0.0 {
                    self.state = FsmState::Idle;
                    self.factor = 0.0;
                } else {
                    let new_progress = (progress + dt_s / dur).min(1.0);
                    if new_progress >= 1.0 {
                        self.state = FsmState::Idle;
                        self.factor = 0.0;
                    } else {
                        self.state = FsmState::RampingBack {
                            progress: new_progress,
                            duration_s: dur,
                        };
                        // Factor decreases from where we were towards 0
                        self.factor = (1.0 - new_progress) * self.factor;
                    }
                }
            }

            // If event reactivates during ramp-back, go back to ramping
            (FsmState::RampingBack { .. }, true) => {
                let current_factor = self.factor;
                self.state = FsmState::Ramping {
                    progress: current_factor,
                    duration_s: ramp_duration_s as f64,
                };
                // factor stays where it was
            }

            // Already idle, no event
            (FsmState::Idle, false) => {
                self.factor = 0.0;
            }
        }

        self.factor
    }
}
