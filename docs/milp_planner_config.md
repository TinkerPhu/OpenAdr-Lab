# MILP Planner Configuration — Simplification Analysis

This document analyses the `[planner]` section of VEN profile YAML files,
identifies what is correctly configurable, what should be standardised as
defaults, what should become a code-level constant, and how the timeline API
should expose planning resolution to the UI.

Reference profiles: `VEN/profiles/ven-1.yaml`, `ven-2.yaml`, `ven-3.yaml`.
Config struct: `VEN/src/profile.rs` → `PlannerConfig`.

---

## Current state across profiles

| Parameter | default | ven-1 | ven-2 | ven-3 |
|---|---|---|---|---|
| `plan_step_s` | 300 | — (300) | 600 | — (300) |
| `plan_horizon_h` | 24 | — (24) | 48 | — (24) |
| `replan_interval_s` | 300 | 300 | 300 | 300 |
| `solver_timeout_s` | 60 | 60 | 60 | 60 |
| `planning_initial_delay_s` | 5 | 5 | 5 | 5 |
| `c_ctrl_imp_malus_eur_kwh` | **0.0** | 0.22 | 0.22 | 0.22 |
| `plan_adoption_threshold_eur` | 0.0 | 0.20 | 0.20 | **0.05** |
| `plan_adoption_decay_s` | 0.0 | 1500 | 1500 | **1200** |
| `phase2_epsilon_eur` | 0.02 | — (0.02) | 1.00 | **5.0** |
| `gate_switch_penalty_eur` | 0.0 | — | 0.50 | — |

Plus ~14 MILP weight fields (`w_energy`, `w_ghg`, `c_bat_wear_eur_kwh`,
`c_ev_startup_eur`, `c_bat_startup_eur`, `c_ev_ramp_eur_kw`, `c_bat_ramp_eur_kw`,
`c_bat_ev_coexist_eur_kwh`, `w_viol`, `pen_imp_eur_kwh`, `pen_exp_eur_kwh`,
`v_ev_extra_eur_kwh`, `v_ev_core_eur_kwh`, `w_tier_penalty_eur`) that **no
production profile ever sets** — all run at their code defaults.

---

## Target planning grid

The target architecture is a 3-tier variable-step grid with 288 total slots
and a 48 h horizon:

| Zone | Range | Step | Slots | Purpose |
|---|---|---|---|---|
| A | 0 – 8 h | 5 min (300 s) | 96 | Near-term: EV deadline, battery, small heater |
| B | 8 – 24 h | 10 min (600 s) | 96 | Overnight scheduling |
| C | 24 – 48 h | 15 min (900 s) | 96 | Inter-day thermal strategy |

Zone A at 5-min resolution is intentional and important for EV session
scheduling where deadline precision matters.

**Zone step constraint:** every zone's `step_s` must be an integer multiple
of Zone A's `step_s`. With Zone A = 300 s this means Zone B and C steps
must be multiples of 300 (e.g. 600 = 2×, 900 = 3×). This ensures
forward-filling to Zone A resolution is pure integer repetition with no
rounding or interpolation. The VEN must validate this at startup and fail
fast if the constraint is violated.

---

## What belongs where

### Keep as configurable — but strip per-profile repetition

`replan_interval_s`, `solver_timeout_s`, and `planning_initial_delay_s` are
legitimate operational tuning knobs. They should remain profile fields so
operators can adjust them without recompiling. However, all production VENs
currently carry identical values that match the code defaults — meaning the
explicit entries add noise without information.

**Action:** raise the code defaults to the correct values and remove the
redundant lines from all production profiles. Add commented-out lines in
each profile showing the default, so operators know what they're inheriting:

```yaml
[planner]
# replan_interval_s = 300     # default
# solver_timeout_s = 60       # default
# planning_initial_delay_s = 5  # default
plan_adoption_threshold_eur = 0.20
```

