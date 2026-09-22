use serde::{Deserialize, Serialize};

/// Asset type classification (§1.1).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AssetType {
    Pv,              // photovoltaic producer
    Battery,         // bidirectional storage
    Ev,              // electric vehicle (consumer, storage-like)
    Heater,          // thermal consumer with storage characteristics
    HeatPump,        // thermal consumer with storage characteristics
    WashingMachine,  // batch consumer
    CookingStove,    // heuristic/uncontrollable consumer
    GenericConsumer, // fallback
    GenericProducer, // fallback
}

/// How adjustable an asset's power consumption/generation is (§1.2). Reported
/// per-asset by `assets::AssetCapability.adjustability` (BL-27) — moved here
/// from `entities/design_vocabulary.rs`'s dead-code quarantine once it became
/// live, referenced code.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PowerAdjustability {
    None,           // observe only (e.g. cooking stove, fixed load)
    Recommendation, // VEN can suggest but not enforce (e.g. washing machine)
    OnOff,          // binary switching — equivalent to Stepped with [0, MaxPower]
    Stepped,        // discrete power levels (e.g. 0/3/6 kW pump, step-controlled charger)
    Stepless,       // continuously adjustable within [min_kw, max_kw]
    Croppable,      // can be curtailed downward only (e.g. PV — can't exceed natural output)
}

/// How an asset's actual power follows the setpoint it is commanded — the
/// asset's own answer to "what will you draw next tick if I command X?".
///
/// Declared by each asset in its `AssetCapability` (state-dependent: a running
/// shiftable load answers differently from a pending one) and interpreted by
/// exactly one pair of functions, `power_drawn_for_setpoint_kw` and its inverse
/// `setpoint_for_power_at_or_below_kw`. The asset's own physics and every
/// projection of what it will draw both call those, so they cannot disagree
/// (R-81: the arbiter used to apply the heater's rounding rule by asset id;
/// R-82: it assumed a command lands instantly).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SetpointResponse {
    /// For a stepped asset: the discrete levels it can actually draw,
    /// ascending (including 0.0 when it can be switched off). Empty for a
    /// continuously adjustable one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub power_steps_kw: Vec<f64>,
    /// Which of `power_steps_kw` a commanded setpoint selects.
    #[serde(default)]
    pub step_rule: StepRule,
    /// A commanded import above 0 but below this is not sustainable and falls
    /// back to 0 — the EV charger's `min_charge_kw` (BL-12). Never applies to
    /// export/discharge. 0.0 = no such floor.
    #[serde(default)]
    pub snap_to_zero_below_kw: f64,
    /// Set when next tick's power does not follow the setpoint commanded now:
    /// a charger whose command only lands a tick later (`response_delay_s`), a
    /// load already running at its fixed rate, an uncontrollable load. `None`
    /// = the setpoint takes effect immediately.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_next_tick_kw: Option<f64>,
}

/// Which step of a `SetpointResponse` a commanded setpoint selects.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StepRule {
    /// Draws the setpoint itself, within the capability's range.
    #[default]
    Continuous,
    /// Draws the nearest reachable step, a tie going to the higher one — each
    /// step is its own contactor, so intermediate values are impossible.
    Nearest,
    /// Any setpoint above zero starts the asset at its full rate; it does not
    /// modulate, and once started it cannot be commanded lower.
    LatchOnFull,
}

impl SetpointResponse {
    /// Follows the setpoint exactly (battery, PV, grid).
    pub fn continuous() -> Self {
        Self::default()
    }

    /// Draws the nearest of `power_steps_kw` (a heater's stages).
    pub fn stepped_nearest(power_steps_kw: Vec<f64>) -> Self {
        Self {
            power_steps_kw,
            step_rule: StepRule::Nearest,
            ..Self::default()
        }
    }

    /// Starts at the highest of `power_steps_kw` for any setpoint above zero
    /// (a shiftable load).
    pub fn latching(power_steps_kw: Vec<f64>) -> Self {
        Self {
            power_steps_kw,
            step_rule: StepRule::LatchOnFull,
            ..Self::default()
        }
    }

    /// Draws `power_kw` next tick whatever it is commanded — an uncontrollable
    /// load, or a command already staged for the next tick.
    pub fn fixed(power_kw: f64) -> Self {
        Self {
            power_next_tick_kw: Some(power_kw),
            ..Self::default()
        }
    }

