//! `rank_levers` must rank every planning pass's levers exactly as it did before the
//! opportunistic objective gave it a second ranking basis (openspec `opportunistic-objective`
//! D0). `frozen_rank_levers` is a verbatim copy of the function as it stood then; the test
//! compares both on many random candidate sets, including equal costs (the stable sort's
//! tie-break is behaviour) and incumbents that are, are not, or are not quite the cheapest.

use rand::{Rng, SeedableRng};

use super::super::arbiter_levers::{Lever, LeverId};
use super::super::LEVER_PREEMPTION_MARGIN_EUR_PER_KWH;
use super::rank_levers;

/// `rank_levers` before `opportunistic-objective`. Do not edit.
fn frozen_rank_levers(mut levers: Vec<Lever>, incumbent_lever: Option<&str>) -> Vec<Lever> {
    levers.sort_by(|a, b| {
        a.marginal_cost_eur_per_kwh
            .partial_cmp(&b.marginal_cost_eur_per_kwh)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if let (Some(incumbent_id), Some(cheapest)) = (incumbent_lever, levers.first().cloned()) {
        if cheapest.id.key() != incumbent_id {
            if let Some(incumbent_idx) = levers.iter().position(|l| l.id.key() == incumbent_id) {
                let incumbent = levers[incumbent_idx].clone();
                let beats_by_more_than_margin = incumbent.marginal_cost_eur_per_kwh
                    - cheapest.marginal_cost_eur_per_kwh
                    > LEVER_PREEMPTION_MARGIN_EUR_PER_KWH;
                if !beats_by_more_than_margin {
                    levers.swap(0, incumbent_idx);
                }
            }
        }
    }
    levers
}

const KINDS: [&str; 5] = [
    "battery",
    "ev",
    "heater_pause",
    "heater_emergency",
    "pv_curtail",
];
const ASSETS: [&str; 3] = ["a", "b", "c"];
/// Few distinct costs, so ties and near-ties (inside the preemption margin) are common.
const COSTS: [f64; 6] = [0.0, 0.0, 0.01, 0.02, 0.08, 0.25];

fn random_lever(rng: &mut rand::rngs::StdRng) -> Lever {
    let kind = KINDS[rng.gen_range(0..KINDS.len())];
    let id = if kind == "pv_curtail" {
        LeverId::site(kind)
    } else {
        LeverId::of_asset(kind, ASSETS[rng.gen_range(0..ASSETS.len())])
    };
    Lever {
        id,
        available_capacity_kw: rng.gen_range(0.0..5.0),
        marginal_cost_eur_per_kwh: COSTS[rng.gen_range(0..COSTS.len())]
            + if rng.gen_bool(0.2) { 0.015 } else { 0.0 },
    }
}

fn summary(levers: &[Lever]) -> Vec<(String, u64, u64)> {
    levers
        .iter()
        .map(|l| {
            (
                l.id.key(),
                l.marginal_cost_eur_per_kwh.to_bits(),
                l.available_capacity_kw.to_bits(),
            )
        })
        .collect()
}

#[test]
fn rank_levers_matches_the_frozen_ranking_on_random_candidates() {
    let mut rng = rand::rngs::StdRng::seed_from_u64(20261016);
    for case in 0..10_000 {
        let levers: Vec<Lever> = (0..rng.gen_range(0..7))
            .map(|_| random_lever(&mut rng))
            .collect();
        let incumbent = match rng.gen_range(0..3) {
            0 => None,
            1 if !levers.is_empty() => Some(levers[rng.gen_range(0..levers.len())].id.key()),
            _ => Some("battery:z".to_string()),
        };
        let got = rank_levers(levers.clone(), incumbent.as_deref());
        let want = frozen_rank_levers(levers, incumbent.as_deref());
        assert_eq!(
            summary(&got),
            summary(&want),
            "case {case}, incumbent {incumbent:?}"
        );
    }
}
