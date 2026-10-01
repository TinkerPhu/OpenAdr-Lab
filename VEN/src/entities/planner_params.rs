use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlannerObjective {
    #[default]
    MinCost,
    MinGhg,
    MinGrid,
    MinImport,
    MaxRevenue,
    Custom,
}

/// A peak-demand penalty rule (WP6.3, BL-09): the planner keeps each fixed
/// `measurement_window_s` window's peak grid import at or below
/// `threshold_kw` via a soft penalty of `penalty_eur_per_kw` per kW over.
/// Deliberately lightweight — not the stateful, persisted billing-period
/// tracker sketched in `entities::design_vocabulary::PenaltyRule`; see
/// `openspec/changes/penalty-threshold-check/design.md` (Decision D4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PenaltyRuleParams {
    pub rule_id: String,
    pub threshold_kw: f64,
    pub measurement_window_s: u64,
    pub penalty_eur_per_kw: f64,
}

#[derive(Debug, Clone)]
pub struct PlannerParams {
    pub plan_step_s: u64,
    pub plan_horizon_h: u64,
    pub plan_zones: Vec<crate::entities::plan::PlanZone>,
    pub replan_interval_s: u64,
    pub w_energy: f64,
    pub w_ghg: f64,
    pub w_grid: f64,
    pub c_bat_wear_eur_kwh: f64,
    pub c_ev_startup_eur: f64,
    pub c_bat_startup_eur: f64,
    pub c_ev_ramp_eur_kw: f64,
    pub c_bat_ramp_eur_kw: f64,
    pub c_bat_ev_coexist_eur_kwh: f64,
    pub w_viol: f64,
    pub pen_imp_eur_kwh: f64,
    pub pen_exp_eur_kwh: f64,
    pub v_ev_extra_eur_kwh: f64,
    /// One-time reward (EUR/kWh of core target) for committing to a soft-deadline EV session.
    pub v_ev_core_eur_kwh: f64,
    pub w_tier_penalty_eur: f64,
    /// Penalty [€/kWh] on controllable-asset import exceeding free PV surplus.
    /// Applies to all assets (heater + EV + net battery + shiftables) as a group.
    /// Set to ~0.20 to make the planner prefer staying within free PV. Default: 0.0 (disabled).
    pub c_ctrl_imp_malus_eur_kwh: f64,
    pub objective: PlannerObjective,
    pub plan_adoption_threshold_eur: f64,
    pub plan_adoption_decay_s: f64,
    pub phase2_epsilon_eur: f64,
    pub solver_timeout_s: u64,
    /// Phase 2's own budget; see `profile::planner::phase2_solver_timeout_s` (R-97).
    pub phase2_solver_timeout_s: u64,
    /// HiGHS optimality-gap tolerance, shared by all three solve call sites and
    /// persisted on `Plan.mip_gap_target`. See `profile::schema::PlannerConfig`
    /// for why it can only be tuned by offline benchmarking.
    pub mip_gap_target: f64,
    pub planning_initial_delay_s: u64,
    /// Per-extra-switch surcharge [EUR] added to the effective acceptance threshold.
    /// 0.0 = disabled (default). Set to match `switching_penalty_eur` so that a noisier
    /// plan must compensate for its extra relay operations to be adopted.
    pub gate_switch_penalty_eur: f64,
    /// WP3.2 — SIMPLE level 1 import cap as a fraction of the contractual
    /// limit (0.0–1.0). Levels 2/3 have fixed semantics (baseline / zero cap);
    /// see `entities::capacity::SimpleWindow`.
    pub simple_level1_import_cap_pct: f64,
    /// WP4.1 (BL-28) — ASAP mode lateness penalty [€/kWh per hour of delay].
    /// Must dominate any plausible tariff spread so ASAP is effectively
    /// cost-blind; 0.0 disables the mode's early-allocation pressure.
    pub asap_lateness_eur_kwh_h: f64,
    /// WP4.1 (BL-28) — reward per kWh of free-energy charging in
    /// OPPORTUNISTIC / *_FREE modes [€/kWh]. Must exceed the feed-in tariff
    /// so consuming PV surplus beats exporting it.
    pub v_ev_free_charge_eur_kwh: f64,
    /// WP4.4 (BL-07) — how slots beyond tariff coverage are priced.
    /// HEURISTIC_FORECAST is a documented stub until Phase 5 (BL-14): it
    /// behaves like LAST_KNOWN and says so in the plan warning.
    pub stale_rate_policy: crate::entities::design_vocabulary::StaleRatePolicy,
    /// WP4.4 — SAFE_AVERAGE percentile over the known import rates (0.0–1.0,
    /// nearest-rank; default 0.8 per REQUIREMENTS §3.2.1).
    pub stale_rate_safe_pctl: f64,
    /// WP6.3 (BL-09) — active peak-demand penalty rules. Empty = feature
    /// disabled, planner behavior unchanged (default).
    pub penalty_rules: Vec<PenaltyRuleParams>,
}

