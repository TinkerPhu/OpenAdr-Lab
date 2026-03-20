# VEN Controller (HEMS) — Full Implementation Plan

## Context

The current VEN is a **reactive event follower**: it reads the active OpenADR event, applies a
mode (PRICE/IMPORT_CAP/etc.), ramps setpoints via an FSM, and reports current state. There is no
planning horizon, no scheduling, no user preferences, no cost optimization.

The `docs/VEN_Controller/` plan (Steps 1–5) describes a full **HEMS (Home Energy Management
System)**: a proactive, optimizing controller that schedules energy tasks over 24-48h, uses
comfort-value bids to balance user priorities against cost and CO2, coordinates with the VTN on
capacity, and exposes flexibility envelopes for VTN dispatch optimization.

This plan implements all five steps in six sequential stages, each test-first using the existing
Behave infrastructure. The current VEN source is aggressively replaced where it conflicts — only
the device physics, HTTP client, and FSM ramp logic survive unmodified.

---

## What to Keep vs. Replace

### Keep (reuse as-is or extend)

| File | Reason |
|---|---|
| `VEN/src/vtn.rs` | VtnClient with token, retry, 401 handling — solid, just extend |
| `VEN/src/config.rs` | Env config — keep, add new env vars |
| `VEN/src/simulator/actors.rs` | EvCharger, Heater, PvInverter physics — keep, add Battery |
| `VEN/src/simulator/power_model.rs` | compute_net_power — keep |
| `VEN/src/simulator/energy.rs` | EnergyCounter — keep |
| `VEN/src/simulator/persist.rs` | Atomic save/load — keep |
| `VEN/src/reactor/interval.rs` | parse_duration_secs + interval timing — keep |
| `VEN/src/reactor/fsm.rs` | FsmState, ReactorFsm ramp logic — keep as Dispatcher's ramp |
| `VEN/src/reactor/trace.rs` | DecisionTrace ring buffer — keep, extend |
| `tests/features/helpers/` | All helpers (api_client, wait, docker_ctl, ui) |
| `tests/features/steps/` | All existing step definitions |
| `tests/docker-compose.test.yml` | Test stack — extend with new services if needed |

### Replace/Rewrite

| File | Replacement |
|---|---|
| `VEN/src/reactor/mod.rs` | → `controller/dispatcher.rs` (Plan-driven execution) |
| `VEN/src/reactor/arbitration.rs` | → `controller/planner.rs` (CalcCache + greedy algorithm) |
| `VEN/src/reporter.rs` | → `controller/openadr_interface.rs` (full obligation tracking) |
| `VEN/src/state.rs` | → extend with Plan, Packets, Rates, Obligations state |
| `VEN/src/models.rs` | → `entities/` module tree (all domain types) |
| `VEN/src/profile.rs` | → extend with full AssetProfile, DefaultValueCurve, PowerAdjustability |
| `VEN/src/main.rs` | → refactored dual-loop (5s dispatch + 5min plan) |

### New Modules

```
VEN/src/
  entities/
    asset.rs              — AssetProfile, AssetState, AssetForecast, AssetFlexibility,
                            AssetLedger, AssetHeuristics
    energy_packet.rs      — EnergyPacket, ValueCurve, DeadlineTier, ComfortRate, PacketStatus
    rate_snapshot.rs      — RateSnapshot, PlannedRates, PastRates, RateHeuristic
    plan.rs               — Plan, PlanTimeSlot, PacketAllocation, FlexibilityEnvelope,
                            PlanWarning, CalcCache
    capacity.rs           — OadrCapacityState, OadrProgramConfig, OadrEventCache,
                            OadrReportObligation
    site_meter.rs         — SiteMeter, AssetLedger, PastEnergySum, DispatchState, DeviceSession
  controller/
    planner.rs            — 8-phase planning algorithm (Step 4)
    dispatcher.rs         — 5s dispatch loop, DeviceSession, setpoint execution
    monitor.rs            — deviation detection, AssetLedger, penalty rules, triggers
    openadr_interface.rs  — event translation, report obligations, capacity reservation
    user_request.rs       — EnergyPacket creation API, UserNotification
```

---

## Stage 1 — Entity Model + Simulator Foundation

**Goal:** All domain types from Step 1 compile and are persisted. Battery actor added. Profile
extended. No behavior changes yet — existing endpoints still work.

### BDD Tests (write first)