    /// Adds the sustainable-import floor below which a command falls back to 0.
    pub fn with_snap_to_zero_below_kw(mut self, snap_to_zero_below_kw: f64) -> Self {
        self.snap_to_zero_below_kw = snap_to_zero_below_kw;
        self
    }

    /// Declares that next tick's power is `power_kw` whatever is commanded now
    /// — a thermostat override, a running batch load, a staged command.
    pub fn with_power_next_tick_kw(mut self, power_kw: f64) -> Self {
        self.power_next_tick_kw = Some(power_kw);
        self
    }

    /// The power this asset will actually draw next tick if commanded
    /// `setpoint_kw`, within its capability range `[min_kw, max_kw]`. THE
    /// answer to that question — no caller may compute its own.
    pub fn power_drawn_for_setpoint_kw(&self, min_kw: f64, max_kw: f64, setpoint_kw: f64) -> f64 {
        match self.power_next_tick_kw {
            Some(power_kw) => power_kw,
            None => self.power_when_command_lands_kw(min_kw, max_kw, setpoint_kw),
        }
    }

    /// The power `setpoint_kw` produces once the command has landed — the same
    /// rules without the response lag. What a lever may count on in steady
    /// state, and what an asset stages while it still draws
    /// `power_next_tick_kw`.
    pub fn power_when_command_lands_kw(&self, min_kw: f64, max_kw: f64, setpoint_kw: f64) -> f64 {
        // Not `clamp`, which panics when a degenerate capability reports
        // min > max; a ceiling below the floor then simply wins.
        let setpoint_kw = setpoint_kw.max(min_kw).min(max_kw);
        match self.step_rule {
            StepRule::Continuous => self.snapped_to_zero_kw(setpoint_kw),
            StepRule::Nearest => nearest_power_step_kw(&self.power_steps_kw, setpoint_kw),
            StepRule::LatchOnFull => {
                let (lowest_kw, highest_kw) = self.step_range_kw(setpoint_kw);
                if setpoint_kw > 1e-6 {
                    highest_kw
                } else {
                    lowest_kw
                }
            }
        }
    }

    /// The setpoint to command so the asset draws no more than `kw` — the
    /// inverse of `power_drawn_for_setpoint_kw`, used when shedding. A lagging
    /// asset still takes the command now (it lands a tick later), so
    /// `power_next_tick_kw` deliberately plays no part here.
    pub fn setpoint_for_power_at_or_below_kw(&self, min_kw: f64, max_kw: f64, kw: f64) -> f64 {
        let kw = kw.max(min_kw).min(max_kw);
        match self.step_rule {
            StepRule::Continuous => self.snapped_to_zero_kw(kw),
            StepRule::Nearest => highest_power_step_at_or_below_kw(&self.power_steps_kw, kw),
            StepRule::LatchOnFull => {
                // All-or-nothing: anything short of the full rate means "don't
                // start" — or, once started, the rate it is stuck at.
                let (lowest_kw, highest_kw) = self.step_range_kw(kw);
                if kw + 1e-9 >= highest_kw {
                    highest_kw
                } else {
                    lowest_kw
                }
            }
        }
    }

    /// `(lowest, highest)` reachable step; without steps, `fallback_kw` for both.
    fn step_range_kw(&self, fallback_kw: f64) -> (f64, f64) {
        match (self.power_steps_kw.first(), self.power_steps_kw.last()) {
            (Some(&lowest_kw), Some(&highest_kw)) => (lowest_kw, highest_kw),
            _ => (fallback_kw, fallback_kw),
        }
    }

    /// BL-12: an import below the sustainable floor falls back to 0. Export
    /// (negative) is never affected.
    fn snapped_to_zero_kw(&self, kw: f64) -> f64 {
        if kw > 0.0 && kw < self.snap_to_zero_below_kw {
            0.0
        } else {
            kw
        }
    }
}

/// The power step a `Stepped` asset actually draws for `setpoint_kw`: the
/// nearest of its `power_steps_kw` (ascending; a tie goes to the higher step).
/// Private: the one way in is `SetpointResponse::power_drawn_for_setpoint_kw`,
/// so no caller can apply a step rule the asset did not declare.
/// Without steps (a continuously adjustable asset) the setpoint passes through.
fn nearest_power_step_kw(power_steps_kw: &[f64], setpoint_kw: f64) -> f64 {
    power_steps_kw
        .iter()
        .copied()
        .reduce(|best_kw, step_kw| {
            // `<=` lets the later (higher) step win a tie.
            if (step_kw - setpoint_kw).abs() <= (best_kw - setpoint_kw).abs() {
                step_kw
            } else {
                best_kw
            }
        })
        .unwrap_or(setpoint_kw)
}