impl Default for PlannerParams {
    fn default() -> Self {
        Self {
            plan_step_s: 600,
            plan_horizon_h: 48,
            plan_zones: vec![crate::entities::plan::PlanZone {
                step_s: 600,
                slots: 288,
            }],
            replan_interval_s: 300,
            w_energy: 1.0,
            w_ghg: 0.0001,
            w_grid: 0.0,
            c_bat_wear_eur_kwh: 0.03,
            c_ev_startup_eur: 0.01,
            c_bat_startup_eur: 0.01,
            c_ev_ramp_eur_kw: 0.005,
            c_bat_ramp_eur_kw: 0.005,
            c_bat_ev_coexist_eur_kwh: 0.5,
            w_viol: 1.0,
            pen_imp_eur_kwh: 10_000.0,
            pen_exp_eur_kwh: 10_000.0,
            v_ev_extra_eur_kwh: 0.10,
            v_ev_core_eur_kwh: 1.0,
            w_tier_penalty_eur: 0.001,
            c_ctrl_imp_malus_eur_kwh: 0.22,
            objective: PlannerObjective::MinCost,
            plan_adoption_threshold_eur: 0.20,
            plan_adoption_decay_s: 1500.0,
            phase2_epsilon_eur: 0.02,
            solver_timeout_s: 60,
            phase2_solver_timeout_s: 15,
            mip_gap_target: 0.02,
            planning_initial_delay_s: 5,
            gate_switch_penalty_eur: 0.0,
            simple_level1_import_cap_pct: 0.5,
            asap_lateness_eur_kwh_h: 10.0,
            v_ev_free_charge_eur_kwh: 0.10,
            stale_rate_policy:
                crate::entities::design_vocabulary::StaleRatePolicy::HeuristicForecast,
            stale_rate_safe_pctl: 0.8,
            penalty_rules: vec![],
        }
    }
}

#[derive(Debug, Clone)]
pub struct SimulatorParams {
    pub tick_s: u64,
    pub persist_every_s: u64,
}

impl Default for SimulatorParams {
    fn default() -> Self {
        Self {
            tick_s: 1,
            persist_every_s: 15,
        }
    }
}

impl PlannerObjective {
    fn as_str(self) -> &'static str {
        match self {
            Self::MinCost => "min_cost",
            Self::MinGhg => "min_ghg",
            Self::MinGrid => "min_grid",
            Self::MinImport => "min_import",
            Self::MaxRevenue => "max_revenue",
            Self::Custom => "custom",
        }
    }
}

impl Serialize for PlannerObjective {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str((*self).as_str())
    }
}

