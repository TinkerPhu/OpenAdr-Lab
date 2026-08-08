# VEN Architecture

**Authoritative reference for VEN components, data flows, planning algorithm, simulator, and API.**
Domain vocabulary is in [docs/REQUIREMENTS.md](../REQUIREMENTS.md).
VTN/BFF architecture is in [docs/architecture/VTN_ARCHITECTURE.md](VTN_ARCHITECTURE.md).

---

## 1. Component Overview

The VEN is a Rust/Axum application. It runs as a Docker container and communicates with the VTN
via the OpenADR 3 REST API. Internally it has two major subsystems: the **HEMS Controller**
(planner-based, multi-step scheduling) and the **Simulator** (physics-based device models).

Each VEN instance loads a per-VEN YAML profile (`profile.rs`) declaring its assets and their
physical parameters. The profile is validated on startup, before any task is spawned: an invalid
profile (out-of-range numeric fields, an absorber referencing an undeclared asset, an empty asset
list) exits with every violation listed at once, rather than starting into an inconsistent state
or failing piecemeal later.

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                              VEN Container                                   │
│                                                                              │
│  ┌─────────────────────────────────────────────────────────────────────────┐ │
│  │                         HEMS Controller                                 │ │
│  │                                                                         │ │
│  │  ┌──────────────┐   ┌──────────────┐   ┌───────────────────────────┐    │ │
│  │  │  OpenADR     │   │    User      │   │     Monitor               │    │ │
│  │  │  Interface   │   │   Request    │   │     (Deviation Detector)  │    │ │
│  │  └──────┬───────┘   └──────┬───────┘   └────────────┬──────────────┘    │ │
│  │         │                  │                        │                   │ │
│  │         └──────────────────┤◄───────────────────────┘                   │ │
│  │                            ▼                                            │ │
│  │                   ┌──────────────┐                                      │ │
│  │                   │   Planner    │ ← PlanTrigger channel                │ │
│  │                   └──────┬───────┘                                      │ │
│  │                          ▼                                              │ │
│  │                   ┌──────────────┐                                      │ │
│  │                   │  Dispatcher  │  (1 s tick)                          │ │
│  │                   └──────────────┘                                      │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
│                                                                              │
│  ┌─────────────────────────────────────────────────────────────────────────┐ │
│  │                     Asset Layer  (Vec<AssetEntry>)                      │ │
│  │                                                                         │ │
│  │  ┌──────────────────────────────────────────────────────────────────┐   │ │
│  │  │  Asset: step() · capability() · simulate_forward()                │   │ │
│  │  │  AssetHandle: id() · current_state() · history(window)            │   │ │
│  │  └──────────────────────────────────────────────────────────────────┘   │ │
│  │          ▲                                           ▲                  │ │
│  │  ┌───────┴────────┐                       ┌──────────┴──────────┐       │ │
│  │  │  AssetConfig   │  ← physics models     │  MeasuredAsset      │       │ │
│  │  │ PV · Battery   │    per asset type     │  (future: real HW,  │       │ │
│  │  │ EV · Heater    │    (implemented)      │   not yet built)    │       │ │
│  │  │ BaseLoad       │                       └─────────────────────┘       │ │
│  │  └───────┬────────┘                                                     │ │
│  │          │ UI only                                                      │ │
│  │  ┌───────▼────────┐                                                     │ │
│  │  │ /sim endpoints │  ← simulation params, overrides, schema, reset      │ │
│  │  └────────────────┘                                                     │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
│                                                                              │
│  REST API (Axum, port 8080 internal / 821x host)                             │
└──────────────────────────────────────────────────────────────────────────────┘
                        │
                        │ OpenADR 3 REST (OAuth2 + polling, default 30 s)
                        ▼
                   ┌──────────┐
                   │   VTN    │
                   └──────────┘
```

**Source layout (current):**
```
VEN/src/
  main.rs              — startup, task spawning; routes registered in routes/mod.rs (§4)
  routes/              — HTTP handlers, one module per resource (adapter ring)
  tasks/                — background loops (sim_tick, planning, poll_*, obligation) (adapter ring)
  services/            — planning/user-request/obligation application logic
  controller/          — dispatcher, monitor, openadr_interface, milp_planner, reporter, timeline, envelope, trace
  entities/            — asset, capacity, device_session, plan, tariff_snapshot, site_meter, user_request
  assets/              — Asset trait implementations (Battery, EvCharger, Heater, PvInverter, BaseLoad)
  simulator/           — SimState, persist, power_model
  reactor/             — REMOVED (see §3.3)
