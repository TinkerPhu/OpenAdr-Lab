---
title: MILP Planner
type: component
created: 2026-07-04
updated: 2026-10-21
synced_commit: 1ad1ab5
sources: [docs/architecture/ven_milp_planner.md, docs/architecture/VEN_ARCHITECTURE.md, VEN/src/controller/milp_planner/, VEN/src/controller/milp_interactions.rs, VEN/src/controller/solver_port.rs, VEN/src/controller/asset_milp_port.rs, VEN/src/tasks/planning/, VEN/src/services/planning/, VEN/src/simulator/plan_context.rs, VEN/src/entities/time_grid.rs, VEN/src/entities/plan_cost.rs, VEN/src/entities/comfort_spend.rs, VEN/src/controller/milp_planner/solver_duals.rs, VEN/src/entities/asset.rs, VEN/src/entities/planner_params.rs, VEN/src/profile/validate.rs, VEN/src/assets/ev_milp.rs, VEN/src/assets/heater_milp.rs]
tags: [planner, milp, highs, optimization]
---

# MILP Planner

The VEN's planning engine: a **two-phase Mixed-Integer Linear Program** solved by HiGHS
via the `good_lp` crate, producing a 48 h asset allocation plan on every replanning cycle
(docs/architecture/ven_milp_planner.md §1). It replaced the earlier greedy scheduler —
see [[milp-over-greedy]].

## Two phases

1. **Cost minimisation** — minimises import cost and CO₂ under capacity limits and
   EV/heater deadlines (`solver_phase1.rs`).
2. **Friction minimisation** — minimises relay switches and ramp changes while staying
   within `phase2_epsilon_eur` of Phase 1's optimum; warm-starts from Phase 1
   (`solver_phase2.rs`).

## One skeleton, three solves

Phase 1, phase 2 and the marginal-cost pass (below) solve the same economic model.
`ModelSkeleton::declare` (`model_skeleton.rs`) builds it once — grid variables, penalty slacks, each
asset's variables, the cross-asset interactions that apply; `cost_expr` is the one economic cost and
`add_constraints` the one set of constraints. `Declaration::{Phase1, Phase2, Pinned}` is the only
statement of how the solves differ, so phase 2's cost cap equals phase 1's optimum term for term and the
priced model cannot drift from the planned one. A new grid variable or objective term is added there,
once. The order variables and constraints are created in is part of the contract (HiGHS receives a
function of it), pinned by `tests/model_fingerprint.rs` and `tests/plans_golden.rs`.

## How assets take part

The planner core names no asset kind; the audit (rules 16 and 17, [[ven-hexagonal-architecture]]) fails
if `controller/milp_planner/`, `milp_interactions.rs` or `entities/plan.rs` does. It plans whatever
contexts it is handed, each by its own roster id (`VEN/src/controller/asset_milp_port.rs`; each kind
implements it in `VEN/src/assets/<kind>_declared.rs`, its model maths in `<kind>_milp.rs`):

- **Declaration**: `AssetMilpContext::declare(n, DeclareMode, vars)` returns a `DeclaredAsset`.
  `DeclareMode::Plan { friction }` declares every decision free (friction auxiliaries only for phase 2);
  `DeclareMode::Pinned(&own AssetSolution)` fixes the asset's own winning mode decisions for the
  marginal-cost pass. One function declares all three.
- **Participation**: `Participation::{Dispatched, Forecast { role, forecast_kw }}`. PV and base load are
  forecast participants (`assets/forecast_declared.rs`): never dispatched, no allocation, no share of the
  malus. PV is curtailable (a used-generation variable bounded by its forecast, with a full-use
  tie-break); base load contributes its forecast as fixed net power. Several PV arrays plan as one site
  forecast; weather reaches the planner only through each PV's own forecast ([[weather-forecast]]). The
  forecast surplus over base load has one computation (`MilpInputs::forecast_surplus_kw`).
