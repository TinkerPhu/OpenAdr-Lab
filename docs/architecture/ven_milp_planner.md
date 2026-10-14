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
| Input tensors | `VEN/src/controller/milp_planner/inputs.rs` (the stage order and the one full spelling of `MilpInputs`), `input_stages.rs` (one function per field group: tariffs, forecasts, capacity limits, baseline override). `MilpInputs` carries the site and the grid only; everything about a schedulable asset is in that asset's own context. Planner tests state an asset the same way, as its context (`tests/fixtures.rs::SyntheticSite`), the battery's built by the real `Battery` |
| Asset port | `VEN/src/controller/asset_milp_port.rs` (`AssetMilpContext`, `DeclaredAsset`, `AssetSolution` and their contract types); each kind implements it in `VEN/src/assets/<kind>_declared.rs`, its model maths in `<kind>_milp.rs` |
| Time grid | `VEN/src/entities/time_grid.rs` (`TimeGrid::from_zones`, `slot_at`: the one slot layout and the one "which slot holds this instant" rule, shared with the assets' MILP contexts and the plan-cycle inputs) |
| Phase 1 solver | `VEN/src/controller/milp_planner/solver_phase1.rs` |
| Phase 2 solver | `VEN/src/controller/milp_planner/solver_phase2.rs` |
| Shared model skeleton | `VEN/src/controller/milp_planner/model_skeleton.rs` |
| Plan translation | `VEN/src/controller/milp_planner/results.rs` |
| Planning loop | `VEN/src/tasks/planning.rs` |
| Acceptance gate | `VEN/src/services/planning.rs` |
| Config | `VEN/src/profile.rs` → `PlannerConfig` |

**One skeleton, three solves.** Phase 1, phase 2 and the marginal-cost pass (§9) solve the same
economic model. `ModelSkeleton::declare` builds it once — grid variables, penalty slacks, each
asset's variables, the cross-asset interactions that apply — `cost_expr` is the one economic cost,
and `add_constraints` the one set of constraints; `Declaration::{Phase1, Phase2, Pinned}` is the
only statement of how the solves differ (assets declared with or without startup/ramp costs, mode
decisions free or pinned). `cost_expr` is phase 1's objective, phase 2's cost cap and the dual
pass's objective, so the cap equals `c_star` term for term by construction. A new grid variable
or objective term is added in `model_skeleton.rs`, once.

**How assets take part.** The planner core knows no asset kind; it plans whatever contexts it is
handed, each by its own roster id, and `scripts/audit_ven_architecture.py` (rules 16 and 17) fails
when a kind is named in `controller/milp_planner/` or `controller/milp_interactions.rs`.

- *Declaration.* `AssetMilpContext::declare(n, DeclareMode, vars)` returns the asset's variables
  for one solve as a `DeclaredAsset`, held in `MilpVarPool.assets` in input order.
  `DeclareMode::Plan { friction }` declares every decision free, with the friction auxiliaries
  only phase 2 prices; `DeclareMode::Pinned(&own AssetSolution)` fixes the asset's own winning
  mode decisions for the marginal-cost pass (§9). One function declares all three, so the solves
  cannot drift apart (R-98).
- *Participation.* A context declares whether the plan dispatches it or only follows its
  forecast (`Participation::{Dispatched, Forecast { role, forecast_kw }}`). PV and base load are
  forecast participants (W3): each builds one `ForecastMilpContext` from its own forecast
  (`PvInverter::uncurtailed_power_kw_at`, or the sim-inject pin; `BaseLoad::forecast_kw_at`) in
  `assets/forecast_declared.rs`. PV is curtailable: it declares its used generation, bounded by
  its forecast, with the full-use tie-break in both objective parts; base load contributes its
  forecast as fixed net power. Forecast participants get no allocation, no planned power, no
  place among the planned assets and no share of the malus; the plan's `pv_forecast_kw`,
  `pv_used_kw` and `baseline_kw` are their site totals. `build_milp_inputs` sums their forecasts
  into `generation_forecast_kw` and `load_forecast_kw`; nothing else in the planner holds a PV or
  base-load series.
- *Balance.* Each asset states its net power per slot (`net_power_expr`, positive = import). The
  power balance is `p_imp − Σ forecast net − Σ dispatched net − p_exp = load_adjust` (the user's
  baseline override, a site statement); the controllable-import malus reads the dispatched sum
  (`dispatched_net_power_expr`). The forecast PV surplus over the base load has one computation,
  `MilpInputs::forecast_surplus_kw` (coexistence, malus, EV free-PV cap, `surplus_available_kw`).
  Forecast participants are declared first, where the grid's own PV variable used to be, and their
  rows and objective parts are placed where its terms were, so a site with PV sends HiGHS the same
  model in the same order as before W3 (`tests/plans_golden.rs`).
- *Objective.* Each asset contributes `ObjectivePart::Cost` (its economics: wear, rewards,
  penalties for a missed promise) to `cost_expr`, and `ObjectivePart::Friction` (startups, ramps,
  relay switching, tier preference) to phase 2's objective, priced by the `FrictionCosts` its
  context was built with from `PlannerParams`.
- *Interactions* couple roles, not kinds: the coexistence penalty pairs every asset offering a
  discharge leg with every one offering an on/off indicator.
- *Readback.* Each asset reads its part of the solution into an `AssetSolution` (net power,
  planned state, storage legs, warnings, its own detail) and seeds phase 2 from its own phase-1
  solution (§4.1).
- *Plan.* Allocations claim PV surplus in the order each asset declares (`surplus_claim_rank`),
  priced by its `AllocationBasis` (§8); planned state and warnings come from the asset, the
  budget warning from `input_warnings`; session flexibility is one rule over each asset's
  `FlexibilityWindow`. Every per-asset fact on the plan is keyed by the asset's id
  (`allocations`, `planned_kw_by_asset`, `planned_state_by_asset`, `session_flexibility`), and
  `Plan.planned_asset_ids` names the assets the plan scheduled, in input order, idle ones
  included. A reader asks `Plan::planned_assets()` which assets a plan covers (it also covers
  plans stored before the list existed); the forecast service and the Planner tab's Decision
  Matrix both do. The storage legs stay internal: the cost breakdown's `c_wear_eur` prices
  their summed throughput. The audit's rule 16 also covers `entities/plan.rs`, so no field
  named for one kind can return to the plan.

Several assets of one kind are therefore planned like any other assets
(`tests/multi_instance.rs`: two batteries, two EVs, coexistence pairs). A VEN profile still holds
one asset per kind (`validate_asset_roster`); that limit, sessions, the arbiter and the UI are the
rest of the way to several assets of a kind on a real VEN. `tests/plans_golden.rs` records the
whole translated `Plan` of every scenario, so a change to how the plan is assembled shows field by
field.

The order in which variables, expressions and constraints are created is part of the contract:
`good_lp` keeps coefficients in an `FnvHashMap`, so what HiGHS receives is a function of that order.
`tests/model_fingerprint.rs` records the model each solve sends (through `probe!`, a macro that is
a no-op outside tests) for nine scenarios and compares it with `tests/golden/model_fingerprints.txt`.
It fails with the scenario and record number of the first difference; regenerate the golden only
for an intended model change (`UPDATE_MODEL_GOLDEN=1`).

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

3. **Block-commitment anchor.** The planner can lock a slot's setpoint for the remainder of its duration (to prevent rapid resetting of heater relays, for example). The anchor is stored as a UTC timestamp. If grids are not aligned, the anchored time falls between two slots in the next replan, breaking the lock. With alignment, the anchor always coincides with a slot boundary. The anchor is a stability preference, never physics: the heater keeps it only up to the last slot its tank can hold (`assets/heater_milp.rs :: anchor_the_tank_can_hold`). A plan in force often fills the tank to exactly its ceiling, and replayed from a tank that gained heat since, the same stages overflow it. A pinned stage under a hard ceiling made the whole site's solve infeasible at presolve, the mirror image of the anchored-off case `heater_block_end` already refuses. The ceiling itself is the tank's maximum or its current energy, whichever is higher (`ceiling_kwh`), so a tank Absorb heated past its maximum doesn't contradict its own starting state either.

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
compute_site_headroom(&sim_guard, wall_now);         // headroom ts = real time
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

`PlanningHorizon` carries `zones: Vec<PlanZone>` (`#[serde(default)]` — stored plans without the field deserialise as `vec![]`). The zone list drives the whole grid: `n_slots = Σ zone.slots`, and per-slot durations come from each zone's `step_s` (`services/planning.rs`). Production profiles carry the 3-tier list; single-zone lists produce a uniform grid (used by test profiles).

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

**A failed solve never competes on cost.** It has nothing to dispatch and reports objective 0 €, so
compared on cost it "wins": it replaced a working plan and then blocked every later one until the
decay ran out (E2E 2026-10-06, the usage-forecast VEN without a plan for 25 minutes). So before
anything else, whatever the trigger: a failed plan never replaces one that solved, and a failed plan
in force yields to the first that solves (`SolveStatus::solved`, which the health check reads too).
Pinned by `test_gate_never_lets_a_failed_solve_displace_a_plan_that_solved` and
`test_gate_replaces_a_failed_plan_with_the_next_one_that_solved`.

**Hard triggers** (any trigger except `Periodic`) bypass the rest of the gate and always adopt.

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

After Phase 1 solves, `ModelSkeleton::warm_start` turns Phase 1's `SolveOutput` into a `Vec<(Variable, f64)>`: the grid's values, then each asset's `DeclaredAsset::warm_start` from its own phase-1 `AssetSolution` (the battery seeds its direction selector from the direction, R-97). Phase 2 gets it via `.with_initial_solution()`, starts at a known feasible point, and typically solves in far fewer branch-and-bound nodes.

### 4.2 Phase 1 from previous plan (planned)

With aligned grids, the active `Plan` is a near-feasible starting point for the next Phase 1 solve. `plan_to_solve_output(plan, n)` reconstructs the key decision variable values:

| MILP variable | Source in Plan |
|---|---|
| `p_imp[t]`, `p_exp[t]` | `slots[t].net_import_kw`, `net_export_kw` |
| each asset's variables | that asset's `slots[t].planned_kw_by_asset[id]` and `planned_state_by_asset[id]`, read back by the asset itself (as its phase-2 warm start reads its own `AssetSolution`) |

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
| `replan_interval_s` | 300 | How often the planning loop fires — the *period*, not a post-cycle sleep; see § Replan scheduling |
| `plan_adoption_threshold_eur` | 0.20 | Minimum improvement to adopt a periodic replan |
| `plan_adoption_decay_s` | 1500 | After this many seconds without adoption, force-adopt |
| `gate_switch_penalty_eur` | 0.0 | Added cost per Zone-A-equivalent heater switch in adoption gate |
| `phase2_epsilon_eur` | 0.02 | Phase 2 may not increase total cost beyond this slack |
| `phase2_solver_timeout_s` | 15 | Phase 2's own wall-clock budget; on expiry the plan keeps phase 1 leftovers it has not yet removed |
| `c_bat_startup_eur` / `c_ev_startup_eur` | 0.01 | Phase-2 cost per battery / EV run start; set per asset mix, see `VEN/profiles/README.md` "Planner smoothing by asset mix" |
| `c_ctrl_imp_malus_eur_kwh` | 0.22 | Malus added to import price to discourage unnecessary import |
| `solver_timeout_s` | 60 | HiGHS wall-time limit per phase |

**What the import malus prices.** `CtrlImportMalusInteraction` (`controller/milp_interactions.rs`)
charges `c_ctrl_imp_malus_eur_kwh` on controllable power (heater + EV + shiftable loads + battery
charge − battery discharge) beyond the slot's PV surplus. Battery discharge counts as negative, so
PV stored in the battery and later fed into the EV or heater is self-consumption and pays no malus.
That routing is intended: on ven-1's plan of 2026-10-05 it holds grid import at 9 kWh over 48 h,
where an unmalused plan imports 23 kWh and is 2.6 EUR cheaper. That's the trade the malus buys.

**Equal-cost EV slots are decided earliest-first.** Battery-to-EV charging costs the same in every
night slot, so *which* slots get it is a tie, and HiGHS breaks ties arbitrarily. Phase 1 then
scattered it into single slots at the EV's minimum power, and phase 2 could not merge them within its
15 s budget (ven-1, 2026-10-05: 13 runs, 14 of them single slots, a matching battery blip under each).
Every EV plan therefore pays the ASAP lateness term at a tie-breaking weight
(`TIE_BREAK_LATENESS_EUR_KWH_H`, 1e-4 EUR/kWh per hour, in `assets/ev_milp.rs`; ASAP sessions keep
their own, larger weight). Earliest first is contiguous, so phase 1 already delivers runs. Pinned
by `controller/milp_planner/tests/phase2_spikes.rs::ven1_live_instance_charges_the_ev_in_runs_not_single_slots`
(the live instance, captured in `ven1_ev_frag_data.rs`); `bench_ven1_ev_fragmentation` measures
every planner knob on the same instance.

## 7. Terminal Energy Reward (c_terminal)

Without a terminal value, the optimizer treats energy stored at the horizon end as
worthless and refuses to pre-heat/pre-charge beyond immediate need (temperature
ceiling, overnight top-up fragmentation). The Phase 1 objective therefore includes a
per-asset reward `−c_terminal × stored_energy[n−1]` — the forward value of 1 kWh
still stored at the horizon end.

**Coefficients** (resolved in `services/planning.rs::build_plan_cycle_inputs`; the
profile can override via `c_terminal_eur_kwh`, default = auto-computation):

| Asset | Coefficient | Rationale |
|---|---|---|
| Heater | `mean(c_imp_eur_kwh) + c_ctrl_imp_malus_eur_kwh` | Filling during PV surplus is always net-positive; cheap overnight ≈ net-neutral; peak-rate filling stays net-negative |
| Battery | `mean(c_imp_eur_kwh) × round_trip_efficiency` | Stored energy offsets later import; the import malus does not apply to storage value |
| EV | `0` | The session deadline constraint (`e_ev[t_dead] ≥ e_target`) already forces delivery; a terminal reward would double-count and could over-charge at peak rates |

The formula is size-independent (EUR/kWh scales with any tank/battery via the energy
variable) and needs no mandatory profile parameter — all inputs already exist at
`build_milp_inputs` time. Side effect: with the tank filled during each solar window,
coast time to `T_min` exceeds two nights, so overnight top-up pulses (plan
fragmentation) disappear for the steady-state case. Cold-start/cloudy-day gaps are
covered by the horizon length instead — the two mechanisms are complementary
(terminal value fixes the ceiling; a ≥48 h horizon shows the next solar window for
coherent coast planning from any start state).

## 8. Per-Allocation Cost Sign Convention

`AssetAllocation.cost_eur` (`results.rs::translate_to_plan`, one value per slot per asset — the
Planner tab's decision-matrix display) and `SessionFlexibility.estimated_cost_eur`
(`session_flexibility.rs::solved_session_cost`, the session-total estimate) must agree in sign on the same
underlying energy, since both price energy covered by PV surplus the same way — as an **opportunity
cost** (forgone export revenue), not a credit:

```
cost_eur = grid_power_kw * import_tariff_eur_kwh * dt_h
         + surplus_power_kw * export_tariff_eur_kwh * dt_h
```

Applies to every allocation priced `AllocationBasis::Load` and to `AllocationBasis::Storage` while
charging (`results.rs::allocation`, the one pricing rule); a storage asset *discharging* is priced
as export revenue instead. This is a
post-solve reporting computation only — no solver objective/constraint depends on it. Regression
coverage: `controller/milp_planner/tests/cost_sign.rs`.

## 9. Marginal Cost (Shadow Price)

`PlanTimeSlot.marginal_cost_import_eur_per_kwh` / `marginal_cost_export_eur_per_kwh` are a
per-slot shadow price on the power-balance constraint, computed once per planning cycle *after*
the winning MILP solve: the same problem is re-solved as a pure LP with every binary decision
fixed to the winning solution's values, and the constraint's dual value is read off that solve.

- **Same model, decisions pinned** (`controller/milp_planner/solver_duals.rs`): every asset
  declares its own variables for the pass through the same `AssetMilpContext::declare` the plan
  used, with `DeclareMode::Pinned` and its own winning `AssetSolution` — its mode decisions
  (battery direction, EV on/off, heater stage and ready flag, a shiftable load's start) become
  continuous variables fixed to the values it read off the winning solution. Every other
  variable and bound is the plan's own, and the grid variables, objective and constraints come from
  the same `ModelSkeleton` the plan was built on (`Declaration::Pinned`), so the priced model cannot
  drift from the planned one.
  (Before R-98 the pass hand-declared its own copies; they drifted until the LP was infeasible
  on every VEN and the marginal cost was always the fallback tariff.) Pinned by each asset's
  `declare_pinned_vars_is_the_free_declaration_*` test and by
  `tests/planner.rs::marginal_cost_solves_with_every_asset_kind_active`.

- **Read-only diagnostic**: never influences `p_imp`/`p_exp` or any allocation — only computed
  after the winning solve is already final.
- **No binding constraint** → equals the slot's plain `import_tariff_eur_kwh` (within solver
  tolerance). **A binding constraint** (e.g. an asset pinned at its max power bound) → differs
  from the plain tariff, reflecting the binding constraint's additional cost.
- **Dual solve failure** doesn't fail the planning cycle — both fields fall back to the plain
  tariff, with a warning logged; every other `Plan` field is unaffected either way.
- Feeds `controller::arbiter`'s marginal-cost-ranked lever selection (§2.1 of
  `VEN_ARCHITECTURE.md`) and the Planner tab's Decision Matrix (marginal-cost heatmap cell
  alongside the plain-tariff row).