```

See `docs/BACKLOG.md §Refactoring` for any pending layout migrations.

---

## 2. HEMS Controller

### 2.1 Components & Responsibilities

| Component | Module | Cycle / Trigger | Owns |
|---|---|---|---|
| **OpenADR Interface** | `controller/openadr_interface` | `POLL_EVENTS_SECS` poll (default 30 s, env-configurable) + event-driven | `TariffSnapshot` / `TariffTimeSeries`, `OadrCapacityState`, `OadrReportObligation` |
| **User Request Manager** | `controller/user_request` | Event-driven (API call) | `UserRequest`, `EvSession` / `HeaterTarget` / `ShiftableLoad` |
| **Monitor** | `controller/monitor` | 1 s tick (`tasks/sim_tick`) | Per-asset energy/cost/CO₂ ledger (`state::AssetLedgerEntry`) |
| **Planner** | `controller/milp_planner`, reached via `SolverPort` | Watch channel + `replan_interval_s` periodic (default 300 s, profile-configurable) | `Plan`, `FlexibilityEnvelope`s, `PlanWarning`s |
| **Dispatcher** | `controller/dispatcher` | 1 s tick (`tasks/sim_tick`) | Per-asset setpoints written to the simulator |
| **Entities** | `entities/` | Shared state | `Plan`, `TariffSnapshot`, `UserRequest`, `EvSession` / `HeaterTarget` / `ShiftableLoad` |

#### OpenADR Interface

Translates between VTN REST JSON and the internal domain model. The only component that
knows about OpenADR HTTP, OAuth, and event payload formats. Transport lives in `vtn.rs`
behind the `VtnPort` trait; parsing is pure functions in `controller/openadr_interface.rs`.

**VTN → internal translation:**

| OpenADR EventType | Internal target | Status |
|---|---|---|
| `PRICE` / `EXPORT_PRICE` | `TariffSnapshot.import_tariff_eur_kwh` / `.export_tariff_eur_kwh` | ✅ implemented (supports looping daily-price events, e.g. `duration: P9999Y`) |
| `GHG` | `TariffSnapshot.co2_g_kwh` | ✅ implemented |
| `IMPORT_CAPACITY_LIMIT` / `EXPORT_CAPACITY_LIMIT` | `OadrCapacityState.import_limit_kw` / `.export_limit_kw` (strictest active event wins) | ✅ implemented |
| `IMPORT_CAPACITY_SUBSCRIPTION` / `IMPORT_CAPACITY_RESERVATION` | `OadrCapacityState.import_subscription_kw` / `.import_reservation_kw` | ✅ implemented |
| `EXPORT_CAPACITY_SUBSCRIPTION` / `EXPORT_CAPACITY_RESERVATION` | `OadrCapacityState` export-side scalar fields (min wins); subscription+reservation form a contracted allowance that binds the solver when tighter than the limit | ✅ implemented |
| `ALERT_GRID_EMERGENCY` / `ALERT_BLACK_START` | `AlertWindow` (window from interval- or event-level `intervalPeriod`); `PlanTrigger::Alert` fires on change; both types clamp planned import to 0 over the window (soft constraint — never infeasible) | ✅ implemented |
| `SIMPLE` (levels 0–3) | `SimpleWindow` — L1 caps import at a configurable % of contract, L2 at baseline, L3 at 0; highest level wins, alerts override | ✅ implemented |
| `DISPATCH_SETPOINT` | `DispatchWindow` — dispatcher steers the battery to the commanded net site power during the window, plan running underneath; alert wins precedence | ✅ implemented |
| `CHARGE_STATE_SETPOINT` | `EvSession` create/modify targeting the given SoC (fraction or percent); event deletion cancels the event-created session | ✅ implemented |

**Internal → VTN report generation** (`controller/reporter.rs`):

| Report obligation | Source | Status |
|---|---|---|
| `USAGE` | Time-weighted mean of net site import power over the obligation interval (`TimeSeries::resample_uniform`) | ✅ implemented |
| `STORAGE_CHARGE_LEVEL` | Point-in-time SoC (EV/battery) sampled at each obligation interval end | ✅ implemented |
| `OPERATING_STATE` | Derived from sample freshness (`reporter.rs::operating_state`: fresh ≤ 120 s → ACTIVE, stale → UNAVAILABLE) — site-level mirror of the `DeviceResponsiveness` vocabulary | ✅ implemented |
| `IMPORT_CAPACITY_RESERVATION` / `EXPORT_CAPACITY_RESERVATION` | Live `SiteFlexibilityEnvelope` up/down kW | ✅ implemented |
| `DEMAND` | — | ❌ not built |
| `USAGE_FORECAST` | Plan-slot forecasts served at their native slot boundaries, descriptor-driven via the obligation machinery; `reportDescriptor.historical: false` on any usage-family payload also requests the forecast path | ✅ implemented |

#### User Request Manager

Translates user-facing energy requests (from `POST /user-requests`) into device-specific
session types (`EvSession`, `HeaterTarget`, `ShiftableLoad`), then emits `PlanTrigger::UserRequest`
to the Planner watch channel.

- Applies default `CompletionPolicy` per asset type
- Calculates energy requirements from SoC delta × capacity for battery-like assets

#### Monitor (Ledger)

Runs every 1 s via `controller::monitor::record_tick()`, called from `tasks/sim_tick/publish.rs`.
Updates the per-asset cumulative energy/cost/CO₂ ledger (`state::AssetLedgerEntry`) using the
current sim snapshot and the tariff active at `now` (Step/LOCF lookup). Only importing assets
accrue cost/CO₂; export is not credited as revenue in the ledger.

#### Dispatcher

Pure-function module (`controller/dispatcher.rs`) driven by the 1 s tick in
`tasks/sim_tick/`. `build_setpoints()` translates the current `PlanTimeSlot` into device
setpoints — narrowed to plan-allocation only:

1. Seeds every asset with its `default_setpoint_kw`
2. For each `AssetAllocation` in the plan slot covering `now`: overwrites that asset's setpoint
3. Caps PV export at the active capacity limit

Reactive adjustment on top of the plan's allocation — including the opportunistic
surplus-EV overlay's role — has moved to the Deviation Arbiter (below). Ledger
accounting is **not** the Dispatcher's responsibility — see Monitor above.

#### Deviation Arbiter (BL-22 resolved)

`controller::arbiter::reconcile`, called once per tick from
`tasks/sim_tick/helpers.rs::build_tick_setpoints` after `dispatcher::build_setpoints`, is the
single owner of every reactive (non-plan, non-VTN-override) actuator adjustment — resolving the
gap R5/BL-22 tracked (`apply_battery_correction_overlay`'s dead-beat P-controller sat unwired,
and the opportunistic EV-surplus overlay ran as a separate, uncoordinated writer). It:

1. Computes this tick's deviation between the plan's expected net site power and a live
   projection (using `SimState::peek_pv_kw`/`peek_base_load_kw` so neither physics-driven input
   is ever one tick stale — the specific lag that caused feature 017's removal, twice; see
   `docs/reference/KEY_LEARNINGS.md`'s Deviation Absorber section). For battery/EV specifically,
   the projection reads `AssetSnapshot.setpoint_kw` (the arbiter's own last-applied command), not
   the plan's static per-slot allocation — reading the static value instead caused a real
   production bug (a tick-to-tick correction runaway/revert cycle, visible as rapid battery
   oscillation on the dashboard) since a correction already applied was invisible to the next
   tick's deviation calc and got re-applied or silently reverted; both `apply_battery_lever` and
   `apply_ev_lever` already used `setpoint_kw` as their own integrator state, so the deviation
   signal now agrees with them.
2. Ranks available levers (battery, EV, heater pause/emergency-mode, PV curtailment backstop) by
   marginal cost (`PlanTimeSlot.marginal_cost_import/export_eur_per_kwh`, `solver-marginal-cost`),
   excluding zero-capacity levers outright and applying preemption-margin/dwell hysteresis so two
   near-equal-cost levers don't chatter tick to tick
3. Feeds absorbed kWh into a per-asset (battery/EV) residual accumulator; a capacity-fraction
   breach past a cooldown emits `PlanTrigger::ResidualThreshold` — accumulator-based, never a raw
   per-tick-deviation trigger

Gated behind `deviation_arbiter_enabled` (`AppState`, default `false`) for a fully reversible
rollout; when disabled, `build_tick_setpoints` takes the pre-arbiter code path unchanged.

Per-tick reasoning (projected net power, residual deviation, active lever) is surfaced via
`GET /arbiter-diagnostics` and a readout in the VEN UI's `ArbiterSettingsCard` — see
`docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md`. The exact convergence/hysteresis
invariants are enforced by `controller/tests/arbiter_tests.rs`, not restated here.

### 2.2 Two-Speed Loop

The controller operates at two timescales:

| Loop | Period | Driver | Purpose |
|---|---|---|---|
| **Fast** (Dispatcher + Monitor) | 1 s | `tasks/sim_tick` Tokio interval | Execute current plan slot; accumulate ledger |
| **Slow** (Planner) | `replan_interval_s` periodic (default 300 s, profile-configurable) + watch channel | `PlanTrigger` watch channel | Produce new Plan from current rates, sessions, and asset state |

The watch channel (`PlanTrigger`) decouples triggering from execution: any component can emit
a trigger; the Planner processes them in order. This prevents redundant replanning while ensuring
every relevant event causes exactly one new plan.

Trigger senders in code today: HTTP routes (`PlanTrigger::UserRequest`), `POST /plan/trigger`
and sim state injection (`PlanTrigger::AssetStateChange`), the event poll loop
(`PlanTrigger::RateChange` — fired for **any** detected change, including capacity changes),
and shiftable-load completion (`PlanTrigger::UserRequest`). `PlanTrigger::Alert` and
`::CapacityChange` are defined but never sent — see the OpenADR Interface event table above.

### 2.3 Planning Algorithm

The Planner is a **3-tier, two-phase MILP solver** (`controller/milp_planner/`), reached
through the `SolverPort` trait (`controller/solver_port.rs`) — `services/planning.rs` is
the only caller of `SolverPort::solve`, so nothing outside the planner module touches
HiGHS or `run_planner` directly.

**Full design reference:** [`docs/architecture/ven_milp_planner.md`](ven_milp_planner.md)

**Key concepts:**

- **Three tiers** with variable step sizes: fine-grained near-horizon (e.g. 5 min slots),
  coarser mid-horizon, sparse far-horizon. Controlled by `PlannerParams.plan_zones`.
- **Assets as MILP variables**: EV continuous power `p_ev_kw[t]`, heater discrete tiers
  (`z_heat_mid[t]`, `z_heat_full[t]`), battery SoC tracking, etc.
- **Session intent as constraints**: `EvSession`/`HeaterTarget`/`ShiftableLoad` provide
  energy target, deadline, and mode; the solver iterates over asset variables, not session
  objects. See §2.3.1 below.
- **Adoption gate**: a new plan is adopted only if it beats the current plan's cost+friction
  by the effective threshold (which decays over the current plan's age), or if the current
  plan's slots have all expired, or on any non-periodic trigger — prevents churn from noise
  replans.

**Stale-rate handling (slots beyond the last known tariff data):** the planner
applies the profile-configured `StaleRatePolicy`
(`planner.stale_rate_policy`, default `HEURISTIC_FORECAST`;
`controller/milp_planner/stale_rates.rs`) to price stale import slots —
`LAST_KNOWN` carries the last rate forward, `SAFE_AVERAGE` takes a percentile of
known rates, `DEFER_TO_FLEXIBLE` prices stale slots at the max known rate so firm
load avoids them, and `HEURISTIC_FORECAST` is a documented stub behaving like
`LAST_KNOWN` until learned rate patterns land (BL-14). Stale slots set
`PlanTimeSlot.rate_estimated = true` and raise a stable-text plan warning
(feeds the notification dedup). Export and CO₂ rates keep Step/LOCF hold; slots
with no tariff data at all fall back to hardcoded defaults (0.25 €/kWh import,
0.08 €/kWh export, 300 g/kWh CO₂).

#### 2.3.1 Session Intent in the MILP

Device sessions (`EvSession`, `HeaterTarget`, `ShiftableLoad`) provide user intent as solver
constraints — the solver does not iterate over session objects directly:

| Session field | MILP use |
|---|---|
| `EvSession.soft_deadline` | `false` → `MilpLoadMode::MustRun`; `true` → `MayRun` |
| `EvSession.departure_time` | → horizon constraint step `t_ev_dead_step` |
| `HeaterTarget` presence | present → `MustRun` (hard deadline); absent → `MayRun` (autonomous, no deadline) |
| `HeaterTarget.ready_by` | → horizon constraint step `t_dead_step` |
| `EvSession.target_soc` / `HeaterTarget.target_temp_c` | → energy/thermal requirement |

Session tracking (accumulated cost, per-slot power history, status lifecycle) is handled
by the Dispatcher and reporting layer — not by the solver.

#### 2.3.2 Peak-Demand Penalty Threshold (WP6.3, BL-09)

A profile may declare zero or more `planner.penalty_rules` entries
(`rule_id`, `threshold_kw`, `measurement_window_s`, `penalty_eur_per_kw`;
`entities::planner_params::PenaltyRuleParams`), each keeping the planner's grid
import at or below `threshold_kw` within fixed, horizon-aligned windows of
`measurement_window_s`. Implemented as a per-solve soft-penalty MILP term
(`controller/milp_planner/penalty.rs`), mirroring the existing `s_imp_viol`
soft-constraint idiom: one shared slack variable per rule per window bounds
every slot's import in that window, penalized in the objective at
`penalty_eur_per_kw` per kW over threshold — once per window, not per slot
(a demand-charge-style peak cost, not an energy cost). The planner reschedules
flexible load away from a threshold breach whenever that costs less than the
accepted penalty; when it can't (or it's cheaper not to), the accepted cost
surfaces as `CostBreakdown.c_peak_penalty_eur` and a `PlanWarning` naming the
breached rule, window, peak, and cost. Feature is off by default
(`penalty_rules: []`); `measurement_window_s` must be a positive multiple of
the effective planning step (`PlannerConfig::effective_step_s()`), validated
at profile load in `profile::validate`.

Deliberately **not** the stateful, persisted billing-period tracker sketched in
`entities::design_vocabulary::PenaltyRule` (rolling averages,
`breached_this_period` surviving restarts, non-peak `PenaltyCondition`
variants) — each solve re-evaluates its own horizon fresh, with no
cross-restart state. See
`openspec/changes/penalty-threshold-check/design.md` (archived once merged;
`docs/history/project_journal.md` has the resolution summary) for the full
set of decisions and rejected alternatives.

### 2.4 Data Flows

**One heartbeat (5 min PlanTimeStep, steady state):**

```
t=0s     Asset Controller polls devices + grid meter
           → AssetState (power, SoC, temperature, IsConnected)
           → SiteMeter.NetImport_kW