impl<'de> Deserialize<'de> for PlannerObjective {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "min_cost" => Ok(Self::MinCost),
            "min_ghg" => Ok(Self::MinGhg),
            "min_grid" => Ok(Self::MinGrid),
            "min_import" => Ok(Self::MinImport),
            "max_revenue" => Ok(Self::MaxRevenue),
            "custom" => Ok(Self::Custom),
            other => Err(serde::de::Error::unknown_variant(
                other,
                &[
                    "min_cost",
                    "min_ghg",
                    "min_grid",
                    "min_import",
                    "max_revenue",
                    "custom",
                ],
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_planner_params_default_has_single_zone() {
        let p = PlannerParams::default();
        assert_eq!(p.plan_zones.len(), 1);
        assert_eq!(p.plan_zones[0].step_s, 600);
        assert_eq!(p.plan_zones[0].slots, 288);
    }
}

/// GB-54 — when the next periodic replan is due.
///
/// Two properties, both of which the previous `sleep(replan_interval_s)` after
/// each cycle lacked:
///
/// 1. **The period is the interval, not the interval plus the solve.** Sleeping
///    *after* a cycle makes a VEN's real period `replan_interval_s + solve_time`,
///    so a VEN whose solve takes 34 s runs on ~334 s. Measured on ven-17:
///    completions 329 s, 376 s and 339 s apart against a nominal 300 s. Different
///    VENs therefore drift at different rates, wander into each other, and — once
///    collided, because collided VENs are all slowed equally and keep their
///    offset — stay collided. On a 4-core host with 17 VENs that burst is what
///    produces `TimeLimit` terminations, not model difficulty: fleet solver
///    wall-time is only ~17 % of the host's core-seconds, yet six solves once
///    landed inside one 90 s window. Anchoring to a grid of absolute time keeps
///    the period at exactly `replan_interval_s` however long a cycle takes.
///
/// 2. **A per-VEN phase, so the grid does not put everyone in lockstep.** A
///    shared absolute grid alone would be strictly worse than the status quo —
///    every VEN would fire on the same instant. `replan_phase_offset_s` spreads
///    them, and being derived from the VEN's name rather than from start-up time
///    is the whole point: a restart must not re-align the fleet. Recreating eight
///    containers in a tight loop is exactly how GB-54 was found.
///
/// A cycle that overruns a whole interval **skips** to the next grid point rather
/// than firing immediately: a VEN that is already too slow is the last one that
/// should be asked to replan back-to-back.
pub fn next_replan_at(
    now: chrono::DateTime<chrono::Utc>,
    replan_interval_s: u64,
    replan_phase_offset_s: u64,
) -> chrono::DateTime<chrono::Utc> {
    let interval = replan_interval_s.max(1) as i64;
    let offset = (replan_phase_offset_s as i64).rem_euclid(interval);
    let now_s = now.timestamp();
    // Grid points are `k * interval + offset`; take the first strictly after now
    // so a cycle finishing exactly on a grid point waits a full interval rather
    // than re-firing instantly.
    let k = (now_s - offset).div_euclid(interval) + 1;
    chrono::DateTime::from_timestamp(k * interval + offset, 0).unwrap_or(now)
}

/// GB-54 — this VEN's stable phase within the replan grid, in seconds.
///
/// Derived from the VEN's name with FNV-1a so it is identical on every start of
/// every build: an offset that moved on restart would re-shuffle the fleet
/// exactly when a deploy has just restarted all of it. FNV is spelled out rather
/// than taken from `DefaultHasher`, whose values Rust does not promise to keep
/// stable across versions.
pub fn replan_phase_offset_s(ven_name: &str, replan_interval_s: u64) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in ven_name.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h % replan_interval_s.max(1)
}

#[cfg(test)]
mod replan_schedule_tests {
    use super::*;
    use chrono::{DateTime, TimeZone, Utc};

    fn at(epoch_s: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(epoch_s, 0).unwrap()
    }

    const INTERVAL: i64 = 300;
    const OFFSET: i64 = 47;

    /// A point on this VEN's replan grid, `k` steps from the epoch.
    ///
    /// These tests state their expectations through this helper rather than
    /// through hand-written epoch constants. The first version of them did the
    /// arithmetic by hand and encoded a false premise — that 1_000_000 lies on a
    /// 300 s grid, when 1_000_000 / 300 = 3333.33 — so four of them failed
    /// against a `next_replan_at` that was answering correctly. The properties
    /// asserted below are unchanged; only the way the expected instants are
    /// derived is, so that the test cannot disagree with the grid it is checking.
    fn grid(k: i64) -> i64 {
        k * INTERVAL + OFFSET
    }

    /// Is this instant on the grid at all?
    fn on_grid(epoch_s: i64) -> bool {
        (epoch_s - OFFSET).rem_euclid(INTERVAL) == 0
    }