- **Balance**: each asset states its net power per slot (positive = import); the power balance is
  `p_imp − Σ forecast net − Σ dispatched net − p_exp = load_adjust` (the user's baseline override).
- **Objective**: `ObjectivePart::Cost` (wear, rewards, penalties for a missed promise) into `cost_expr`,
  `ObjectivePart::Friction` (startups, ramps, relay switching, tier preference) into phase 2 only, priced
  by the `FrictionCosts` built from `PlannerParams`.
- **Interactions** couple roles, not kinds: the coexistence penalty pairs every asset offering a
  discharge leg with every one offering an on/off indicator.
- **Readback and plan**: each asset reads its part into an `AssetSolution` and seeds phase 2 from its own
  phase-1 solution. Every per-asset fact on the plan is keyed by asset id (`allocations`,
  `planned_kw_by_asset`, `planned_state_by_asset`, `session_flexibility`, `forecast_by_asset`), and
  `Plan.planned_asset_ids` names the assets scheduled, idle ones included; readers ask
  `Plan::planned_assets()`. Allocations claim PV surplus in each asset's `surplus_claim_rank`. The
  plan keeps `pv_forecast_kw`, `pv_used_kw`, `baseline_kw` as site totals.

A profile may hold several assets of one kind (`validate_asset_roster`); two batteries or two EVs plan
like any others (`tests/multi_instance.rs`). The [[ven-ui]] Decision Matrix and the forecast service read
the plan through `planned_assets()`.

## Without a solve

Under the [[opportunistic-objective]] the cycle does not solve: `plan_from_forecast` steps a clone of the
site slot by slot and translates the trace into a `SIMULATED` plan that is always adopted.

## Replan scheduling

The periodic trigger is anchored to absolute time: `next_replan_at(now, replan_interval_s,
replan_phase_offset_s)` (`entities/planner_params.rs`) returns the first instant strictly after `now` with
`t ≡ offset (mod interval)`. The period is `replan_interval_s` whatever the solve costs (a post-cycle sleep
would stretch it by the solve time and let fleet members drift into each other), and each VEN has a stable
phase (`replan_phase_offset_s` hashes the VEN name with FNV-1a), so a fleet does not solve in one burst on
a shared host. A cycle that overruns skips to the next grid point. Hard triggers bypass the grid, except
that `RateChange` is held for a jittered interval (`planner.rate_change_trigger_delay_s` 30,
`..._jitter_pct` 100, so uniform over 0–60 s) because a VTN rate update reaches every VEN at once;
`Alert`, `CapacityChange` and `UserRequest` are never delayed. Pinned by `replan_schedule_tests` and
`rate_change_delay_tests`.

## Key mechanics

- **Grid-signal import caps** (Phase 3): alerts, SIMPLE levels 1–3, and capacity
  subscription+reservation allowances all converge on the per-slot contractual
  import cap (`p_imp_max_cont_kw`, `input_stages.rs::capacity_limits`) — alert → 0 (overrides all),
  SIMPLE L1 → `simple_level1_import_cap_pct` × contract, L2 → baseline forecast,
  L3 → 0, reservation allowance → min with the limit. The cap is a *soft*
  constraint (slack + violation penalty, warned in the plan), so no signal
  combination can make the solve infeasible and user deadlines yield
  automatically. See [[openadr-interface]] for the parsing side.
- **Plan grid**: 3-tier variable-step zones, `now` truncated to the Zone-A boundary —
  the full reasoning (gate stability, warm-start continuity, block-commitment anchor)
  is in [[three-tier-plan-grid]].
- **Sessions as constraints**: `EvSession`/`HeaterTarget`/`ShiftableLoad` enter the MILP
  as energy targets, deadline steps, and `MilpLoadMode` (MustRun/MayRun/MustNotRun) —
  the solver iterates over asset variables, never session objects
  (docs/architecture/VEN_ARCHITECTURE.md §2.3.1); see [[hems-planning]].
- **Adoption gate** (`VEN/src/services/planning/`): a failed solve never competes on cost (it reports 0 €
  and would otherwise replace a working plan): a failed plan never displaces one that solved, and a failed
  plan in force yields to the first that solves. Then a periodic plan is adopted only if
  it beats the current plan's cost+friction by the effective threshold plus an optional
  per-extra-heater-switch surcharge (`gate_switch_penalty_eur`); hard triggers, a fully
  decayed threshold, or a current plan whose slots have all expired always adopt. Gate
  decay must measure real age, so it always receives `wall_now`, never the aligned
  timestamp (ven_milp_planner.md §2.2).
- **Request-mode session translation** (Phase 4, WP4.1 / BL-28): the EV path branches
  on `EvSession.mode` in `assets/ev_milp.rs::from_state` — ASAP adds a lateness
  penalty (`asap_lateness_eur_kwh_h`, default 10 €/kWh·h → cost-blind front-loading);
  OPPORTUNISTIC / ASAP_FREE / BY_DEADLINE_FREE cap charging per slot at the free
  energy (PV surplus over baseline, opened fully when the import rate ≤ 0) and reward
  each charged kWh (`v_ev_free_charge_eur_kwh`); MAX_COST adds a hard budget
  constraint on charging cost with a per-kWh completion reward, so an unaffordable
  target degrades to partial charging + a plan warning instead of an infeasible solve.
  The per-slot data these modes need arrives through the new
  `AssetMilpContext::inject_grid_slots` hook (default no-op), called by `run_planner`
  after `build_milp_inputs` — the MILP core still never imports asset types.
  Heater/shiftable sessions store the mode but the planner ignores it there (BL-28
  resolution). Two solver-shape lessons: the legacy `e_ev_extra` reward is
  structurally inert (upper-bound-only coupling — R-18 in TECHNICAL_DEBTS.md), and
  any soft incentive weaker than `phase2_epsilon_eur` gets traded away by Phase 2's
  friction smoothing (ASAP_FREE's invariant is therefore "front-loaded up to the
  friction budget", not "earliest slot saturated").
- **Stale-rate policy dispatch** (Phase 4, WP4.4 / BL-07 — resolved): `TariffTimeSeries`
  now records `import_coverage_end`; `build_milp_inputs` fills slots beyond it via
  `milp_planner/stale_rates.rs` per the profile's `stale_rate_policy` — LAST_KNOWN
  repeats, SAFE_AVERAGE takes the `stale_rate_safe_pctl` nearest-rank percentile,
  DEFER_TO_FLEXIBLE prices stale slots at the max known rate (defers discretionary
  load into covered slots), HEURISTIC_FORECAST (default) is a documented stub →
  LAST_KNOWN until Phase 5 (BL-14). Stale slots set `PlanTimeSlot.rate_estimated`
  (no longer hardcoded false) and the plan carries one stable-text warning, which the
  [[notifications]] feed dedups into a single Warn. Export/CO₂ keep step-hold +
  defaults (0.08 export, 300 g/kWh) — the policy governs the import price that
  actually drives scheduling ([[tariffs-and-capacity]]).
- **Asset isolation**: asset physics enter as `Vec<Box<dyn AssetMilpContext>>` — the
  planner never imports concrete asset types ([[ven-hexagonal-architecture]]).
  `tasks/planning/mod.rs` reaches the solver through the `SolverPort` trait
  (`controller/solver_port.rs`), not `run_planner()` directly — `MilpSolver`
  (`milp_planner/mod.rs`) is the real implementation, and `services::PlanningService::solve_plan`
  is the only caller of `SolverPort::solve`. The actual HiGHS call still runs inside
  `spawn_blocking` (MILP solving takes 18–60 s on Node1; the sim mutex is cloned and
  released first) — the port adds a swappable seam, not a different execution model.
- **Cross-asset interactions** (`controller/milp_interactions.rs`): pluggable
  `AssetInteraction` objects add coupled terms — `BatEvCoexist` (McCormick-linearised
  penalty on battery discharge during PV-covered EV charging) and `CtrlImportMalus`
  (slack penalty when controllable load exceeds free PV surplus). Active only when their
  coefficient is non-zero.
- **Heater anchor**: after adoption, the current heater block's tier binaries are pinned
  for the next solves until the block ends (`services::planning::build_heater_anchor`,
  `heater_block_ends` keyed by heater id) — prevents near-future chattering; hard triggers clear the
  anchor. Off-blocks are never anchored (would drive the tank below its domain bound
  and make the MILP infeasible).
- **Terminal energy rewards**: battery and heater get an end-of-horizon stored-energy
  credit auto-computed from the mean import tariff (battery: × round-trip efficiency;
  heater: + ctrl-import malus), profile-overridable — stops the optimizer from draining
  storage right before the horizon edge (`tasks/planning/cycle.rs`).
- **Phase 2 is a hard-bounded lexicographic pass, not a weighted blend**: it adds the
  constraint `phase1_cost ≤ C* + phase2_epsilon_eur` and then minimises switching/
  startup/ramp/tier-preference terms only — never trades cost for friction beyond that
  epsilon. Setting `phase2_epsilon_eur: 0.0` disables Phase 2 entirely (single-pass
  Phase 1 only). If Phase 2 comes back infeasible, the planner logs
  `"phase2 infeasible, falling back to phase1"` and adopts the Phase 1 schedule directly
  rather than crashing.
- **Equal-cost EV slots are decided earliest-first**: battery-to-EV charging costs the same in every night
  slot, and HiGHS breaks such ties arbitrarily (single-slot runs phase 2 could not merge). Every EV plan pays
  the ASAP lateness term at a tie-breaking weight (`TIE_BREAK_LATENESS_EUR_KWH_H`, 1e-4, `assets/ev_milp.rs`);
  earliest-first is contiguous. Pinned by `tests/phase2_spikes.rs`.
- **Initial-slot pinning**: slot 0's heater mode variables (`z_heat_mid[0]`,
  `z_heat_full[0]`) are fixed to the live heater's actual power state at planning time, so
  `sw[0]` — and its Phase 2 switching penalty — reflects a real transition, not a solver
  artifact of an unconstrained first slot.
- **Adoption threshold decay**: `plan_adoption_decay_s` (default 0, no decay) linearly
  decays `plan_adoption_threshold_eur` to zero as the current plan ages, so a plan that
  once looked "good enough" doesn't block replans indefinitely as conditions drift.
- **`solver_timeout_s`** (profile field, default 60 s) bounds the HiGHS time limit per phase;
  `phase2_solver_timeout_s` (15 s) is phase 2's own budget; `mip_gap_target` is a measured, per-profile
  knob (heater VENs; benchmarks in `docs/reference/R97_PLANNER_BENCHMARKS.md`) — see [[reliability-and-config]] for this and the other profile-driven
  config knobs.
- **Marginal-cost extension** (`solver_duals.rs::solve_marginal_costs`,
  `docs/architecture/ven_milp_planner.md` §9): a second, cheap LP solve per planning cycle,
  re-declaring every winning MILP binary as a fixed continuous value (`min == max ==
  winning_value` — HiGHS returns all-zero duals on any model with an integer column) and
  reading the real shadow price off each slot's power-balance row. Exposed as
  `PlanTimeSlot.marginal_cost_import_eur_per_kwh` / `marginal_cost_export_eur_per_kwh` —
  directional because the objective can be kinked at zero net exchange (self-consumption
  maximisation, autarky). This is a **read-only diagnostic** feeding the UI's Decision Matrix
  "Marginal €" column; it does not drive scheduling itself, but it is the signal
  [[deviation-arbiter]] ranks its real-time levers by. A dual-solve failure doesn't fail the
  cycle — both fields fall back to the plain tariff, logged as a warning.
- **Per-allocation cost sign convention** (BL-40, `docs/architecture/ven_milp_planner.md` §8):
  `AssetAllocation.cost_eur` (`results.rs::translate_to_plan`) prices energy covered by PV
  surplus as an **opportunity cost** (forgone export revenue, `+surplus_power_kw ×
  export_tariff_eur_kwh × dt_h`), not a credit — matching `session_flexibility.rs::solved_session_cost`'s
  convention so the Planner tab's per-slot decision matrix and the session-total estimate never
  disagree in sign on the same energy. Applies to EV/heater/shiftable-load/battery-*charging*;
  battery-*discharging* uses an unrelated revenue formula. Post-solve reporting only — no
  solver objective/constraint depends on it.
- **Session comfort curve** (BL-34, `docs/architecture/ven_milp_planner.md` §10): a resolved
  `ComfortRate` curve (`entities/asset.rs` — `{fill, max_marginal_price, max_marginal_co2}`
  points, `ComfortRate::value_at_fill` linearly interpolating, clamped outside the stored
  range) now flows `POST /user-requests` → `UserRequest` → `EvSession`/`HeaterTarget` → the
  MILP context, replacing fixed `PlannerParams` reward constants for two request shapes:
  `ev_milp.rs::from_state`'s `ByDeadline`/`Asap` arm sources `v_core_eur`/`v_extra_eur_kwh`
  from the curve (every other EV mode already redirects `v_extra_eur_kwh` to an unrelated
  signal, unaffected); `heater_milp.rs` gets a new `comfort_full_reward_eur_kwh` objective
  term at `z_heat_full[t]`, phase-gated to Phase 2 only (mirroring `w_tier_penalty_eur`'s own
  split — Phase 1 has no counterbalancing tier cost, so an unconditional reward would bias
  Phase 1's coarse allocation for free). No curve / empty `comfort_rates` (VTN-commanded
  sessions, e.g. a `CHARGE_STATE_SETPOINT`-created `EvSession` — BL-41 removed the direct-CRUD
  `/ev-session`/`/heater-target` create routes, see [[hems-planning]]) falls back to the
  passed-in global defaults exactly. Both halves of the EV reward — `v_core_eur`/`z_ev_core` and `v_extra_eur_kwh`/`e_ev_extra` — drive allocation (`ev_energy` equals core plus extra by equality, so a reward is banked only by charging); the heater's tier reward is coupled to real tank-energy dynamics. A curve can be entered as total spend from the current charge (`entities/comfort_spend.rs`, converted at the boundary into ordinary bids), and the maximum a curve can cost is the area under it over the asset's own bands (`entities::comfort::max_cost_eur`), shown beside the plan's spend (`entities::plan_cost::asset_planned_cost`, also behind a session's estimated cost). BDD coverage proving the curve
  actually reshapes a plan (not just unit-level reward wiring):
  `tests/features/ven_comfort_curve.feature`. See [[hems-planning]] for where
  `comfort_rates` is resolved and attached to a session.