/// The highest of `power_steps_kw` (ascending) at or below `kw` — what to
/// command when a stepped asset must draw no more than `kw`. Below the lowest
/// step, the lowest step. Without steps, `kw` itself. Private for the same
/// reason as `nearest_power_step_kw`.
fn highest_power_step_at_or_below_kw(power_steps_kw: &[f64], kw: f64) -> f64 {
    let lowest_kw = match power_steps_kw.first() {
        Some(&lowest_kw) => lowest_kw,
        None => return kw,
    };
    power_steps_kw
        .iter()
        .rev()
        .copied()
        .find(|&step_kw| step_kw <= kw + 1e-9)
        .unwrap_or(lowest_kw)
}

/// How to handle completion when the last DeadlineTier expires (§1.10).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CompletionPolicy {
    /// Terminate immediately → PARTIAL_COMPLETED if FillPercentage < 1.0.
    Stop,
    /// Keep going, bidding at PostDeadlineComfortBid for priority.
    Continue,
}

/// What triggered a plan recomputation (§1.5).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlanTrigger {
    Periodic,         // regular planning cycle (every PlanTimeStep)
    RateChange,       // new PRICE/GHG/EXPORT_PRICE event from VTN
    CapacityChange,   // new capacity limit/reservation from VTN
    Alert,            // emergency/flex alert from VTN
    UserRequest,      // new or modified device session / user request
    AssetStateChange, // device connected/disconnected/failed
    /// The deviation arbiter's accumulated absorbed-kWh (per SoC-coupled
    /// asset) crossed its capacity-fraction threshold since the last plan
    /// adoption — an accumulator/hysteresis signal, deliberately not a
    /// raw-per-tick-deviation trigger (see `docs/reference/KEY_LEARNINGS.md`'s Deviation
    /// Absorber section on why the removed feature 017's raw-deviation trigger caused
    /// spurious replans). Rate-limited by a cooldown — see
    /// `AppState::last_residual_trigger_at`.
    ResidualThreshold,
}

/// A trigger, and what caused it.
///
/// The kind alone cannot answer "did this VEN replan *because of* that event"
/// — §6.3's reaction chain needs the event ids, and a fleet view that reports
/// "the first plan cycle within 15 minutes" is reporting a coincidence with a
/// respectable name. Carried alongside the kind rather than inside it: the
/// variants are matched in 150-odd places that do not care why.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanTriggerSignal {
    pub trigger: PlanTrigger,
    /// The VTN event ids behind this trigger, when it came from events at all.
    /// Empty for periodic cycles, user requests and asset state changes —
    /// which is a fact about those triggers, not missing data.
    pub event_ids: Vec<String>,
}

impl PlanTriggerSignal {
    /// A trigger with no event behind it.
    pub fn bare(trigger: PlanTrigger) -> Self {
        Self {
            trigger,
            event_ids: Vec::new(),
        }
    }

    /// A trigger caused by specific VTN events.
    pub fn caused_by(trigger: PlanTrigger, event_ids: Vec<String>) -> Self {
        Self { trigger, event_ids }
    }
}

/// One point on the comfort/value curve (§2.7).
/// MaxMarginalPrice is a priority bid, not the actual price paid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComfortRate {
    pub fill: f64,               // 0.0..1.0 task completion fraction
    pub max_marginal_price: f64, // max €/kWh the user bids — determines priority
    pub max_marginal_co2: f64,   // max gCO2/kWh user accepts at this fill level
}

impl ComfortRate {
    /// Interpolate an arbitrary `ComfortRate` field (selected by `extract`) at a fill level.
    /// `rates` must be sorted non-decreasing by `fill` (guaranteed by
    /// `services/comfort.rs::validate_curve` for any persisted curve) and non-empty. Exact
    /// breakpoint queries return the stored value; mid-curve queries interpolate linearly
    /// between the two bracketing points; queries outside the stored range clamp to the
    /// nearest boundary breakpoint.
    fn interpolate_at_fill(
        rates: &[ComfortRate],
        fill: f64,
        extract: impl Fn(&ComfortRate) -> f64,
    ) -> f64 {
        if fill <= rates[0].fill {
            return extract(&rates[0]);
        }
        let last = rates.len() - 1;
        if fill >= rates[last].fill {
            return extract(&rates[last]);
        }
        let hi = rates.iter().position(|r| r.fill >= fill).unwrap();
        let lo = hi - 1;
        let (r_lo, r_hi) = (&rates[lo], &rates[hi]);
        if r_hi.fill == r_lo.fill {
            return extract(r_hi);
        }
        let t = (fill - r_lo.fill) / (r_hi.fill - r_lo.fill);
        extract(r_lo) + t * (extract(r_hi) - extract(r_lo))
    }

