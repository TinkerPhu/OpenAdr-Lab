# VEN MILP Planner — Architecture Reference

**Scope:** Design decisions, data structures, and invariants for the VEN's MILP-based planning engine.
The planner replaced the earlier greedy scheduler. The VEN_ARCHITECTURE.md overview diagram still applies; this document expands section 2.3 of that file.

---

## 1. Overview

The VEN runs a two-phase Mixed-Integer Linear Program (MILP) at every replanning cycle to produce a 48 h asset allocation plan. The solver is HiGHS, accessed via the `good_lp` Rust crate.

**Phase 1 — cost minimisation:** minimises import cost and CO₂, respects capacity limits and EV/heater deadlines.  
**Phase 2 — friction minimisation:** minimises unnecessary relay switches and ramp changes while keeping total cost within `phase2_epsilon_eur` of Phase 1's optimum. Phase 2 warm-starts from Phase 1's solution.

Key source files:

| Concern | File |
|---|---|
| Entry point | `VEN/src/controller/milp_planner/mod.rs` |
| Input tensors | `VEN/src/controller/milp_planner/inputs.rs` |
| Phase 1 solver | `VEN/src/controller/milp_planner/solver_phase1.rs` |
| Phase 2 solver | `VEN/src/controller/milp_planner/solver_phase2.rs` |
| Plan translation | `VEN/src/controller/milp_planner/results.rs` |
| Planning loop | `VEN/src/tasks/planning.rs` |
| Acceptance gate | `VEN/src/services/planning.rs` |
| Config | `VEN/src/profile.rs` → `PlannerConfig` |

---

## 2. Plan Slot Grid

### 2.1 Target architecture — three resolution zones

The planning horizon is divided into zones of increasing step size:

| Zone | Range | Step | Slots | Purpose |
|---|---|---|---|---|
| A | 0 – 8 h | 5 min (300 s) | 96 | Near-term: EV deadline, battery, heater cycles |
| B | 8 – 24 h | 10 min (600 s) | 96 | Overnight scheduling |
| C | 24 – 48 h | 15 min (900 s) | 96 | Inter-day thermal strategy |

Zone step constraint: every zone's `step_s` must be an integer multiple of Zone A's `step_s` (validated at startup). This ensures forward-filling Zone B/C data to Zone A resolution is exact integer repetition with no interpolation.

Zones are configured in the profile under `planner.plan_zones`. Production profiles carry the 3-tier list above; test profiles use a single coarse zone for fast solver runs. When `plan_zones` is absent from a profile, the code default (3-tier) applies.

### 2.2 Plan slot alignment

**Rule: `now` is always truncated to the nearest Zone-A step boundary before building the planning horizon.**

```
now_aligned = floor(wall_clock_unix_ts / step_A_s) * step_A_s
```

**Why this is critical — not just cosmetic:**

1. **Gate stability.** The VEN replans every `replan_interval_s` (300 s). Without alignment, two consecutive replans produce plans with different slot boundaries:
   - Replan at 14:23:47 → slot grid starts at 14:23:47, 14:28:47, …
   - Replan at 14:28:47 → slot grid starts at 14:28:47, 14:33:47, …
   
   The acceptance gate compares these plans to decide whether to adopt the new one. When grids are misaligned, slot `t` in the new plan covers a different time range than slot `t` in the old plan, making the cost comparison meaningless — the gate either over-accepts or under-accepts.

   With alignment, all replans within the same Zone-A window share identical grids. The gate compares plans slot-for-slot (except for zone-crossing boundries where step size changes while the time window shifts). The decision is reliable.

2. **Warm-start continuity.** The new plan is warm-started from the previous plan's allocation values (see §4). This only works when the grids align: slot `t` of the new plan must correspond to the same physical time window as slot `t` of the previous plan. Alignment guarantees this for all slots except the very first (which may have advanced one step if a zone boundary was crossed).

3. **Block-commitment anchor.** The planner can lock a slot's setpoint for the remainder of its duration (to prevent rapid resetting of heater relays, for example). The anchor is stored as a UTC timestamp. If grids are not aligned, the anchored time falls between two slots in the next replan, breaking the lock. With alignment, the anchor always coincides with a slot boundary.

4. **UI readability.** Aligned timestamps display as clean clock times (14:20:00, 14:25:00, 14:30:00) rather than arbitrary seconds.