- **Peak-demand penalty threshold** (WP6.3, BL-09, `docs/architecture/VEN_ARCHITECTURE.md`
  §2.3.2): a profile may declare `planner.penalty_rules` (`entities::planner_params::
  PenaltyRuleParams` — `rule_id`, `threshold_kw`, `measurement_window_s`,
  `penalty_eur_per_kw`), each keeping grid import at or below `threshold_kw` within
  fixed, horizon-aligned windows. Implemented in `controller/milp_planner/penalty.rs`
  as a per-solve soft-penalty term mirroring the existing `s_imp_viol` idiom: one
  shared slack per rule per window bounds every slot's import in that window
  (`p_imp[t] <= threshold_kw + s_penalty[window]`), penalized once per window (not
  per slot — a demand-charge-style peak cost, not an energy cost). Threaded through
  both solver phases and the marginal-cost dual solve via `ModelSkeleton::add_constraints`'s
  existing shared constraint function. Deliberately **not** the stateful, persisted
  billing-period tracker sketched in `entities::design_vocabulary::PenaltyRule`
  (rolling averages, `breached_this_period` surviving restarts) — each solve
  re-evaluates its own horizon fresh, no cross-restart state; that heavier feature
  remains a separate, unbuilt future item if real multi-day billing tracking is ever
  needed. Surfaces as `CostBreakdown.c_peak_penalty_eur` and a `PlanWarning` when a
  window still breaches after solving; `Plan.penalty_rules_active` drives a new
  "Peak demand" row in the VEN UI's Decision Matrix ([[ven-ui]]). Off by default
  (empty `penalty_rules`). Also fixed in passing: `profile::validate`'s new
  window-multiple check must use `PlannerConfig::effective_step_s()`, not the raw
  (and, when `plan_zones` is set, silently ignored) `plan_step_s` field.