t=0.05s  Dispatcher reads current PlanTimeSlot
           → DispatchCommand[] to Simulator
           → AccumulatedCost/CO₂ updated in asset ledger

t=0.1s   Monitor
           → AssetLedger updated (energy/cost/CO₂ per asset)

t=30s    OpenADR Interface polls VTN (POLL_EVENTS_SECS, default 30 s)
           → New events → translate to TariffSnapshot, OadrCapacityState
           → PlanTrigger::RateChange if anything changed (see §2.2 — this fires for
             capacity changes too; there is no separate CapacityChange trigger in use)

t=300s   Planner (if triggered, or on replan_interval_s timeout — default 300 s)
           → Reads all state
           → Produces new Plan
           → Emits FlexibilityEnvelopes
           → Writes PlanWarnings → UserNotifications
```

❌ **GAP** (`docs/BACKLOG.md` BL-20): the last line above overstates current behaviour —
`PlanWarning`s are written into the `Plan`, but no `UserNotifications` feed exists
anywhere in the VEN today (no queue, no route, no UI surface). `UserNotificationSeverity`
(`entities/design_vocabulary.rs`) is the only trace of this intended feature.

---

## 3. Asset Layer

### 3.0 Asset Abstraction

Each asset exposes a uniform interface to the controller. The controller never calls
physics functions directly or reads simulation parameters.

```rust
/// Physics contract for one asset type. Implemented by Battery, EvCharger, Heater,
/// PvInverter, BaseLoad (VEN/src/assets/*.rs).
trait Asset: Send + Sync {
    fn step(&self, state: &AssetState, setpoint_kw: f64, dt: Duration) -> (AssetState, f64);
    fn capability(&self, state: &AssetState) -> AssetCapability;
    fn simulate_forward(&self, initial: &AssetState, setpoints: &[(DateTime<Utc>, f64)]) -> Trajectory; // default impl
    fn simulate_free(&self, initial: &AssetState, duration: Duration) -> Trajectory;                    // default impl
    fn capability_trajectory(&self, initial: &AssetState, duration: Duration, resolution: Duration)
        -> Vec<(DateTime<Utc>, AssetCapability)>;                                                        // default impl