**First-slot convention:** The first slot may start up to `step_A_s − 1` seconds in the past. The dispatcher treats this as the currently executing slot and applies its setpoints immediately upon plan adoption. This is intentional: the planner re-optimises from the current state regardless of where within the slot we are.

### Timestamp inventory — complete reference

Three distinct "now" concepts coexist in the system. Each has a specific role; mixing them causes silent bugs.

| Name | Where set | Value | Used for |
|---|---|---|---|
| `wall_now` | `tasks/planning.rs`, top of loop | `Utc::now()` | `Plan.created_at`; post-solve operations (gate decay, envelope, status report) |
| `now` (aligned) | `tasks/planning.rs`, immediately after | `align_to_step(wall_now, step_s)` | All slot timestamps, tariff sampling, deadlines, heater anchor, MILP inputs |
| request `now` | `routes/timeline.rs`, per HTTP request | `Utc::now()` at handler entry | Timeline now-point `ts`; grid window boundaries |

Derived values that consumers observe:

| Field / JSON key | Value | Meaning |
|---|---|---|
| `plan.created_at` | `wall_now` | Real age of the plan — gate decay measures `wall_now_current − plan.created_at` |
| `plan.horizon.start_time` | aligned `now` | Grid origin; first slot starts here; always a multiple of `step_s` from epoch |
| `zones[0].from` in API response | `plan.horizon.start_time` | Grid origin visible to the UI — equals aligned `now`, not the request time |
| now-point `ts` in timeline | request `now` | Exact moment of the HTTP request; not snapped to grid |

**Why `wall_now` is mandatory for gate decay:**  
`evaluate_acceptance_gate` computes `elapsed_s = (now_arg − current.created_at)`. `current.created_at` is a wall-clock time. If `now_arg` is the aligned time and `step_s > replan_interval_s` (e.g. step_s=600, replan=300), the aligned time does not advance every cycle. Consecutive calls would give `elapsed_s ≤ 0`, clamped to zero — the gate decay would be permanently disabled for those cycles. The post-solve calls therefore always pass `wall_now`:

```rust
// tasks/planning.rs — after spawn_blocking returns:
plan.created_at = wall_now;                          // real age for gate decay
adopt_if_warranted(..., wall_now).await;             // gate uses wall time
compute_envelope(&sim_snap, wall_now);               // envelope ts = real time
build_status_report(..., wall_now);                  // report ts = real time
```

**Implementation of alignment:**
```rust
let wall_now = Utc::now();
let now = align_to_step(wall_now, planner.plan_step_s);
```

`align_to_step` is a pure free function in `tasks/planning.rs`:
```rust
fn align_to_step(raw: DateTime<Utc>, step_s: u64) -> DateTime<Utc> {
    let ts = raw.timestamp();
    let step = step_s as i64;
    DateTime::<Utc>::from_timestamp(ts - ts.rem_euclid(step), 0)
        .expect("step-aligned timestamp is always valid")
}
```

**Timeline now-point:** `GET /timeline/all` includes a now-point at the exact request wall-clock time with the live simulator value at that instant. It is **not** snapped to the aligned grid and may fall between two plan slots. The UI renders it as a real-observation marker, distinct from plan-forecast points.

### 2.3 `PlanZone` and `PlanningHorizon.zones`

`PlanZone` is defined once in `entities/plan.rs` (domain layer) and carries one zone's step and slot count:
```rust
pub struct PlanZone { pub step_s: u64, pub slots: usize }
```

`PlanningHorizon` carries `zones: Vec<PlanZone>` (`#[serde(default)]` — old stored plans without the field deserialise as `vec![]`). For uniform-step plans (current Part A), `zones` always contains a single entry mirroring `step_size_s` and `num_steps`. Part B will populate it with 3 entries for the 3-tier horizon.

**Architecture note:** `profile::PlannerConfig.plan_zones` also uses `Vec<PlanZone>` — it imports the same type from `entities/plan`. There is no separate profile-layer zone type and no mapping step. This is the correct dependency direction: infra (`profile.rs`) imports domain (`entities/plan.rs`), never the reverse.

### 2.4 Cumulative slot times

Slots are not computed as `now + t × step_s` (which only works for uniform grids). Instead a cumulative-seconds array is built once from `dt_h` (per-slot durations):

```rust
let mut cum_s = vec![0i64];
for &d in &dt_h { cum_s.push(cum_s.last().unwrap() + (d * 3600.0) as i64); }
// slot t starts at:  now + Duration::seconds(cum_s[t])
// slot t ends at:    now + Duration::seconds(cum_s[t+1])
```

