//! What the fleet delivered to its owners over a window (`fleet-service-level`).
//!
//! `fleet_store` holds what the sites did and `fleet_plan` what they intended; this reads the two
//! energies every asset declares on its snapshot: `unserved_energy_kwh` (the level its owner is
//! missing) and `stored_energy_kwh` (what its store holds). It names no asset kind: a new kind
//! that declares them is counted by construction.
//!
//! Unserved energy is the sum of the *increases* of the level between consecutive stored samples:
//! an EV leaving short, a tank dropping under its floor and a job missing its deadline are all a
//! step up, and a recovery is a step down that adds nothing. The carry-over is the last stored
//! energy minus the first. A sample without the figure is skipped, never read as zero, and a total
//! says how many VENs it is built from.

use serde_json::{json, Value};
use std::collections::BTreeMap;

use chrono::{DateTime, Utc};

const UNSERVED_FIELD: &str = "unserved_energy_kwh";
const STORED_FIELD: &str = "stored_energy_kwh";

/// One asset's declared figures over a window, in time order, only where the asset declared them.
#[derive(Debug, Default, PartialEq)]
pub struct AssetSeries {
    pub ven_name: String,
    pub asset_id: String,
    pub unserved_kwh: Vec<f64>,
    pub stored_kwh: Vec<f64>,
}

/// The sum of the positive steps of a level; `None` when there is no sample to step between.
pub fn accrued_increase(levels: &[f64]) -> Option<f64> {
    (!levels.is_empty()).then(|| levels.windows(2).map(|w| (w[1] - w[0]).max(0.0)).sum())
}

/// Last minus first; `None` below two samples, where there is nothing to compare.
pub fn carry_over(levels: &[f64]) -> Option<f64> {
    (levels.len() >= 2).then(|| levels[levels.len() - 1] - levels[0])
}

/// Turn ordered `(ven, minute, assets)` rows (one per VEN and minute, `assets` the snapshot's
/// per-asset object) into one series per VEN and asset. Separate from the query so the shaping is
/// testable without a database.
pub fn shape_samples(rows: Vec<(String, DateTime<Utc>, Option<Value>)>) -> Vec<AssetSeries> {
    let mut by_asset: BTreeMap<(String, String), AssetSeries> = BTreeMap::new();
    for (ven_name, _minute, assets) in rows {
        let Some(Value::Object(assets)) = assets else {
            continue;
        };
        for (asset_id, asset) in assets {
            let series = by_asset
                .entry((ven_name.clone(), asset_id.clone()))
                .or_insert_with(|| AssetSeries {
                    ven_name: ven_name.clone(),
                    asset_id,
                    ..Default::default()
                });
            if let Some(v) = asset.get(UNSERVED_FIELD).and_then(Value::as_f64) {
                series.unserved_kwh.push(v);
            }
            if let Some(v) = asset.get(STORED_FIELD).and_then(Value::as_f64) {
                series.stored_kwh.push(v);
            }
        }
    }
    by_asset.into_values().collect()
}

/// A sum over the parts that have a value, and how many that is; `None` when none has.
fn total(parts: impl Iterator<Item = Option<f64>>) -> (Option<f64>, usize) {
    let present: Vec<f64> = parts.flatten().collect();
    (
        (!present.is_empty()).then(|| present.iter().sum()),
        present.len(),
    )
}

fn quantity(sum_kwh: Option<f64>, contributing_vens: usize) -> Value {
    json!({ "kwh": sum_kwh, "contributingVens": contributing_vens })
}

