# OpenAdr-Lab — Application Documentation

> Audience: code reviewers and users. This document covers purpose, features, architecture, and operational guidance.

---

## Table of Contents

0. [Glossary](#0-glossary)
1. [Purpose & Overview](#1-purpose--overview)
2. [Feature Reference](#2-feature-reference)
   - 2.1 [Simulation Engine](#21-simulation-engine)
   - 2.2 [Energy Planning (MILP)](#22-energy-planning-milp)
   - 2.3 [Real-time Deviation Absorption](#23-real-time-deviation-absorption)
   - 2.4 [User Energy Requests](#24-user-energy-requests)
   - 2.5 [VTN Integration (OpenADR 3)](#25-vtn-integration-openadr-3)
   - 2.6 [Report Obligations](#26-report-obligations)
   - 2.7 [Flexibility Envelope](#27-flexibility-envelope)
   - 2.8 [Simulation Injection & Overrides](#28-simulation-injection--overrides)
   - 2.9 [Persistence & Recovery](#29-persistence--recovery)
   - 2.10 [Observability](#210-observability)
   - 2.11 [Time-Series Architecture](#211-time-series-architecture)
   - 2.12 [MILP Formulation](#212-milp-formulation)
3. [HTTP API Reference](#3-http-api-reference)
4. [Architecture](#4-architecture)
   - 4.1 [Philosophy](#41-philosophy)
   - 4.2 [Deployment Topology](#42-deployment-topology)
   - 4.3 [Ring Map (Hexagonal Architecture)](#43-ring-map-hexagonal-architecture)
   - 4.4 [Module Responsibilities](#44-module-responsibilities)
   - 4.5 [Background Tasks](#45-background-tasks)
   - 4.6 [State Management & Locking](#46-state-management--locking)
   - 4.7 [Control Flow End-to-End](#47-control-flow-end-to-end)
5. [Configuration Reference](#5-configuration-reference)
6. [Deployment](#6-deployment)
7. [Testing](#7-testing)

---

## 0. Glossary

### Organisations & Roles

| Term | Definition |
|------|-----------|
| **Utility** | Electric company that generates, transmits, and/or distributes electricity. Operates the grid and runs DR programs. |
| **DSO** | Distribution System Operator — entity responsible for the local distribution network (e.g. EWZ, Enedis, Bayernwerk Netz). |
| **TSO** | Transmission System Operator — entity responsible for the high-voltage transmission grid (e.g. Swissgrid, TenneT, RTE). |
| **Aggregator** | Company that bundles many small DER/load resources into a portfolio large enough to participate in wholesale markets or utility DR programs. |
| **Prosumer** | End customer that both consumes and produces electricity (e.g. home with solar and battery). |

### OpenADR Protocol

| Term | Definition |
|------|-----------|
| **OpenADR** | Open Automated Demand Response — open standard protocol for communicating DR signals between utilities/aggregators and customer energy management systems. This project uses OpenADR 3. |
| **VTN** | Virtual Top Node — the server side of OpenADR. Creates programs, sends events to VENs, receives reports. |
| **VEN** | Virtual End Node — the client side of OpenADR. Receives events, controls local devices, reports telemetry. |
| **BFF** | Backend For Frontend — API proxy between the VTN UI and the VTN server. Not part of the OpenADR spec; an architectural pattern used in this lab. |

### HEMS Domain Entities

| Term | Definition |
|------|-----------|
| **HEMS** | Home Energy Management System — software that monitors and controls energy flows within a site to minimise cost or respond to DR signals. |
| **EnergyPacket** | A schedulable unit of energy delivery: fixed kWh for a specific asset, with a time window and lifecycle status (`PENDING → ACTIVE → COMPLETED / ABANDONED`). Packets are intent-tracking and reporting metadata — see §2 for the important note on their role in MILP planning. |
| **FlexibilityEnvelope** | The range of power the HEMS can flex (increase or decrease) per time slot, as seen from the grid — exposed to aggregators to estimate available DR capacity. |
| **UserRequest** | An explicit energy delivery request by the occupant (e.g. "charge EV to 80% by 07:00"). The planner honours these as FIRM or soft constraints. Modes: `ASAP`, `BY_DEADLINE`, `MAX_COST`, `OPPORTUNISTIC`. |
| **AssetLedger** | Cumulative energy accounting per asset: total kWh imported/exported, associated cost, and CO₂. In-memory only; resets on VEN restart. |
| **OadrEventSnapshot** | A point-in-time capture of all time-varying OpenADR event data at one poll tick: import tariff, export tariff, CO₂ intensity, import/export capacity limits. |

### Sign Convention

**Positive = power imported from grid. Negative = power exported to grid.**

This convention applies uniformly at the site boundary (utility meter) to setpoints, ledger entries, reports, and all power values in this project.

```
                                     <──── negative (export) ────
                                          ╭─────────────────────────╮
╭───────╮     ╭────────────────────╮      │  central connection     │
│Utility│<===>│  Utility Meter     │<====>│  board (Σ P = 0)        │<====> Assets
╰───────╯     ╰────────────────────╯      ╰─────────────────────────╯
               import tariff →                 ──── positive (import) ────>
               ← export tariff
```

Within the site: `Σ P = P_util − (P_consume + P_generate + P_store + P_release) = 0`. Generation (`P_pv`) and battery discharge are negative by definition; they result in net export **only if** their total magnitude exceeds simultaneous consumption.

> **Reference:** [docs/REQUIREMENTS.md §2](docs/REQUIREMENTS.md)

---

## 1. Purpose & Overview

**OpenAdr-Lab** is a self-hosted laboratory platform for experimenting with demand-response (DR) automation under the **OpenADR 3.0** protocol. It simulates a residential or small-commercial energy site with multiple controllable assets (battery, EV, heat pump, PV, base load) managed by a Virtual End Node (VEN). A co-hosted Virtual Top Node (VTN), running the open-source **openleadr-rs** stack, issues programs, events, and pricing signals to which the VEN responds.

The platform has two primary audiences:

| Audience | Use |
|----------|-----|
| **Researchers / tinkerers** | Observe, experiment with, and tune DR algorithms without touching real hardware |
| **Protocol implementors** | Verify OpenADR 3 message flows, report obligations, and program/event parsing against a live VTN |

Three independent VEN instances run simultaneously (ven-1, ven-2, ven-3), each with its own asset profile, planner parameters, and state — enabling multi-VEN coordination experiments.

---

## 2. Feature Reference

### 2.1 Simulation Engine
*(FR-SIM-01, FR-SIM-02, FR-SIM-07, FR-SIM-08)*

Each VEN hosts a physics engine that advances asset states every simulation tick (default: 1 second).

**Simulated assets:**

| Asset | Physical model | Controllable |
|-------|---------------|--------------|
| Battery | SoC rate = `setpoint_kw / capacity_kwh`; round-trip efficiency losses | Yes — continuous setpoint (kW) |
| EV | SoC rate while plugged; V2G-capable (bidirectional); departure guard enforces target SoC | Yes — charge/discharge kW |
| Heat pump / Heater | Thermal mass ODE: `dT/dt = (T_amb − T) / τ + Q / C`; multi-tier (off/mid/full) | Yes — discrete tiers (off → mid → full) |
| PV | Irradiance-driven sinusoidal model with EMA smoothing; non-curtailable | No (read-only) |
| Base load | Fixed consumption profile; non-controllable | No (read-only) |
| Grid (virtual) | Aggregates all asset powers; clamps to VTN import/export limits | Via VTN limits |

**V2G note:** V2G (vehicle-to-grid discharge) is controlled by `max_discharge_kw` in the EV profile section. The code default is `0.0` (V2G disabled). Set it to the EV's discharge capability (e.g., `7.4`) to enable bidirectional operation. The ven-1 profile does not set `max_discharge_kw`, so it uses the default of 0.0 (charge-only).

**Switching / startup penalties:** Phase 2 of the MILP applies operational friction costs to all controllable assets, not only the heater. Battery cycling has a configurable wear cost (`c_bat_wear_eur_kwh`). EV charging has a `v_ev_extra_eur_kwh` reward and tier penalty coefficients. Heater uses startup cost `c_startup_eur` and the Phase 2 objective coefficient. The heater column in the table above notes "multi-tier" because its discrete control (off/mid/full) makes the switching cost especially visible, but the friction mechanism is common to all assets.

**Asset detail reference:**

| Asset | Profile section | Key parameters | Forecast | History |
|-------|----------------|----------------|----------|---------|
| Battery | `assets: - type: battery` | `capacity_kwh`, `max_charge_kw`, `max_discharge_kw`, `round_trip_efficiency`, `min_soc` | `GET /forecast/battery` | `GET /history/battery` |
| EV | `assets: - type: ev` | `battery_kwh`, `max_charge_kw`, `max_discharge_kw` (0 = charge-only), `soc_target` | `GET /forecast/ev` | `GET /history/ev` |
| Heater | `assets: - type: heater` | `max_kw`, `thermal_mass_kwh_per_c`, `k_loss_kw_per_c`, `temp_min_c`, `temp_max_c` | `GET /forecast/heater` | `GET /history/heater` |
| PV | `assets: - type: pv` | `rated_kw`, `peak_hour` (solar noon), `ema_alpha` (smoothing) | `GET /forecast/pv` | `GET /history/pv` |
| Base load | `assets: - type: base_load` | `baseline_kw` | `GET /forecast/base_load` | `GET /history/base_load` |

**Shared physics between simulation and planning:** Every asset type implements two separate forward-step paths using the same underlying ODE:
- *Simulation step* (`step()`) — called each tick with the actual setpoint; updates the live `AssetState`.
- *Forecast* (`forecast()`) — called by the MILP result translator and timeline API; runs the same ODE forward in time from current state to project e.g. battery SoC trajectory or heater temperature trajectory over the plan horizon.

This means the forecast shown in the timeline is guaranteed to be consistent with what the simulator would produce under the planned setpoints.

The simulation can run in **headless mode** (pure physics) or with **sensor injection** (see §2.8) to override individual parameters.

**Grid virtual asset (`AssetState::Grid`):** The Grid is not a physical device; it is a derived accounting view recomputed each tick from the sum of all other asset powers:

```rust
pub struct GridState {
    pub net_power_kw: f64,     // positive = importing from grid, negative = exporting
    pub import_limit_kw: f64,  // from active VTN IMPORT_CAPACITY_LIMIT events; always ≥ 0
    pub export_limit_kw: f64,  // from active VTN EXPORT_CAPACITY_LIMIT events; always ≤ 0
}
```

The Grid asset has no setpoint and is never controllable directly; it reflects the site's instantaneous balance.

**Static vs. dynamic asset limits:**

*Static limits* are constants in the YAML profile (e.g., `max_charge_kw`, `temp_max_c`, `rated_kw`). They are loaded at startup and do not change during a run.

*Dynamic limits* are derived each tick from the current `AssetState`. They are never persisted — always recomputed:

| Asset | Dynamic limit | Condition |
|-------|--------------|-----------|
| Battery | max discharge → 0 kW | `soc ≤ min_soc` |
| Battery | max charge → 0 kW | `soc ≥ 1.0` |
| EV | max charge/discharge → 0 kW | `plugged = false` |
| Heater | available power quantised | thermostat state (off / mid / full) |
| Site | import/export ceiling | active VTN capacity events (30 s poll) |

**Clamp formulas** (sign convention: positive = import/charge, negative = export/discharge):

| Asset | Expression |
|-------|-----------|
| Battery charge/discharge | `setpoint_kw.clamp(-max_discharge_kw, max_charge_kw)` |
| EV charge/discharge | `setpoint_kw.clamp(-max_discharge_kw, max_charge_kw)` |
| PV export curtailment | `raw_kw.max(export_limit_kw)` when limit ≤ 0 (prevents export beyond limit) |
| Heater thermal energy | `e_kwh.clamp(0.0, (temp_max_c − temp_min_c) × thermal_mass_kwh_per_c)` |
| Deviation absorber correction | `delta_kw.clamp(-headroom_kw, headroom_kw)` |

> **Reference:** [asset_simulation.md](docs/architecture/asset_simulation.md) · [ven_asset_interface_spec.md](docs/architecture/ven_asset_interface_spec.md)

### 2.2 Energy Planning (MILP)
*(UC-03, UC-04, UC-05, UC-07, UC-10; FR-ASSET-02)*

A mixed-integer linear program (MILP) solved by **HiGHS** computes a 24-hour optimal energy schedule every planning interval (default: 5 minutes) or on demand.

**Inputs to the solver:**
- Current asset SoC / temperature / capability
- Tariff time series (import/export prices, CO₂ intensity)
- VTN capacity limits per slot
- Asset physics constraints (max charge rate, min SoC floor, comfort deadband)
- Active user energy requests (deadlines, energy budgets)
- Planner objective (cost minimisation, emissions, grid stress, or weighted multi-objective)

**Output — a Plan:**
- Per-asset power allocation (kW) for each 5-minute slot over the horizon (default `plan_step_s` = 300 s)
- Cost breakdown per slot (grid cost, battery cycling cost, discomfort penalty)
- Plan warnings (infeasibility, capacity breaches)
- Flexibility envelope per slot (how much more or less can be offered to the grid)

**Plan stability and defragmentation — current implementation:**

Two mechanisms address plan quality beyond raw cost:

- **Defragmentation (implemented — Phase 2):** Phase 2 of the two-phase MILP minimises operational friction (startup penalties, switching costs, ramp costs) subject to keeping the total economic cost within `phase2_epsilon_eur` of the Phase 1 optimum. This consolidates short on/off bursts into longer contiguous blocks and avoids unnecessary mode switches, which produces more stable, less fragmented schedules and more reliable forecast reports.

**Independence of objectives:** Yes, the two objectives are fully independent and the guarantee is structurally enforced. `c_star` is determined by Phase 1 and frozen as a hard constraint for Phase 2: `phase1_cost ≤ c_star + epsilon`. Phase 2 has an entirely separate objective function (friction only) and operates on its own copy of the model. It cannot "compensate" by reducing Phase 1's economic cost — Phase 1's optimal solution is a floor, not a variable. The warm-start hint passes Phase 1's solution to Phase 2 as an initial incumbent, which improves solve speed, but the hard constraint still holds. Setting epsilon = 0 makes the constraint equality (`phase1_cost = c_star`), making Phase 2 strictly Pareto-optimal in both dimensions simultaneously.

- **Stability (implemented — acceptance gate):** Periodic replans are only adopted if the new plan's total cost (economic + friction) improves by more than an `effective_threshold`. The effective threshold decays linearly with plan age, so older plans are progressively more likely to be replaced. This prevents constant churning on minor cost oscillations while still allowing stale plans to be updated.

**Design note — post-report plan protection (not implemented):**

The suggestion is to add a protection score to adopted+reported plans so that future periodic replans face a higher cost-improvement bar. Analysis:

| | Detail |
|---|---|
| **Pro** | Prevents VTN forecast oscillation: once a consumption trajectory is reported, changing it without DR justification erodes forecast reliability. |
| **Pro** | Aligns VEN behaviour with VTN expectations: the VTN is tracking what the VEN committed to; frequent re-commitments reduce trust. |
| **Pro** | Low conceptual complexity: can be approximated by resetting the acceptance-gate decay timer on report submission rather than on plan adoption. |
| **Con** | No per-slot granularity in current reporting — reports submit *averages* over intervals, not full plan trajectories. Protecting per-slot allocations would require tracking which slots were reported. |
| **Con** | Risk of stale lock-in if hard triggers are under-specified (e.g., EV disconnect mid-plan must bypass the gate, or the plan never adapts). |
| **Con** | A full "protection grade vector" (varying protection per slot) is considerably more complex and may be premature given the current reporting model. |

**Recommendation:** The simplest effective approach is to record the last-report timestamp and use it as the reference for decay, instead of plan adoption time. A freshly reported plan gets a full decay window, giving it natural protection without adding a new data structure. A full per-slot protection vector should only be considered once per-slot reporting to the VTN is implemented.

**Two-phase lexicographic solve:**
1. **Phase 1 (MIP — cost minimisation)** — minimises economic cost only (import tariff, export revenue, battery cycling). No startup/ramp auxiliary variables; finds the optimal cost floor `c_star`.
2. **Phase 2 (MIP — friction minimisation)** — minimises operational friction (startup penalties, ramp costs, switching penalties, tier penalties) subject to the constraint `phase1_cost ≤ c_star + phase2_epsilon_eur` (default ε = 0.02 €). Phase 1's solution is used as the warm-start incumbent so Phase 2 immediately has a feasible integer point. Setting `phase2_epsilon_eur = 0` collapses to a single-phase solve.

The planner runs in a blocking Tokio thread to avoid starving the async runtime. See §2.12 for the full MILP formulation.

**Configuring Phase 2:** Set `phase2_epsilon_eur` in the `planner:` section of the profile YAML (default: `0.02`). This is the maximum extra cost (in €) that Phase 2 may spend while defragmenting. Increasing it allows more aggressive defragmentation at a small cost premium. Setting it to `0.0` disables Phase 2 entirely (single-phase solve — purely cost-optimal, potentially fragmented). Phase 1's cost optimum is protected by the hard constraint `phase1_cost ≤ c_star + phase2_epsilon_eur`; Phase 2 can only minimise friction within that budget, never worsen the economic result beyond ε.

**Acceptance gate:** A new plan is adopted only if its total cost is below a threshold relative to the current plan. Hard triggers (VTN rate change, capacity alert, user request, device deviation) bypass the gate.

**Threshold decay:** The effective threshold decays linearly with plan age:
```
effective_threshold = plan_adoption_threshold_eur × max(0, 1 − elapsed_s / plan_adoption_decay_s)
```
When `elapsed_s ≥ plan_adoption_decay_s` the plan is considered fully decayed and any periodic replan replaces it unconditionally — even at equal cost. This prevents stale plans from persisting indefinitely. Example: `threshold = 0.20 €`, `decay_s = 3600` — after 30 minutes the effective threshold drops to `0.10 €`; after 1 hour the plan is force-replaced.

> **Reference:** [VEN_ARCHITECTURE.md](docs/architecture/VEN_ARCHITECTURE.md) · [heater_tank_milp_planning_model.md](docs/architecture/heater_tank_milp_planning_model.md)

### 2.3 Real-time Deviation Absorption
*(UC-08, UC-09; FR-ASSET-01, FR-ASSET-04)*

Between planning cycles, a **two-tier control architecture** keeps the site on its MILP plan:

#### Tier 1 — Absorber (every tick, ~1 second)

The absorber corrects deviations between planned and actual grid power without triggering a full replan.

**Deviation computation:**
```
deviation_kw = actual_net_kw − planned_net_kw
```
Positive = site is importing more than planned; correction must reduce import (reduce heater, curtail EV, discharge battery).  
Negative = site is importing less than planned (e.g. PV spike); correction must increase import (charge battery, ramp EV up).

**Note — PV curtailment not yet implemented:** The absorber and dispatcher currently treat PV as non-curtailable (read-only). Some OpenADR `LOAD_DISPATCH` signals may require export curtailment; this would need a controllable PV inverter model and a curtailment setpoint path.

**Per-asset correction state machine:**

Each asset in the absorber configuration independently tracks a *correction overlay* (delta from the MILP setpoint):

```
Idle  ─── |deviation| > dead_band ──►  Correcting
           overlay = planned_sp + delta        │
                                               │ |deviation| ≤ dead_band
                                               │ for dead_band_clearing_ticks ticks
                                               ▼
                                          Settling
                                          (overlay ramps to 0 over 1 tick)
                                               │
                                               ▼
                                          Idle  (back to clean MILP setpoint)
```

The `dead_band_clearing_ticks` wait-gate prevents chattering: if the deviation momentarily dips inside the dead-band but then rises again, the settling counter resets to zero and the overlay is held.

**Worked example:** `dead_band_kw = 0.1`, `dead_band_clearing_ticks = 1`.

- *Tick 1:* Plan = 2.0 kW grid import. Actual = 2.3 kW. `deviation_kw = +0.3` (exceeds dead-band). Battery is Idle → transitions to Correcting; absorber applies –0.3 kW overlay (battery discharges 0.3 kW extra).
- *Tick 2:* Battery discharge covers the gap. Actual = 2.05 kW. `deviation_kw = +0.05` (< dead-band). Battery enters Settling; `dead_band_clearing_ticks = 1` → satisfied in one tick → transitions to Idle. Overlay cleared, battery returns to MILP setpoint.
- *If at Tick 2 the deviation spiked again (0.2 kW):* The settling counter resets to zero and the battery remains in Correcting until the deviation stays below dead-band for the full clearing count.

**Constraints applied before each asset correction:**

| Constraint | Effect |
|------------|--------|
| Dead-band (`dead_band_kw`, default 0.1 kW) | Corrections smaller than this are ignored entirely |
| Asset priority order (profile-configured) | Assets are tried in priority order: lower number = first (default: battery 0, EV 1) |
| Headroom bounds | Battery: bounded by SoC vs. min-SoC floor; EV charge: bounded by curtailable setpoint (down) or remaining capacity-to-target (up); Heater: discrete step (off → mid → max) |
| Relay wear linger (`min_state_linger_s`) | Asset is skipped if fewer than `min_state_linger_s` seconds have passed since its last state change |
| EV departure guard (`ev_departure_guard_s`) | If an EV session is active, departure is within the guard window (ven-1 profile: 1800 s), AND the EV's current SoC is below its target, the absorber will NOT reduce EV charging (positive deviation). The guard does not apply when SoC ≥ target (already satisfied), when deviation is negative (surplus absorption — EV charging is always increased), or when no session is active |

**SSE telemetry:** When a correction is applied the absorber broadcasts a `PlannerEvent::CorrectionActive` SSE event with planned vs. actual net power and the correction magnitude. When the correction clears it emits `PlannerEvent::CorrectionCleared`. Events are deduplicated: a new SSE fires only when the total correction changes by > 0.2 kW.

**Use cases for correction SSE events:**
- **UI status indicator:** The frontend can show a live "absorber active" indicator (e.g., yellow badge) when `CorrectionActive` is received, returning to green on `CorrectionCleared`. This gives the operator real-time visibility of how much the site deviates from plan.
- **DR compliance monitoring:** A client subscribing to the SSE stream can log correction magnitude and duration; sustained corrections are a signal that the MILP plan needs re-tuning or the asset is degraded.
- **Tier-2 trigger transparency:** The SSE shows the residual being accumulated toward Tier-2 escalation, making it auditable when a full replan was triggered and why.

#### Tier 2 — DeviceDeviation escalation

After the absorber runs, the uncovered `residual_kw` (what could not be absorbed) is accumulated tick-by-tick. When the residual exceeds the dead-band for `deviation_trigger_ticks` consecutive ticks, a `DeviceDeviation` trigger is sent to the planning loop, which wakes up immediately and runs a full MILP replan. This ensures sustained under- or over-performance that the absorber cannot handle is corrected at the planning level.

**Summary:** Tier 1 handles fast, transient deviations within seconds at zero planning cost. Tier 2 escalates only when Tier 1 is genuinely exhausted over multiple ticks.

> **Reference:** [VEN_ARCHITECTURE.md](docs/architecture/VEN_ARCHITECTURE.md)

### 2.4 User Energy Requests
*(UC-01, UC-02, UC-06, UC-09; FR-ASSET-04, FR-OA-04)*

Users can request energy services with deadline constraints:

| Session type | What it does |
|--------------|-------------|
| **EV charge** | Charge EV to a target SoC by a specified departure time |
| **EV discharge (V2G)** | Export from EV battery to grid/home up to a budget |
| **Heater** | Reach and hold a temperature target |
| **Shiftable load** | Schedule a fixed-energy task (e.g., dishwasher) within a window |
| **Baseline override** | Manually shift the expected base load profile for a period |

**On heat pump / AC as shiftable loads:**

The compressor minimum-run-time constraint is real hardware behaviour — once started, the compressor must run for a minimum block (typically 5–20 min) to avoid damage. A washing machine has the same structural property: once started, it must run for its full cycle. So the analogy has merit on the surface.

The problem is that a shiftable load model encodes **a fixed energy block with a variable start time** — the only degree of freedom is *when* the block begins. This is correct for a washing machine (1.0 kWh, always 60 min, run it anywhere in the 8-hour window). It breaks down for a heat pump because:

1. **The block size is not fixed.** The energy needed to heat a tank from current temperature to target depends on instantaneous thermal state, ambient temperature, and heat loss during the run itself. At planning time (5 min ahead), the block size is knowable; at 6-hour-ahead planning it is not, because thermal drift between now and the planned start slot changes it continuously.

2. **The comfort constraint persists between jobs.** A dishwasher has no comfort constraint between cycles. A heat pump must keep the space/tank within `[temp_min, temp_max]` at all times — including during the off-periods between compressor runs. If the thermal mass cools below `temp_min` before the scheduled start, the compressor must turn on regardless of the plan. The shiftable model has no mechanism to express this; the continuous ODE + per-slot temperature bounds does.

3. **Minimum run time is a constraint on the ODE model, not a reason to replace it.** The correct way to add compressor protection is a minimum-on MILP constraint: if `z_heat[t] = 1`, then `z_heat[t+1 .. t+k] = 1`. This is a small addition to the existing heater MILP formulation. It does not require discarding the thermal state variable.

**Conclusion:** Add `min_run_slots` (and `min_off_slots`) as configurable parameters on the heater MILP asset. For a **resistive heater or electric boiler** (no compressor), set both to `0` — the constraint is a no-op and the model is unconstrained on switching. For a **heat pump** (compressor), set `min_run_slots` to the compressor's minimum on-time (e.g., `3` for a 5-min slot = 15 min) and `min_off_slots` to the restart lockout time. One unified continuous model with configurable timing constraints handles both device types correctly. The shiftable load category remains appropriate only for truly job-oriented loads (dishwasher, washing machine, EV charge session) where a fixed energy block per activation is the correct model.

Requests are tracked through states: `Pending → Scheduled → Active → Completed / Failed`.  
The planner integrates user requests as hard constraints (must-meet) or soft constraints (best-effort) depending on configuration.

**Opportunistic EV charging** can be toggled independently: when on, any surplus generation charges the EV even without an explicit user request.

**Interaction between user requests and the deviation absorber:**

User requests (EV session, heater target, shiftable load) are inputs to the MILP planner — they translate into hard or soft constraints on the resulting `Plan`. The absorber then operates on deviations from that plan. They do not compete:

```
User request (POST /ev-session)
        │
        ▼ (on hard trigger)
MILP planner incorporates request as plan constraint
        │
        ▼
New Plan adopted (EV charging slot already included)
        │
        ▼
sim_tick Phase 2: dispatcher reads plan → setpoints include EV charging
        │
        ▼
sim_tick Phase 3: absorber compares actual vs planned grid import
                  EV departure guard prevents cutting EV if soc < target
```

The absorber respects user intent through the departure guard and through the plan itself (which already scheduled the request). There is no priority conflict because the plan is the single source of truth for all asset targets.

**Opportunistic loading — current state:** Only the EV has an opportunistic surplus-charging overlay (`apply_surplus_ev_overlay` in `dispatcher.rs`). It activates when no EV session is active, the EV is plugged and below its SoC target, and PV is generating a surplus. This is an explicit opt-in toggle (`PUT /ev-settings`).

**Opportunistic heating / boiler — not yet implemented.** Extending the surplus overlay to the heater (pre-heat when PV surplus is available) would follow the same pattern: a dispatcher-level overlay that fires when no HeaterTarget session is active. The absorber priority list could naturally govern the order (battery → EV → heater), reusing the existing `priority` field in the absorber asset config.

> **Reference:** [heater_tank_milp_planning_model.md](docs/architecture/heater_tank_milp_planning_model.md) · [VEN_ARCHITECTURE.md](docs/architecture/VEN_ARCHITECTURE.md)

**Energy packets and MILP scheduling — important distinction:** Energy packets (§0 Glossary) are an intent-tracking and reporting layer, not MILP scheduling variables. The MILP decision variables (`p_ev[t]`, `z_heat_mid[t]`, `p_bat_ch[t]`, etc.) drive the actual schedule. A packet contributes its `request_mode`, `deadline`, and `target_energy_kwh` as constraints or reward terms to the MILP, but the per-slot schedule lives in `PlanTimeSlot.allocations` — not on the packet. Packet lifecycle states (`PENDING → ACTIVE → COMPLETED`) serve the dispatcher and reporting layers and are independent of the solver.

### 2.5 VTN Integration (OpenADR 3)
*(FR-OA-01, FR-OA-02, FR-OA-03, FR-OA-07, FR-OA-08, OA-01 through OA-08)*

The VEN polls the VTN continuously over authenticated HTTPS.

**Polling loops:**

| Loop | Default interval | What it fetches |
|------|-----------------|----------------|
| Programs | 30 s (code default); 300 s in Docker Compose | Active demand-response programs |
| Events | 30 s | Price, GHG, curtailment, and alert signals |
| Reports | 60 s | Confirmation of received reports |

**On program polling frequency:** Programs are indeed long-lived — a VTN program defines the pricing structure and reporting obligations for an entire DR campaign (days to weeks). They change rarely compared to events (which carry the actual hourly price signals). The 30 s code default is conservative; the Docker Compose override of 300 s (5 min) reflects this. All polling intervals are already configurable via environment variables: `POLL_PROGRAMS_SECS`, `POLL_EVENTS_SECS`, `POLL_REPORTS_SECS` (see §5).

**Authentication:** OAuth 2.0 client-credentials flow. The token is cached with a 60-second safety margin and automatically refreshed on expiry or a 401 response.

**Token refresh — current behaviour and enhancement path:** The token is cached and re-fetched only when within 60 seconds of expiry or on a 401 response. The 60-second safety margin is hardcoded in `vtn.rs`. Making it configurable (or disabling proactive refresh in favour of pure 401-driven refresh) is a small enhancement not yet implemented. For most lab use, the current behaviour is negligible traffic — one token fetch per ~hour.

**Signal implementation status:**

| Signal / payload type | Parsed? | Effect |
|----------------------|---------|--------|
| `PRICE` | Yes | Sets import tariff time series → feeds MILP cost minimisation |
| `EXPORT_PRICE` | Yes | Sets export tariff time series → feeds MILP export revenue |
| `GHG` | Yes | Sets CO₂ intensity series → feeds multi-objective (emissions mode) planner |
| `IMPORT_CAPACITY_LIMIT` | Yes | Hard import ceiling per interval; MILP enforces as soft constraint with penalty; strictest of concurrent events wins |
| `EXPORT_CAPACITY_LIMIT` | Yes | Same for export |
| `IMPORT_CAPACITY_SUBSCRIPTION` | Yes | Parsed and stored in `OadrCapacityState`; available to MILP capacity layer |
| `IMPORT_CAPACITY_RESERVATION` | Yes | Parsed and stored in `OadrCapacityState`; reported back to VTN |
| `SIMPLE` | No — triggers replan only | Any new/expired event fires a `RateChange` watch signal → planning loop wakes; the numeric level (0–3) is not translated into a curtailment setpoint |
| `LOAD_DISPATCH` | No — triggers replan only | Same mechanism as `SIMPLE` |
| `ALERT_*` (all variants) | No — triggers replan only | No dedicated parser; event arrival/departure is the trigger |
| All other types | Not implemented | See full taxonomy below |

**The VEN always triggers an immediate replan on any event arrival or departure**, regardless of payload type. This provides a functional fallback for unrecognised signal types.

**Full OpenADR 3.0 signal taxonomy** (source: `openleadr-wire/src/event.rs`):

*Price signals:*

| Type | Value | Description | Implemented |
|------|-------|-------------|-------------|
| `PRICE` | float (EUR/kWh) | Import electricity price; drives MILP cost minimisation | Yes |
| `EXPORT_PRICE` | float (EUR/kWh) | Export electricity price; sets MILP export revenue | Yes |

*Environmental:*

| Type | Value | Description | Implemented |
|------|-------|-------------|-------------|
| `GHG` | float (g CO₂/kWh) | Grid carbon intensity; feeds multi-objective CO₂-weighted planning | Yes |

*Capacity management (Dynamic Operating Envelopes):*

| Type | Value | Description | Implemented |
|------|-------|-------------|-------------|
| `IMPORT_CAPACITY_LIMIT` | float (kW) | Hard ceiling on site import power; MILP soft constraint | Yes |
| `EXPORT_CAPACITY_LIMIT` | float (kW) | Hard ceiling on site export power; MILP soft constraint | Yes |
| `IMPORT_CAPACITY_SUBSCRIPTION` | float (kW) | Subscribed import capacity; parsed and stored | Parsed only |
| `IMPORT_CAPACITY_RESERVATION` | float (kW) | Reserved import capacity; parsed, stored, reported | Parsed only |
| `IMPORT_CAPACITY_RESERVATION_FEE` | float | Fee for import capacity reservation | Not implemented |
| `IMPORT_CAPACITY_AVAILABLE` | float (kW) | Available import capacity from grid operator | Not implemented |
| `IMPORT_CAPACITY_AVAILABLE_PRICE` | float | Price for available import capacity | Not implemented |
| `EXPORT_CAPACITY_SUBSCRIPTION` | float (kW) | Subscribed export capacity | Not implemented |
| `EXPORT_CAPACITY_RESERVATION` | float (kW) | Reserved export capacity | Not implemented |
| `EXPORT_CAPACITY_RESERVATION_FEE` | float | Fee for export capacity reservation | Not implemented |
| `EXPORT_CAPACITY_AVAILABLE` | float (kW) | Available export capacity from grid operator | Not implemented |
| `EXPORT_CAPACITY_AVAILABLE_PRICE` | float | Price for available export capacity | Not implemented |

*Control signals:*

| Type | Value | Description | Implemented |
|------|-------|-------------|-------------|
| `SIMPLE` | integer 0–3 | Level-based DR: 0=normal, 1=moderate, 2=high, 3=emergency | Not implemented (triggers replan only) |
| `DISPATCH_SETPOINT` | float (W) | Absolute power setpoint for site or asset | Not implemented |
| `DISPATCH_SETPOINT_RELATIVE` | float (W) | Relative power change (signed) | Not implemented |
| `CHARGE_STATE_SETPOINT` | float (%) | Target state of charge for battery/EV | Not implemented |
| `OLS` | float 0.0–1.0 | Operating Limit Setpoint — fraction of rated power | Not implemented |
| `CURVE` | point pairs | Volt-var or other characteristic curves | Not implemented |
| `CONTROL_SETPOINT` | depends | Generic control setpoint | Not implemented |

*Alert signals* (all trigger replan via event arrival; payload not parsed):

| Type | Description |
|------|-------------|
| `ALERT_GRID_EMERGENCY` | Grid emergency — requires immediate load shed |
| `ALERT_BLACK_START` | Black start event — grid restoration sequence |
| `ALERT_POSSIBLE_OUTAGE` | Outage warning — pre-emptive load reduction |
| `ALERT_FLEX_ALERT` | Flexibility shortfall — voluntary DR requested |
| `ALERT_FIRE`, `ALERT_FREEZING`, `ALERT_WIND`, `ALERT_TSUNAMI`, `ALERT_AIR_QUALITY`, `ALERT_OTHER` | Environmental/safety alerts |

*Device-specific:*

| Type | Value | Description | Implemented |
|------|-------|-------------|-------------|
| `CTA2045_REBOOT` | 0=soft, 1=hard | CTA-2045 device reboot command | Not implemented |
| `CTA2045_SET_OVERRIDE_STATUS` | 0/1 | CTA-2045 override flag | Not implemented |
| `Private(String)` | any | Custom private event types | Not implemented |

**VEN-initiated capacity requests — not implemented.** The struct `OadrCapacityRequest` exists in the codebase but has no callers. The VEN currently only receives capacity constraints from the VTN; it cannot initiate a reservation request (e.g., "I need 11 kW for 2 hours"). The VEN-to-VTN request model is defined in the OpenADR 3.0 spec but is not implemented in this lab.

**Operator motivation profiles — not implemented.** The VEN is hardcoded to minimise import cost (weighted by tariff and CO₂ intensity). The OpenADR concept doc defines operator profiles (cost-optimiser, compliance-driven, comfort-priority, EV fleet, DSO-contracted), but no profile selection mechanism exists in the VEN configuration. All three VEN instances use the same MILP objective structure.

> **Reference:** [VTN_ARCHITECTURE.md](docs/architecture/VTN_ARCHITECTURE.md) · [concept_vtn_ven_demand_response_simulation.md](docs/architecture/concept_vtn_ven_demand_response_simulation.md)

### 2.6 Report Obligations
*(FR-OA-04)*

When a VTN program specifies reporting requirements, the VEN fulfils them automatically.

**Flow:**
1. VTN program carries `OadrReportObligation` (interval, attributes, baseline type)
2. The obligation service tracks due timestamps
3. When due, the reporter computes:
   - Actual asset power averages over the interval
   - Baseline comparison (if required)
   - Accumulated energy, cost, CO₂
4. The report is POSTed to the VTN `/reports` endpoint
5. The obligation is marked fulfilled

> **Reference:** [VTN_ARCHITECTURE.md](docs/architecture/VTN_ARCHITECTURE.md) · [VEN_ARCHITECTURE.md §5.2](docs/architecture/VEN_ARCHITECTURE.md)

### 2.7 Flexibility Envelope
*(FR-ASSET-01, FR-ASSET-02, UC-05, UC-07)*

At any moment the VEN can report its site-level flexibility to the VTN:

- **`up_kw`** — how much grid import can be reduced right now (demand reduction)
- **`down_kw`** — how much grid import can be increased (load increase, e.g., charge EV faster)
- **`up_duration_s`** / **`down_duration_s`** — estimated duration before the constraint binds

This is used by the VTN operator to understand available DR capacity across VENs.

**How the flexibility envelope is calculated** (`controller/envelope.rs`, `compute_envelope()`):

For each controllable asset snapshot in `SimSnapshot`:
```
up_kw   += max(asset.power_kw − asset.cap_max_export_kw, 0)
down_kw += max(asset.cap_max_import_kw − asset.power_kw, 0)
```
- `up_kw` is how much current import can be *reduced* (asset has headroom to discharge / reduce draw).
- `down_kw` is how much current import can be *increased* (asset has headroom to charge more).
- PV and base load have a point-range capability (`cap_max_import_kw = cap_max_export_kw = current_power_kw`), so they contribute 0 to both directions.

Duration estimates use stored energy:
```
up_duration_s   = available_discharge_kwh / up_kw × 3600
down_duration_s = available_charge_kwh    / down_kw × 3600
```
Both are `None` when the corresponding `kw` value is below a near-zero threshold. The envelope is recomputed each tick in Phase 5 and served via `GET /flexibility`.

> **Reference:** [ven_asset_interface_spec.md](docs/architecture/ven_asset_interface_spec.md)

### 2.8 Simulation Injection & Overrides
*(FR-SIM-03, FR-SIM-09)*

For experimentation, any simulated physics parameter can be overridden via the API without restarting:

| Injection mode | Fields | Behaviour |
|---------------|--------|-----------|
| **A — one-shot** | `battery_soc`, `ev_soc`, `heater_temp_c` | Applied once to physics state, then cleared automatically on the next tick |
| **B — frozen + EMA return** | `pv_irradiance`, `base_load_kw` | Value is held constant while the override is active. On release, the physics model blends back toward the natural value exponentially (EMA) |
| **C — frozen + snap** | `ev_plugged`, `ev_soc_target`, `heater_setpoint_c`, `heater_temp_min/max_c`, `ambient_temp_c`, `grid_import/export_limit_kw` | Value is held constant while active; on release snaps immediately to the profile default |
| **D — planning only** | `pv_plan_kw` | Seen by the MILP planner only; has no effect on the physics simulator |

**Mode C — frozen + snap — rationale:** Mode C applies to fields that have no meaningful "natural return trajectory". For example, `ev_soc_target` is a configuration value, not a physics state — there is no EMA blend-back to a natural value. Similarly, `ev_plugged` is a discrete boolean, `ambient_temp_c` is an external boundary condition, and grid limits are VTN-imposed thresholds. Releasing any of these with EMA blending (Mode B) would produce nonsensical intermediate values. Mode C simply holds the overridden value until explicitly released, then snaps to the profile default. In the UI, Mode C fields appear as persistent override inputs that stay active until the user clears them — they are not self-resetting.

**Why `pv_plan_kw` is planning-only (Mode D):** The MILP planner needs a PV forecast (what will PV generate over the next 24 hours?) to optimise battery and EV charging schedules. `pv_plan_kw` overrides this forecast *inside the solver* without touching the physics simulation. The real PV simulator continues running its irradiance model and its output continues to appear in `GET /history/pv` and the real-time grid balance. The timeline (`GET /timeline/pv`) shows the *planned* PV trajectory, which will reflect the override; `GET /history/pv` shows actual simulated output. This separation is intentional: you can test "what if PV generates flat 5 kW?" in the planner without corrupting the physics state, which matters for absorption/deviation calculations.

Supported overrides include: PV irradiance, base-load power, battery SoC, EV SoC, ambient temperature, grid import/export limits, and asset setpoints.

A `POST /sim/inject/reset` clears all active overrides simultaneously.

> **Reference:** [VEN_ARCHITECTURE.md](docs/architecture/VEN_ARCHITECTURE.md) · [ven_asset_interface_spec.md](docs/architecture/ven_asset_interface_spec.md)

### 2.9 Persistence & Recovery
*(FR-SIM-07)*

VEN state is persisted in two separate mechanisms:

- **Sim physics state** (asset SoCs, temperatures, setpoints, history) is written to `/data/state.json` inside the tick loop at every `persist_every_s` ticks (ven-1 profile: 15 s). This happens in Phase 8 of the tick and also on graceful shutdown (Ctrl-C).
- **AppState** (polled programs, events, sensor snapshots) is written separately by the `state_persist` background task, if `PERSIST_PATH` is configured.

On restart, the physics state is reloaded and simulation resumes from where it left off. Profile parameters (physics constants, planner weights) are recomputed from the YAML profile file and merged back into the loaded state, so config changes take effect cleanly without manual state migration.

**What is persisted per asset (`sim_state.json`):**

| Asset | Persisted fields |
|-------|-----------------|
| Battery | `soc` (0–1), `actual_power_kw`, `setpoint_kw`, cumulative `energy_kwh` |
| EV | `soc` (0–1), `plugged` (bool), `actual_power_kw`, `setpoint_kw`, cumulative `energy_kwh` |
| Heater | `temperature_c`, `actual_power_kw`, `setpoint_kw`, cumulative `energy_kwh` |
| PV | `actual_power_kw`, `setpoint_kw`, cumulative `energy_kwh` |
| Base load | `actual_power_kw`, `setpoint_kw`, cumulative `energy_kwh` |
| Grid | net / import / export power and cumulative energy totals (`GridMeter`) |
| Tick timestamp | `last_tick: DateTime<Utc>` — used to advance time on restart |

**What is NOT persisted (cleared on restart):**

- `AssetHistoryBuffer` (3600-entry power history ring buffer) — starts empty on each boot
- PV and base-load EMA smoothing state — reset to zero
- Active plan and MILP schedule — recomputed on first planning cycle

**Plan persistence — not yet implemented.** On restart the VEN recomputes a fresh plan from current sim state. This means `GET /timeline/*` is unavailable until the first plan is computed (typically within seconds), and any pending report obligations that reference the pre-restart plan's trajectory are approximated from the new plan. For production DR deployments where continuous VTN forecast reporting is required, plan serialisation to `/data/state.json` on plan adoption (and reload on startup) is a necessary future enhancement.

> **Reference:** [VEN_ARCHITECTURE.md](docs/architecture/VEN_ARCHITECTURE.md)

### 2.10 Observability
*(FR-SIM-10, FR-OA-06)*

| Signal | Endpoint / mechanism |
|--------|---------------------|
| Health check | `GET /health` — returns 200 OK with VEN name and uptime |
| Prometheus metrics | `GET /metrics` — counters and gauges for ticks, solves, reports |
| Controller trace | `GET /trace/events` (SSE stream) and `GET /trace/history` — last 500 controller events with timestamps, event type, and payload |
| Structured logs | JSON tracing output via `tracing-subscriber`; level controlled by `RUST_LOG` |
| Planner progress SSE | Server-Sent Events pushed during MILP solve (solving started, phase progress, plan adopted) |

**Asset power log — `AssetHistoryBuffer`:** Each asset maintains a 3600-entry ring buffer of per-second history, accessible via `GET /history/:asset_id`. This provides ~1 hour of 1-second resolution power, SoC, and temperature traces.

Each entry (`HistoryPoint`) stores:
```rust
pub struct HistoryPoint {
    pub ts:       DateTime<Utc>,
    pub power_kw: f64,        // signed: positive = import, negative = export
    pub state:    AssetState, // full state snapshot (SoC, temperature, etc.)
}
```

Public ring-buffer API (`assets/mod.rs`):

| Method | Description |
|--------|-------------|
| `push(point)` | Appends a point; evicts oldest when buffer is full (capacity 3600) |
| `slice(window)` | Returns all points in `[now − window, now]`, ordered ascending |
| `latest()` | Most recent `HistoryPoint` |
| `power_at(t)` | Last-observation-carried-forward power at or before time `t` |
| `recent_avg_power(window)` | Time-weighted average power over window (LOCF between points) |

Configurable aggregation windows and retention beyond 3600 seconds are not yet implemented. For reporting, the reporter calls `recent_avg_power()` over the obligation interval directly from the ring buffer. The history buffer is cleared on restart (not persisted).

> **Reference:** [VEN_ARCHITECTURE.md](docs/architecture/VEN_ARCHITECTURE.md)

### 2.11 Time-Series Architecture

The system deals with multiple time series from different sources with different natural periods (1-second sim ticks, 5-minute planning slots, 1-hour day-ahead tariffs, 3–6-hour capacity events). Handling them correctly requires explicit interpolation rules.

#### Interpolation semantics

| Signal class | Examples | Rule | Notes |
|---|---|---|---|
| **Piecewise-constant** | Tariff (€/kWh), capacity limit (kW), `SIMPLE` level | **Step / LOCF** — value holds from breakpoint until the next | Linear interpolation is wrong here (implies a ramp, which is false) |
| **Continuous physical** | Power (kW), temperature (°C), SoC (%) | **Linear** — interpolate between measured points | Carrying last value flat is wrong here |
| **Cumulative** | Energy (kWh), cost (€) | **Sum within bucket** — never interpolate | Use bucket aggregation only |

**LOCF** = Last Observation Carried Forward: the value at time `t` is the most recent value at or before `t`. Correct for tariffs and any "signal that takes effect and stays in effect" until the next event.

#### Target abstraction — `TimeSeries<T>`

The target architecture (tracked in BACKLOG RF-05 and RF-06) replaces all ad-hoc lookups with one reusable type:

```rust
TimeSeries<T> {
    points:        Vec<(DateTime<Utc>, T)>,
    interpolation: Interpolation,  // Step | Linear | None
}

enum Interpolation {
    Step,    // LOCF — correct for tariffs, capacity limits, discrete states
    Linear,  // correct for power, temperature, SoC
    None,    // cumulative values — aggregate only, never interpolate
}
```

Key methods:
- `at(ts)` — evaluate at any timestamp using the declared interpolation rule
- `resample(grid)` — project onto an arbitrary timestamp grid
- `merge(series[])` — union-of-breakpoints merge across multiple series
- `bucket(width, agg)` — downsample with `mean` (power), `last` (states), or `sum` (energy)

#### Tariff boundary alignment

The MILP planning grid uses a fixed 5-minute slot width. A planning slot may straddle a tariff boundary (e.g., a slot from 10:55 to 11:00 spans a €0.20→€0.15 transition at 11:00). The target behaviour is a time-weighted average:

```
effective_tariff(slot) = Σ( tariff_i × overlap(slot, interval_i) ) / slot.duration
```

The current implementation samples the tariff at `slot.start` only, which assigns the wrong rate to the tail of any slot that crosses a boundary.

#### Capacity flattening

When multiple VTN events define overlapping import or export capacity limits, the effective constraint is the minimum (most restrictive) across all overlapping intervals:

```
effective_limit(slot) = min(capacity_i for all intervals overlapping slot)
```

#### Slot classification

Each planning slot is classified at plan-build time:

| Class | Definition | Effect |
|---|---|---|
| **FIRM** | Within `now + NearHorizonDuration` (default 2 h) | Must be executed; dispatcher reads these |
| **FLEXIBLE** | Beyond the near-horizon boundary | Can be revised at next replan |

**Early firm-up:** If rate variance across the FLEXIBLE window is < 10% (flat tariff), FLEXIBLE slots may firm up early to simplify execution.

**StaleRatePolicy:** When the VTN is unreachable and no tariff data is available for future slots, the policy (default: `HEURISTIC_FORECAST`) governs how the planner fills those slots. See `docs/REQUIREMENTS.md §3.2.1` for all policy options.

> **Reference:** [VEN_ARCHITECTURE.md §5](docs/architecture/VEN_ARCHITECTURE.md)

---

### 2.12 MILP Formulation

The planner uses a two-phase Mixed-Integer Linear Program (MILP) solved by HiGHS. This section defines the mathematical structure. For configuration, see §5 and §2.2.

#### Decision variables

| Symbol | Description | Unit | Bounds |
|---|---|---|---|
| `p_imp[t]` | Grid import power at slot `t` | kW | ≥ 0 |
| `p_exp[t]` | Grid export power at slot `t` | kW | ≥ 0 |
| `u_grid[t]` | Binary: site is net importing at slot `t` | — | {0, 1} |
| `s_imp_viol[t]` | Import capacity violation slack | kW | ≥ 0 |
| `s_exp_viol[t]` | Export capacity violation slack | kW | ≥ 0 |
| `p_ch[t]` | Battery charge power | kW | [0, `p_ch_max`] |
| `p_dis[t]` | Battery discharge power | kW | [0, `p_dis_max`] |
| `u_bat[t]` | Binary: battery is active (charging or discharging) | — | {0, 1} |
| `e_bat[t]` | Battery stored energy at slot `t` | kWh | [`e_min`, `e_max`] |
| `delta_active[t]` | Battery idle→active transition (startup) | — | {0, 1}, Phase 2 only |
| `delta_ramp[t]` | Battery net-power ramp magnitude | kW, Phase 2 only | ≥ 0 |
| `p_ev[t]` | EV charge power | kW | [0, `p_ev_max`] |
| `z_ev_on[t]` | Binary: EV charger is active | — | {0, 1} |
| `delta_ev[t]` | EV off→on transition (startup) | — | {0, 1}, Phase 2 only |
| `z_heat_mid[t]` | Binary: heater at mid tier (3 kW) | — | {0, 1} |
| `z_heat_full[t]` | Binary: heater at full tier (6 kW) | — | {0, 1} |
| `sw[t]` | Heater relay switch event magnitude | — | ≥ 0 |
| `e_tank[t]` | Heater tank stored thermal energy | kWh | [`e_tank_min`, `e_tank_max`] |
| `y_shift[j]` | Binary: shiftable load starts at valid slot `j` | — | {0, 1} |

`dt_h` = slot width in hours (default: 5 min = 1/12 h). `n` = number of planning slots (default: 288 for a 24-hour horizon).

#### Phase 1 — cost minimisation

Phase 1 minimises the total economic cost over the planning horizon. No startup, ramp, or switching auxiliary variables are included; they are zero-cost in this phase.

```
minimise Σ_{t=0}^{n-1} [
    w_energy · dt_h · c_imp[t] · p_imp[t]     (grid import cost)
  − w_energy · dt_h · c_exp[t] · p_exp[t]     (grid export revenue)
  + w_ghg    · dt_h · g_co2[t] · p_imp[t]     (CO₂ penalty)
  + w_grid   · dt_h · (p_imp[t] + p_exp[t])   (grid stress penalty)
  + w_import · dt_h · p_imp[t]                 (import minimisation bias)
  + w_viol   · dt_h · pen_imp · s_imp_viol[t]  (capacity violation)
  + w_viol   · dt_h · pen_exp · s_exp_viol[t]
  + c_bat_wear · dt_h · (p_ch[t] + p_dis[t])  (battery wear)
]
```

where `c_imp[t]` = import tariff (€/kWh), `c_exp[t]` = export tariff (€/kWh), `g_co2[t]` = CO₂ intensity (kg/kWh), and weights `w_*` are configurable in the profile YAML.

The result of Phase 1 is stored as `c_star` (the optimal economic cost floor).

#### Phase 2 — friction minimisation

Phase 2 re-runs the MILP with a separate objective: minimise operational friction. It adds startup, ramp, and switching auxiliary variables (`delta_active`, `delta_ramp`, `delta_ev`, `sw`). The Phase 1 cost is enforced as a hard upper bound:

```
minimise Σ_{t=0}^{n-1} [
    c_bat_startup · delta_active[t]   (battery startup penalty)
  + c_bat_ramp   · delta_ramp[t]     (battery ramp penalty)
  + c_ev_startup · delta_ev[t]       (EV startup penalty)
  + c_ev_ramp    · delta_ev_ramp[t]  (EV ramp penalty)
  + c_heater_sw  · sw[t]             (heater relay switching penalty)
]

subject to: phase1_cost_expr(Phase2_vars) ≤ c_star + phase2_epsilon_eur
```

The warm-start hint provides Phase 1's solution as the initial MIP incumbent, so Phase 2 immediately has a feasible integer point (important for solving in time on the Pi4's ARM CPU).

#### Independence of objectives

`c_star` is determined by Phase 1 and frozen as a hard constraint for Phase 2. Phase 2 has its own copy of all decision variables and cannot "trade" economic cost for friction reduction — it can only stay within the cost budget while finding a less fragmented schedule. Setting `phase2_epsilon_eur = 0.0` makes the constraint exact equality (`phase1_cost = c_star`), making the result simultaneously Pareto-optimal on both dimensions.

#### Objective profiles

Each `PlannerObjective` preset selects a fixed Phase 1 weight vector. The objective can be changed at runtime via `PUT /plan/objective` (triggers an immediate replan) or set statically in the profile YAML under `planner: objective:`.

| Preset | Purpose | Active weights | Zeroed |
|--------|---------|---------------|--------|
| `min_cost` **(default)** | Minimise electricity cost with light environmental nudges | `w_energy=1.0`, `w_ghg=0.20` (≈€200/tonne CO₂), `w_grid=0.02` €/kWh exchange, `c_bat_wear=0.03` €/kWh | `w_import` |
| `min_ghg` | Minimise grid carbon emissions; ignore monetary cost | `w_ghg=10.0` €/kgCO₂ | `w_energy`, `w_grid`, `w_import`, `c_bat_wear` |
| `min_grid` | Minimise total grid exchange (self-consumption / peak-shaving) | `w_grid=1.0` €/kWh on `p_imp + p_exp` | `w_energy`, `w_ghg`, `w_import`, `c_bat_wear` |
| `min_import` | Minimise grid import volume (autarky objective) | `w_import=1.0` €/kWh on `p_imp` only | `w_energy`, `w_ghg`, `w_grid`, `c_bat_wear` |
| `max_revenue` | Maximise export revenue minus import cost; pure tariff arbitrage | `w_energy=1.0`, `c_bat_wear=0.03` €/kWh | `w_ghg`, `w_grid`, `w_import` |
| `custom` | Use the individual weight fields from the profile YAML directly | All of `w_energy`, `w_ghg`, `w_grid`, `c_bat_wear_eur_kwh` from YAML | `w_import` (not a YAML-exposed field) |

**`min_cost` vs `max_revenue`:** Both use full energy-cost weighting (`w_energy=1.0`) and identical battery-wear cost. The only difference is that `min_cost` adds a CO₂ nudge (`w_ghg=0.20`) and a grid-stress nudge (`w_grid=0.02`), which can shift a small fraction of load toward cleaner or lower-exchange slots when the tariff saving is otherwise a tie. `max_revenue` disables these nudges entirely — it is purely financial.

**`min_ghg`:** Battery wear cost is zeroed to allow the battery to time-shift freely between high- and low-CO₂ windows without a monetary penalty overriding the carbon signal.

**`min_grid`:** Penalises the sum `p_imp[t] + p_exp[t]` every slot. This pushes the planner toward keeping all generation and consumption on-site (PV → battery → loads) and avoids round-tripping power through the grid even when tariffs would otherwise make export-then-reimport profitable.

**`min_import`:** Penalises import volume only, not export. Use when the goal is near-zero grid draw (e.g. island-mode simulation or a flat-rate import contract where volume matters more than price timing).

#### Constraint families

| Family | What it enforces |
|---|---|
| **Power balance (Kirchhoff)** | `p_imp[t] + p_pv[t] + p_dis[t] = p_base[t] + p_ev[t] + p_heat[t] + p_shift[t] + p_ch[t] + p_exp[t]` — site net power sums to zero each slot |
| **Import/export mutual exclusion** | `p_imp[t] ≤ p_imp_max · u_grid[t]` and `p_exp[t] ≤ p_exp_max · (1 − u_grid[t])` — prevents simultaneous import and export |
| **VTN capacity limits** | `p_imp[t] ≤ p_imp_max_cont[t] + s_imp_viol[t]` with penalty on slack — soft capacity ceiling from VTN events |
| **Battery SoC continuity** | `e_bat[t+1] = e_bat[t] + dt_h · (eff_ch · p_ch[t] − p_dis[t] / eff_dis)` — round-trip efficiency applied separately to charge/discharge |
| **Battery mutual exclusion** | `u_bat[t]` binary; charge and discharge cannot occur in the same slot above a threshold |
| **EV charger bounds** | `p_ev[t] ≤ p_ev_max · z_ev_on[t]`; availability mask forces `p_ev[t] = 0` when EV is absent or outside departure window |
| **EV energy requirement** | Cumulative `p_ev[t] · dt_h` over the horizon meets the session energy target (hard if `MustRun`, reward-weighted if `MayRun`) |
| **Heater relay schema** | `z_heat_mid[t] + z_heat_full[t] ≤ 1`; power = `p_mid · z_heat_mid[t] + p_full · z_heat_full[t]` — mutual exclusivity of mid/full tiers |
| **Heater thermal continuity** | `e_tank[t+1] = e_tank[t] + dt_h · (p_heat[t] − Q_dem[t])` with temperature bounds enforced via `e_tank_min/max` |
| **Heater switching** | `sw[t] ≥ |z_heat_mid[t] − z_heat_mid[t−1]|` and `|z_heat_full[t] − z_heat_full[t−1]|`; 0↔6 kW transitions incur 20% higher penalty (two relays switch) |
| **Shiftable load** | Each shiftable load must start exactly once in its valid window: `Σ_j y_shift[j] = 1` |
| **Cost lock (Phase 2)** | `phase1_cost_expr(vars) ≤ c_star + phase2_epsilon_eur` — the hard bound that makes Phase 2 Pareto-safe |

> **Reference:** [VEN_ARCHITECTURE.md](docs/architecture/VEN_ARCHITECTURE.md) · [heater_tank_milp_planning_model.md](docs/architecture/heater_tank_milp_planning_model.md)

---

### 2.13 Functional Requirements Cross-Reference

Quick-reference table mapping every FR code to its one-line description. Full requirement text lives in [`docs/REQUIREMENTS.md §4`](docs/REQUIREMENTS.md).

#### OpenADR Integration (FR-OA / OA)

| Code | Description |
|------|-------------|
| [FR-OA-01](#25-vtn-integration-openadr-3) | VEN MUST poll `/events` at 30 s fixed interval to detect new, updated, or deleted events |
| [FR-OA-02](#25-vtn-integration-openadr-3) | VEN MUST obtain and refresh OAuth2 token before calling any VTN endpoint |
| [FR-OA-03](#25-vtn-integration-openadr-3) | VEN MUST detect event deletion and treat it as cancellation; roll back active DR response |
| [FR-OA-04](#26-report-obligations) | VEN MUST submit reports for any active `reportDescriptor` obligation from event payloads |
| [FR-OA-06](#210-observability) | All timestamps MUST be UTC, ISO 8601 / RFC 3339 format |
| [FR-OA-07](#25-vtn-integration-openadr-3) | On VTN communication failure, VEN MUST back off exponentially (1–15 min) and continue on last-known state |
| [FR-OA-08](#25-vtn-integration-openadr-3) | Event priority: lower number = higher priority; newer event breaks ties |
| [OA-01](#25-vtn-integration-openadr-3) | Emergency Load Shed — respond within one poll cycle (30 s); acknowledge event; correct timing |
| [OA-02](#25-vtn-integration-openadr-3) | Renewable Export Limitation — enforce `EXPORT_CAPACITY_LIMIT` per interval |
| [OA-03](#25-vtn-integration-openadr-3) | Time-of-Use / Dynamic Price — handle multi-interval uniform pricing; update on late VTN corrections |
| [OA-04](#25-vtn-integration-openadr-3) | Planned Peak Shaving — track event lifecycle; handle modifications |
| [OA-05](#25-vtn-integration-openadr-3) | EV Charging Management — resolve overlapping events by priority; apply group membership |
| [OA-06](#25-vtn-integration-openadr-3) | Battery Dispatch Window — honour directional control (charge vs. discharge) per interval |
| [OA-07](#25-vtn-integration-openadr-3) | Program Enrollment / Connectivity — acknowledge no-op events; send telemetry on schedule |
| [OA-08](#25-vtn-integration-openadr-3) | Event Cancellation — detect event deletion on poll; perform clean rollback; maintain state consistency |

#### Asset Interface (FR-ASSET)

| Code | Description |
|------|-------------|
| [FR-ASSET-01](#27-flexibility-envelope) | Every asset MUST implement `current() → f64` — present power in kW |
| [FR-ASSET-02](#27-flexibility-envelope) | Every asset MUST implement `forecast(horizon) → Vec<(DateTime, f64)>` — predicted power over horizon |
| [FR-ASSET-03](#27-flexibility-envelope) | Every asset MUST implement `history(window) → Vec<(DateTime, f64)>` — recorded power history |
| [FR-ASSET-04](#23-real-time-deviation-absorption) | Asset simulation backend MUST be encapsulated; controller accesses only via three-window interface |
| [FR-ASSET-05](#23-real-time-deviation-absorption) | Simulated and measured assets of same type MUST be interchangeable from controller's perspective |

#### Simulator (FR-SIM)

| Code | Description |
|------|-------------|
| [FR-SIM-01](#21-simulation-engine) | Simulator MUST model at minimum: PV, battery, EV, heater, base load |
| [FR-SIM-02](#21-simulation-engine) | Asset model MUST be generic (`Vec<AssetEntry>`) — adding new type requires no core loop changes |
| [FR-SIM-03](#28-simulation-injection--overrides) | PV generation MUST be derived from irradiation: `P_pv = P_max × (irradiance_W_m2 / irradiance_stc)` |
| [FR-SIM-04](#21-simulation-engine) | Battery MUST support bidirectional power, round-trip efficiency, SOC bounds |
| [FR-SIM-05](#21-simulation-engine) | EV MUST support minimum charge rate (1.5 kW), stepless adjustment, 10 s response delay model |
| [FR-SIM-06](#21-simulation-engine) | Heater MUST implement thermal model: `dT/dt = (P_heater × efficiency − ambient_loss) / thermal_mass` |
| [FR-SIM-07](#29-persistence--recovery) | Simulator state MUST persist to `/data/sim_state.json` and survive VEN restart |
| [FR-SIM-08](#21-simulation-engine) | Profile configuration MUST be loaded from `VEN/profiles/<ven-id>.yaml` via `PROFILE_PATH` env var |
| [FR-SIM-09](#28-simulation-injection--overrides) | `POST /sim/override` MUST be a full-replace operation (not a patch) |
| [FR-SIM-10](#210-observability) | `GET /sim/schema` MUST return JSON schema for profile YAML to support tooling |

---

## 3. HTTP API Reference

All endpoints are served by each VEN on its configured port (default: `8211` for ven-1, `8212` for ven-2, `8213` for ven-3). CORS is open to all origins.

### System

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/health` | Health check |
| `GET` | `/metrics` | Prometheus metrics |

### OpenADR State

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/programs` | Currently polled VTN programs |
| `GET` | `/events` | Currently polled VTN events |
| `GET` | `/sensors` | Current sensor snapshot |
| `POST` | `/sensors` | Push external sensor reading |

### Simulation

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/sim` | Full simulator state snapshot |
| `GET` | `/sim/schema` | UI control descriptors per asset |
| `POST` | `/sim/reset/:asset_id` | Reset asset physics to profile defaults |
| `PUT` | `/sim/config/battery` | Update battery physics config |
| `GET` | `/sim/inject` | Current injection overrides |
| `POST` | `/sim/inject` | Apply injection override(s) |
| `POST` | `/sim/inject/reset` | Clear all injection overrides |

### Planning & HEMS

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/plan` | Current active plan (slots, allocations, cost) |
| `PUT` | `/plan/objective` | Change planner objective (cost / emissions / grid) |
| `POST` | `/plan/trigger` | Force immediate replan |
| `GET` | `/plan/events` | SSE stream of planner lifecycle events (solve start/progress/adopted) |
| `GET` | `/tariffs` | Current tariff time series |
| `GET` | `/capacity` | VTN-imposed site capacity limits |
| `GET` | `/obligations` | Active report obligations |
| `GET` | `/flexibility` | Site flexibility envelope |
| `GET` | `/ledger` | Cumulative asset energy / cost / CO₂ |

### User Requests & Device Sessions

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/user-requests` | List all user requests |
| `POST` | `/user-requests` | Create new energy request |
| `DELETE` | `/user-requests/:id` | Cancel a user request |
| `GET` | `/ev-session` | Current EV session state |
| `POST` | `/ev-session` | Start a new EV session |
| `DELETE` | `/ev-session` | End the current EV session |
| `GET` | `/ev-settings` | Get opportunistic EV charging settings |
| `PUT` | `/ev-settings` | Toggle opportunistic EV charging |
| `GET` | `/heater-target` | Get current heater temperature target |
| `POST` | `/heater-target` | Set heater temperature target |
| `DELETE` | `/heater-target` | Clear heater temperature target |
| `GET` | `/shiftable-loads` | List shiftable load tasks |
| `POST` | `/shiftable-loads` | Schedule a shiftable load |
| `DELETE` | `/shiftable-loads/:id` | Cancel a shiftable load task |
| `GET` | `/baseline-override` | Current baseline override |
| `POST` | `/baseline-override` | Set a baseline override period |
| `DELETE` | `/baseline-override` | Clear baseline override |

### Assets

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/forecast/:asset_id` | Forward power forecast for asset |
| `GET` | `/history/:asset_id` | Historical power / state trace |
| `GET` | `/capability/:asset_id` | Current max import / export kW |

### Timeline

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/timeline/all` | Planned trajectories for all assets |
| `GET` | `/timeline/:asset_id` | Planned trajectory for one asset |

### Reports

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/reports` | Aggregated report list |
| `POST` | `/reports` | Create and submit a report |
| `PUT` | `/reports/:id` | Update a report |

### Diagnostics

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/trace/events` | SSE stream of controller events |
| `GET` | `/trace/history` | Last 500 controller events |

---

## 4. Architecture

### 4.1 Philosophy

The VEN backend follows **Hexagonal Architecture** (Ports and Adapters) combined with **Clean Architecture** layering. The core rule is:

> **Inner rings never import outer rings.**

This keeps domain logic free of infrastructure concerns and makes all external dependencies (VTN, solver, simulator) replaceable via traits (ports). The UI is a thin read/write layer on top of the HTTP API with no business logic.

**Key design decisions:**

- **Snapshot-and-release locking**: A lock is acquired, data is cloned, the lock is dropped, then computation (including the potentially multi-second MILP solve) proceeds on the snapshot. This prevents lock contention between the real-time tick loop and the planning loop.
- **No DTO normalization**: Field names from the OpenADR spec flow unchanged through all layers (backend → BFF → UI), reducing vocabulary divergence and debugging overhead.
- **Unit suffixes on physical quantities**: Every variable or field representing a physical quantity carries its unit as a suffix (e.g., `power_kw`, `energy_kwh`, `soc_pct`, `temperature_c`). This is a hard convention.
- **File size limits**: No `VEN/src/` file exceeds 500 lines; `tasks/` files must stay below 200 lines. This enforces single-responsibility at the file level.

> **Reference:** [VEN_ARCHITECTURE.md](docs/architecture/VEN_ARCHITECTURE.md)

### 4.2 Deployment Topology

```
┌─────────────────────────────────────────────────────────┐
│  Raspberry Pi 4  (Docker Compose — Pi4-Server)          │
│                                                         │
│  ┌─────────────────────────────────────────────────┐    │
│  │  VTN Stack                                      │    │
│  │  openleadr-rs (Rust)  ←→  PostgreSQL            │    │
│  │  BFF Proxy            ←→  VTN UI (React)        │    │
│  └─────────────────────────────────────────────────┘    │
│            ↑  /programs  /events  /reports              │
│            │  OAuth 2.0  (HTTP)                         │
│  ┌─────────┴───────────────────────────────────────┐    │
│  │  VEN Stack (×3 independent instances)           │    │
│  │                                                 │    │
│  │  ven-1 :8211   ven-2 :8212   ven-3 :8213       │    │
│  │  (Rust / Axum)                                  │    │
│  │                                                 │    │
│  │  VEN UI (React)  :8214                          │    │
│  └─────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────┘
```

Network: a shared Docker bridge `vtn_openadr-net` connects all containers. State files are volume-mounted at `/data/state.json` per VEN.

> **Reference:** [VEN_ARCHITECTURE.md](docs/architecture/VEN_ARCHITECTURE.md)

### 4.3 Ring Map (Hexagonal Architecture)

```
╔══════════════════════════════════════════════════════╗
║  Adapters  (routes/, tasks/)                         ║
║  ┌────────────────────────────────────────────────┐  ║
║  │  Application  (services/)                      │  ║
║  │  ┌──────────────────────────────────────────┐  │  ║
║  │  │  Domain  (entities/, controller/)        │  │  ║
║  │  │  ┌────────────────────────────────────┐  │  │  ║
║  │  │  │  Infra  (assets/, simulator/,      │  │  │  ║
║  │  │  │          vtn.rs, milp_planner/)    │  │  │  ║
║  │  │  └────────────────────────────────────┘  │  │  ║
║  │  └──────────────────────────────────────────┘  │  ║
║  └────────────────────────────────────────────────┘  ║
╚══════════════════════════════════════════════════════╝
```

**Port obligations (must use traits, never concrete types):**

| Port | Caller | Implementor |
|------|--------|-------------|
| `SimulatorPort` | `services/` | `simulator/` |
| `SolverPort` | `services/` | `controller/milp_planner/` |
| `VtnPort` | `services/` | `vtn.rs` |
| `AssetMilpContext` | `milp_planner/` | per-asset impls in `assets/` |

**Verifiable invariants** (must be empty before any PR):
```sh
grep -r "use crate::profile" VEN/src/entities VEN/src/controller VEN/src/routes
grep -r "use crate::assets::" VEN/src/controller/milp_planner
grep "serde_json::Value" VEN/src/vtn.rs
```

> **Reference:** [VEN_ARCHITECTURE.md](docs/architecture/VEN_ARCHITECTURE.md)

### 4.4 Module Responsibilities

#### `src/main.rs`
Entry point. Constructs `AppState` (all runtime state), `AppCtx` (shared application context — cloned by Axum per request), wires the Axum router, and spawns seven background task loops.

#### `src/state.rs` — AppState
Thread-safe container with three independently locked sections:

| Section | Contents |
|---------|----------|
| `PollingState` | Programs, events, reports from VTN polls |
| `ControllerSimState` | Asset snapshots, simulation state, injection overrides, controller trace ring buffer |
| `HemsState` | Active plan, tariffs, capacity limits, asset ledger, user requests, device sessions |

**Invariant:** No function may hold more than one section lock at a time.

#### `src/entities/` — Domain types
Pure data: no I/O, no async, no framework imports.

- `plan.rs` — `Plan`, `PlanTimeSlot`, `AssetAllocation`, `CostBreakdown`, `PlanWarning`
- `asset.rs` — `AssetType`, `PlanTrigger`, `CompletionPolicy`, `UserRequestMode`
- `asset_params.rs` — `AssetParams` (per-type physics configuration)
- `capacity.rs` — `OadrCapacityState`, `OadrReportObligation`
- `device_session.rs` — `EvSession`, `HeaterTarget`, `ShiftableLoad`, `BaselineOverride`
- `user_request.rs` — `UserRequest`, `UserRequestStatus`, `SessionType`
- `tariff_snapshot.rs` — `TariffTimeSeries` (import/export prices, CO₂)
- `planner_params.rs` — `PlannerObjective`, `PlannerParams`

#### `src/assets/` — Asset physics
One file per asset type. Each implements `capability()` (current kW bounds) and `step()` (ODE advance). Physics constants are injected as typed `*Params` structs constructed in the infra layer — no direct profile imports.

#### `src/simulator/` — Simulation engine
Manages the collection of `AssetEntry` objects; advances physics each tick; maintains 3600-row per-asset history ring buffers (`VecDeque`); persists state to disk.

**On history / forecast ownership and the dual-simulator idea:**

The observation is architecturally sound. Currently `simulator/` owns both the physics step (state update) and the history ring buffers. The `forecast()` method lives on each asset type in `assets/`, which is the right location — it reuses the same ODE but runs forward in time without side effects. The history buffer placement in `simulator/` is a pragmatic colocation choice (the simulator already holds `AssetEntry` per asset, which is the natural place to accumulate history), not a clean domain boundary.

The two-simulator split you describe maps to:
- **Device state simulator** — the current `simulator/` + `assets/*/step()`: advances physics, tracks live state, persists to disk. Replaces real hardware sensors and actuators in the lab context.
- **Device forecast simulator** — the current `assets/*/forecast()` + `controller/timeline.rs`: projects asset state forward under planned setpoints. Required by the MILP planner and the timeline API.

These are already separated at the *code* level (different functions, no shared mutable state). They are not separated at the *module* level — the history buffer lives in the state simulator even though it is read by the forecast path indirectly via snapshots. A future refactor could move `AssetHistoryBuffer` into `assets/` alongside `forecast()`, making the boundary explicit. This would be a clean improvement but is low-priority as long as the current architecture correctly enforces the dependency rule (simulator/ does not import controller/).

#### `src/controller/` — Control domain
The largest domain package. Sub-modules:

| Sub-module | Responsibility |
|------------|---------------|
| `milp_planner/` | HiGHS MILP formulation, two-phase solve, result translation |
| `dispatcher.rs` | Translate plan allocations → per-asset setpoints each tick |
| `absorber.rs` | Tier-1 real-time deviation correction; returns residual for Tier-2 escalation |
| `envelope.rs` | Compute site flexibility envelope |
| `timeline.rs` | Extrapolate per-asset planned trajectories (SoC, temperature, power) |
| `reporter.rs` | Compute interval report payloads |
| `trace.rs` | `ControllerTrace` — 500-entry ring buffer of controller events |
| `openadr_interface.rs` | Parse VTN event signals into domain types |
| `vtn_port.rs` | `VtnPort` trait |
| `simulator_port.rs` | `SimulatorPort` trait + `SimSnapshot` / `GridSnapshot` types |

**Opportunistic load location:** The opportunistic EV surplus-charging overlay lives in `controller/dispatcher.rs` as `apply_surplus_ev_overlay()`, not in `assets/ev.rs`. This is intentional: the surplus calculation requires knowledge of the full site balance (PV output, base load, existing setpoints) which the dispatcher already computes. The `assets/mod.rs` trait defines a `surplus_charge_kw()` method as a per-asset capability hook, but the orchestration belongs in the dispatcher where the site-level surplus is known. For a heater opportunistic pre-heat overlay, the same pattern applies: add a `surplus_heat_kw()` hook to the heater asset and call it from `dispatcher.rs` after the EV overlay. A separate `surplus_orchestrator` module would only make sense if the logic grows complex (e.g., cross-asset surplus arbitration with priority weights), which at that point could reuse the absorber's priority configuration.

#### `src/services/` — Application services
Stateless orchestration; calls ports, updates state.

| Service | Responsibility |
|---------|---------------|
| `planning.rs` | Acceptance gate logic; plan adoption decisions |
| `hems.rs` | EV session lifecycle; heater target updates |
| `user_request.rs` | Create / validate / transition user requests |
| `obligation.rs` | Identify due obligations; mark fulfilled |

#### `src/tasks/` — Background async loops
Each loop is a `tokio::spawn`'d future that runs forever:

| Task file | What it does |
|-----------|-------------|
| `poll_events.rs` | Polls VTN events; parses signals; triggers replan on rate change |
| `poll_programs.rs` | Polls VTN programs |
| `poll_reports.rs` | Polls VTN report confirmations |
| `sim_tick/tick.rs` | Main simulation + dispatch + absorption loop (1 s default) |
| `planning.rs` | MILP planning loop (5 min default; immediate on hard trigger) |
| `obligation.rs` | Report obligation fulfillment loop |
| `state_persist.rs` | Periodic state-to-disk serialisation |

#### `src/routes/` — HTTP adapters
Axum handlers. Extract state from `AppCtx`, delegate to services or state accessors, return JSON. No business logic. Modules mirror the API surface (system, events, sim, hems, assets, timeline, reports, trace).

> **Reference:** [VEN_ARCHITECTURE.md](docs/architecture/VEN_ARCHITECTURE.md)

#### `src/vtn.rs` — VTN HTTP client
OAuth2 bearer-authenticated `reqwest` client. Implements `VtnPort`. Token cached in `Arc<RwLock<Option<Token>>>` with auto-refresh.

#### `src/profile.rs` — Configuration / YAML profile
Loads asset physics parameters, planner weights, poll intervals, and grid limits from a YAML file at startup. Profile values are injected into inner layers as typed structs — never imported directly by domain code.

#### `src/common/` — Shared utilities
`TimeSeries` with `Linear` / `Step` interpolation; `time_weighted_mean`; `Aggregation` enum.

### 4.5 Background Tasks

Seven `tokio::spawn` loops run concurrently:

```
┌────────────┐   ┌──────────────┐   ┌────────────┐
│poll_events │   │poll_programs │   │poll_reports│
│  (30 s)    │   │(30 s default)│   │  (60 s)    │
└─────┬──────┘   └──────────────┘   └────────────┘
      │  rate-change / alert signal
      │  → watch channel send
      ▼
┌─────────────────────────────────────────────────┐
│  planning loop  (300 s periodic + hard trigger) │
│   1. snapshot state (clone, drop lock)          │
│   2. solve MILP in blocking thread              │
│   3. acceptance gate                            │
│   4. adopt plan → write HemsState               │
└────────────────────┬────────────────────────────┘
                     │ new plan written
                     ▼
┌─────────────────────────────────────────────────┐
│  sim_tick loop  (1 s)                           │
│   Phase 1: apply one-shot injections            │
│   Phase 2: build setpoints (dispatcher)         │
│   Phase 3: Tier-1 absorber → correct deviation  │
│   Phase 4: physics tick (step all assets)       │
│   Phase 5: update snapshots, history, envelope  │
│   Phase 6: Tier-2 residual accumulation →       │
│            DeviceDeviation trigger if sustained │
│   Phase 7: measurement reports (if due)         │
│   Phase 8: persist sim state to disk            │
└─────────────────────────────────────────────────┘
      ┌──────────────┐
      │obligation    │
      │check loop    │  → compute report → POST VTN
      └──────────────┘
      ┌──────────────┐
      │state_persist │  → serialize AppState (events,
      │ (if enabled) │    programs, sensor) to JSON
      └──────────────┘
```

Note: `state_persist` is only spawned when `PERSIST_PATH` is set. Sim physics state is persisted inside the tick loop (Phase 8), using the same directory.

> **Reference:** [VEN_ARCHITECTURE.md](docs/architecture/VEN_ARCHITECTURE.md)

### 4.6 State Management & Locking

`AppState` wraps three `Arc<RwLock<…>>` sections. The locking protocol is strict:

1. **Never hold two locks simultaneously** — prevents deadlock by design
2. **Snapshot-and-release** — for expensive operations (MILP solve, report computation):
   ```
   let snapshot = {
       let guard = state.read().await;
       guard.data.clone()
   }; // lock dropped here
   expensive_compute(snapshot);
   ```
3. **Read-heavy** — `RwLock` allows many concurrent readers; writers are rare (plan adoption, tick update)

> **Reference:** [VEN_ARCHITECTURE.md](docs/architecture/VEN_ARCHITECTURE.md)

### 4.7 Control Flow End-to-End

A complete demand-response cycle:

```
VTN issues new PRICE event
        │
        ▼
poll_events detects rate change
        │
        ├── parses price signals → HemsState.planned_tariffs
        └── sends RateChange watch signal → planning loop wakes immediately
                │
                ▼
        planning loop snapshots HemsState + SimState (clone, drop lock)
                │
                ▼
        MILP solve (HiGHS, blocking Tokio thread)
        [Phase 1: MIP (cost minimisation) → Phase 2: MIP (friction minimisation)]
                │
                ▼
        acceptance_gate() evaluates cost delta vs. current plan
                │
                ▼ (accepted)
        new Plan written to HemsState
                │
                ▼
        sim_tick (every 1 s):
          Phase 2: dispatcher → planned setpoints for current slot
          Phase 3: Tier-1 absorber → correct deviations
                    │ residual uncorrectable?
                    ▼ (sustained N ticks)
          Phase 6: DeviceDeviation watch signal → planning loop wakes
          Phase 7/8: measurement reports + persist sim state
                │
                ▼
        obligation loop:
          due obligations → reporter.compute() → POST /reports to VTN
```

**Per-tick Dispatcher → Asset call chain** (`tasks/sim_tick/`, `controller/dispatcher.rs`, `simulator/mod.rs`):

```
Step 1 — Build setpoints  (dispatcher.rs: build_setpoints())
  ├─ Initialise all assets to their default setpoints
  ├─ Find current slot in active MILP plan
  ├─ Overwrite with plan allocations (battery, EV, heater)
  ├─ Apply heater thermostat override if no plan allocation
  ├─ Enforce EXPORT_CAPACITY_LIMIT on PV (curtailment)
  └─ Apply opportunistic surplus EV charging overlay

Step 2 — Physics tick  (simulator/mod.rs: SimState::tick())
  For each asset in Vec<AssetEntry>:
    ├─ Apply Behaviour B/C environment overrides
    │   (PV irradiance, EV plug state, ambient temp, etc.)
    └─ cfg.step(&state, setpoint_kw, dt_s)  →  (new_state, actual_kw)
        Asset-specific physics:
          Battery  → SoC integration + efficiency losses
          EV       → SoC integration + plug gate + minimum charge rate
          Heater   → thermal ODE dT/dt + thermostat bounds
          PV       → irradiance model + EMA smoothing + export clamp
          BaseLoad → fixed profile lookup

Step 3 — Deviation absorption  (controller/absorber.rs)
  Compare actual_kw vs. planned_kw per asset
  Correct deviations within per-asset headroom
  Sustained residual → fires DeviceDeviation watch → triggers replan

Step 4 — Finalise  (tasks/sim_tick/helpers.rs: finalize_tick_outputs())
  ├─ Push HistoryPoint to each asset's ring buffer
  ├─ Recompute GridState (net_power_kw, import_limit_kw, export_limit_kw)
  └─ Recompute site FlexibilityEnvelope
```

> **Reference:** [VEN_ARCHITECTURE.md](docs/architecture/VEN_ARCHITECTURE.md)

---

### 4.8 VTN Internal Architecture

#### 4.8.1 openleadr-rs VTN Server

The VTN is implemented in Rust (Axum) via the `openleadr-rs` git submodule (`openleadr-rs/`), tracking the fork `TinkerPhu/openleadr-rs`.

**Responsibilities:**
- OAuth2 authorization server — token endpoint is `POST /auth/token` (not `/oauth/token`)
- Full OpenADR 3 API: programs, events, reports, VENs, resources
- Event lifecycle management (create, update, delete)
- Report ingestion and storage

**Database:** PostgreSQL 16 (`vtn-db-1`, host port 8201). SQLx manages a 15-table schema; migrations apply automatically on first boot. The database is not exposed to VENs — only the VTN reads it.

**Token TTL:** 2,592,000 s (30 days). Clients refresh on 401.

**Fixture users (seeded at boot):** `any-business`, `ven-manager`, `user-manager`, `business-1`, `ven-1`.

**Field names:** No DTO normalization — upstream OpenADR field names (`programName`, `venName`, `eventName`, `createdDateTime`) pass through all layers unchanged (backend, BFF, UI).

#### 4.8.2 BFF Dual-Credential Pattern

The VTN BFF (Rust Axum, port 8220) sits between the browser UI and the VTN API. VTN RBAC requires two separate credentials because no single role covers both operator and admin operations:

| Credential | Role | Authorized endpoints |
|-----------|------|---------------------|
| `any-business` | Business operator | `GET/POST/PUT/DELETE /programs`, `/events`, `/reports` |
| `ven-manager` | VEN admin | `GET/POST/PUT/DELETE /vens` |

The BFF holds two independent `VtnClient` instances, one per credential. Each token auto-refreshes on 401. The browser communicates with the BFF via session-scoped API keys, not OAuth credentials.

**Report constraint:** `POST /reports` requires VEN role. The BFF's `any-business` credential cannot create reports. VENs submit reports directly to the VTN; the BFF only proxies report reads.

#### 4.8.3 OpenADR Message Sequences

Six core flows (full sequence diagrams in [`docs/architecture/VTN_ARCHITECTURE.md §4`](docs/architecture/VTN_ARCHITECTURE.md)):

| Flow | Summary |
|------|---------|
| VEN startup | Token fetch → program poll → event poll (all within first 30 s cycle) |
| Event distribution | VEN polls `/events` every 30 s; VTN responds with current event list |
| Event update | VEN detects changed `modificationDateTime`; re-evaluates active DR response |
| Event cancellation | VEN detects deletion on poll; rolls back active DR response; re-plans |
| Token lifecycle | 30-day TTL; VEN refreshes on 401; no proactive refresh |
| Report submission | VEN → `POST /reports` directly to VTN (not via BFF) on obligation schedule |

#### 4.8.4 Docker Network Topology

**Host port mapping:**

| Container | Host Port | Role |
|-----------|-----------|------|
| `vtn-vtn-1` | 8200 | openleadr-rs VTN API |
| `vtn-db-1` | 8201 | PostgreSQL 16 |
| `ven-ven-1-1` | 8211 | VEN instance (ven-1) |
| `ven-ven-2-1` | 8212 | VEN instance (ven-2) |
| `ven-ven-3-1` | 8213 | VEN instance (ven-3) |
| `ven-ui-1` | 8214 | React VEN Web UI |
| `vtn-bff-1` | 8220 | Rust Axum BFF |
| `vtn-ui-1` | 8221 | React VTN UI (nginx) |

**Docker network:** `vtn_openadr-net` (named from compose project `vtn`). VEN compose files join it as `external: true`. Container-to-container DNS uses Docker service names (`vtn`, `ven-1`, etc.). Host access uses `Pi4-Server:<host-port>`.

**Compose layout:**
```
/srv/docker/openadr_lab/
  VTN/   → compose project name: vtn  (VTN, BFF, DB, VTN UI)
  VEN/   → compose project name: ven  (VEN instances, VEN UI)
  tests/ → test compose files
  openleadr-rs/  → git submodule
```

> **Reference:** [VTN_ARCHITECTURE.md](docs/architecture/VTN_ARCHITECTURE.md)

---

### 4.9 Design Decisions

Rationale for key architectural choices, sourced from [`docs/architecture/VEN_ARCHITECTURE.md §6`](docs/architecture/VEN_ARCHITECTURE.md).

**D-01: Two-phase MILP solver (supersedes initial greedy design)**  
The codebase uses a two-phase MILP (HiGHS) for scheduling. VEN_ARCHITECTURE.md §6 D-01 records an earlier design decision to use a priority-based greedy planner ("not LP/MILP"), but this was superseded when the MILP implementation was adopted. The two-phase structure (Phase 1: cost minimisation; Phase 2: friction/switching minimisation under the Phase 1 cost bound) is what is actually implemented. See §2.2 and §2.12.

**D-02: In-memory ledger**  
`AssetLedger` is kept in memory only and resets on restart. Persistent billing-period data is stored at the VTN as reports. Local persistence adds complexity for little benefit in a lab context.

**D-03: Reactor removed (spec kit 001)**  
The reactor FSM (Idle → Delaying → Ramping → Holding → RampingBack) and its arbitration logic were removed because the Dispatcher silently overwrote the reactor's output for any asset with a plan allocation, making the reactor redundant. The controller is now the single control authority. `GET /trace` still exists and records Dispatcher decisions.

**D-04: Generic asset model (spec kit 002)**  
`SimState.assets: Vec<AssetEntry>` with enum dispatch (`AssetState`, `AssetConfig`). The earlier named-field model required touching every layer when adding a new asset type. The generic model isolates new types to their own module — no changes to the simulator loop, API handlers, or profile parser.

**D-05: `OadrEventSnapshot` unification**  
All time-varying VTN signals (price, CO₂, capacity limits) are stored in one struct per poll tick. A separated-field model caused temporal alignment bugs when price and capacity signals had different poll timestamps. The unified struct guarantees all fields are co-valid at the same timestamp.

**D-06: `POST /sim/override` is full-replace**  
The override endpoint replaces the entire override struct (not a PATCH). Partial-patch semantics require null-vs-absent disambiguation that is error-prone for callers. Full-replace is explicit: callers must set all fields they want active.

**D-07: 30 s fixed poll interval**  
Event polling is fixed at 30 s. This balances VTN load against response latency. The 30–60 s range from the original system design was narrowed to 30 s fixed in implementation; configurable jitter is not implemented in the lab.

---

## 5. Configuration Reference

### Environment Variables

The code default for `LISTEN_ADDR` is `0.0.0.0:8080`. Docker Compose maps external ports (8211/8212/8213) to the container's port 8080.

| Variable | Code default | Docker Compose (ven-1) | Description |
|----------|-------------|------------------------|-------------|
| `LISTEN_ADDR` | `0.0.0.0:8080` | `0.0.0.0:8080` | Axum bind address |
| `VTN_BASE_URL` | — (required) | `http://vtn:3000` | VTN base URL |
| `CLIENT_ID` | — (required) | `ven-1` | OAuth2 client ID |
| `CLIENT_SECRET` | — (required) | `ven-1` | OAuth2 client secret |
| `VEN_NAME` | `ven-1` | `ven-1` | VEN identifier (sent in reports) |
| `PROFILE_PATH` | unset | `/config/profile.yaml` | Asset/planner YAML profile |
| `PERSIST_PATH` | unset | `/data/state.json` | State persistence file (disables persistence if unset) |
| `POLL_EVENTS_SECS` | `30` | `30` | VTN event poll interval (s) |
| `POLL_PROGRAMS_SECS` | `30` | `300` | VTN program poll interval (s) |
| `POLL_REPORTS_SECS` | `60` | unset (uses default) | VTN report poll interval (s) |
| `RUST_LOG` | `info` | `info` | Tracing log level |

### Profile YAML (key sections)

Profile files live in `VEN/profiles/` (one per VEN). Below is a representative excerpt from `ven-1.yaml`:

```yaml
assets:
  - type: ev
    id: ev
    max_charge_kw: 7.4
    initial_soc: 0.40
    battery_kwh: 60.0
    soc_target: 0.80
  - type: pv
    id: pv
    rated_kw: 8.0
  - type: battery
    id: battery
    capacity_kwh: 10.0
    max_charge_kw: 5.0
    max_discharge_kw: 5.0
    initial_soc: 0.50
    round_trip_efficiency: 0.92
    min_soc: 0.10
  - type: base_load
    id: base_load
    baseline_kw: 0.4

simulator:
  tick_s: 1
  persist_every_s: 15
  report_interval_s: 60

planner:
  plan_adoption_threshold_eur: 0.20
  replan_interval_s: 300
  deviation_trigger_ticks: 120

absorber:
  enabled: true
  dead_band_kw: 0.1
  dead_band_clearing_ticks: 1
  assets:
    - id: battery
      priority: 0
      min_state_linger_s: 0
    - id: ev
      priority: 1
      min_state_linger_s: 0
      ev_departure_guard_s: 1800
```

Note: not all VENs carry all asset types. VEN-1 has EV + PV + battery + base load (no heater). VEN-2 has heater + PV. Assets not present in the profile are simply absent from the simulation.

**Heater asset profile parameters** (ven-2.yaml):

```yaml
assets:
  - type: heater
    id: heater
    max_kw: 6.0          # full power tier
    mid_kw: 3.0          # mid power tier (0 / mid / max are the three discrete levels)
    temp_initial_c: 60.0
    temp_min_c: 40.0     # tank hysteresis lower bound
    temp_max_c: 80.0     # tank hysteresis upper bound
    thermal_mass_kwh_per_c: 2.3   # thermal capacity of the tank
    k_loss_kw_per_c: 0.05         # standby heat loss rate
```

| Parameter | Description | Default |
|-----------|-------------|---------|
| `max_kw` | Full-tier electrical power (kW) | required |
| `mid_kw` | Mid-tier electrical power (kW) | required |
| `temp_min_c` | Minimum tank temperature — comfort lower bound (°C) | required |
| `temp_max_c` | Maximum tank temperature — safety upper bound (°C) | required |
| `temp_initial_c` | Initial tank temperature at sim start (°C) | required |
| `thermal_mass_kwh_per_c` | Tank thermal capacity (kWh/°C) | required |
| `k_loss_kw_per_c` | Standby heat loss rate (kW/°C) | required |
| `min_run_slots` | Minimum consecutive 5-min slots heater must stay ON after a switch | planned — not yet a YAML parameter; hardcoded default = 0 (no minimum) |
| `min_off_slots` | Minimum consecutive 5-min slots heater must stay OFF after a switch | planned — not yet a YAML parameter; hardcoded default = 0 (no minimum) |

`min_run_slots` and `min_off_slots` model compressor protection for heat-pump assets — once started, a compressor must run for a minimum block (e.g., `3` slots = 15 min) to avoid damage. For a purely resistive heater or boiler, set both to `0`. These parameters are described in §2.4; implementation as YAML-configurable fields is planned.

---

## 6. Deployment

### Running on Pi4-Server

All Docker operations run via SSH on the Pi4-Server in `/srv/docker/openadr_lab`.

**Start the full stack:**
```sh
ssh pi4-server
cd /srv/docker/openadr_lab
docker compose up -d
```

**View VEN logs:**
```sh
docker compose logs -f ven-1
```

**Rebuild after code change:**
```sh
docker compose build ven
docker compose up -d ven-1 ven-2 ven-3
```

**Never stop containers outside this project** — other productive containers share the host.

### Local Rust Development (WSL)

For local `cargo check` and unit tests (no HiGHS integration):
```sh
wsl cargo check          # inside VEN/
wsl cargo test           # unit tests only
```

For full integration tests including HiGHS, use the Pi4-Server Docker stack.

### VEN UI

The React UI is served from the same Docker Compose stack on port 8214. It connects to all three VEN instances and provides:
- Per-asset control sliders and switches
- Real-time plan and timeline visualisation
- Event / program monitoring
- Ledger and cost tracking
- Controller trace diagnostics

### VEN Provisioning

VENs are provisioned via the VTN admin API. Four steps, three different credential roles:

```
Step 1 — Create user account (user-manager role)
  POST /users
  body: { "reference": "ven-1", "description": "VEN 1", "roles": [] }
  → returns { "id": "<user-uuid>" }

Step 2 — Add OAuth credential to user (user-manager role)
  POST /users/{user-uuid}/credentials
  body: { "client_id": "ven-1", "client_secret": "ven-1" }

Step 3 — Create VEN entity (ven-manager role)
  POST /vens
  body: { "venName": "ven-1" }
  → returns { "id": "<ven-uuid>" }

Step 4 — Assign VEN role to user (user-manager role)
  PUT /users/{user-uuid}
  body: {                             ← FULL body required (not a patch)
    "reference": "ven-1",
    "description": "VEN 1",
    "roles": [{ "role": "VEN", "id": "<ven-uuid>" }]
  }
```

**Important:** Step 4 is a full-replace PUT. The `roles` array must include all roles, not just the new one. The VTN does not support PATCH on users.

**VEN identity model:**
- `ven_id` — stable UUID assigned at `POST /vens`
- OAuth `client_id` / `client_secret` — used for token acquisition
- `venName` — human-readable name, used in event `targets` filtering

**Target filtering:** Programs and events with `targets: [{ type: "VEN_NAME", values: ["ven-1"] }]` are visible only to the named VEN(s). Programs/events with `targets: null` are open to all VENs.

> **Reference:** [VTN_ARCHITECTURE.md §5](docs/architecture/VTN_ARCHITECTURE.md)

---

## 7. Testing

### BDD Integration Tests (Behave + Playwright)

Located in `tests/features/`. Run against a live Docker Compose stack.

**Coverage areas:**
- VTN authentication and health
- VEN event and program polling
- Enrollment lifecycle
- Asset simulation physics (EV, heater, battery)
- User request creation and completion
- Timeline, reporting, and obligation fulfilment
- UI end-to-end scenarios (Playwright)

**Running:**
```sh
cd tests
behave features/ven_polling.feature    # single feature
behave                                  # all features
```

### Rust Unit Tests

```sh
wsl cargo test -p ven   # inside VEN/
```

Unit test coverage includes:
- Physics models (battery SoC, heater thermal ODE)
- MILP solver formulation and result parsing
- Planner acceptance gate
- Report obligation scheduling
- Controller absorber dead-band logic

### Architecture Invariant Checks

Run before any VEN PR to verify ring-map compliance:

```sh
# Must return empty — no profile imports in inner rings
grep -r "use crate::profile" VEN/src/entities VEN/src/controller VEN/src/routes

# Must return empty — no concrete asset imports in MILP planner
grep -r "use crate::assets::" VEN/src/controller/milp_planner

# Must return empty — no raw JSON Values in VTN client
grep "serde_json::Value" VEN/src/vtn.rs
```