    // Identity/history — callable only via AssetHandle; panic on a bare physics type.
    fn id(&self) -> &str;
    fn current_state(&self) -> AssetState;
    fn history(&self, window: Duration) -> Vec<HistoryPoint>;
}
```

`AssetConfig` (`VEN/src/assets/mod.rs`) is the concrete enum-dispatch implementation —
one variant per asset type (`Battery`, `Ev`, `Heater`, `Pv`, `BaseLoad`) — holding the
physics parameters loaded from the profile. `AssetHandle` wraps a `(&AssetConfig,
&AssetEntry)` pair to implement the identity/history methods. Per-asset history is a
ring buffer (`AssetHistoryBuffer`, 3600 points ≈ 1 h at 1 s tick) with LOCF lookups and
time-weighted averaging.

| Implementation | Backend | Status |
|---|---|---|
| `AssetConfig` (Battery/Ev/Heater/Pv/BaseLoad variants) | Physics model (sin, SoC, thermal), `VEN/src/assets/` | ✅ implemented — all current VENs |
| `MeasuredAsset` | Real sensor / hardware API | Future — not yet built |

From the controller's perspective a future `MeasuredAsset` would be identical to
`AssetConfig`: swapping one for the other should require no changes outside that asset's
module. This is a design intent for future real deployments, not a present capability.

**Simulation parameters** (irradiation curve, initial SOC, rated power, thermal constants)
are only accessible through the `/sim` API endpoints. The controller never reads them.

**Full trait contract:** [`docs/architecture/ven_asset_interface_spec.md`](ven_asset_interface_spec.md).

### 3.1 Generic Asset Model

The simulator implements the asset interface using a generic model: `SimState.assets: Vec<AssetEntry>`.

```rust
struct SimState {
    assets: Vec<AssetEntry>,
    grid:   GridMeter,
}

struct AssetEntry {
    id:         String,
    state:      AssetState,    // enum dispatch to per-type physics
    setpoint:   f64,           // last commanded value from Dispatcher
    last_power_kw: f64,        // result of last physics tick
    energy:     EnergyCounter, // cumulative kWh for this asset
}
```

`AssetState` is an enum (`PvInverter(PvState)`, `EvCharger(EvState)`, `Battery(BatteryState)`,
`Heater(HeaterState)`, `BaseLoad(BaseLoadState)`). Each variant implements the physics tick.

Adding a new asset type requires only a new enum variant and its actor module — no changes to
the simulator loop, API handlers, or profile parser.

**API compatibility:** `GET /sim` returns both the new `assets: HashMap<String, AssetSnapshot>`
and backward-compatible named fields (`ev`, `heater`, `pv`, `battery`, `base_load_w`) derived
from typed `AssetState`. This allows UI and tests to migrate incrementally.

**Profile format:**
```yaml
assets:
  - type: ev
    id: ev
    max_charge_kw: 7.4
    capacity_kwh: 50.0
    initial_soc: 0.20
  - type: battery
    id: battery
    max_charge_kw: 2.0
    max_discharge_kw: 2.0
    capacity_kwh: 10.0
```

### 3.2 Physics Models Per Asset Type

#### PV Inverter

Irradiation is the primary simulated quantity; P_pv is derived from it:

```
irradiation(t) = irradiation_peak × sin(π × (hour − 6) / 12)   for 06:00 ≤ hour ≤ 18:00
irradiation(t) = 0                                               otherwise (clamped)