## File map

| Concern | File |
|---|---|
| Entry point (`run_planner`) + `SolverPort` impl (`MilpSolver`) | `VEN/src/controller/milp_planner/mod.rs` |
| `SolverPort` trait + `SolveRequest` | `VEN/src/controller/solver_port.rs` |
| Input tensors | `inputs.rs` (stage order, the full `MilpInputs` literal), `input_stages.rs` (one function per field group) |
| Weights, `MilpInputs`, `SolveOutput` | `types.rs` |
| Asset port (trait + declared/solution structs) | `VEN/src/controller/asset_milp_port.rs`; per kind `assets/<kind>_declared.rs` |
| Phase 1 / Phase 2 | `solver_phase1.rs` / `solver_phase2.rs` |
| Shared model skeleton (variables, cost, constraints) | `model_skeleton.rs` |
| Stale-rate policy dispatch (WP4.4) | `stale_rates.rs` |
| Request-mode EV semantics (WP4.1) | `VEN/src/assets/ev_milp.rs` (via `AssetMilpContext`) |
| Cross-asset interactions | `VEN/src/controller/milp_interactions.rs` |
| Marginal-cost dual extraction (§9) | `solver_duals.rs` |
| Plan translation + fallback plan | `results.rs` |
| Per-session flexibility envelopes | `session_flexibility.rs` |
| Planning loop | `VEN/src/tasks/planning/mod.rs` (entry) + `cycle.rs` (terminal rewards, cycle inputs) |
| Acceptance gate + heater anchor | `VEN/src/services/planning/` (`service.rs`) |
| Slot layout, "which slot holds this instant" | `VEN/src/entities/time_grid.rs` |
| Forecast plan (no solve) | `milp_planner/forecast_plan.rs`, `forecast_readback.rs` |
| SimState-coupled cycle helpers (sim clone, PV-inject patch, `build_asset_contexts`) | `VEN/src/simulator/plan_context.rs` |

The split between the gate and the `SimState` rows is the port rule: `services/planning/`
holds only pure/port-based logic, while everything needing the concrete
`SimState` lives in the infra ring next to the simulator
([[ven-hexagonal-architecture]]).

Per-slot baselines for `base_load` come from its own heuristic-aware forecast ([[heuristics-pipeline]]),
falling back to the profile's flat `baseline_kw` on cold-start. Shiftable-load starts use a deterministic earliest-start tie-break,
so equal-cost windows always resolve to the same schedule across replans.

The terminal-energy reward (`c_terminal`) model — why heater/battery terminal
state is credited at horizon end and the per-asset coefficient table — is
documented in `docs/architecture/ven_milp_planner.md` §7.

Downstream, the plan is executed slot-by-slot by the [[dispatcher]], with real-time
deviation correction owned by the [[deviation-arbiter]] when enabled.