/// The report over a window: per asset, per VEN and for the fleet.
pub fn report(series: &[AssetSeries]) -> Value {
    let mut by_ven: BTreeMap<&str, Vec<&AssetSeries>> = BTreeMap::new();
    for s in series {
        by_ven.entry(&s.ven_name).or_default().push(s);
    }
    let vens: Vec<(&str, Option<f64>, Option<f64>, Value)> = by_ven
        .iter()
        .map(|(ven_name, assets)| {
            let (unserved, _) = total(assets.iter().map(|a| accrued_increase(&a.unserved_kwh)));
            let (carry, _) = total(assets.iter().map(|a| carry_over(&a.stored_kwh)));
            let assets: Vec<Value> = assets
                .iter()
                .map(|a| {
                    json!({
                        "assetId": a.asset_id,
                        "unservedEnergyKwh": accrued_increase(&a.unserved_kwh),
                        "carryOverKwh": carry_over(&a.stored_kwh),
                    })
                })
                .collect();
            (*ven_name, unserved, carry, json!(assets))
        })
        .collect();
    let (unserved_kwh, unserved_vens) = total(vens.iter().map(|v| v.1));
    let (carry_kwh, carry_vens) = total(vens.iter().map(|v| v.2));
    json!({
        "unservedEnergyKwh": quantity(unserved_kwh, unserved_vens),
        "carryOverKwh": quantity(carry_kwh, carry_vens),
        "vens": vens.iter().map(|(ven_name, unserved, carry, assets)| json!({
            "venName": ven_name,
            "unservedEnergyKwh": unserved,
            "carryOverKwh": carry,
            "assets": assets,
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(minute: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_790_000_000 + minute * 60, 0).unwrap()
    }

    fn row(ven: &str, minute: i64, assets: Value) -> (String, DateTime<Utc>, Option<Value>) {
        (ven.to_string(), t(minute), Some(assets))
    }

    #[test]
    fn two_departures_add_up_their_steps() {
        assert_eq!(
            accrued_increase(&[0.0, 15.0, 15.0, 0.0, 6.0, 6.0]),
            Some(21.0)
        );
    }

    #[test]
    fn a_dropped_sample_keeps_the_step_it_hid() {
        assert_eq!(
            accrued_increase(&[0.0, 0.0, 15.0]),
            accrued_increase(&[0.0, 15.0])
        );
    }

    #[test]
    fn a_decrease_adds_nothing() {
        assert_eq!(accrued_increase(&[15.0, 9.0, 0.0]), Some(0.0));
    }

    #[test]
    fn carry_over_is_the_last_minus_the_first() {
        assert_eq!(carry_over(&[120.0, 110.0, 96.0]), Some(-24.0));
    }

    #[test]
    fn a_window_with_nothing_to_compare_has_no_answer_not_zero() {
        assert_eq!(accrued_increase(&[]), None);
        assert_eq!(carry_over(&[5.0]), None);
    }

    #[test]
    fn a_sample_without_the_figure_is_skipped_never_read_as_zero() {
        let series = shape_samples(vec![
            row(
                "ven-1",
                0,
                json!({"ev": {"unserved_energy_kwh": 0.0, "stored_energy_kwh": 20.0}}),
            ),
            row("ven-1", 1, json!({"ev": {}})),
            row(
                "ven-1",
                2,
                json!({"ev": {"unserved_energy_kwh": 12.0, "stored_energy_kwh": 8.0}}),
            ),
        ]);
        assert_eq!(series.len(), 1);
        assert_eq!(series[0].unserved_kwh, vec![0.0, 12.0]);
        assert_eq!(series[0].stored_kwh, vec![20.0, 8.0]);
    }

    #[test]
    fn a_kind_the_code_has_never_seen_is_counted_when_it_declares() {
        let series = shape_samples(vec![
            row(
                "ven-1",
                0,
                json!({"heat_pump_1": {"unserved_energy_kwh": 0.0}}),
            ),
            row(
                "ven-1",
                1,
                json!({"heat_pump_1": {"unserved_energy_kwh": 3.0}}),
            ),
        ]);
        let out = report(&series);
        assert_eq!(out["unservedEnergyKwh"]["kwh"], json!(3.0));
    }

    #[test]
    fn an_asset_that_declares_neither_figure_adds_nothing() {
        let series = shape_samples(vec![
            row("ven-1", 0, json!({"pv": {"power_kw": 1.0}})),
            row("ven-1", 1, json!({"pv": {"power_kw": 2.0}})),
        ]);
        let out = report(&series);
        assert_eq!(
            out["unservedEnergyKwh"],
            json!({"kwh": null, "contributingVens": 0})
        );
        assert_eq!(
            out["carryOverKwh"],
            json!({"kwh": null, "contributingVens": 0})
        );
    }

    #[test]
    fn the_fleet_total_says_how_many_vens_it_is_built_from() {
        let mut rows = Vec::new();
        for ven in ["ven-1", "ven-2"] {
            rows.push(row(
                ven,
                0,
                json!({"ev": {"unserved_energy_kwh": 0.0, "stored_energy_kwh": 30.0}}),
            ));
            rows.push(row(
                ven,
                1,
                json!({"ev": {"unserved_energy_kwh": 10.0, "stored_energy_kwh": 18.0}}),
            ));
        }
        // a third VEN stopped publishing: it has no row at all
        let out = report(&shape_samples(rows));
        assert_eq!(
            out["unservedEnergyKwh"],
            json!({"kwh": 20.0, "contributingVens": 2})
        );
        assert_eq!(
            out["carryOverKwh"],
            json!({"kwh": -24.0, "contributingVens": 2})
        );
        assert_eq!(out["vens"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn a_ven_with_no_samples_is_absent_not_zero() {
        let out = report(&shape_samples(vec![]));
        assert_eq!(
            out["unservedEnergyKwh"],
            json!({"kwh": null, "contributingVens": 0})
        );
        assert_eq!(out["vens"], json!([]));
    }
}