Feature: `tests/features/ven_entity_model.feature`
- GET /sim includes `battery` field when battery configured in profile
- Battery SoC charges/discharges bidirectionally
- Profile YAML loads PowerAdjustability, DefaultValueCurve fields
- GET /packets returns empty list (placeholder endpoint)
- GET /plan returns null (no plan yet)

### Implementation

1. **`entities/` module** — Implement all structs from Step 1:
   - `AssetProfile` (PowerRange, PowerAdjustability, DefaultValueCurve, ThermalModelParams, MinSoC)
   - `EnergyPacket`, `ValueCurve`, `DeadlineTier`, `ComfortRate`, `PacketStatus` enum
   - `RateSnapshot`, `PlannedRates`, `PastRates`
   - `Plan`, `PlanTimeSlot` (FIRM/FLEXIBLE), `PacketAllocation`, `FlexibilityEnvelope`, `PlanWarning`
   - `OadrCapacityState`, `OadrReportObligation`, `OadrEventCache`
   - `SiteMeter`, `AssetLedger`, `DispatchState`, `DeviceSession`
   - All enumerations: `PowerAdjustability`, `DeviceResponsiveness`, `RequestMode`,
     `CompletionPolicy`, `PlanTrigger`

2. **`simulator/actors.rs`** — Add `Battery` actor:
   - Bidirectional storage: SoC (0..1), MaxCharge_kW, MaxDischarge_kW, Capacity_kWh, RoundTripEfficiency
   - `update(dt_s, commanded_kw) -> f64` — positive=charge, negative=discharge
   - Hard stops at 0%/100% SoC, clamped to MinSoC from profile

3. **`profile.rs`** — Extend:
   - Add `battery: Option<BatteryConfig>` to `DeviceConfig`
   - Add `PowerAdjustability` to each device config
   - Add `default_value_curve: Option<Vec<ComfortRate>>` per asset
   - Add `thermal_model: Option<ThermalModelParams>` to HeaterConfig

4. **`simulator/mod.rs`** — Add Battery to SimState, SimSnapshot, tick()

5. **`state.rs`** — Extend AppState with:
   - `active_packets: Vec<EnergyPacket>`
   - `active_plan: Option<Plan>`
   - `planned_rates: Vec<RateSnapshot>`
   - `capacity_state: OadrCapacityState`
   - `report_obligations: Vec<OadrReportObligation>`
   - `past_energy_sum: VecDeque<PowerSnapshot>`
   - `asset_ledgers: HashMap<String, AssetLedger>`

6. **`main.rs`** — Add stub routes: `GET /packets`, `GET /plan`, `GET /rates`

### Files Modified
- `VEN/src/entities/` (new module tree)
- `VEN/src/simulator/actors.rs` (add Battery)
- `VEN/src/simulator/mod.rs` (add battery to SimState/SimSnapshot/tick)
- `VEN/src/profile.rs` (extend with battery, PowerAdjustability, DefaultValueCurve)
- `VEN/src/state.rs` (extend AppState)
- `VEN/src/main.rs` (add stub routes)
- `VEN/Cargo.toml` (no new deps expected)
- `VEN/profiles/ven-*.yaml` (add battery sections where applicable)

---

## Stage 2 — OpenADR Interface + Rate System

**Goal:** Parse multi-interval rate events into a RateSnapshot array. Track report obligations.
Expand reporting to USAGE_FORECAST, DEMAND, STORAGE_*, capacity. The Planner does not exist yet
— rate data is stored and exposed via API; the existing reactor still runs in parallel.

### BDD Tests (write first)

Feature: `tests/features/ven_rate_system.feature`
- VEN with day-ahead 24-interval PRICE event → GET /rates returns 24 rate snapshots
- VEN with GHG event → rates include CO2 values
- VEN with EXPORT_PRICE event → rates include export price
- Event with reportDescriptors → GET /obligations returns pending obligations
- After obligation DueAt passes → report POSTed to VTN with correct type
- IMPORT_CAPACITY_LIMIT event → OadrCapacityState import limit updated
- GET /capacity returns current subscription/reservation/limit state

### Implementation