The values must be the same across all VENs. There is currently no
enforcement mechanism for this beyond convention — per-VEN drift (as
already seen with `plan_adoption_decay_s`) shows that convention alone
fails. When fleet-wide config grows further, a shared `common.yaml` loaded
by all VENs is the right next step; for now, consistent defaults plus
commented documentation is sufficient.

### Fix the default — wrong fallback

`c_ctrl_imp_malus_eur_kwh` defaults to `0.0` in `PlannerConfig::default()`,
but every profile sets it to `0.22`. A fresh profile that omits this line
silently disables the import malus.

**Action:** change the code default to `0.22`. Remove the explicit setting
from all profiles.

### Standardise and raise the defaults

The gate parameters are set to the same value in ven-1 and ven-2 but differ
in ven-3 (0.05 / 1200) without documented justification:

- `plan_adoption_threshold_eur`: 0.20 (ven-1, ven-2) vs. 0.05 (ven-3)
- `plan_adoption_decay_s`: 1500 (ven-1, ven-2) vs. 1200 (ven-3)

The ven-1 / ven-2 values (0.20 / 1500) are intentional and documented in
`milp_storage_planning.md`. The ven-3 divergence appears to be drift.

**Action:** raise code defaults to `0.20` and `1500`. Remove from all
production profiles.

### Replace `plan_step_s` / `plan_horizon_h` with a zone list

A single `plan_step_s` integer cannot describe the 3-tier grid.
`plan_horizon_h` follows directly from the zone definitions. Both fields
are replaced by a structured `plan_zones` list:

```yaml
# Production profiles omit plan_zones — the 3-tier default applies.
# Test profiles can declare a single coarse zone for fast solver runs:
[planner]
plan_zones = [
  { step_s = 3600, slots = 24 }   # 1h × 24 = 24h, fast E2E tests
]
```

The VEN builds its planning grid from the `plan_zones` list. When the field
is absent, the 3-tier default is used. Zone step validation (multiple-of-first
constraint) runs at startup regardless of source.

This preserves test flexibility — `test.yaml` (currently `plan_step_s: 3600`,
24 slots) and `no_pv_test.yaml` (currently `plan_step_s: 1800`, 48 slots)
both translate cleanly to single-zone lists. Production profiles carry no
zone config at all.

### Keep as explicit config — but validate at startup

`phase2_epsilon_eur` and `gate_switch_penalty_eur` cannot be cleanly
auto-computed once the step size is variable, because the effective switching
cost per relay operation varies by zone:

| Zone | step | `switching_penalty_eur: 3.0` | effective cost/switch |
|---|---|---|---|
| A | 5 min | 3.0 × (5/60) | **0.25 EUR** |
| B | 10 min | 3.0 × (10/60) | **0.50 EUR** |
| C | 15 min | 3.0 × (15/60) | **0.75 EUR** |

A reasonable target for `phase2_epsilon_eur` is 2× the maximum zone cost
(= 2 × 0.75 = **1.50 EUR** for the above heater). For a VEN without a heater
these parameters are irrelevant (leave at 0.0).

`gate_switch_penalty_eur` in the current implementation counts heater relay
transitions uniformly across zones. This is an approximation: a Zone-C switch
costs 3× a Zone-A switch internally in the MILP but is treated equally by the
gate. When the 3-tier grid is implemented, `count_heater_switches` should
weight each transition by its zone's `dt_h` to stay consistent with MILP costs.

**Current ven-3 misconfiguration:** `phase2_epsilon_eur: 5.0` with
`switching_penalty_eur: 0.50` and 5-min slots → effective cost = 0.083 EUR/switch,
target epsilon ≈ 0.17 EUR. The configured value is ~30× too large, which lets
Phase 2 override Phase 1 cost results in pathological cases.

**Action:** keep `phase2_epsilon_eur` and `gate_switch_penalty_eur` as explicit
profile fields, but add a startup validation that warns (or errors) when
`phase2_epsilon_eur` exceeds a sanity bound derived from the heater's
`switching_penalty_eur` and the longest zone step. Fix ven-3 immediately.