    /// Interpolate `max_marginal_price` at an arbitrary fill level. See `interpolate_at_fill`.
    pub fn value_at_fill(rates: &[ComfortRate], fill: f64) -> f64 {
        Self::interpolate_at_fill(rates, fill, |r| r.max_marginal_price)
    }

    /// Interpolate `max_marginal_co2` at an arbitrary fill level. See `interpolate_at_fill`.
    pub fn co2_value_at_fill(rates: &[ComfortRate], fill: f64) -> f64 {
        Self::interpolate_at_fill(rates, fill, |r| r.max_marginal_co2)
    }
}

#[cfg(test)]
mod power_step_tests {
    use super::{highest_power_step_at_or_below_kw, nearest_power_step_kw};

    const STEPS: [f64; 3] = [0.0, 1.75, 3.5];

    #[test]
    fn nearest_power_step_kw_rounds_to_the_closest_step() {
        assert_eq!(nearest_power_step_kw(&STEPS, 1.2), 1.75);
        assert_eq!(nearest_power_step_kw(&STEPS, 0.8), 0.0);
        assert_eq!(nearest_power_step_kw(&STEPS, 3.0), 3.5);
    }

    #[test]
    fn nearest_power_step_kw_tie_goes_to_the_higher_step() {
        // Matches the heater's former `(setpoint / p_step).round()`: 0.875 = 0.5 steps → 1.
        assert_eq!(nearest_power_step_kw(&STEPS, 0.875), 1.75);
    }

    #[test]
    fn nearest_power_step_kw_clamps_outside_the_range() {
        assert_eq!(nearest_power_step_kw(&STEPS, -2.0), 0.0);
        assert_eq!(nearest_power_step_kw(&STEPS, 9.0), 3.5);
    }

    #[test]
    fn highest_power_step_at_or_below_kw_never_rounds_up() {
        // GB-47: shedding 0.72 kW from 1.75 leaves 1.03 — must command 0, not
        // a value the heater would round back up to 1.75.
        assert_eq!(highest_power_step_at_or_below_kw(&STEPS, 1.03), 0.0);
        assert_eq!(highest_power_step_at_or_below_kw(&STEPS, 1.75), 1.75);
        assert_eq!(highest_power_step_at_or_below_kw(&STEPS, 3.4), 1.75);
        assert_eq!(highest_power_step_at_or_below_kw(&STEPS, -0.1), 0.0);
    }
}

#[cfg(test)]
mod setpoint_response_tests {
    use super::SetpointResponse;

    const HEATER_STAGES: [f64; 3] = [0.0, 1.75, 3.5];
    const UNBOUNDED: (f64, f64) = (-100.0, 100.0);

    fn drawn(response: &SetpointResponse, setpoint_kw: f64) -> f64 {
        response.power_drawn_for_setpoint_kw(UNBOUNDED.0, UNBOUNDED.1, setpoint_kw)
    }

    fn command_for(response: &SetpointResponse, kw: f64) -> f64 {
        response.setpoint_for_power_at_or_below_kw(UNBOUNDED.0, UNBOUNDED.1, kw)
    }

    #[test]
    fn power_drawn_for_setpoint_kw_continuous_follows_the_setpoint() {
        assert_eq!(drawn(&SetpointResponse::continuous(), 2.3), 2.3);
        assert_eq!(drawn(&SetpointResponse::continuous(), -4.0), -4.0);
    }

    #[test]
    fn power_drawn_for_setpoint_kw_clamps_to_the_capability_range() {
        let response = SetpointResponse::continuous();
        assert_eq!(response.power_drawn_for_setpoint_kw(-2.0, 5.0, 7.0), 5.0);
        assert_eq!(response.power_drawn_for_setpoint_kw(-2.0, 5.0, -9.0), -2.0);
        // A PV inverter's "uncurtailed" sentinel resolves to its live ceiling
        // instead of needing a magnitude check at the call site.
        assert_eq!(
            response.power_drawn_for_setpoint_kw(-3.2, 0.0, -f64::MAX),
            -3.2
        );
    }