    #[test]
    fn next_replan_at_lands_on_the_grid_point_for_this_vens_phase() {
        // From anywhere strictly inside a step, the answer is that step's end:
        // on the grid, in the future, and at most one interval away.
        for probe in [grid(3333) + 1, grid(3333) + 150, grid(3333) + INTERVAL - 1] {
            let next = next_replan_at(at(probe), INTERVAL as u64, OFFSET as u64).timestamp();
            assert!(on_grid(next), "{next} is not on the grid");
            assert!(next > probe, "{next} must be after {probe}");
            assert!(
                next - probe <= INTERVAL,
                "{next} is more than one interval after {probe}"
            );
            assert_eq!(next, grid(3334), "the next grid point after {probe}");
        }
    }

    #[test]
    fn next_replan_at_keeps_the_period_at_the_interval_however_long_a_cycle_took() {
        // The drift GB-54 is about: whatever a cycle that woke on a grid point
        // spends solving, the next replan is one interval after it WOKE, not one
        // interval after it finished. Checked across a range of solve durations
        // precisely because the old flat sleep was correct only at 0.
        let woke = grid(3334);
        for solve_s in [0, 1, 34, INTERVAL - 1] {
            let next =
                next_replan_at(at(woke + solve_s), INTERVAL as u64, OFFSET as u64).timestamp();
            assert_eq!(
                next - woke,
                INTERVAL,
                "a {solve_s}s solve must not stretch the period"
            );
        }
    }

    #[test]
    fn next_replan_at_skips_rather_than_catches_up_after_an_overrun() {
        // A cycle that ate more than a whole interval gets the NEXT grid point,
        // not an immediate re-fire — the slowest VEN is the last one that should
        // be asked to solve back-to-back.
        let woke = grid(3334);
        let finished = woke + 700; // 2.33 intervals
        let next = next_replan_at(at(finished), INTERVAL as u64, OFFSET as u64).timestamp();
        assert!(
            next > finished,
            "must be in the future, not a catch-up burst"
        );
        assert!(
            next - finished <= INTERVAL,
            "must not idle past one interval"
        );
        assert_eq!(
            next,
            grid(3337),
            "the next grid point, skipping the missed ones"
        );
    }

    #[test]
    fn next_replan_at_on_an_exact_grid_point_waits_a_full_interval() {
        let point = grid(3334);
        assert_eq!(
            next_replan_at(at(point), INTERVAL as u64, OFFSET as u64).timestamp(),
            point + INTERVAL,
            "landing exactly on a grid point must not re-fire instantly"
        );
    }

    #[test]
    fn replan_phase_offset_is_stable_for_a_name_and_inside_the_interval() {
        for name in ["ven-1", "ven-19", "ven-20"] {
            let a = replan_phase_offset_s(name, 300);
            assert_eq!(a, replan_phase_offset_s(name, 300), "{name} must not move");
            assert!(a < 300, "{name} offset {a} must be inside the interval");
        }
    }

    #[test]
    fn replan_phase_offset_spreads_the_fleet_across_the_interval() {
        // The property that matters: 20 VENs must not cluster. With 20 names in a
        // 300 s grid, require them to occupy at least 4 of 6 fifty-second buckets
        // — a weak bound that a constant or near-constant hash would fail.
        let mut buckets = [0_usize; 6];
        for i in 1..=20 {
            buckets[(replan_phase_offset_s(&format!("ven-{i}"), 300) / 50) as usize] += 1;
        }
        let used = buckets.iter().filter(|&&c| c > 0).count();
        assert!(
            used >= 4,
            "offsets cluster into {used} of 6 buckets: {buckets:?}"
        );
    }

    #[test]
    fn a_zero_interval_cannot_panic_or_divide_by_zero() {
        // Defensive: validate.rs rejects 0, but these are pure functions that
        // must not be a panic site if that ever changes.
        assert_eq!(replan_phase_offset_s("ven-1", 0), 0);
        assert!(next_replan_at(at(1_000_000), 0, 0).timestamp() > 1_000_000);
    }
}