1. **`controller/openadr_interface.rs`** — Full OpenADR Interface:
   - `parse_rate_snapshots(event) -> Vec<RateSnapshot>` — extract all intervals from PRICE/EXPORT_PRICE/GHG events
   - `parse_capacity_event(event) -> OadrCapacityState` — IMPORT/EXPORT_CAPACITY_LIMIT/SUBSCRIPTION/RESERVATION
   - `extract_report_obligations(event) -> Vec<OadrReportObligation>` — from reportDescriptors
   - `build_usage_report(obligation, past_energy_sum) -> Value`
   - `build_demand_report(obligation, asset_states) -> Value`
   - `build_forecast_report(obligation, plan) -> Value`
   - `build_storage_report(obligation, sim_state) -> Value`
   - `build_capacity_request(flexibility) -> Value`
   - `process_alert_event(event) -> PlanTrigger`
   - `process_dispatch_setpoint(event) -> DispatchOverride` (bypasses planner)
   - Multi-program conflict resolution: lowest price wins; strictest capacity limit wins

2. **Event polling loop** — Extend in `main.rs`:
   - After fetching events: call `openadr_interface.process_events(events)`
   - Updates `planned_rates`, `capacity_state`, `report_obligations` in AppState
   - PastRates: move RateSnapshots to past as their TimeStamp passes

3. **Obligation tick** — New loop in `main.rs` checks DueAt every 5s:
   - Build appropriate report type → POST via VtnClient
   - Mark obligation Fulfilled
   - Dedup by shortest interval when multiple programs overlap

4. **Routes** — Add: `GET /rates`, `GET /obligations`, `GET /capacity`

5. **`VEN/src/vtn.rs`** — Extend: `fetch_events_by_program(program_id)` for per-program polling

### Files Modified
- `VEN/src/controller/openadr_interface.rs` (new)
- `VEN/src/main.rs` (extend polling loops, obligation tick, add routes)
- `VEN/src/state.rs` (obligation/rate accessors)
- `VEN/src/vtn.rs` (extend)
- `tests/features/ven_rate_system.feature` (new)
- `tests/features/steps/rate_steps.py` (new)

---

## Stage 3 — EnergyPacket + Planner (Algorithm)

**Goal:** The 8-phase planning algorithm (Step 4) produces a Plan from RateSnapshots +
EnergyPackets. The Dispatcher is still the old reactor — plan output is stored but not yet
executed. EnergyPackets are hardcoded in profiles (not yet user-created). Focus is correctness
of the optimization.

### BDD Tests (write first)

Feature: `tests/features/ven_planner.feature`
- Profile with EV packet (LatestEnd=+8h, TargetSoC=80%) + flat PRICE event → Plan allocates EV to FIRM slots
- High-price event → EV packet deferred to after high-price window
- Low-price event → EV packet prioritized in cheap slots
- PV-only profile + EV packet → PV surplus fills EV before grid import
- Battery profile + PV surplus → surplus stored in battery (when import price > export price)
- Battery profile → discharges in expensive slots (arbitrage)
- FlexibilityEnvelope generated for unallocated far-horizon energy
- Tier fallback: EV not completable in tier-1 budget → falls to tier-2

### Implementation