    #[test]
    fn power_drawn_for_setpoint_kw_stepped_draws_the_nearest_stage() {
        let heater = SetpointResponse::stepped_nearest(HEATER_STAGES.to_vec());
        assert_eq!(drawn(&heater, 1.2), 1.75);
        assert_eq!(
            drawn(&heater, 0.875),
            1.75,
            "a tie goes to the higher stage"
        );
        assert_eq!(drawn(&heater, 0.8), 0.0);
    }

    #[test]
    fn power_drawn_for_setpoint_kw_latching_starts_at_full_power() {
        // R-81: a shiftable load ignores the setpoint's magnitude — any
        // command above zero starts it at its full rate.
        let pending = SetpointResponse::latching(vec![0.0, 2.0]);
        assert_eq!(drawn(&pending, 0.5), 2.0);
        assert_eq!(drawn(&pending, 0.0), 0.0);

        // Once running there is no off step left to reach.
        let running = SetpointResponse::latching(vec![2.0]);
        assert_eq!(drawn(&running, 0.0), 2.0);
    }

    #[test]
    fn power_drawn_for_setpoint_kw_lagging_asset_ignores_this_tick_s_command() {
        // R-82: the EV applies the command accepted last tick, so a shed
        // commanded now yields nothing until the next one.
        let ev = SetpointResponse::fixed(7.0);
        assert_eq!(drawn(&ev, 0.0), 7.0);
        assert_eq!(drawn(&ev, 11.0), 7.0);
    }

    #[test]
    fn power_drawn_for_setpoint_kw_snaps_an_unsustainable_import_to_zero() {
        let ev = SetpointResponse::continuous().with_snap_to_zero_below_kw(1.4);
        assert_eq!(drawn(&ev, 0.9), 0.0);
        assert_eq!(drawn(&ev, 1.4), 1.4);
        assert_eq!(drawn(&ev, -0.9), -0.9, "V2G discharge has no such floor");
    }

    #[test]
    fn setpoint_for_power_at_or_below_kw_never_rounds_up_to_a_higher_stage() {
        // GB-47: shedding 0.72 kW from 1.75 leaves 1.03 — commanding that
        // would round back up to the stage it came from and shed nothing.
        let heater = SetpointResponse::stepped_nearest(HEATER_STAGES.to_vec());
        assert_eq!(command_for(&heater, 1.03), 0.0);
        assert_eq!(command_for(&heater, 3.4), 1.75);
        assert_eq!(command_for(&heater, -0.1), 0.0);
    }

    #[test]
    fn setpoint_for_power_at_or_below_kw_latching_is_all_or_nothing() {
        let pending = SetpointResponse::latching(vec![0.0, 2.0]);
        assert_eq!(command_for(&pending, 1.9), 0.0, "cannot run at part load");
        assert_eq!(command_for(&pending, 2.0), 2.0);

        let running = SetpointResponse::latching(vec![2.0]);
        assert_eq!(command_for(&running, 0.0), 2.0, "cannot be interrupted");
    }

    #[test]
    fn setpoint_for_power_at_or_below_kw_ignores_the_response_lag() {
        // The command still has to be given now for it to land next tick.
        let mut lagging = SetpointResponse::continuous().with_snap_to_zero_below_kw(1.4);
        lagging.power_next_tick_kw = Some(7.0);
        assert_eq!(command_for(&lagging, 3.0), 3.0);
        assert_eq!(command_for(&lagging, 0.9), 0.0, "still snaps to zero");
    }

    #[test]
    fn power_drawn_for_setpoint_kw_without_steps_passes_the_setpoint_through() {
        let mut odd = SetpointResponse::stepped_nearest(Vec::new());
        assert_eq!(drawn(&odd, 2.2), 2.2);
        odd.step_rule = super::StepRule::LatchOnFull;
        assert_eq!(drawn(&odd, 2.2), 2.2);
        assert_eq!(command_for(&odd, 2.2), 2.2);
    }
}

#[cfg(test)]
mod comfort_rate_tests {
    use super::ComfortRate;

    fn curve() -> Vec<ComfortRate> {
        vec![
            ComfortRate {
                fill: 0.0,
                max_marginal_price: 0.30,
                max_marginal_co2: 300.0,
            },
            ComfortRate {
                fill: 1.0,
                max_marginal_price: 0.10,
                max_marginal_co2: 50.0,
            },
        ]
    }