Reverse mapping (time offset → slot index) uses binary search on `cum_s`:
```rust
let idx = cum_s.partition_point(|&s| s <= offset_s).saturating_sub(1).min(n - 1);
```

---

## 3. Plan Adoption Gate

The gate (`services/planning.rs :: evaluate_acceptance_gate`) decides whether to replace the active plan with the newly solved plan.

**Hard triggers** (any trigger except `Periodic`) bypass the gate and always adopt.

**Periodic replans** are adopted only if the improvement exceeds the threshold after accounting for switch costs:

```
improvement = current_plan.objective_eur - new_plan.objective_eur  (adjusted for slot overlap)
surcharge   = extra_heater_switches × gate_switch_penalty_eur
adopt       = improvement > threshold_eur + surcharge
```

**Switch count weighting (3-tier):** `count_heater_switches` returns a Zone-A-normalised float. Each transition in a Zone-C slot (900 s) contributes `900 / 300 = 3.0` instead of 1.0, consistent with the MILP's internal switching cost (which scales by `dt_h`). Profile value `gate_switch_penalty_eur` is interpreted as "EUR per Zone-A-equivalent switch."

---

## 4. Warm Starting

### 4.1 Phase 2 from Phase 1 (existing)

After Phase 1 solves, `build_phase2_warm_start()` converts Phase 1's `SolveOutput` into a `Vec<(Variable, f64)>` and passes it to Phase 2 via `.with_initial_solution()`. Phase 2 starts at a known feasible point, typically solving in far fewer branch-and-bound nodes.

### 4.2 Phase 1 from previous plan (planned)

With aligned grids, the active `Plan` is a near-feasible starting point for the next Phase 1 solve. `plan_to_solve_output(plan, n)` reconstructs the key decision variable values:

| MILP variable | Source in Plan |
|---|---|
| `p_imp[t]`, `p_exp[t]` | `slots[t].net_import_kw`, `net_export_kw` |
| `p_bat_ch[t]`, `p_bat_dis[t]` | `slots[t].bat_charge_kw`, `bat_discharge_kw` |
| `e_bat[t]` | `soc_trajectory_kwh[t]` |
| `p_ev[t]` | `slots[t].planned_kw_by_asset["ev"]` |
| `z_heat_mid[t]`, `z_heat_full[t]` | approximated from `planned_kw_by_asset["heater"]` |

The warm start is skipped if the slot count differs (horizon change), which the alignment invariant makes rare (only at zone-A boundaries, every 300 s).

---

## 5. Timeline API and Zone Metadata

`GET /timeline/all` returns:
```json
{
  "zones": [
    { "from": "<ISO>", "to": "<ISO>", "step_s": 300 },
    { "from": "<ISO>", "to": "<ISO>", "step_s": 600 },
    { "from": "<ISO>", "to": "<ISO>", "step_s": 900 }
  ],
  "timelines": { "ev": [...], "battery": [...], ... }
}
```

**Forward-fill:** Plan data is serialised at Zone A resolution (300 s). A Zone-B slot (600 s) produces two identical points 300 s apart; a Zone-C slot (900 s) produces three. Historical simulator data is passed through at its native poll interval.

**UI usage:** The `zones` array drives `<ReferenceArea>` background shading in the Controller charts — Zone A transparent (i=0), Zone B slight, Zone C darker — so users can visually identify the resolution of far-future forecasts.

---

## 6. Configuration Summary

All planner configuration lives in `VEN/src/profile.rs → PlannerConfig`. Key parameters:

| Parameter | Default | Meaning |
|---|---|---|
| `plan_zones` | 3-tier (A/B/C) | Zone step and slot count definitions |
| `replan_interval_s` | 300 | How often the planning loop fires |
| `plan_adoption_threshold_eur` | 0.20 | Minimum improvement to adopt a periodic replan |
| `plan_adoption_decay_s` | 1500 | After this many seconds without adoption, force-adopt |
| `gate_switch_penalty_eur` | 0.0 | Added cost per Zone-A-equivalent heater switch in adoption gate |
| `phase2_epsilon_eur` | 0.02 | Phase 2 may not increase total cost beyond this slack |
| `c_ctrl_imp_malus_eur_kwh` | 0.22 | Malus added to import price to discourage unnecessary import |
| `solver_timeout_s` | 60 | HiGHS wall-time limit per phase |