## 10. Session Comfort Curve

A session's resolved `ComfortRate` curve (`entities/asset.rs`; a `{fill, max_marginal_price,
max_marginal_co2}` bid-price point, see `services/comfort.rs::effective_comfort_rates` for
override-vs-default resolution) is carried from the `POST /user-requests` route through
`UserRequest` → `EvSession`/`HeaterTarget` → the MILP context, and shapes reward coefficients
that were previously fixed `PlannerParams` constants. `ComfortRate::value_at_fill()` linearly
interpolates the curve's `max_marginal_price` at an arbitrary fill (clamped outside the stored
range).

- **EV** (`ev_milp.rs::from_state`, `UserRequestMode::ByDeadline`/`Asap` arm only): `v_core_eur
  = e_core_kwh × value_at_fill(curve, 0.0)`, `v_extra_eur_kwh = value_at_fill(curve, 1.0)`. Every
  other request mode (`Opportunistic`, `MaxCost`, `ByDeadlineFree`) already redirects
  `v_extra_eur_kwh` to an unrelated signal (free-energy incentive, budget reward) and is
  unaffected.
- **Heater** (`heater_milp.rs`): `comfort_full_reward_eur_kwh = value_at_fill(curve, 1.0)`, a new
  objective term `−comfort_full_reward_eur_kwh × dt × z_heat_full[t]` next to the existing tier
  penalty. Phase-gated to Phase 2 only (`0.0` in Phase 1), mirroring `w_tier_penalty_eur`'s own
  phase split — Phase 1 has no counterbalancing tier cost, so an unconditional reward there would
  be a free bias toward full-tier in the coarse allocation.
- **No curve / empty `comfort_rates`** (the legacy `POST /ev-session`/`POST /heater-target`
  direct routes, or a VTN-commanded session): falls back to the passed-in global defaults
  exactly, reproducing pre-feature behavior.
- **Battery, PV, base-load**: unaffected — no session-intent path exists for them
  (`services/user_request.rs` has no `create_battery`/equivalent).

EV's `v_extra_eur_kwh` reward is genuinely coupled to real charging: `EvMilpContext::constraints`
ties `ev_energy` to `e_ev_extra` by *equality* (`ev_energy == e_core_kwh [× z_ev_core] +
e_ev_extra`, not just an upper bound), so the solver can only "bank" the reward by actually
charging that extra energy (fixed 2026-08-11, formerly R-18 in
`docs/reference/TECHNICAL_DEBTS.md`). Both halves — `v_core_eur`/`z_ev_core` (whether to commit to
core at all) and `v_extra_eur_kwh`/`e_ev_extra` (whether to top off beyond core) — now drive
allocation. The heater's tier reward has no equivalent gap — `z_heat_mid`/`z_heat_full` are
coupled to real tank-energy dynamics.

Regression coverage: `entities/asset.rs::comfort_rate_tests`, `ev_milp.rs`'s
`from_state_by_deadline_soft_sources_v_core_eur_from_curve`/`..._falls_back_to_global_defaults`,
`heater_milp.rs`'s `test_comfort_full_reward_*`/`from_state_sources_comfort_full_reward_*`,
`controller/milp_planner/tests/modes.rs`'s
`test_by_deadline_soft_comfort_curve_shapes_core_commitment` (core half) and
`test_by_deadline_hard_extra_reward_drives_extra_charging` (extra half, R-18 fix).

## Replan scheduling: an absolute grid with a per-VEN phase

The periodic trigger is anchored to absolute time, not to the end of the previous cycle.
`entities::planner_params::next_replan_at(now, replan_interval_s, replan_phase_offset_s)` returns
the first instant strictly after `now` that satisfies
`t ≡ replan_phase_offset_s (mod replan_interval_s)`, and the planning loop
(`tasks/planning/mod.rs`) waits until that instant.

Two properties follow, and both exist because of GB-54:

1. **The period is `replan_interval_s`, whatever the cycle cost.** Sleeping for the interval
   *after* a cycle — what the loop did before — makes the real period
   `replan_interval_s + solve_time`. ven-17, solving in ~34 s, ran on a ~334 s period:
   observed completion gaps of 329 s, 376 s and 339 s against a nominal 300 s.
2. **Each VEN sits at its own phase in that grid.** `replan_phase_offset_s(ven_name, interval)`
   hashes the VEN's name with FNV-1a, so the offset is identical on every start of every build.
   Stability is the point, not spread alone: an offset derived from start-up time would
   re-randomise on restart, and a deploy restarts the whole fleet at once. FNV is written out
   rather than taken from `DefaultHasher`, whose output Rust does not promise to keep stable
   across versions.

A cycle that overruns a whole interval **skips** to the next grid point instead of firing
immediately — a VEN already too slow is the last one that should replan back-to-back.

Why it matters: without (1), fleet members drift at per-VEN rates, wander into each other, and
then stay together, because every VEN in a collided group is slowed equally and keeps its offset.
On a host with fewer cores than VENs that burst converts directly into phase-1/phase-2
`TimeLimit` terminations — on Node2 (4 cores, 17 VENs) fleet solver wall-time was only ~17 % of
available core-seconds while six solves once landed inside a single 90 s window. Without (2), the
grid in (1) would be strictly worse than a post-cycle sleep, since every VEN would fire on the
same instant.

Pinned by the `replan_schedule_tests` module in `entities/planner_params.rs`.

### Hard triggers: `RateChange` is held, the others are not

The grid above covers `Periodic` replans only. Hard triggers (`RateChange`,
`CapacityChange`, `Alert`, `UserRequest`, `AssetStateChange`) bypass it by design — the point of a
hard trigger is to react now — which left one fleet-wide burst untouched: a VTN rate update
reaches every VEN at once. ven-17 and ven-19 were observed waking at the same centisecond
(`2026-10-02T04:18:56.86`) on a single broadcast.

`RateChange` alone is therefore held for a jittered interval before its cycle runs
(`entities::planner_params::rate_change_delay_s`, applied in `tasks/planning/mod.rs` *before* the
cycle reads its clock, so the plan is built from the time it is solved at):

| profile field | default | meaning |
|---|---|---|
| `planner.rate_change_trigger_delay_s` | 30 | base hold in seconds; `0` reacts immediately |
| `planner.rate_change_trigger_jitter_pct` | 100 | symmetric spread, in percent of the base |

The delay is `base * (1 + (2*draw - 1) * pct/100)` for a uniform `draw`, floored at zero — so the
defaults spread uniformly over `[0, 60 s]`. **The jitter reaching zero is the point**: a fixed
fleet-wide delay moves the collision 30 s later instead of breaking it up, and some VENs should
still react at once.

Why these defaults: the cost is bounded at 60 s of tariff-reaction latency against a 300 s
periodic grid that would catch the change anyway, on a plan spanning 48 h in 5-minute slots. The
benefit is 20 VENs arriving one every ~3 s. It ships enabled because the storm was measured, not
hypothesised, and affects every multi-VEN host.

The other hard triggers are deliberately **not** delayed: `Alert` and `CapacityChange` are safety
and contractual limits whose entire value is immediacy, and `UserRequest` has a person waiting at
a UI. `validate.rs` rejects a negative jitter, and a delay `>= replan_interval_s` — which the
periodic grid would overtake, making the held trigger redundant.

Pinned by the `rate_change_delay_tests` module in `entities/planner_params.rs`.