P_pv(t) = −P_max × (irradiation(t) / irradiation_stc)
```

`irradiation_stc` = 1000 W/m² (Standard Test Conditions reference).
Irradiation is clamped to zero outside daylight hours regardless of manual UI overrides.
Sign convention: `P_pv` is negative (generation, exported or self-consumed).
Curtailment: if `ExportCapacityLimit` is set and `|P_pv| > limit`, the inverter is cropped to `−limit`.

**Forecast:** `PvAsset.forecast(horizon)` applies the same irradiation model over future
time slots. The planner calls this — it does not contain a PV formula of its own.

**Export curtailment as a planner decision**: the MILP has a real decision variable,
`p_pv_used[t]` (`GridMilpVars`, `controller/milp_interactions.rs`), bounded
`0 <= p_pv_used[t] <= p_pv_kw[t]` and substituted for the raw forecast in the power-balance
constraint in both solver phases. No cost term is attached to curtailment itself — every real
cost term already favors using PV, so the solver only curtails when doing so relieves an active
export-capacity constraint. A small `PV_USE_TIEBREAK_EUR_PER_KWH` bias (mirroring the
pre-existing `SHIFT_TIEBREAK_EUR_PER_SLOT` pattern) keeps HiGHS's MIP-gap tolerance and Phase 2's
friction-only objective from curtailing PV for no cost benefit — the tie-break must be mirrored
into every cost-cap expression that recomputes Phase 1's true objective (a missed mirror caused
Phase 2 to go infeasible on every solve whenever PV was used at all, fixed in `c27b296`).
`PlanTimeSlot.pv_used_kw` exposes the decision alongside `pv_forecast_kw`; the VEN UI's plan
power-stack chart shows the curtailed amount when present.

The resolved limit reaches the simulator every tick: `SimState::tick()` takes a
`pv_export_limit_override` parameter applied to `PvInverter.export_limit_kw`;
`dispatcher::resolve_pv_export_limit_kw` computes it as the most restrictive of the live
VTN/sim-inject capacity cap, the plan's own curtailment target, and (when the deviation arbiter
is enabled) its backstop tighten value — `PvCurtailmentSource` tags which one won.

**Ground-truth precedence** (ven-1 only, when configured): a real measured PV reading
outranks the weather-derived estimate, which in turn outranks the sin model — see
[`docs/architecture/real_measurement_mqtt.md`](real_measurement_mqtt.md).

#### Battery

```
dSOC/dt = P_charge × efficiency / capacity_kwh   (charging: P > 0)
dSOC/dt = P_discharge / capacity_kwh              (discharging: P < 0)
```

Hard bounds: `SOC ∈ [MinSoC, MaxSoC]`. Power clamped to `[MinPower_kW, MaxPower_kW]`.

#### EV Charger

Stepless, range `[min_charge_kw, max_charge_kw]`. Minimum active charge rate = 1.5 kW
(cannot charge below minimum once active). Discharge not modelled (charge-only in lab).
SOC integration same as battery. Response delay ~10 s (modelled as single-step lag).

#### Heater (Thermal Model)

```
dT/dt = (P_heater × efficiency − ambient_loss_rate × (T_room − T_ambient)) / thermal_mass
```

`ambient_loss_rate` default: 0.1 kW/°C. Thermostat override at `T_min` / `T_max` bounds.
Power levels: discrete `[0, 3, 6]` kW (STEPPED adjustability).

**Comfort band vs. safety envelope** (`VEN/src/assets/heater.rs`, `87d6037`): `temp_min_c`/
`temp_max_c` are a comfort/service band, not the asset's true physical limits — the low side has
no physical harm (the tank just drifts toward ambient), while the high side sits well inside a
separate, wider **safety envelope** (`temp_safety_max_c`, per-profile — e.g. `ven-2.yaml`'s tank
is 40–80 °C comfort / 90 °C true safety ceiling). A `HeaterEmergencyMode` enum
(`Normal`/`Curtail`/`Absorb`) exposes that envelope: `Curtail` suppresses the forced-on emergency
heat at `temp_min_c`, letting the tank drift with no physical floor; `Absorb` suppresses the
forced-off ceiling at `temp_max_c`, allowing heating up to `temp_safety_max_c` instead. Each
direction leaves the *other* bound's normal behavior untouched. Settable via `SimInjectState`
(manual/test/demo) or, when `deviation_arbiter_enabled`, automatically by the arbiter's heater
lever once the deviation's marginal cost crosses `HEATER_COMFORT_OVERRIDE_EUR_PER_KWH` — but no
VTN emergency directive drives it yet, and the MILP itself still plans only within the comfort
band (it doesn't know the safety envelope exists).

#### Base Load

Static consumption profile (`W` constant or time-varying). Not controllable.
Represents appliances, lighting, standby — the uncontrollable fraction of site demand.

**Ground-truth replace** (ven-1 only, when configured): a real measured baseline-load
reading replaces the synthetic profile+noise outright — see
[`docs/architecture/real_measurement_mqtt.md`](real_measurement_mqtt.md).

### 3.3 Control Path

The controller is the **single control authority** — exactly one writer produces the
`Setpoints` struct each cycle. (A separate reactive FSM layer alongside the planner was
rejected: two independent writers to `Setpoints` make arbitration ambiguous, with the
Dispatcher silently overriding one of them. Transition smoothing, where needed, lives in
the Dispatcher execution layer.)

**Control path:**
```
VTN events → openadr_interface → rates + capacity constraints
                                            │
User requests ──────────────────────────────┤
                                            ▼
                                        Planner
                                            │
                                        Dispatcher → Simulator setpoints