1. **`controller/planner.rs`** — 8-phase algorithm from Step 4:

   **Phase 1 PREPARE** — Build planning grid
   - PlanTimeSlot[] from now to max(MinPlanTime, furthest packet deadline)
   - Classify FIRM (≤ FirmBoundary = now + NearHorizonDuration, default 4h) vs FLEXIBLE
   - Urgency override: packets with TimePressure ≥ 2.0 force their slots FIRM
   - Populate each slot: ImportPrice, ExportPrice, CO2Rate, ImportCapacityLimit
   - StaleRatePolicy: LAST_KNOWN (default) | HEURISTIC_FORECAST | DEFER_TO_FLEXIBLE
   - GridEffectiveCost = ImportPrice + (CO2Rate × CO2Weight) for all slots
   - Baseline: sum AssetForecast.Profile[] for uncontrollable assets

   **Phase 2 SCORE** — CalcCache per (packet, FIRM slot)
   - SurplusForPacket_kW: PV surplus available after baseline subtraction
   - EffectiveCost: blended (surplus → ExportPrice as opp. cost; grid → ImportPrice + CO2)
   - ProjectedFill: (PastEnergy + planned energy in prior slots) / TargetEnergy
   - ComfortBid: linear interpolation of ComfortRate[] at ProjectedFill
   - TimeSlack = FirmSlotsUntilDeadline − SlotsNeededToComplete
   - TimePressure: 3.0 (≤0 slack) | 2.0 (1 slot) | 1.5 (≤3 slots) | 1.0 (comfortable)
   - MarginalValue = ComfortBid × TimePressure
   - Eligibility: EffectiveCost ≤ ComfortBid AND ≤ ActiveTier.MaxMarginalRate AND within budget

   **Phase 3 ALLOCATE CONSUMPTION** — Greedy fill FIRM slots
   - Sort eligible (packet, slot) pairs by MarginalValue descending
   - Allocate power top-down until ImportCapacityLimit / AssetPowerRange exhausted
   - Write PacketAllocation[] per FIRM slot

   **Phase 4 ALLOCATE STORAGE** — Battery arbitrage
   - Identify cheap charge slots and expensive discharge slots
   - Round-trip efficiency gate: discharge_price × efficiency > charge_price
   - Clamp to available SoC headroom / power limits

   **Phase 5 RESIDUAL PV SURPLUS** — Export unclaimed surplus at ExportPrice

   **Phase 6 PENALTY CHECK** — PenaltyRule rolling average check; reschedule if avoidance cheaper than breach

   **Phase 7 FLEXIBILITY ENVELOPES** — Far-horizon FLEXIBLE slots
   - Per packet with unallocated energy: FlexibilityEnvelope(energy_needed, power_range, time_window, rate_range)

   **Phase 8 FINALIZE**
   - Write Plan (FIRM slots + FlexibilityEnvelopes)
   - Update EnergyPacket.PlannedPowerProfile, EstimatedCost, EstimatedCO2, EstimatedCompletion
   - Detect PlanWarnings (budget threshold, tier infeasibility, stale rates)

2. **`entities/energy_packet.rs`** — EnergyPacket lifecycle:
   - PENDING → SCHEDULED (Planner)
   - SCHEDULED → ABANDONED (Planner, infeasible)
   - Status ownership strictly enforced (Step 3 §9)

3. **Plan cycle** in `main.rs`:
   - 5-minute periodic loop + trigger-based (RATE_CHANGE, USER_REQUEST, ALERT)
   - ReplanCooldown: 30s (skipped for ALERT)
   - `planner.run(rates, packets, assets, capacity, now) -> Plan`

4. **AssetForecast** — simplified for Stage 3:
   - PV: sinusoidal irradiance model projected over planning horizon
   - EV: always available (no disconnect model yet)
   - Heater: constant load from ThermalModelParams + ambient_temp_c
   - SITE_RESIDUAL: constant baseline from profile

5. **Profile-seeded packets** (YAML):
   ```yaml
   packets:
     - asset: ev
       target_soc: 0.80
       latest_end: "+8h"
       comfort_rates:
         - {fill: 0.0, bid: 0.35}
         - {fill: 1.0, bid: 0.05}
   ```

6. **Routes**: `GET /plan`, `GET /packets`

### Files Modified
- `VEN/src/controller/planner.rs` (new)
- `VEN/src/entities/energy_packet.rs` (full lifecycle)
- `VEN/src/entities/plan.rs` (Plan, PlanTimeSlot, PacketAllocation, FlexibilityEnvelope)
- `VEN/src/entities/rate_snapshot.rs` (full RateSnapshot)
- `VEN/src/main.rs` (5min plan loop, plan/packets routes)
- `VEN/src/state.rs` (Plan storage, packet list)
- `VEN/src/profile.rs` (add packets section)
- `VEN/profiles/ven-*.yaml` (add packet definitions)
- `tests/features/ven_planner.feature` (new)
- `tests/features/steps/planner_steps.py` (new)

---

## Stage 4 — Dispatcher + Monitor

**Goal:** Replace the old reactor with a Dispatcher that executes FIRM plan slots. Add Monitor
for deviation detection, AssetLedger, and PlanTrigger emission. The two-speed loop is fully
operational: 5s dispatch + 5min plan. System now behaves as a real HEMS.

### BDD Tests (write first)

Feature: `tests/features/ven_dispatcher.feature`
- Plan allocates EV 7kW in FIRM slot → sim EV charges at 7kW within that slot
- Heater at thermostat max → Dispatcher respects physical limit (clamps setpoint)
- EV SoC reaches target → EnergyPacket status becomes COMPLETED
- EV unplugged mid-charge → DeviceSession FAILED, packet FAILED, PlanTrigger emitted
- SITE_RESIDUAL deviation > threshold sustained → PlanTrigger.DEVICE_DEVIATION → replan
- AssetLedger accumulates kWh per asset per billing period
- POST /packets creates EnergyPacket → appears in next plan cycle