    #[test]
    fn value_at_fill_exact_breakpoint_returns_stored_price() {
        let rates = curve();
        assert_eq!(ComfortRate::value_at_fill(&rates, 0.0), 0.30);
        assert_eq!(ComfortRate::value_at_fill(&rates, 1.0), 0.10);
    }

    #[test]
    fn value_at_fill_mid_curve_interpolates_linearly() {
        let rates = curve();
        assert!((ComfortRate::value_at_fill(&rates, 0.5) - 0.20).abs() < 1e-9);
        assert!((ComfortRate::value_at_fill(&rates, 0.25) - 0.25).abs() < 1e-9);
    }

    #[test]
    fn value_at_fill_out_of_range_clamps_to_nearest_breakpoint() {
        let rates = curve();
        assert_eq!(ComfortRate::value_at_fill(&rates, -0.5), 0.30);
        assert_eq!(ComfortRate::value_at_fill(&rates, 1.5), 0.10);
    }

    #[test]
    fn value_at_fill_three_point_curve_interpolates_within_bracket() {
        let rates = vec![
            ComfortRate {
                fill: 0.0,
                max_marginal_price: 0.30,
                max_marginal_co2: 0.0,
            },
            ComfortRate {
                fill: 0.5,
                max_marginal_price: 0.20,
                max_marginal_co2: 0.0,
            },
            ComfortRate {
                fill: 1.0,
                max_marginal_price: 0.10,
                max_marginal_co2: 0.0,
            },
        ];
        assert!((ComfortRate::value_at_fill(&rates, 0.75) - 0.15).abs() < 1e-9);
    }

    // ── co2_value_at_fill: same interpolation, independent axis (BL-17 comfort bidding) ──

    #[test]
    fn co2_value_at_fill_exact_breakpoint_returns_stored_co2() {
        let rates = curve();
        assert_eq!(ComfortRate::co2_value_at_fill(&rates, 0.0), 300.0);
        assert_eq!(ComfortRate::co2_value_at_fill(&rates, 1.0), 50.0);
    }

    #[test]
    fn co2_value_at_fill_mid_curve_interpolates_linearly() {
        let rates = curve();
        assert!((ComfortRate::co2_value_at_fill(&rates, 0.5) - 175.0).abs() < 1e-9);
        assert!((ComfortRate::co2_value_at_fill(&rates, 0.25) - 237.5).abs() < 1e-9);
    }

    #[test]
    fn co2_value_at_fill_out_of_range_clamps_to_nearest_breakpoint() {
        let rates = curve();
        assert_eq!(ComfortRate::co2_value_at_fill(&rates, -0.5), 300.0);
        assert_eq!(ComfortRate::co2_value_at_fill(&rates, 1.5), 50.0);
    }

    #[test]
    fn co2_value_at_fill_three_point_curve_interpolates_within_bracket() {
        let rates = vec![
            ComfortRate {
                fill: 0.0,
                max_marginal_price: 0.30,
                max_marginal_co2: 300.0,
            },
            ComfortRate {
                fill: 0.5,
                max_marginal_price: 0.20,
                max_marginal_co2: 200.0,
            },
            ComfortRate {
                fill: 1.0,
                max_marginal_price: 0.10,
                max_marginal_co2: 50.0,
            },
        ];
        assert!((ComfortRate::co2_value_at_fill(&rates, 0.75) - 125.0).abs() < 1e-9);
    }

    /// Price and CO2 axes are independent — a curve where they move in *opposite*
    /// directions across fill must interpolate each correctly on its own, proving
    /// `co2_value_at_fill` isn't accidentally reading the price field (or vice versa).
    #[test]
    fn price_and_co2_axes_interpolate_independently_on_a_diverging_curve() {
        let rates = vec![
            ComfortRate {
                fill: 0.0,
                max_marginal_price: 0.10, // price rises with fill...
                max_marginal_co2: 300.0,  // ...while CO2 bid falls with fill
            },
            ComfortRate {
                fill: 1.0,
                max_marginal_price: 0.50,
                max_marginal_co2: 20.0,
            },
        ];
        assert!((ComfortRate::value_at_fill(&rates, 0.5) - 0.30).abs() < 1e-9);
        assert!((ComfortRate::co2_value_at_fill(&rates, 0.5) - 160.0).abs() < 1e-9);
    }
}