```

**Tracing:** `GET /trace/events` serves an in-memory ring buffer of `ControllerEvent`s
(capacity 500) with controller-level decisions — rate/capacity changes, plan cycles,
request transitions. `GET /trace/history` serves per-asset recent history.

---

## 4. API Contract

All routes are registered in `VEN/src/routes/mod.rs::build_router`. CORS is open. All
handlers receive `State(ctx: AppCtx)`.

### 4.1 Infrastructure

| Method | Path | Description |
|---|---|---|
| GET | `/health` | Returns `"ok"` |
| GET | `/metrics` | Prometheus metrics (text format) |

### 4.2 OpenADR Proxy

Forwards queries to the VTN via `VtnClient`.

| Method | Path | Description |
|---|---|---|
| GET | `/events` | Active OpenADR events from VTN; optional `?limit=N` |
| GET | `/programs` | Available programs from VTN |

### 4.3 Sensors

Manual sensor snapshot — UI and test injection.

| Method | Path | Description |
|---|---|---|
| GET | `/sensors` | Current sensor snapshot (temperature, power, voltage) |
| POST | `/sensors` | Create/update sensor snapshot (local only, not sent to VTN) |

### 4.4 Reports

VTN report submission.

| Method | Path | Description |
|---|---|---|
| GET | `/reports` | Reports submitted to VTN by this VEN |
| POST | `/reports` | Submit new report to VTN (proxied) |
| PUT | `/reports/:id` | Update existing report at VTN (proxied) |

### 4.5 Simulator

Physics-based device simulation.

| Method | Path | Description |
|---|---|---|
| GET | `/sim` | Full simulator state: device states, power flows, energy counters |
| GET | `/sim/schema` | JSON schema for the profile YAML |
| POST | `/sim/reset/:asset_id` | Reset a specific asset to its profile defaults |
| PUT | `/sim/config/battery` | Update battery configuration at runtime |
| GET | `/sim/inject` | Current injection overrides (`SimInjectState`) |
| POST | `/sim/inject` | Set one or more injection overrides — **full-replace** semantics (see D-06) |
| POST | `/sim/inject/reset` | Clear all injection overrides |
| POST | `/plan/trigger` | Force a `PlanTrigger::AssetStateChange` replan |

`POST /sim/inject` replaced the earlier `/sim/override` and supports four independent
behaviour classes (`state.rs::SimInjectState`):

| Class | Fields | Semantics |
|---|---|---|
| A — one-shot | `battery_soc`, `ev_soc`, `heater_temp_c` | Applied once to physics state, then cleared automatically |
| B — frozen + EMA return | `pv_irradiance` (+`pv_irradiance_alpha`), `base_load_kw` (+`base_load_alpha`) | Held while active; EMA-blended back to the natural model on release |
| C — frozen + snap | `ev_plugged`, `ev_soc_target`, `heater_setpoint_c`, `heater_temp_min_c`, `heater_temp_max_c`, `ambient_temp_c`, `grid_import_limit_kw`, `grid_export_limit_kw` | Held while active; snaps to the profile default on release |
| D — planning-only | `pv_plan_kw` | Pins the PV forecast for all horizon slots; no physics effect, no replan trigger |

### 4.6 Timeline & Asset Forecast

| Method | Path | Description |
|---|---|---|
| GET | `/timeline/all` | Merged past+future timeline for all known assets + grid (registered before `/timeline/:asset_id` — more specific route first) |
| GET | `/timeline/:asset_id` | Merged past+future timeline for one asset |
| GET | `/forecast/:asset_id` | Physics-projected future power for one asset |
| GET | `/history/:asset_id` | Raw per-asset history slice |
| GET | `/capability/:asset_id` | Point-in-time feasible power range (`AssetCapability`) |

### 4.7 HEMS Controller

| Method | Path | Stage | Description |
|---|---|---|---|
| GET | `/tariffs` | 2 | `TariffSnapshot` array parsed from active events |
| GET | `/capacity` | 2 | `OadrCapacityState` parsed from active events |
| GET | `/obligations` | 2 | Pending report obligations extracted from events |
| GET | `/plan` | 3 | Active Plan or `null` |
| PUT | `/plan/objective` | 3 | Override the active `PlannerObjective` |
| GET | `/plan/events` | 3 | SSE stream of `PlannerEvent`s (`SolvingStarted`/`SolvingProgress`/`PlanReady`) |
| GET | `/ledger` | 4 | Per-asset cumulative energy / cost / CO₂ ledger |
| GET | `/user-requests` | 5 | All active user energy task requests |
| POST | `/user-requests` | 5 | Create user request → `EvSession` / `HeaterTarget` / `ShiftableLoad` |
| DELETE | `/user-requests/:id` | 5 | Cancel request → marks it `Cancelled` |
| GET | `/flexibility` | 5 | `SiteFlexibilityEnvelope` derived from live asset state (refreshed every dispatcher tick, independent of the active plan) |
| GET / POST / DELETE | `/ev-session` | 5 | Read / create / end the active `EvSession`; `DELETE` also transitions any linked `Active` `UserRequest` to `Completed` before clearing the session |
| GET / PUT | `/ev-settings` | 5 | Opportunistic surplus-EV-charging overlay toggle |
| GET / POST / DELETE | `/heater-target` | 5 | Read / create / clear the active `HeaterTarget` |
| GET / POST | `/shiftable-loads` | 5 | List / create shiftable loads |
| DELETE | `/shiftable-loads/:id` | 5 | Remove a shiftable load |
| GET / POST / DELETE | `/baseline-override` | 5 | Read / create / clear additive baseline-load adjustments |

### 4.8 Trace

| Method | Path | Description |
|---|---|---|
| GET | `/trace/events` | `ControllerEvent` log (ring buffer, capacity 500), newest first; optional `?limit=N` |
| GET | `/trace/history` | Per-asset recent history slice |

### 4.9 Recorded History — Storage Model Summary

| Endpoint | What it records | Storage | Max history |
|---|---|---|---|
| `GET /trace/events` | Controller-level decisions (rate/capacity changes, plan cycles, request transitions) | In-memory ring buffer (500 entries) | Variable — depends on event frequency, not a fixed duration |
| `GET /ledger` | Cumulative totals per asset since startup | In-memory, 1 s updates | Since restart |
| `GET /reports` | Discrete report snapshots sent to VTN | Stored at VTN | Indefinite |
| `GET /timeline/:asset_id` / `/timeline/all` | Per-asset physics history merged with future plan slots | In-memory ring buffer (3600 points ≈ 1 h at 1 s tick) + full plan horizon | ≈ 1 h past + plan horizon future |

`/timeline` is the closest thing to a continuous power time series today (measured watts
in the past window, planned watts in the future window).

### 4.9a Forecast Accuracy Tracking (schema v8)

Tracks how well the planner's own forecasts held up against what actually happened, for PV,
base_load, and site-residual — the three assets whose power is forecast rather than
user-commanded. Persisted in the `forecast_accuracy_samples` table (`asset_id`, `lead_kind`
[`near`/`far`], `target_ts`, `predicted_kw`, `predicted_at`, `actual_kw`, `actual_at`), indexed
on `(asset_id, target_ts)`; pruned alongside the other history tables via `prune_before`, keyed
on `target_ts`.

**Capture** — every plan cycle, `record_forecast_accuracy_samples` (`services/forecast.rs`,
called from `finish_plan_cycle`) builds a *near* sample from `plan.slots[1]` and a *far* sample
from `plan.slots.last()` (no-op below 2 slots) for each tracked asset: PV from
`slot.pv_forecast_kw`, negated to match the actual/tick sign convention (negative = export) —
the solver's own field is a non-negative generation magnitude; base_load and site-residual via
`AssetHeuristics::sample_kw(slot.start)`, each skipped if no heuristic exists yet for that asset.
Written through `HistoryPort::append_forecast_samples` off the async runtime
(`spawn_blocking`), best-effort (log-and-continue on failure), same pattern as
`history_sampler::write_window`.

**Reconciliation** — piggybacks on `history_sampler`'s existing 1-minute tick flush: after a
window's ticks are appended, `HistoryPort::reconcile_forecast_actuals` fills `actual_kw`/
`actual_at` on any open sample whose `asset_id` matches and whose `target_ts` falls in
`[tick.ts, tick.ts + window_s)`; an already-reconciled row is never overwritten by a later call.

**Route**: `GET /history/forecast-accuracy?from=&to=&asset_id=&lead_kind=` (see DOCUMENTATION.md
§History Store for the full persisted-history route list) — `resolve_range` plus optional
`asset_id`/`lead_kind` filters; an invalid `lead_kind` value returns 400.

**UI**: the History page overlays near/far forecast lines on the PV, base_load, and
site-residual `AssetTimelineChart` cells only (the tracked-asset set). The overlay is folded
into the same per-timestamp array the actual Power line reads, via the shared
`mergeTimestampedSeries`/`locfFillKeys` utilities (`components/charts/mergeSeries.ts`) rather
than given its own `data` override — recharts resolves tooltip hover by array index, not by
re-matching timestamps across a series' independently-indexed `data`, so a forecast line on its
own coarser (~5 min) array previously caused hover to show the actual line's value from an
unrelated time; the sparse forecast samples are forward-filled (LOCF) across the shared array so
their `stepAfter` line (matching the actual line's own step interpretation) has a value at every
one-minute slot, not just the sample points. Every multi-series chart in `VEN/ui` (not just
`AssetTimelineChart`) is built on this same merge utility, structurally preventing the same
class of bug elsewhere.

### 4.10 Operational Diagnostics (WP-T1–T8, `docs/history/project_journal.md` — search "WP-T")

Backend-process/task health and operational status, distinct from HEMS controller state (4.7) —
surfaced on the VEN UI Dashboard and under its Diagnostics nav group, per the ui-transparency
principle (every backend capability needs an inspectable UI surface, not just a route).

| Method | Path | Description |
|---|---|---|
| GET | `/vtn/status` | Componentised VTN connection health (last successful poll, last error) |
| GET | `/tasks/status` | Per-supervised-task status (last panic/restart, without digging through logs) |
| GET | `/events/log` | Snapshot of VEN-operational failures (distinct from OpenADR events) |
| GET | `/events/log/events` | SSE stream of the same |
| GET | `/reports/submissions` | Per-report submission outcome (accepted/rejected/error), keyed to the Reports page |
| GET | `/notifications`, `/notifications/history`, `/notifications/events` | User-facing notifications: current, history, SSE stream |
| GET | `/metrics` | Prometheus metrics; VEN UI's Metrics page groups these under human-readable categories by default, with a raw-view toggle |
| GET | `/measurement` | Real-measurement MQTT feed status (PV, baseline load): raw reading, freshness, source-alive — see [`docs/architecture/real_measurement_mqtt.md`](real_measurement_mqtt.md) |

### 4.11 Fleet Dashboard — Multi-Host VEN Discovery (BL-41)

The VEN UI's fleet dropdown (`VEN/ui/src/api/venRegistry.ts`) discovers VENs beyond the
hardcoded `ven-1`/`ven-2`/`ven-3` trio (`DEFAULT_VENS`) via the VTN's own VEN registry,
proxied at `/api/vens-registry`. Each discovered VEN's dashboard base URL is resolved as
follows:

- **Same Docker host as the UI** (the common case — fleet VENs, `ven-1..3`): resolved via
  the dynamic nginx route `/api/dyn/{venName}` (`VEN/ui/nginx.conf`), which relies on
  Docker's embedded DNS resolving the VEN's container name — this only works when the VEN
  container shares a Docker network with the `ui` container.
- **Different physical host** (e.g. a VEN running on a second machine administered by the
  same VTN): the VEN object carries an optional `DASHBOARD_URL` attribute — a full origin
  string (e.g. `http://192.168.1.104:8211`) set via `scripts/seed_vtn.py`'s
  `provision_vens`/`_ensure_dashboard_url_attribute`, using the VTN's generic
  `attributes: ValuesMap[]` mechanism (the same one used for the WP4.5 `PERSONA` tag).
  When present, `mergeVens`/`fetchDiscoveredVens` use that origin directly as the VEN's
  base URL — the browser fetches the VEN's API straight, with no proxy hop. This works
  because VEN's axum router already sets `CorsLayer::new().allow_origin(Any)`
  (`VEN/src/routes/mod.rs`), and the entire `VEN/ui` data layer already treats a VEN's base
  URL as an opaque prefix (`ApiClient.baseUrl`, consumed uniformly by every hook including
  the `EventSource` stream).

