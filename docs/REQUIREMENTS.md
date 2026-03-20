# OpenADR Lab — Requirements

**Authoritative reference for domain vocabulary and functional requirements.**
VEN architecture is in [docs/architecture/VEN_ARCHITECTURE.md](architecture/VEN_ARCHITECTURE.md).
VTN/BFF architecture is in [docs/architecture/VTN_ARCHITECTURE.md](architecture/VTN_ARCHITECTURE.md).

---

## 1. Purpose & Scope

This document defines:

- The domain vocabulary for the entire project (single source of truth for all terms)
- The local entity model (entities not defined by the OpenADR spec)
- Functional requirements for the VEN HEMS controller and simulator

OpenADR 3 entities are **referenced by spec citation only** — they are not redefined here.
The authoritative spec is in `docs/openadr_3_1_specs/` (markdown versions).

**Scope:** Single Raspberry Pi lab deployment hosting one VTN and up to three VENs. Each VEN
models a residential site with a HEMS controller. The system is not production-grade but
follows production-inspired design patterns.

---

## 2. Glossary

### 2.1 Organisations & Roles

| Term | Full Name | Definition |
|---|---|---|
| **Utility** | Electric Utility | Company that generates, transmits, and/or distributes electricity to customers. Operates the grid and runs demand response programs. Examples: PG&E, E.ON, ConEdison. |
| **DSO** | Distribution System Operator | Entity responsible for operating the local electricity distribution network. In Europe, DSOs are often separate from energy retailers. Examples (CH): EWZ, BKW Netz, SH Power; (DE): Bayernwerk Netz, Netze BW; (FR): Enedis (formerly ERDF). |
| **TSO** | Transmission System Operator | Entity responsible for the high-voltage transmission grid. Examples (CH): Swissgrid; (DE): TenneT DE, Amprion, 50Hertz, TransnetBW; (FR): RTE (Réseau de Transport d'Électricité). |
| **Aggregator** | DR Aggregator | Company that bundles many small DER/load resources from multiple customers into a single portfolio large enough to participate in wholesale energy markets or utility DR programs. |
| **Prosumer** | Producer + Consumer | End customer that both consumes and produces electricity (e.g. a home with solar panels and a battery). |

### 2.2 OpenADR Protocol

| Term | Full Name | Definition |
|---|---|---|
| **OpenADR** | Open Automated Demand Response | Open standard protocol for communicating DR signals between utilities/aggregators and customer energy management systems. Current version is OpenADR 3. |
| **VTN** | Virtual Top Node | The server side of OpenADR. Operated by the utility, DSO, or aggregator. Creates programs and sends events to VENs. Receives reports back. |
| **VEN** | Virtual End Node | The client side of OpenADR. Runs at the customer site. Receives events from the VTN, controls local devices, and reports back telemetry. |
| **BFF** | Backend For Frontend | API proxy between the VTN UI and the VTN. Handles authentication and simplifies the API for the web frontend. Not part of the OpenADR spec — an architectural pattern used in this lab. |

### 2.3 HEMS — Home Energy Management System

| Term | Full Name | Definition |
|---|---|---|
| **HEMS** | Home Energy Management System | Software system that monitors and controls energy flows within a home or small site. Coordinates DERs (solar, battery, EV charger, HVAC) to minimise cost, maximise self-consumption, or respond to DR signals. In this lab the HEMS controller runs inside the VEN. |
| **Planner** | Energy Planner | Component of the HEMS that schedules future energy use. Given tariff forecasts, device constraints, and DR obligations, it builds an optimal time-slot plan (FIRM + FLEXIBLE slots) using a greedy algorithm. |
| **Dispatcher** | Setpoint Dispatcher | Real-time control loop (1 s tick) that translates the planner's slot schedule into live device setpoints and accumulates the asset ledger. |
| **Asset Ledger** | — | Cumulative energy accounting record maintained by the Dispatcher. Tracks how much energy each asset imported/exported and the associated cost/revenue. **In-memory only; resets on restart. POTENTIAL FOR PERSISTANCE, needs VEN Persistance** |
| **Energy Packet** | — | A schedulable unit of energy delivery: a fixed amount of energy (kWh) for a specific asset, with a time window and status (`PENDING → ACTIVE → COMPLETED / ABANDONED`). |
| **FIRM slot** | Firm Commitment Slot | A planner output slot that must be executed — driven by a hard user request or minimum SOC constraint. Cannot be deferred. |
| **FLEXIBLE slot** | Flexible Opportunity Slot | A planner output slot that can be shifted or cancelled if constraints change. Typically price-driven charging windows. |
| **OadrEventSnapshot** | — | `RateSnapshot` [RENAME → `OadrEventSnapshot`] — A point-in-time capture of all time-varying OpenADR events at one poll tick: import/export tariff (€/kWh), CO₂ intensity, and capacity limits. Unified in one row for temporal correlation — all fields are valid at the same timestamp. Price fields originate from `PRICE`/`EXPORT_PRICE` events; capacity fields from `IMPORT_CAPACITY_LIMIT` / `EXPORT_CAPACITY_LIMIT` events. |
| **Capacity State** | — | Current grid capacity constraints parsed from a VTN subscription/reservation event: subscribed import (kW), subscribed export (kW), reserved import (kW), reserved export (kW). Distinct from per-interval capacity limits (which live in OadrEventSnapshot). |
| **User Request** | — | An explicit energy delivery request submitted by the occupant (e.g. "charge EV to 80% by 07:00"). The planner honours these as FIRM slots. Supports `ASAP`, `BY_DEADLINE`, `MAX_COST`, `OPPORTUNISTIC` modes. |
| **SOC target** | State-of-Charge Target | Desired battery or EV charge level (%) at a given deadline. The planner back-calculates the required charging power and window to meet it. |
| **FlexibilityEnvelope** | — | The range of power the HEMS can flex (increase or decrease) in each time slot, as seen from the grid. Exposed to aggregators to let them predict available DR capacity. |
| **Baseline** | Demand Baseline | The expected energy consumption without any DR intervention. Used in M&V (Measurement & Verification) calculations to measure actual load reduction. **Distinct from Forecast** (see below). |
| **Forecast** | Planner Forecast | The planner's per-slot prediction of energy consumption or generation. May use physics models, heuristics, or historical patterns. A forward-looking planning input, not a historical M&V reference. |

### 2.4 Energy & Grid Concepts

| Term | Full Name | Definition |
|---|---|---|
| **DR** | Demand Response | Strategy where electricity consumers reduce or shift energy usage in response to grid signals (high prices, grid emergencies). |
| **DER** | Distributed Energy Resource | Any small-scale energy generation or storage device connected to the distribution grid: solar panels, batteries, EV chargers, controllable loads. |
| **HVAC** | Heating, Ventilation, and Air Conditioning | Building climate control system. A prime target for DR programs due to high energy intensity and thermal storage characteristics. |
| **EV** | Electric Vehicle | Vehicle powered by electricity. EV chargers are significant controllable loads — a Level 2 charger draws 7–19 kW. |
| **SOC** | State of Charge | Battery charge level expressed as a percentage (0–100%). |
| **Curtailment** | Load Curtailment | Actively reducing electricity consumption in response to a DR signal. |
| **M&V** | Measurement & Verification | Process of verifying that DR actually delivered the promised load reduction. Uses the Baseline as the reference. |
| **tariff** | Electricity Tariff | Price in €/kWh (or currency/kWh). Applies to energy quantity. **Not to be confused with rate.** |
| **rate** | Power Rate | Cost in €/h or currency/h. Applies to power over time. In this project "tariff" is used for €/kWh values everywhere in documentation. API legacy endpoint `GET /rates` [RENAME → `GET /tariffs`] returns tariff data. |

### 2.5 Sign Convention (Grid Boundary)

**Positive = power imported from grid. Negative = power exported to grid.**

This convention applies uniformly at the site boundary (utility meter) to: setpoints, ledger entries, reports, and all power values in this project.

```
                                         <──── negative (export) ────
                                              ╭─────────────────────────╮
╭───────╮     ╭────────────────────────╮      │  central connection     │
│Utility│<===>│  Utility Energy Meter  │<====>│  board (Σ P = 0)        │<====> Assets
╰───────╯     ╰────────────────────────╯      ╰─────────────────────────╯
                    import tariff →                 ──── positive (import) ────>
                    ← export tariff
```

`P_util` is a **single signed value** at the meter — the physical connection to the grid cannot import and export simultaneously. `P_import` and `P_export` are not two separate measurements; they are two names for the same `P_util` conditioned on its sign: `P_import = P_util` when `P_util ≥ 0` (site is net consuming), `P_export = P_util` when `P_util ≤ 0` (site is net producing). Exactly one is non-zero at any instant.

Within the site: `Σ(P) = P_util − (P_consume + P_generate + P_store + P_release) = 0 W`.

Generation (`P_generate`) and battery discharge (`P_release`) have negative values by definition.
They result in net export to grid **only if** their magnitude exceeds simultaneous consumption.

### 2.6 Units

| Unit | Meaning | Context |
|---|---|---|
| **W / kW / MW** | Power (instantaneous) | 1 kW = 1,000 W; 1 MW = 1,000 kW |
| **kWh** | Energy | Power × time; running 10 kW for 1 h = 10 kWh |
| **€/kWh** | Tariff | Cost per unit of energy |
| **€/h** | Rate | Cost per unit of time (power-based billing) |
| **gCO₂eq/kWh** | Grid carbon intensity | Used in GHG event payloads |
| **gCO₂eq/h** | Carbon Rate | GHG production per unit of time |
| **%** | State of Charge | Battery/EV charge level (0–100%) |

### 2.7 ISO 8601 Duration Syntax

OpenADR uses ISO 8601 duration format: `P[n]Y[n]M[n]DT[n]H[n]M[n]S`.

| Duration | Meaning |
|---|---|
| `PT5M` | 5 minutes |
| `PT15M` | 15 minutes |
| `PT30M` | 30 minutes |
| `PT1H` | 1 hour |
| `P1D` | 1 day |

**Key rule:** `M` before `T` = months; `M` after `T` = minutes. `P2M` = 2 months, `PT2M` = 2 minutes.

---

## 3. Domain Model

### 3.1 OpenADR Entities (citation only)

The following entities are defined by the OpenADR 3 specification. This project uses them
as-is. See `docs/openadr_3_1_specs/2_OpenADR 3.1.0_Definition_20250801.md` for authoritative definitions.

| Entity | OpenADR 3 spec section | Notes |
|---|---|---|
| Program | §5.2 | DR program with enrollment targets, duration, and policy |
| Event | §5.3 | Timed signal carrying one or more typed payload intervals |
| Report | §5.4 | Telemetry submission from VEN to VTN |
| VEN | §5.1 | Virtual End Node identity and resource registrations |
| Resource | §5.5 | Named device/meter within a VEN |
| Interval | §5.3.2 | Single time slot within an event, carrying `payloads[]` |
| Payload | §5.3.3 | `{type: EventType, values: [Value]}` within an interval |

**OpenADR event types used in this lab** (full list in spec §5.3.3):

| EventType | What it signals |
|---|---|
| `PRICE` | Import electricity tariff (€/kWh) |
| `EXPORT_PRICE` | Export electricity tariff (€/kWh) |
| `GHG` | Grid carbon intensity (gCO₂/kWh) |
| `IMPORT_CAPACITY_LIMIT` | Hard import cap per interval (kW) |
| `EXPORT_CAPACITY_LIMIT` | Hard export cap per interval (kW) |
| `IMPORT_CAPACITY_SUBSCRIPTION` / `_RESERVATION` | Subscribed/reserved capacity (kW) |
| `EXPORT_CAPACITY_SUBSCRIPTION` / `_RESERVATION` | Subscribed/reserved capacity (kW) |
| `SIMPLE` | Curtailment level 0–3 (see note on profiles below) |
| `DISPATCH_SETPOINT` | Absolute power setpoint (kW) |
| `CHARGE_STATE_SETPOINT` | Battery/EV target SOC (%) |
| `ALERT_GRID_EMERGENCY` / `ALERT_FLEX_ALERT` / etc. | Grid alerts |

**OpenADR report payload types used in this lab** (full list in spec §5.4.2):

`USAGE`, `DEMAND`, `BASELINE`, `STORAGE_CHARGE_LEVEL`, `STORAGE_MAX_CHARGE_POWER`,
`STORAGE_MAX_DISCHARGE_POWER`, `OPERATING_STATE`, `USAGE_FORECAST`,
`IMPORT_CAPACITY_RESERVATION`, `EXPORT_CAPACITY_RESERVATION`.

**OpenADR 3 Certification Profiles:** OpenADR 3 introduced named certification profiles to
avoid ambiguous interpretations of payload types like `SIMPLE`. In OpenADR 2.0b, `SIMPLE`
carried implicit meaning that varied by deployment (curtailment level, price tier, or shed
percentage — depending on the utility's convention). OpenADR 3 makes the meaning explicit
through profiles: a profile defines which payload types a VEN/VTN must support and how to
interpret them. Two profiles are defined in v3.1.0:
- **Continuous Pricing (CP)** — VEN receives `PRICE`, `GHG`, and `ALERT` payloads and
  optimises locally; no direct control mandate.
- **Baseline Profile (BP)** — General flexibility system; covers direct control and dispatch
  setpoints.

VENs may implement any combination of profiles. VTNs must implement all profiles for
commercial certification. This lab does not implement profile-based certification; it uses
the raw payload types directly. Future VEN versions may add profile-aware interpretation of
`SIMPLE` (mapping levels 0–3 to explicit actions) once the use case is defined.

**Cancellation:** OpenADR 3 has no `cancel` status on events. Cancellation is achieved by
deleting the event via `DELETE /events/{id}`. VENs detect removal on the next poll cycle.

### 3.2 Local Entities (defined here)

These entities are not part of the OpenADR spec. They exist in the VEN application.

#### 3.2.1 Enumerations

**AssetType**
```
PV               — photovoltaic producer
BATTERY          — bidirectional storage
EV               — electric vehicle (consumer, storage-like)
HEATER           — thermal consumer with storage characteristics
HEAT_PUMP        — thermal consumer with storage characteristics
WASHING_MACHINE  — batch consumer
COOKING_STOVE    — heuristic/uncontrollable consumer
SITE_RESIDUAL    — virtual asset: unmodeled site consumption
GENERIC_CONSUMER / GENERIC_PRODUCER — fallbacks
```

**PowerAdjustability**
```
NONE             — observe only
ON_OFF           — binary switching (treated as STEPPED with [0, MaxPower])
STEPPED          — discrete power levels (e.g. 0/3/6 kW)
STEPLESS         — continuously adjustable within range
CROPPABLE        — can be curtailed downward only (e.g. PV)
RECOMMENDATION   — VEN can suggest but not enforce
```

**DeviceResponsiveness** — health/communication quality of a device
```
RESPONSIVE       — confirms setpoints within expected delay
DEGRADED         — responds but outside expected parameters
UNRESPONSIVE     — not confirming setpoint changes
OFFLINE          — not communicating at all
```

**EnergyPacketStatus**
```
PENDING          — not yet started
SCHEDULED        — planned start time assigned
ACTIVE           — currently executing
PAUSED           — temporarily suspended
COMPLETED        — target energy/SoC reached
PARTIAL_COMPLETED — deadline reached with less than 100% fill
ABANDONED        — all tiers exhausted or user cancelled
FAILED           — device failure prevented completion
```

**PlanTrigger** — what caused the Planner to replan
```
PERIODIC         — regular cycle
RATE_CHANGE      — new PRICE/GHG/EXPORT_PRICE event from VTN
CAPACITY_CHANGE  — new capacity limit/reservation from VTN
ALERT            — emergency/flex alert from VTN
USER_REQUEST     — new or modified EnergyPacket from user
DEVICE_DEVIATION — significant actual vs. planned deviation
ASSET_STATE_CHANGE — device connected/disconnected/failed
```

**UserRequestMode**
```
ASAP             — as soon as possible, cost-aware
ASAP_FREE        — as soon as possible, only free energy
BY_DEADLINE      — complete by deadline, cost-aware
BY_DEADLINE_FREE — complete by deadline, only free energy
MAX_COST         — complete whenever, within cost limit
OPPORTUNISTIC    — use only free/surplus energy, no deadline
```

**CompletionPolicy** — what happens when the last DeadlineTier expires and the packet is incomplete
```
STOP             — terminate immediately (→ PARTIAL_COMPLETED if fill < 1.0)
CONTINUE         — keep going at PostDeadlineComfortBid priority
```

Defaults per asset type: `BATTERY → STOP`, `EV → CONTINUE (low bid)`,
`WASHING_MACHINE → CONTINUE (high bid)`, `HEAT_PUMP / HEATER → STOP`.

**StaleRatePolicy** — how the Planner handles slots beyond the last known tariff data
```
LAST_KNOWN       — repeat the last known tariff
HEURISTIC_FORECAST — use learned day-of-week / time-of-day patterns
DEFER_TO_FLEXIBLE — mark all unknown slots FLEXIBLE (most conservative)
SAFE_AVERAGE     — use a configurable safety tariff (e.g. 80th percentile)
```
Default: `HEURISTIC_FORECAST`.

#### 3.2.2 Core Value Types

**PowerRange**
```
MinPower_kW      — minimum controllable power (negative = export)
MaxPower_kW      — maximum power
PowerSteps       — discrete levels (null if STEPLESS)
```

**OadrEventSnapshot** (`RateSnapshot` [RENAME → `OadrEventSnapshot`])
```
TimeStamp             — RFC 3339
ImportPrice           — €/kWh (tariff for grid import)
ExportPrice           — €/kWh (tariff for export; also used as opportunity cost for PV self-consumption)
ImportCO2             — gCO₂/kWh (from GHG event)
ImportCapacityLimit   — kW | null (from IMPORT_CAPACITY_LIMIT event interval)
ExportCapacityLimit   — kW | null (from EXPORT_CAPACITY_LIMIT event interval)
```

Rationale for unified struct: all fields are co-valid at the same timestamp, enabling the planner
to correlate price and capacity signals without temporal alignment errors.

#### 3.2.3 Asset Profiles (configuration entities)

**AssetProfile** — physical + behavioural description of one device
```
AssetID          — unique string
AssetType
PowerRange
Adjustability    — PowerAdjustability
ForecastSource   — WEATHER_MODEL | DEVICE_CLOUD | PHYSICAL_MODEL | HEURISTIC | MANUAL | NONE
ThermalModelParams (optional) — for HEATER / HEAT_PUMP: ambient loss rate, mass, efficiency
MinSoC / MaxSoC (optional)   — for BATTERY / EV: hard bounds
```

#### 3.2.4 Scheduling Entities

**EnergyPacket**
```
PacketID, AssetID
TargetEnergy_kWh, EarliestStart, LatestEnd
CompletionPolicy, PostDeadlineComfortBid
Status: EnergyPacketStatus
ValueCurve (DeadlineTiers[] + ComfortRates[])
PastPowerProfile   — [EnergySnapshot] actual execution history
PlannedPowerProfile — [EnergySnapshot] forward schedule
AccumulatedCost_EUR, AccumulatedCO2_g
FillPercentage     — 0.0–1.0
```

**UserRequest**
```
RequestID, AssetID
Mode: UserRequestMode
Deadlines: [DeadlineTier]   — each has Deadline, MaxCost, MaxMarginalRate
LinkedPacketID
```

**FlexibilityEnvelope** — per unallocated packet, in FLEXIBLE horizon
```
AssetID, PacketID
EnergyNeeded_kWh, MaxPower_kW
WindowStart, WindowEnd
MaxAcceptableRate   — min(ComfortBid, tier ceiling)
BudgetRemaining_EUR
EstimatedCost_EUR
```

#### 3.2.5 Plan & Execution Entities

**Plan** — output of one Planner invocation
```
CreatedAt, TriggerCause: PlanTrigger
FirmSlots[]    — PlanTimeSlot with PacketAllocations
FlexSlots[]    — PlanTimeSlot with FlexibilityEnvelopes
PlanWarnings[]
EstimatedCompletion per packet
```

**AssetLedger** — per-asset cumulative accounting
```
AssetID
PeriodStart
TotalConsumption_kWh, TotalProduction_kWh
TotalImportCost_EUR, TotalExportRevenue_EUR
TotalCO2_g
TrackedByPackets_kWh, UntrackedEnergy_kWh
```
**Storage: in-memory only. Resets on VEN restart.**

### 3.3 Grid as Virtual Site Boundary

The site boundary is modelled as a virtual Kirchhoff node where `Σ P = 0 W` at all times.
The utility meter measures the net flow across this boundary.

Key implications:
- Consumed and generated power are additive in the power balance because they carry predefined signs (consume = positive, generate = negative).
- The tariff for import and export are both positive scalars, so `price = P_util × tariff(t, sign(P_util)) × dt` gives a positive cost when importing and negative cost (revenue) when exporting.
- Generation and battery discharge result in net export to the utility **only if** their total magnitude exceeds simultaneous site consumption.
- **SITE_RESIDUAL** is a virtual asset representing unmodelled consumption: `P_residual = P_utility − Σ P_modelled_assets`.

---

## 4. Functional Requirements

### 4.1 OpenADR Compliance Obligations

| Requirement | Description |
|---|---|
| **FR-OA-01** | VEN MUST poll `/events` at 30 s fixed interval to detect new, updated, or deleted events. |
| **FR-OA-02** | VEN MUST obtain and refresh an OAuth2 token before calling any VTN endpoint. Token expires in 30 days; refresh on 401 response. |
| **FR-OA-03** | VEN MUST detect event deletion (next poll returns fewer events) and treat it as cancellation; roll back any active DR response on that event. |
| **FR-OA-04** | VEN MUST submit reports for any active `reportDescriptor` obligation extracted from event payloads. |
| **FR-OA-06** | All timestamps MUST be UTC, ISO 8601 / RFC 3339 format. |
| **FR-OA-07** | On VTN communication failure, VEN MUST back off exponentially (1 min → 2 min → 4 min → 8 min → max 15 min) and continue operating on last-known state. |
| **FR-OA-08** | VEN MUST handle event priority: lower priority number = higher priority. Newer event breaks ties at equal priority. |

> **Note — VEN-side target filtering (FR-OA-05 removed):** OpenADR 3.1 §"Object Privacy"
> assigns target filtering to the **VTN**, not the VEN. On a VEN request the VTN SHALL
> perform VEN_NAME matching and return an empty set for events the VEN is not targeted by.
> Additionally, the VTN strips VEN_NAME entries from the `targets` field in responses so
> VENs cannot infer which other VENs have access to an event (privacy). Both behaviours are
> implemented in openleadr-rs (`event.rs` — SQL-level filtering + `strip_ven_name_targets()`,
> merged in PR #374). The VEN therefore never receives events it should not act on and has
> no need to perform its own target matching.

### 4.2 HEMS Controller Requirements (UC-01–UC-12)

These are derived from Step 5 use cases. Each states the **intent and required outcome**.
For full step-by-step traces see `docs/VEN_Controller/Step5_UseCases.md` [ARCHIVED].

| UC | Name | Intent | Required Outcome |
|---|---|---|---|
| **UC-01** | EV overnight charge | User requests EV to 80% SoC by deadline with budget | Planner schedules FIRM slots during off-peak; defers peak hours; issues budget warning if cost exceeds limit |
| **UC-02** | Washing machine batch run | User starts wash cycle (CONTINUE policy) | Planner allocates batch window; CONTINUE policy ensures cycle completes past deadline if needed; mid-cycle interruption is avoided |
| **UC-03** | PV surplus cascade | PV generation exceeds consumption | System self-consumes first, stores surplus in battery if available, exports residual; no unnecessary grid import while PV is generating |
| **UC-04** | Day-ahead price update | VTN sends new PRICE event | `RATE_CHANGE` trigger fires replanning; FLEXIBLE slots are re-evaluated against new tariffs; FIRM slots within near-horizon are preserved |
| **UC-05** | VTN sends favourable far-horizon price | VTN publishes low-price window in advance | FLEXIBLE slots firm up to that window; FlexibilityEnvelope is reported to VTN as capacity reservation |
| **UC-06** | Grid emergency alert | VTN sends `ALERT_GRID_EMERGENCY` | Planner creates high-priority synthetic packet; FIRM slots are immediately adjusted to shed or limit import; VTN receives compliance report |
| **UC-07** | VTN capacity reservation | VTN sets import/export limits | Planner treats capacity limits as hard constraints; FlexibilityEnvelopes are computed within the remaining window and reported back |
| **UC-08** | EV disconnects mid-charge | EV cable removed while packet is ACTIVE | `ASSET_STATE_CHANGE` trigger fires; packet transitions to FAILED; Planner replans without EV; user is notified |
| **UC-09** | Tier fallback on time constraint | Packet deadline approaches with insufficient budget | Planner progresses through DeadlineTiers; switches to lower-cost / CONTINUE policy; notifies user of estimated partial completion |
| **UC-10** | Peak demand penalty avoidance | MeasurementWindow about to breach penalty threshold | Planner evaluates penalty cost vs. avoidance cost; reschedules allocations to stay below threshold if cheaper |
| **UC-11** | Consumption-only site | No PV, no battery | Algorithm produces valid plan using only grid import; no surplus cascade; FlexibilityEnvelope is zero export |
| **UC-12** | VTN communication loss | VTN unreachable for extended period | Planner applies `StaleRatePolicy` (default: `HEURISTIC_FORECAST`) for unknown future slots; reports last-known state; resumes normal operation on reconnect |

**Additional use cases (UC-13, UC-14 — in spec doc):**
- **UC-13**: VTN sends `DISPATCH_SETPOINT` — bypasses Planner, Dispatcher applies setpoint directly, compliance report submitted.
- **UC-14**: Thermal feedback loop — heat pump thermal model drives energy needs; schedule adjusts dynamically as outdoor temperature changes.

### 4.3 OpenADR Use Case Requirements (from USE-CASES.md)

| UC | Name | Key Requirement |
|---|---|---|
| **OA-01** | Emergency Load Shed | VEN MUST respond within one poll cycle (30 s); acknowledge event; correct start/stop timing |
| **OA-02** | Renewable Export Limitation | VEN MUST enforce `EXPORT_CAPACITY_LIMIT` per interval; handle ramp-down + ramp-up sequence |
| **OA-03** | Time-of-Use / Dynamic Price | VEN MUST handle multi-interval uniform pricing; update on late VTN corrections |
| **OA-04** | Planned Peak Shaving | VEN MUST track event lifecycle (far-future → near → active → past); handle event modifications |
| **OA-05** | EV Charging Management | VEN MUST resolve overlapping events by priority; apply group membership logic |
| **OA-06** | Battery Dispatch Window | VEN MUST honour directional control (charge vs. discharge) per interval |
| **OA-07** | Program Enrollment / Connectivity | VEN MUST acknowledge no-op events and send telemetry on schedule |
| **OA-08** | Event Cancellation | VEN MUST detect event deletion on next poll; perform clean rollback; maintain state consistency |

### 4.4 Asset Interface Requirements

Every asset (simulated or measured) MUST expose the same three-window interface to the
controller. The controller MUST NOT contain asset-specific formulas or physics. Whether
an asset is backed by a physics simulator, a real sensor, or a cloud API is an implementation
detail invisible to the planner, dispatcher, and monitor.

| Requirement | Description |
|---|---|
| **FR-ASSET-01** | Every asset MUST implement `current() → f64` — the present power in kW (sign convention: positive = import/consume, negative = export/generate). |
| **FR-ASSET-02** | Every asset MUST implement `forecast(horizon: Duration) → Vec<(DateTime, f64)>` — predicted power over the planning horizon, derived from the asset's own model (physics, heuristics, or external data). The planner MUST call this; it MUST NOT compute asset forecasts inline. |
| **FR-ASSET-03** | Every asset MUST implement `past(window: Duration) → Vec<(DateTime, f64)>` — recorded power history over the given window. For simulated assets this is the simulation record; for measured assets it is sensor readings. |
| **FR-ASSET-04** | The asset's simulation backend (physics model, irradiation curve, thermal model, etc.) MUST be encapsulated within the asset. Only the UI/test layer may read or write simulation parameters (via `/sim` endpoints). The controller layers (planner, dispatcher, monitor) MUST access assets only through the three-window interface above. |
| **FR-ASSET-05** | A simulated asset and a measured asset of the same type MUST be interchangeable from the controller's perspective — swapping one for the other MUST require no changes outside the asset's own module. |

### 4.5 Simulator Requirements

| Requirement | Description |
|---|---|
| **FR-SIM-01** | Simulator MUST model at minimum: PV, battery, EV, heater, base load. |
| **FR-SIM-02** | Asset model MUST be generic (`Vec<AssetEntry>`) — adding a new asset type must not require changes to the core simulator loop. |
| **FR-SIM-03** | PV generation MUST be derived from irradiation: `P_pv = P_max × (irradiation_W_m2 / irradiation_stc_W_m2)`. The default irradiation model follows `irradiation = irradiation_peak × sin(π × (hour − 6) / 12)` for 06:00–18:00, zero otherwise. Irradiation MUST be clamped to zero outside daylight hours regardless of manual overrides. Sign convention: `P_pv` is negative (generation/export). — *UI note: the current irradiation override in the simulator UI does not enforce the day/night clamp; this is a separate UI bug.* |
| **FR-SIM-04** | Battery MUST support bidirectional power (charge = positive, discharge = negative), round-trip efficiency, and SOC bounds. |
| **FR-SIM-05** | EV MUST support minimum charge rate (1.5 kW), stepless adjustment, 10 s response delay model. |
| **FR-SIM-06** | Heater MUST implement thermal model: `dT/dt = (P_heater × efficiency − ambient_loss) / thermal_mass`. |
| **FR-SIM-07** | Simulator state MUST persist to `/data/sim_state.json` (atomic write) and survive VEN restart. |
| **FR-SIM-08** | Profile configuration MUST be loaded from `VEN/profiles/<ven-id>.yaml` via `PROFILE_PATH` environment variable. |
| **FR-SIM-09** | `POST /sim/override` MUST be a full-replace operation (not a patch). |
| **FR-SIM-10** | `GET /sim/schema` MUST return the JSON schema for the profile YAML to support tooling. |
