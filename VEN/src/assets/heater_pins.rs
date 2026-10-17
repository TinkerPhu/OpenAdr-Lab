// ── Heater stages fixed before the solve ─────────────────────────────────────
// Decides which heater slots the MILP may not choose freely, and at what power:
// what the thermostat forces (physics, from `Heater::forced_kw_ahead`) over the
// plan in force's anchor (a stability preference). Reads only the heater's own
// forecast and the tank bounds; never re-states the thermostat rule itself.
// Split out of `heater_milp.rs` (500-production-line cap, `ven-architecture`).

use super::{AssetState, Heater};
use crate::entities::time_grid::slot_hours;
use crate::entities::units::energy_kwh;

/// The power fixed for each slot before the solve: what the thermostat will force from
/// now (`Heater::forced_kw_ahead`, R-94) over the plan in force's `anchored_kw`, so the
/// plan stops proposing dispatch the heater refuses. `(e_init_kwh, q_dem_kw)`: the
/// MILP's starting tank energy and constant demand; `tank_bounds_kwh`: its tank bounds.
pub(super) fn fixed_stage_kw(
    cfg: &Heater,
    state: &AssetState,
    anchored_kw: Vec<Option<f64>>,
    (e_init_kwh, q_dem_kw): (f64, f64),
    tank_bounds_kwh: (f64, f64),
    cum_s: &[i64],
) -> Vec<Option<f64>> {
    let forced_kw = match state {
        AssetState::Heater(s) => cfg.forced_kw_ahead(s, q_dem_kw, &slot_hours(cum_s)),
        _ => Vec::new(),
    };
    lay_forced_over_anchor(
        anchored_kw,
        &forced_kw,
        (e_init_kwh, q_dem_kw),
        tank_bounds_kwh,
        cum_s,
    )
}

/// `forced_kw` wins over `anchored_kw`, and an anchor is cut where the tank could not
/// hold it. A forced slot is never dropped -- the heater draws it whatever the plan
/// says -- so it is laid over the anchor both before the replay (the anchor continues
/// from the forced trajectory) and after it.
fn lay_forced_over_anchor(
    anchored_kw: Vec<Option<f64>>,
    forced_kw: &[f64],
    (e_init_kwh, q_dem_kw): (f64, f64),
    tank_bounds_kwh: (f64, f64),
    cum_s: &[i64],
) -> Vec<Option<f64>> {
    let force = |mut kw: Vec<Option<f64>>| {
        if kw.len() < forced_kw.len() {
            kw.resize(forced_kw.len(), None);
        }
        for (slot, &forced) in kw.iter_mut().zip(forced_kw) {
            *slot = Some(forced);
        }
        kw
    };
    let held = anchor_the_tank_can_hold(
        force(anchored_kw),
        e_init_kwh,
        tank_bounds_kwh,
        q_dem_kw,
        cum_s,
    );
    force(held)
}

/// The stage the heater was last observed at, rounded to the nearest reachable level:
/// where the MILP's switching count starts.
pub(super) fn initial_stage(cfg: &Heater, state: &AssetState) -> f64 {
    let actual_kw = match state {
        AssetState::Heater(s) => s.actual_power_kw,
        _ => 0.0,
    };
    let (p_step_kw, n_stages) = (cfg.p_step_kw(), cfg.power_stages as f64);
    kw_to_stage(actual_kw, p_step_kw, n_stages).unwrap_or_else(|| {
        if p_step_kw > 0.0 {
            (actual_kw / p_step_kw).round().clamp(0.0, n_stages)
        } else {
            0.0
        }
    })
}

/// The anchor (the plan in force's heater block) up to the last slot the tank can hold:
/// replayed from a tank that gained heat since, a block that filled it exactly overflows,
/// and a pinned stage under a hard ceiling fails the whole site's solve at presolve. The
/// anchor is a stability preference, never physics. `(floor_kwh, ceiling_kwh)`: tank bounds.
fn anchor_the_tank_can_hold(
    mut anchored_kw: Vec<Option<f64>>,
    e_init_kwh: f64,
    (floor_kwh, ceiling_kwh): (f64, f64),
    q_dem_kw: f64,
    cum_s: &[i64],
) -> Vec<Option<f64>> {
    let dt_h = slot_hours(cum_s);
    let mut e_kwh = e_init_kwh;
    for t in 0..anchored_kw.len() {
        let Some(kw) = anchored_kw[t] else { break };
        e_kwh += energy_kwh(kw - q_dem_kw, dt_h.get(t).copied().unwrap_or(0.0));
        if e_kwh > ceiling_kwh + 1e-9 || e_kwh < floor_kwh - 1e-9 {
            anchored_kw[t..].iter_mut().for_each(|a| *a = None);
            break;
        }
    }
    anchored_kw
}

/// Map a planned heater power [kW] to a stage index.
/// Returns `Some(k)` when `kw` is within 0.1 kW of `k × p_step_kw` for some
/// reachable stage, or `None` when it matches no stage (anchor left free).
pub(super) fn kw_to_stage(kw: f64, p_step_kw: f64, n_stages: f64) -> Option<f64> {
    const TOL: f64 = 0.1;
    if p_step_kw <= 0.0 {
        return if kw.abs() < TOL { Some(0.0) } else { None };
    }
    let k = (kw / p_step_kw).round();
    if !(0.0..=n_stages).contains(&k) {
        return None;
    }
    ((kw - k * p_step_kw).abs() < TOL).then_some(k)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn five_minute_grid(n: i64) -> Vec<i64> {
        (0..=n).map(|i| i * 300).collect()
    }

    #[test]
    fn lay_forced_over_anchor_lays_forced_slots_over_the_anchor() {
        let anchor = vec![Some(3.0), Some(3.0), Some(3.0), None];
        let fixed = lay_forced_over_anchor(
            anchor,
            &[0.0, 0.0],
            (2.0, 0.5),
            (-5.0, 5.0),
            &five_minute_grid(4),
        );
        assert_eq!(fixed, vec![Some(0.0), Some(0.0), Some(3.0), None]);
    }

    #[test]
    fn lay_forced_over_anchor_pins_forced_slots_without_an_anchor() {
        let fixed = lay_forced_over_anchor(
            vec![],
            &[0.0, 0.0],
            (2.0, 0.5),
            (-5.0, 5.0),
            &five_minute_grid(4),
        );
        assert_eq!(fixed, vec![Some(0.0), Some(0.0)]);
    }

    #[test]
    fn lay_forced_over_anchor_keeps_forced_slots_where_the_anchor_is_cut() {
        // The anchor after the forced slots overflows the tank (ceiling 2.05 kWh): it is
        // cut, the forced slots are not.
        let anchor = vec![None, None, Some(6.0), Some(6.0)];
        let fixed = lay_forced_over_anchor(
            anchor,
            &[0.0, 0.0],
            (2.0, 0.0),
            (-5.0, 2.05),
            &five_minute_grid(4),
        );
        assert_eq!(fixed, vec![Some(0.0), Some(0.0), None, None]);
    }
}
