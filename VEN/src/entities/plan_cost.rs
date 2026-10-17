//! What a plan spends on one asset: the one rule for "cost of this asset's allocations", used
//! by the session flexibility estimate and by the comfort-curve card.
//!
//! Reads the plan's slots only. It never decides what an asset needs or what a curve is worth.

use chrono::{DateTime, Utc};

use super::plan::PlanTimeSlot;
use super::units::dt_h_from_duration;

/// Cost [€] and CO2 [g] of the asset's allocations in the slots starting before `until`: the
/// grid share is priced at the slot import tariff and the PV-surplus share at its export
/// opportunity price (the revenue forgone by not exporting it). CO2 comes from the allocation
/// (grid share only; surplus is zero-carbon). `None` when no slot before `until` allocates
/// anything to the asset, so a caller can fall back to a pre-solve estimate.
pub fn asset_planned_cost(
    slots: &[PlanTimeSlot],
    asset_id: &str,
    until: DateTime<Utc>,
) -> Option<(f64, f64)> {
    let mut found = false;
    let mut cost_eur = 0.0;
    let mut co2_g = 0.0;
    for slot in slots.iter().filter(|s| s.start < until) {
        let dt_h = dt_h_from_duration(slot.end - slot.start);
        for alloc in slot.allocations.iter().filter(|a| a.asset_id == asset_id) {
            found = true;
            cost_eur += (alloc.grid_power_kw * slot.import_tariff_eur_kwh
                + alloc.surplus_power_kw * slot.export_tariff_eur_kwh)
                * dt_h;
            co2_g += alloc.co2_g;
        }
    }
    found.then_some((cost_eur, co2_g))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::plan::AssetAllocation;
    use crate::services::test_support::plans::flat_plan;
    use chrono::{Duration, TimeZone};

    fn alloc(asset_id: &str, grid_kw: f64, surplus_kw: f64, co2_g: f64) -> AssetAllocation {
        AssetAllocation {
            asset_id: asset_id.into(),
            power_kw: grid_kw + surplus_kw,
            surplus_power_kw: surplus_kw,
            grid_power_kw: grid_kw,
            marginal_value: 0.0,
            cost_eur: 0.0,
            co2_g,
        }
    }

    fn start() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 4, 11, 6, 0, 0).unwrap()
    }

    #[test]
    fn asset_planned_cost_prices_grid_at_import_and_surplus_at_export() {
        let mut plan = flat_plan(3600, 2, start());
        for s in &mut plan.slots {
            s.import_tariff_eur_kwh = 0.30;
            s.export_tariff_eur_kwh = 0.10;
        }
        plan.slots[0].allocations.push(alloc("ev", 2.0, 1.0, 40.0));
        plan.slots[1].allocations.push(alloc("ev", 4.0, 0.0, 80.0));
        plan.slots[1]
            .allocations
            .push(alloc("heater", 9.0, 0.0, 9.0));
        let (cost_eur, co2_g) =
            asset_planned_cost(&plan.slots, "ev", start() + Duration::hours(2)).unwrap();
        // slot 0: (2 x 0.30 + 1 x 0.10) x 1 h = 0.70; slot 1: 4 x 0.30 x 1 h = 1.20
        assert!((cost_eur - 1.90).abs() < 1e-9, "{cost_eur}");
        assert!((co2_g - 120.0).abs() < 1e-9, "{co2_g}");
    }

    #[test]
    fn asset_planned_cost_stops_at_until() {
        let mut plan = flat_plan(3600, 2, start());
        plan.slots[0].import_tariff_eur_kwh = 0.30;
        plan.slots[1].import_tariff_eur_kwh = 0.30;
        plan.slots[0].allocations.push(alloc("ev", 1.0, 0.0, 0.0));
        plan.slots[1].allocations.push(alloc("ev", 1.0, 0.0, 0.0));
        let (cost_eur, _) =
            asset_planned_cost(&plan.slots, "ev", start() + Duration::hours(1)).unwrap();
        assert!((cost_eur - 0.30).abs() < 1e-9, "{cost_eur}");
    }

    #[test]
    fn asset_planned_cost_is_none_without_an_allocation() {
        let plan = flat_plan(3600, 2, start());
        assert!(asset_planned_cost(&plan.slots, "ev", start() + Duration::hours(2)).is_none());
    }
}