### Implementation

1. **`controller/dispatcher.rs`** — Replaces `reactor/mod.rs`:
   - 5s tick: reads `ActivePlan.current_slot()` → DispatchCommand per PacketAllocation
   - Auto-follow compensation: NetDeviation = Σ(ActualPower) − Σ(PlannedPower) → distribute
   - DeviceSession: created on SCHEDULED→ACTIVE; tracks AccumulatedCost_EUR, AccumulatedCO2_g
   - COMPLETED: PastEnergy ≥ TargetEnergy
   - PARTIAL_COMPLETED: now ≥ LatestEnd AND CompletionPolicy=STOP AND fill < 1.0
   - FAILED: asset OFFLINE
   - PAUSED: VTN DISPATCH_SETPOINT override or explicit user pause
   - Reuses `reactor/fsm.rs` for setpoint ramping within each slot

2. **`controller/monitor.rs`** — New:
   - SITE_RESIDUAL = SiteMeter.NetImport − Σ(other assets' ActualPower)
   - Deviation detection: |NetDeviation| > threshold sustained N ticks → PlanTrigger.DEVICE_DEVIATION
   - AssetLedger: per-asset cumulative TotalConsumption_kWh, TotalImportCost_EUR, TotalCO2_g
   - Monthly rollover: archive ledger, create new
   - PenaltyRule: configurable peak demand threshold + MeasurementWindow RollingAverage breach check
   - LatestStart timeout: PENDING packet past LatestStart → ABANDONED
   - StaleContinue timeout: CONTINUE packet no progress → ABANDONED
   - PastEnergySum: append net_import each dispatch tick; VecDeque retained 90 days

3. **Main loop refactor** — `main.rs`:
   - Remove `Reactor::evaluate()` call entirely
   - 5s loop: `dispatcher.tick()` then `monitor.tick()`
   - PlanTrigger channel via `tokio::sync::watch<PlanTrigger>`
   - Plan loop: `tokio::select!` on trigger OR 5-min timer → `planner.run()`

4. **`POST /packets`** — Raw EnergyPacket creation:
   ```json
   { "asset_id": "ev", "target_soc": 0.8, "latest_end": "2026-03-09T07:00:00Z",
     "comfort_rates": [...], "completion_policy": "STOP" }
   ```
   Emits PlanTrigger.USER_REQUEST

5. **Routes**: `GET /ledger`, `GET /dispatch-state`, `GET /site-meter`

6. **Retire**: `VEN/src/reactor/mod.rs`, `VEN/src/reactor/arbitration.rs` (delete)

### Files Modified
- `VEN/src/controller/dispatcher.rs` (new — replaces reactor/mod.rs)
- `VEN/src/controller/monitor.rs` (new)
- `VEN/src/reactor/mod.rs` (deleted)
- `VEN/src/reactor/arbitration.rs` (deleted)
- `VEN/src/main.rs` (dual-loop refactor, trigger channel, POST /packets)
- `VEN/src/state.rs` (ledger, past_energy_sum, dispatch_state accessors)
- `tests/features/ven_dispatcher.feature` (new)
- `tests/features/steps/dispatcher_steps.py` (new)

---

## Stage 5 — User Request Manager + Full OpenADR Coordination

**Goal:** Complete User Request Manager with proper ValueCurve building and UserNotifications.
Complete OpenADR outbound (flexibility reports, capacity reservation). Full AssetForecast with
heuristics. VEN UI updated with schedule and notification views.

### BDD Tests (write first)

Feature: `tests/features/ven_user_request.feature`
- POST /requests with EV + deadline + max_price → EnergyPacket created, plan runs, GET /requests shows estimate
- Multi-tier request (tonight for €5, Friday for €1) → single packet with 2 DeadlineTiers
- Cancel request (DELETE /requests/:id) → packet ABANDONED, plan reruns
- CHARGE_STATE_SETPOINT VTN event → EnergyPacket created or existing packet modified
- Request for NONE-adjustability asset (stove) → rejected with explanation

Feature: `tests/features/ven_flexibility.feature`
- FlexibilityEnvelope in plan → GET /flexibility returns envelope
- VTN capacity shortfall detected → capacity reservation report POSTed to VTN
- VTN CAPACITY_RESERVATION grant → OadrCapacityState updated → plan reruns → slot allocation expands
- ALERT_GRID_EMERGENCY event → synthetic high-priority EnergyPacket → immediate replan (skips cooldown)

### Implementation

1. **`controller/user_request.rs`** — User Request Manager:
   - `POST /requests` body:
     ```json
     { "asset_id": "ev",
       "request_mode": "BY_DEADLINE",
       "deadlines": [
         { "latest_end": "2026-03-09T07:00:00Z", "max_total_cost": 5.0, "max_marginal_rate": 0.35 },
         { "latest_end": "2026-03-13T18:00:00Z", "max_total_cost": 1.0, "max_marginal_rate": 0.15 }
       ],
       "completion_policy": "STOP",
       "comfort_rates": [ ... ]   // optional — overrides DefaultValueCurve
     }
     ```
   - Validation: asset exists, IsConnected, PowerAdjustability ≠ NONE/RECOMMENDATION for control
   - TargetEnergy from TargetSoC: `(TargetSoC − CurrentSoC) × Capacity_kWh / Efficiency`
   - TargetEnergy from ThermalModelParams: computed + refreshed each plan cycle
   - Multi-deadline → one EnergyPacket with DeadlineTier[] (never multiple packets per asset)
   - CONTINUE packets: implicit post-deadline tier appended
   - ValueCurve: DefaultValueCurve from AssetProfile, user ComfortRates override
   - UserNotification: initial estimate; tier fallback warning; budget approach warning
   - CHARGE_STATE_SETPOINT handling: modify existing packet or create new with DefaultValueCurve
   - Routes: `GET /requests`, `POST /requests`, `PUT /requests/:id`, `DELETE /requests/:id`, `GET /notifications`

2. **AssetForecast — full implementation** in Planner:
   - EV: AvailabilityWindows from profile (`connect_at`, `disconnect_at`)
   - Heater/HeatPump: outdoor temp from ExternalDataSource or `ambient_temp_c` config override
   - COOKING_STOVE/WASHING_MACHINE: DaytimeProfile heuristic (loaded from AssetHeuristics)
   - SITE_RESIDUAL: daily learned profile from PastEnergySum (UpdateHeuristics nightly 23:55)
   - ExternalDataSource: configurable HTTP API fetch; fallback to fixed config value

3. **Capacity reservation flow** in OpenADR Interface:
   - Post-plan: scan PlannedEnergySum for far slots exceeding ImportSubscription
   - If shortfall > 0 AND CAPACITY_AVAILABLE event seen: cost/benefit → POST reservation report
   - On VTN CAPACITY_RESERVATION event → update OadrCapacityState → PlanTrigger.CAPACITY_CHANGE

4. **Full flexibility reporting**:
   - UP_REGULATION_AVAILABLE: CanDecreaseConsumption + CanIncreaseProduction per asset
   - DOWN_REGULATION_AVAILABLE: Σ FlexibilityEnvelope.MaxPower + CanIncreaseConsumption
   - USAGE_FORECAST: FIRM slots → point forecast; FLEXIBLE → range [0, MaxPower] in window

5. **Batch consumer actors** in `simulator/actors.rs`:
   - `WashingMachine`: ON/OFF, fixed 1 kWh per 2h cycle, RECOMMENDATION mode
   - `CookingStove`: NONE adjustability, observe-only heuristic load profile

6. **VEN UI** — `VEN/ui/`:
   - "Schedule" tab: EnergyPacket list (status, fill%, estimated cost/CO2, deadline)
   - Create packet form (asset, deadline, budget, tier options)
   - FIRM slots timeline visualization with per-packet color allocation
   - Notifications panel (UserNotification list, dismiss action)

### Files Modified
- `VEN/src/controller/user_request.rs` (new)
- `VEN/src/controller/openadr_interface.rs` (extend: capacity reservation, full flex reports)
- `VEN/src/controller/planner.rs` (extend: full AssetForecast, AssetHeuristics UpdateHeuristics)
- `VEN/src/simulator/actors.rs` (add WashingMachine, CookingStove)
- `VEN/src/main.rs` (add request/notification routes)
- `VEN/ui/` (new Schedule tab, notification panel, plan timeline chart)
- `tests/features/ven_user_request.feature` (new)
- `tests/features/ven_flexibility.feature` (new)
- `tests/features/steps/user_request_steps.py` (new)

---

## Stage 6 — Full Use Case Tests + Validation (Step 5)

**Goal:** BDD coverage for all 12 use cases from Step 5. Fix any gaps found. Performance and
reliability validation. **Before starting this stage: read and incorporate
`docs/VEN_Controller/Step6_Validation.zip`** — its contents may add validation scenarios.

### BDD Tests (the entire stage IS tests — write first, implement fixes)

**Feature: `tests/features/ven_uc_normal.feature`**

- **UC-01 EV overnight charge**
  - User creates request: EV to 80% by 07:00, tier-1 €5, tier-2 €1
  - PRICE event (cheap 22:00-06:00, expensive 06:00-22:00)
  - Plan allocates EV to cheap FIRM slots + FlexibilityEnvelope for far slots
  - EV reaches 80% by 07:00, AccumulatedCost ≤ €5

- **UC-02 Washing machine batch**
  - User creates washing machine request: CONTINUE policy
  - Plan schedules start during cheap window
  - Mid-cycle high-price event → CONTINUE packet stays running (bid > grid cost)
  - Cycle completes; EnergyPacket status = COMPLETED

- **UC-03 PV surplus cascade**
  - Profile: PV 8kW, EV (partial fill), Battery (50% SoC), ExportPrice €0.08
  - At peak sun: PV surplus 6.7kW available
  - Decision order: EV self-consume first → Battery store second → Grid export last
  - Verified via PacketAllocation SurplusPower vs. GridPower split

- **UC-04 Day-ahead price update from VTN**
  - VTN posts 24-interval PRICE event → PlanTrigger.RATE_CHANGE
  - Replan: EV packet shifts allocations from 14:00 (expensive) to 22:00 (cheap)
  - EstimatedCost decreases after replan

**Feature: `tests/features/ven_uc_vtn_coordination.feature`**

- **UC-05 Far-horizon favorable pricing**
  - VTN posts cheap rate for tomorrow 03:00-05:00 → FLEXIBLE slots for that window
  - Planner keeps as FLEXIBLE (no urgency)
  - VTN confirms pricing → slots firm up in next plan cycle

- **UC-06 Grid emergency alert**
  - ALERT_GRID_EMERGENCY event received → synthetic EnergyPacket (bid €5, TimePressure 3.0)
  - Immediate replan (no cooldown): EV charging paused, heater reduced
  - MarginalValue of emergency packet beats all others

- **UC-07 Capacity reservation**
  - EV + Heater tomorrow need 15kW; ImportSubscription = 10kW
  - Planner detects 5kW shortfall → PlanWarning
  - OpenADR Interface posts IMPORT_CAPACITY_RESERVATION report to VTN
  - VTN sends CAPACITY_RESERVATION grant (15kW) → PlanTrigger.CAPACITY_CHANGE → replan
  - Tomorrow's EV+Heater slots now within capacity

**Feature: `tests/features/ven_uc_edge_cases.feature`**

- **UC-08 EV disconnects mid-charge**
  - EV ACTIVE (50% fill) → ev_plugged=false → DeviceSession FAILED → packet FAILED
  - PlanTrigger.ASSET_STATE_CHANGE → replan: EV removed from allocations
  - EV reconnects → new PENDING packet created (via user request or CONTINUE policy)

- **UC-09 Tier fallback**
  - EV request: tier-1 (tonight, €0.20/kWh), tier-2 (tomorrow, €0.15/kWh)
  - High price all day → tier-1 slots ineligible (EffectiveCost > ComfortBid)
  - Planner: tier-1 infeasible → fallback to tier-2
  - UserNotification: "Tier 1 infeasible, falling back to Tier 2"
  - Tier-2 allocated tomorrow

- **UC-10 Peak demand penalty avoidance**
  - PenaltyRule: peak > 12kW sustained 15min → €50 breach cost
  - Plan period 17:00-19:00: EV 7kW + Heater 5kW = 12kW (borderline)
  - Monitor RollingAverage approaches threshold → PlanTrigger
  - Replan: Heater deferred to 21:00, peak reduced to 7kW

**Feature: `tests/features/ven_uc_stress.feature`**

- **UC-11 Consumption-only site (no PV, no battery)**
  - Profile: EV only, no PV, no battery
  - Algorithm runs without surplus cascade (Phase 5 no-op)
  - EV allocated purely from grid in cheap slots

- **UC-12 Multi-asset coordination under import cap**
  - Profile: EV 7kW + Heater 5kW + Battery 2kW, ImportCapacityLimit 10kW
  - Multiple EnergyPackets active simultaneously
  - Greedy allocation respects 10kW cap: higher MarginalValue wins scarce capacity
  - Auto-follow compensation distributes deviations across battery

### Performance Validation
- Planner 48h horizon (576 slots × 5 packets): completes in < 100ms (Pi4 ARM64)
- PlanTrigger latency: ALERT → replan complete < 500ms
- Memory: VEN process < 50MB RSS during steady-state

### Validation Checklist
- All 12 UC scenarios pass in `docker-compose.test.yml` test stack on Pi4-Server
- `cargo test --workspace --jobs 2` passes (unit tests: CalcCache math, lifecycle transitions,
  FlexibilityEnvelope bounds, RateSnapshot parsing, EnergyCounter, Battery actor)
- No OOM during build (`docker compose run --build` + `--jobs 2`)
- Ledger cost attribution correct vs. SiteMeter PastEnergySum (± 1% tolerance)
- FlexibilityEnvelopes appear in BFF `/reports` after VEN submission
- Existing Behave suite (`ven_integration`, `ven_simulator`, `use_cases`, etc.) still passes

### Deferred (not in this plan)
- AssetHeuristics online learning from PastEnergySum (use static profile for now)
- Real external weather API (use fixed `ambient_temp_c` from config/override)
- Multi-VEN fleet optimization at VTN level
- Read `Step6_Validation.zip` before Stage 6 begins — may add additional scenarios

---

## Cross-Cutting Decisions

### Module structure
```
VEN/src/
  entities/          — pure data structures (no async, no IO)
  controller/        — business logic (planner, dispatcher, monitor, openadr, user_request)
  simulator/         — device physics (retain existing structure, add Battery)
  reactor/           — keep fsm.rs, interval.rs, trace.rs; delete mod.rs, arbitration.rs
  vtn.rs             — HTTP client (keep, extend)
  config.rs          — env config (extend)
  state.rs           — shared AppState (extend)
  main.rs            — wiring + HTTP routes
```

### Key configurable constants (profile YAML `controller:` section)
| Parameter | Default | Notes |
|---|---|---|
| `plan_step_s` | 300 | Planning grid resolution (5 minutes) |
| `dispatch_step_s` | 5 | Dispatcher tick rate |
| `near_horizon_h` | 4 | FIRM/FLEXIBLE boundary |
| `min_plan_time_h` | 24 | Minimum planning horizon |
| `replan_cooldown_s` | 30 | Minimum time between replans (skipped for ALERT) |
| `stale_rate_policy` | `LAST_KNOWN` | LAST_KNOWN / HEURISTIC_FORECAST / DEFER_TO_FLEXIBLE |
| `co2_weight` | 0.0001 | €/g_CO2 = €100/tonne |
| `deviation_threshold_kw` | 0.5 | NetDeviation trigger for DEVICE_DEVIATION |
| `deviation_sustain_s` | 30 | Sustained deviation before trigger |

### PlanTrigger channel
`tokio::sync::watch<PlanTrigger>` — broadcast to planner loop. Enum:
`PERIODIC | RATE_CHANGE | CAPACITY_CHANGE | DEVICE_DEVIATION | USER_REQUEST | ALERT`

### Backward compatibility
Preserved endpoints (existing tests + VEN UI):
`/sim`, `/trace`, `/sensors`, `/events`, `/programs`, `/reports`, `/health`, `/metrics`

New endpoints added (stages 1-5):
`/packets`, `/requests`, `/plan`, `/rates`, `/obligations`, `/capacity`,
`/flexibility`, `/ledger`, `/dispatch-state`, `/site-meter`, `/notifications`

---

## Test Execution Reference

```bash
# Per-stage feature test (on Pi4-Server):
cd /srv/docker/openadr_lab
docker compose -f tests/docker-compose.test.yml down -v
docker compose -f tests/docker-compose.test.yml run --build --rm test-runner \
  behave tests/features/ven_<feature>.feature

# Full suite after stage complete:
docker compose -f tests/docker-compose.test.yml run --build --rm test-runner

# Cargo unit tests (local or Pi):
cd VEN && cargo test --workspace --jobs 2

# Build VEN image only (Pi4):
ssh Pi4-Server "cd /srv/docker/openadr_lab && docker compose build ven-ven-1"
```