### Leave as-is — useful escape hatches

The 14 MILP weight fields (`w_energy`, `w_ghg`, `c_bat_wear_eur_kwh`, etc.)
are never set in production profiles and all run at their code defaults. They
add profile surface area but are purely additive — omitting them never causes
silent misbehaviour, unlike the `c_ctrl_imp_malus_eur_kwh` trap above. They
serve as documented escape hatches for future tuning without requiring code
changes.

---

## Timeline API and UI resolution

### Forward-fill to Zone A resolution

The timeline API serialises all plan slots at Zone A resolution (300 s / 5 min).
Zone B and C slots are forward-filled: a 10-min Zone B slot produces two
identical 5-min points; a 15-min Zone C slot produces three. Because every
zone's step is a multiple of Zone A's step (see constraint above), filling is
always an exact integer repetition — no interpolation, no rounding.

For 5 assets over 48 h this yields 576 points per asset, which is not heavy.
The UI is fully zone-agnostic and requires no changes when zone boundaries shift.

### Zone metadata in the response envelope

Forward-filling makes Zone B and C values indistinguishable from genuine 5-min
data. The API therefore includes a zone list in the response envelope — once per
response, not per point:

```json
{
  "zones": [
    { "from_s": 0,     "to_s": 28800,  "step_s": 300 },
    { "from_s": 28800, "to_s": 86400,  "step_s": 600 },
    { "from_s": 86400, "to_s": 172800, "step_s": 900 }
  ],
  "points": [ ... ]
}
```

`from_s` and `to_s` are seconds relative to the response's anchor timestamp.
The UI derives a point's zone via binary search on `from_s`.

This enables:

1. **Background shading** — `<ReferenceArea>` bands per zone visually
   communicate that Zone C values are coarser estimates than Zone A values.
2. **Tooltip resolution context** — "Resolution: 15 min (Zone C)" when hovering
   over a far-future point.
3. **Correct step rendering** — each point holds its value for exactly
   `step_s` seconds, not until the next 5-min tick. Charts already render
   plan data as step functions; the zone metadata makes the step *width*
   explicit.

Per-point `resolution_s` fields are not used — zone membership is per-zone
information, and encoding it per-point costs 576 redundant integers per asset
for no additional value.

For responses from test profiles (single coarse zone, e.g. 3600 s), the zone
list contains one entry. The UI code is unchanged.

---

## Summary of actions

| Action | Files affected |
|---|---|
| Strip `replan_interval_s`, `solver_timeout_s`, `planning_initial_delay_s` from profiles; add commented defaults | `ven-1.yaml`, `ven-2.yaml`, `ven-3.yaml` |
| Change `c_ctrl_imp_malus_eur_kwh` default to `0.22`; remove from profiles | `profile.rs`, all profiles |
| Change `plan_adoption_threshold_eur` default to `0.20`; remove from profiles | `profile.rs`, all profiles |
| Change `plan_adoption_decay_s` default to `1500`; remove from profiles | `profile.rs`, all profiles |
| Replace `plan_step_s` / `plan_horizon_h` with `plan_zones` list; add zone step validation | `profile.rs`, solver, `test.yaml`, `no_pv_test.yaml` |
| Fix ven-3 `phase2_epsilon_eur: 5.0` → correct value | `ven-3.yaml` |
| Add startup validation for `phase2_epsilon_eur` vs. heater switching cost and longest zone step | `profile.rs` or `main.rs` |
| Update `count_heater_switches` to weight by zone `dt_h` | `services/planning.rs` |
| Forward-fill timeline points to Zone A resolution in the API serialiser | `routes/` or timeline service |
| Add `zones` list to timeline API response envelope | API response type, frontend API types |
| Update UI to render zone background bands and tooltip resolution context | Controller charts |
