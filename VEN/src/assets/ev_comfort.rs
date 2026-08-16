//! BL-34 / BL-17: resolves an EV session's comfort curve (price + CO2 bid) into the
//! reward scalars `EvMilpContext::from_state` uses for its `ByDeadline`/`Asap` arm —
//! the only mode where the curve retains its original "reward for completing core /
//! reward for topping off beyond core" meaning (every other mode redirects
//! `v_extra_eur_kwh` to an unrelated signal, so the curve doesn't apply there).
//!
//! Split out of `ev_milp.rs` to keep it under the VEN/src/ 500-production-line cap
//! (`ven-architecture` rule, `.claude/CLAUDE.md`) — this was already the densest arm
//! in `from_state` before the CO2 axis existed.

use crate::entities::asset::ComfortRate;
use crate::entities::device_session::EvSession;

/// Price and CO2 reward scalars for the `ByDeadline`/`Asap` mode arm. CO2 fields are
/// already monetized (€/kWh, via `w_ghg_eur_kg`) so `EvMilpContext`'s objective can
/// treat them exactly like the price reward — no unit conversion left to do there.
pub(super) struct EvComfortReward {
    pub v_core_eur_kwh: f64,
    pub v_extra_eur_kwh: f64,
    pub v_core_co2_eur_kwh: f64,
    pub v_extra_co2_eur_kwh: f64,
}

/// Empty curve (no session-intent comfort override was ever resolved for this session,
/// e.g. the legacy `/ev-session` route or a VTN-commanded session) keeps the passed-in
/// global price defaults and zero CO2 reward — matches pre-BL-34 behavior exactly for
/// price, and BL-17's "no bid expressed" default for CO2.
pub(super) fn resolve_ev_comfort_reward(
    session: &EvSession,
    v_ev_core_eur_kwh: f64,
    v_ev_extra_eur_kwh: f64,
    w_ghg_eur_kg: f64,
) -> EvComfortReward {
    if session.comfort_rates.is_empty() {
        return EvComfortReward {
            v_core_eur_kwh: v_ev_core_eur_kwh,
            v_extra_eur_kwh: v_ev_extra_eur_kwh,
            v_core_co2_eur_kwh: 0.0,
            v_extra_co2_eur_kwh: 0.0,
        };
    }
    let v_core_eur_kwh = ComfortRate::value_at_fill(&session.comfort_rates, 0.0);
    let v_extra_eur_kwh = ComfortRate::value_at_fill(&session.comfort_rates, 1.0);
    let v_core_co2_eur_kwh =
        (ComfortRate::co2_value_at_fill(&session.comfort_rates, 0.0) / 1000.0) * w_ghg_eur_kg;
    let v_extra_co2_eur_kwh =
        (ComfortRate::co2_value_at_fill(&session.comfort_rates, 1.0) / 1000.0) * w_ghg_eur_kg;
    EvComfortReward {
        v_core_eur_kwh,
        v_extra_eur_kwh,
        v_core_co2_eur_kwh,
        v_extra_co2_eur_kwh,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::asset::ComfortRate;
    use crate::entities::design_vocabulary::UserRequestMode;
    use chrono::Utc;

    fn session_with_rates(rates: Vec<ComfortRate>) -> EvSession {
        EvSession {
            id: uuid::Uuid::new_v4(),
            target_soc: 0.8,
            departure_time: Utc::now() + chrono::Duration::hours(4),
            mode: UserRequestMode::ByDeadline,
            soft_deadline: false,
            budget_eur: None,
            comfort_rates: rates,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn empty_curve_keeps_global_price_defaults_and_zero_co2() {
        let session = session_with_rates(vec![]);
        let r = resolve_ev_comfort_reward(&session, 1.0, 0.05, 0.5);
        assert_eq!(r.v_core_eur_kwh, 1.0);
        assert_eq!(r.v_extra_eur_kwh, 0.05);
        assert_eq!(r.v_core_co2_eur_kwh, 0.0);
        assert_eq!(r.v_extra_co2_eur_kwh, 0.0);
    }

    #[test]
    fn non_empty_curve_sources_price_and_co2_from_fill_0_and_1() {
        let session = session_with_rates(vec![
            ComfortRate {
                fill: 0.0,
                max_marginal_price: 1.2,
                max_marginal_co2: 300.0,
            },
            ComfortRate {
                fill: 1.0,
                max_marginal_price: 0.05,
                max_marginal_co2: 100.0,
            },
        ]);
        // w_ghg = 0.5 EUR/kgCO2: 300 g/kWh -> 0.15 EUR/kWh, 100 g/kWh -> 0.05 EUR/kWh
        let r = resolve_ev_comfort_reward(&session, 999.0, 999.0, 0.5);
        assert!((r.v_core_eur_kwh - 1.2).abs() < 1e-9);
        assert!((r.v_extra_eur_kwh - 0.05).abs() < 1e-9);
        assert!((r.v_core_co2_eur_kwh - 0.15).abs() < 1e-9);
        assert!((r.v_extra_co2_eur_kwh - 0.05).abs() < 1e-9);
    }
}