A VEN without `DASHBOARD_URL` is unaffected — same-host resolution via `/api/dyn/{venName}`
is unchanged, so this is purely additive for cross-host fleets.

---

## 5. Time-Series Alignment

The system deals with multiple time series that originate from different sources and carry
different natural periods. They rarely align on a common grid:

| Series | Typical period | Origin | Type |
|---|---|---|---|
| Asset power (sim) | 1 s | Simulator tick | Continuous physical |
| Planning grid slots | 60–300 s (configurable) | Planner | Derived |
| PRICE / GHG events | 1 h (day-ahead) | VTN OpenADR | Piecewise-constant |
| Capacity limit events | 3–6 h | VTN OpenADR | Piecewise-constant |
| SIMPLE / alert events | Variable | VTN OpenADR | Piecewise-constant |
| Report obligations | 15–30 min (typical) | VTN event `reportDescriptors` | Aggregation target |
| UI chart buckets | Variable (display width) | Browser | Downsampled |

### 5.1 Interpolation Semantics by Signal Type

Different signal types require different interpolation rules. Mixing rules is a source of
silent bugs (e.g. linearly interpolating a tariff implies a continuous ramp, which is wrong).

| Signal type | Examples | Correct rule | Wrong |
|---|---|---|---|
| **Piecewise-constant** | Tariff (€/kWh), capacity limit (kW), SIMPLE level | **Step / LOCF** — value holds until the next breakpoint | Linear interpolation |
| **Continuous physical** | Power (kW), temperature (°C), SOC (%) | **Linear** between measured points | Carrying last value flat |
| **Cumulative** | Energy (kWh), cost (€) | **Sum within bucket** — never interpolate | Any interpolation |

