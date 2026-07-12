#!/usr/bin/env python3
"""WP4.5 — persona presets for the diverse-fleet experiments (Phase 4).

A persona is pure configuration on top of WP4.1 (UserRequestMode +
budget_eur on EV sessions) and WP4.2 (comfort-curve overrides):

  eco       — OPPORTUNISTIC EV charging (free energy only), aggressive
              (low-bid) comfort curve, low base load, battery likely.
  comfort   — ASAP EV charging (cost-blind), flat high-bid comfort curve,
              higher base load; convenience beats price.
  commuter  — EV must be ready BY_DEADLINE at 07:00, low daytime base
              load, a charging-budget ceiling recorded on the session
              (the hard cap binds only in MAX_COST mode today).

Consumed by scripts/gen_fleet_profiles.py (asset-mix nudges + manifest
persona tag) and experiments/run_experiment.py --personas (per-VEN EV
session + comfort-curve override at experiment start).

Self-check: python3 scripts/personas.py
"""

import random

PERSONAS = {
    "eco": {
        # asset-mix nudges for gen_fleet_profiles.gen_profile
        "ev_probability": 1.0,
        "battery_probability": 0.9,
        "base_load_range_kw": (0.2, 0.4),
        # runtime session defaults (run_experiment --personas)
        "ev_mode": "OPPORTUNISTIC",
        "ev_target_soc": 0.8,
        "ev_departure_hour_utc": None,  # no deadline
        "ev_budget_eur": None,
        # WP4.2 comfort-curve override (asset "ev"): only cheap energy is worth it
        "comfort_curve": [
            {"fill": 0.5, "max_marginal_price": 0.10, "max_marginal_co2": 0.0},
            {"fill": 0.8, "max_marginal_price": 0.06, "max_marginal_co2": 0.0},
            {"fill": 1.0, "max_marginal_price": 0.03, "max_marginal_co2": 0.0},
        ],
    },
    "comfort": {
        "ev_probability": 0.8,
        "battery_probability": 0.5,
        "base_load_range_kw": (0.5, 0.8),
        "ev_mode": "ASAP",
        "ev_target_soc": 0.9,
        "ev_departure_hour_utc": None,
        "ev_budget_eur": None,
        # flat, high bids: comfort at (almost) any price
        "comfort_curve": [
            {"fill": 0.5, "max_marginal_price": 0.45, "max_marginal_co2": 0.0},
            {"fill": 1.0, "max_marginal_price": 0.40, "max_marginal_co2": 0.0},
        ],
    },
    "commuter": {
        "ev_probability": 1.0,
        "battery_probability": 0.3,
        "base_load_range_kw": (0.2, 0.4),
        "ev_mode": "BY_DEADLINE",
        "ev_target_soc": 0.85,
        "ev_departure_hour_utc": 7,  # ready at 07:00 (next occurrence)
        "ev_budget_eur": 2.0,
        "comfort_curve": [
            {"fill": 0.6, "max_marginal_price": 0.30, "max_marginal_co2": 0.0},
            {"fill": 1.0, "max_marginal_price": 0.15, "max_marginal_co2": 0.0},
        ],
    },
}


def parse_persona_spec(spec):
    """"eco:0.4,comfort:0.4,commuter:0.2" → [("eco", 0.4), ...]. Ratios are
    normalized; unknown persona names raise."""
    pairs = []
    for part in spec.split(","):
        name, _, ratio = part.partition(":")
        name = name.strip()
        if name not in PERSONAS:
            raise ValueError(f"unknown persona '{name}' (known: {sorted(PERSONAS)})")
        pairs.append((name, float(ratio or 1.0)))
    total = sum(r for _, r in pairs)
    if total <= 0:
        raise ValueError("persona ratios must sum to > 0")
    return [(n, r / total) for n, r in pairs]


def assign_personas(count, spec, rng: random.Random):
    """Deterministic (seeded) persona list of length `count`: proportional
    counts by ratio (largest-remainder rounding), then a seeded shuffle."""
    pairs = parse_persona_spec(spec)
    quotas = [(name, ratio * count) for name, ratio in pairs]
    counts = {name: int(q) for name, q in quotas}
    remainder = count - sum(counts.values())
    for name, _ in sorted(quotas, key=lambda x: -(x[1] - int(x[1])))[:remainder]:
        counts[name] += 1
    out = [name for name, c in counts.items() for _ in range(c)]
    rng.shuffle(out)
    return out


if __name__ == "__main__":
    rng = random.Random(42)
    assigned = assign_personas(10, "eco:0.4,comfort:0.4,commuter:0.2", rng)
    assert len(assigned) == 10, assigned
    assert assigned.count("eco") == 4 and assigned.count("comfort") == 4, assigned
    assert assigned.count("commuter") == 2, assigned
    # Determinism: the same seed yields the same assignment.
    assert assigned == assign_personas(10, "eco:0.4,comfort:0.4,commuter:0.2", random.Random(42))
    # Rounding: remainders go to the largest fractional quotas.
    assert len(assign_personas(7, "eco:0.5,comfort:0.5", random.Random(1))) == 7
    try:
        parse_persona_spec("hedonist:1.0")
        raise AssertionError("unknown persona must raise")
    except ValueError:
        pass
    print("personas.py self-check OK:", assigned)