**LOCF** = Last Observation Carried Forward — the value at time `t` is the most recent value
at or before `t`. Correct for tariffs and any signal that "takes effect and stays in effect".

### 5.2 Implementation — `common::TimeSeries`

A single reusable abstraction (`VEN/src/common/mod.rs`) backs all three time-series
consumers — tariffs, obligation reports, and timeline resampling — so there is one
interpolation/aggregation implementation, not one per consumer.

```rust
struct TimeSeries {
    samples:       Vec<(DateTime<Utc>, f64)>,
    interpolation: Interpolation,  // Step | Linear
}

enum Interpolation {
    Step,    // LOCF — used for tariffs, capacity limits
    Linear,  // used for power, temperature, SOC
}

impl TimeSeries {
    fn interpolate_at(&self, ts: DateTime<Utc>) -> Option<f64>;
    fn time_weighted_mean(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Option<f64>;
    fn resample_to_grid(&self, timestamps: &[DateTime<Utc>]) -> TimeSeries;
    fn resample_uniform(&self, width: Duration, agg: Aggregation) -> TimeSeries; // Mean | Min | Max
}
```

**Consumers today:**
- **Tariffs** (`entities/tariff_snapshot.rs::TariffTimeSeries`): three Step-interpolated
  `TimeSeries` (import, export, CO₂) built from `TariffSnapshot`s at the OpenADR interface
  boundary.
- **Obligation reports** (`controller/reporter.rs`): `resample_uniform` buckets net site
  power onto the obligation's `intervalPeriod` grid; SoC is sampled at each interval end.
- **Timeline** (`controller/timeline.rs`): uniform-grid resampling with LOCF time-weighted
  averaging for the UI chart.

**Event merge** (`openadr_interface.rs`): when multiple events define the same
interval, events are pre-sorted by ascending `priority` (newer `createdDateTime`
breaking ties) so the highest-priority event is processed last and wins the
last-write-wins merge (BL-02).

The MILP planner prices each slot at its **time-weighted mean** tariff
(`TimeSeries::time_weighted_mean` via `milp_planner/inputs.rs` and
`stale_rates.rs`), so a slot straddling a tariff boundary blends both rates —
import, export, and CO₂ alike.

### 5.3 OpenADR Spec Position

The spec defines interval structure but leaves VEN-side alignment to the implementer:
- Mixed `intervalPeriod` granularities within a single event (or across events) are legal.
- Reports may use `dataQuality = ESTIMATED` for interpolated/inferred values — acknowledged but unspecified.
- Event `priority` is defined but conflict resolution for overlapping same-type payloads is not specified; priority-based ordering before merge is the correct interpretation.

---

## 6. Design Decisions

### D-01: MILP Planner (replaces greedy scheduler)

**Decision:** 3-tier MILP solver via HiGHS.
**Rationale:** The greedy scheduler was replaced when more assets and tighter constraints were
added. HiGHS solves the residential-scale problem (24–48 h, 3–15 assets) in 5–10 s on Node1,
which is acceptable for a 20–300 s replan interval. The adoption gate filters noise replans.
See `docs/architecture/ven_milp_planner.md` for full design rationale.

### D-02: In-Memory Ledger

**Decision:** The per-asset ledger (`state::AssetLedgerEntry`) is in-memory only; resets on restart.
**Rationale:** The ledger is a running total for the current session. Persistent billing-period
data is stored at the VTN as reports. Local persistence adds complexity for little benefit in
a lab context.

### D-03: Reactor Removed (spec kit 001)

See §3.3. Controller is the single control authority.

### D-04: Generic Asset Model (spec kit 002)

**Decision:** `SimState.assets: Vec<AssetEntry>` with enum dispatch.
**Rationale:** The hardcoded named-field model required touching every layer when adding an
asset type. The generic model isolates new asset types to their own module.

### D-05: OadrEventSnapshot Unification

**Decision:** `TariffSnapshot` holds all time-varying signals
(price, CO₂, capacity limits) in one struct per poll tick.
**Rationale:** All fields are co-valid at the same timestamp. A unified struct eliminates
temporal alignment bugs that arise when price and capacity signals are stored separately.
See REQUIREMENTS §3.2.2.

### D-06: POST /sim/inject is Full-Replace

**Decision:** `POST /sim/inject` replaces the entire injection-override struct (see §4.5
for the endpoint's four behaviour classes; it superseded the earlier `/sim/override`).
**Rationale:** Partial-patch semantics (PATCH) require null-vs-absent disambiguation.
Full-replace is simpler and explicit. Callers must set all fields they want active.

### D-07: Event Poll Interval — Configurable, Not Fixed

**Decision:** Event polling defaults to 30 s (`POLL_EVENTS_SECS` env var, default 30;
`POLL_PROGRAMS_SECS`/`POLL_REPORTS_SECS` default 30/60) rather than a hardcoded constant.
**Rationale:** Balances VTN load against response latency; making it env-configurable
lets a deployment tune this per VTN without a rebuild. Configurable jitter is not
implemented in the lab.
